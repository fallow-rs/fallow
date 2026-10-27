//! Destructured object bindings (`const { a: { b } = fallback } = source`).
//!
//! Each default re-walks its left pattern against the fallback, so nested
//! defaults double the work at every level. One walk of a declaration visits
//! each (pattern, fallback) pair once, and a step budget bounds the rest. The
//! budget is far above the size of a pattern that a person writes, so a wide
//! pattern keeps all of its credits.

use oxc_ast::ast::{BindingPattern, Expression, ObjectPattern, ObjectPropertyKind};
use rustc_hash::FxHashSet;

use super::super::ModuleInfoExtractor;
use super::{DirectObjectBindingScope, direct_object_binding_expression_path};
use crate::js_text::JsText;

/// Upper bound on the work of one declaration: pattern nodes, fallback
/// properties, and recorded targets.
const MAX_OBJECT_PATTERN_STEPS: usize = 1 << 16;

/// The state of one walk: the remaining steps and the (pattern, fallback)
/// pairs that the walk has already visited, keyed by AST node address.
struct PatternWalk {
    steps: usize,
    visited_fallbacks: FxHashSet<(usize, usize)>,
}

impl PatternWalk {
    fn new() -> Self {
        Self {
            steps: MAX_OBJECT_PATTERN_STEPS,
            visited_fallbacks: FxHashSet::default(),
        }
    }

    fn take(&mut self, steps: usize) -> bool {
        let Some(rest) = self.steps.checked_sub(steps) else {
            self.steps = 0;
            return false;
        };
        self.steps = rest;
        true
    }

    fn first_fallback_visit(
        &mut self,
        pattern: &BindingPattern<'_>,
        fallback: &Expression<'_>,
    ) -> bool {
        let key = (
            std::ptr::from_ref(pattern) as usize,
            std::ptr::from_ref(fallback) as usize,
        );
        self.visited_fallbacks.insert(key)
    }
}

impl ModuleInfoExtractor {
    pub(in crate::visitor) fn record_direct_object_binding_destructure(
        &mut self,
        pattern: &ObjectPattern<'_>,
        init: &Expression<'_>,
    ) {
        let Some(source_path) = direct_object_binding_expression_path(init) else {
            return;
        };
        let mut walk = PatternWalk::new();
        self.record_direct_object_binding_object_pattern(pattern, &source_path, &mut walk);
    }

    fn record_direct_object_binding_object_pattern(
        &mut self,
        pattern: &ObjectPattern<'_>,
        source_path: &str,
        walk: &mut PatternWalk,
    ) {
        if !walk.take(1) {
            return;
        }
        for property in &pattern.properties {
            let Some(property_name) = property.key.static_name() else {
                continue;
            };
            self.record_direct_object_binding_pattern(
                &property.value,
                &format!("{source_path}.{}", property_name.text()),
                walk,
            );
        }
    }

    fn record_direct_object_binding_pattern(
        &mut self,
        pattern: &BindingPattern<'_>,
        source_path: &str,
        walk: &mut PatternWalk,
    ) {
        if !walk.take(1) {
            return;
        }
        match pattern {
            BindingPattern::BindingIdentifier(binding) => {
                self.copy_direct_object_binding_targets(source_path, binding.name.as_str());
            }
            BindingPattern::ObjectPattern(pattern) => {
                self.record_direct_object_binding_object_pattern(pattern, source_path, walk);
            }
            BindingPattern::AssignmentPattern(assignment) => {
                self.record_direct_object_binding_pattern(&assignment.left, source_path, walk);
                self.record_direct_object_binding_fallback(
                    &assignment.left,
                    &assignment.right,
                    walk,
                );
            }
            BindingPattern::ArrayPattern(_) => {}
        }
    }

    fn record_direct_object_binding_fallback(
        &mut self,
        pattern: &BindingPattern<'_>,
        fallback: &Expression<'_>,
        walk: &mut PatternWalk,
    ) {
        if !walk.first_fallback_visit(pattern, fallback) || !walk.take(1) {
            return;
        }
        match pattern {
            BindingPattern::BindingIdentifier(binding) => match fallback {
                Expression::NewExpression(new_expression) => {
                    if let Some(class_name) = self.direct_new_expression_class_name(new_expression)
                    {
                        self.record_direct_object_binding_target(
                            binding.name.as_str(),
                            binding.name.to_string(),
                            class_name,
                            DirectObjectBindingScope::BindingOwner,
                        );
                    }
                }
                Expression::ObjectExpression(object) => {
                    if !walk.take(object.properties.len()) {
                        return;
                    }
                    self.record_direct_object_binding_targets_at_path(
                        binding.name.as_str(),
                        binding.name.as_str(),
                        object,
                        DirectObjectBindingScope::BindingOwner,
                    );
                }
                fallback => {
                    if let Some(fallback_path) = direct_object_binding_expression_path(fallback) {
                        self.copy_direct_object_binding_targets(
                            &fallback_path,
                            binding.name.as_str(),
                        );
                    }
                }
            },
            BindingPattern::ObjectPattern(pattern) => match fallback {
                Expression::ObjectExpression(object) => {
                    // Each pattern property scans the fallback properties.
                    let lookups = pattern
                        .properties
                        .len()
                        .saturating_mul(object.properties.len());
                    if !walk.take(lookups) {
                        return;
                    }
                    for property in &pattern.properties {
                        let Some(property_name) = property.key.static_name() else {
                            continue;
                        };
                        let Some(value) = object.properties.iter().rev().find_map(|candidate| {
                            let ObjectPropertyKind::ObjectProperty(candidate) = candidate else {
                                return None;
                            };
                            (candidate.key.static_name().as_ref() == Some(&property_name))
                                .then_some(&candidate.value)
                        }) else {
                            continue;
                        };
                        self.record_direct_object_binding_fallback(&property.value, value, walk);
                    }
                }
                fallback => {
                    if let Some(fallback_path) = direct_object_binding_expression_path(fallback) {
                        self.record_direct_object_binding_object_pattern(
                            pattern,
                            &fallback_path,
                            walk,
                        );
                    }
                }
            },
            BindingPattern::AssignmentPattern(assignment) => {
                self.record_direct_object_binding_fallback(&assignment.left, fallback, walk);
                self.record_direct_object_binding_fallback(
                    &assignment.left,
                    &assignment.right,
                    walk,
                );
            }
            BindingPattern::ArrayPattern(_) => {}
        }
    }
}
