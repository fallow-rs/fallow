//! Destructured object bindings (`const { a: { b } = fallback } = source`).
//!
//! Each default re-walks its left pattern against the fallback, so nested
//! defaults double the work at every level. A step budget bounds the walk.

use oxc_ast::ast::{BindingPattern, Expression, ObjectPattern, ObjectPropertyKind};

use super::super::ModuleInfoExtractor;
use super::{DirectObjectBindingScope, direct_object_binding_expression_path};
use crate::js_text::JsText;

/// Upper bound on pattern nodes that one destructuring declaration visits.
const MAX_OBJECT_PATTERN_STEPS: usize = 4096;

fn take_step(budget: &mut usize) -> bool {
    let Some(rest) = budget.checked_sub(1) else {
        return false;
    };
    *budget = rest;
    true
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
        let mut budget = MAX_OBJECT_PATTERN_STEPS;
        self.record_direct_object_binding_object_pattern(pattern, &source_path, &mut budget);
    }

    fn record_direct_object_binding_object_pattern(
        &mut self,
        pattern: &ObjectPattern<'_>,
        source_path: &str,
        budget: &mut usize,
    ) {
        if !take_step(budget) {
            return;
        }
        for property in &pattern.properties {
            let Some(property_name) = property.key.static_name() else {
                continue;
            };
            self.record_direct_object_binding_pattern(
                &property.value,
                &format!("{source_path}.{}", property_name.text()),
                budget,
            );
        }
    }

    fn record_direct_object_binding_pattern(
        &mut self,
        pattern: &BindingPattern<'_>,
        source_path: &str,
        budget: &mut usize,
    ) {
        if !take_step(budget) {
            return;
        }
        match pattern {
            BindingPattern::BindingIdentifier(binding) => {
                self.copy_direct_object_binding_targets(source_path, binding.name.as_str());
            }
            BindingPattern::ObjectPattern(pattern) => {
                self.record_direct_object_binding_object_pattern(pattern, source_path, budget);
            }
            BindingPattern::AssignmentPattern(assignment) => {
                self.record_direct_object_binding_pattern(&assignment.left, source_path, budget);
                self.record_direct_object_binding_fallback(
                    &assignment.left,
                    &assignment.right,
                    budget,
                );
            }
            BindingPattern::ArrayPattern(_) => {}
        }
    }

    fn record_direct_object_binding_fallback(
        &mut self,
        pattern: &BindingPattern<'_>,
        fallback: &Expression<'_>,
        budget: &mut usize,
    ) {
        if !take_step(budget) {
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
                        self.record_direct_object_binding_fallback(&property.value, value, budget);
                    }
                }
                fallback => {
                    if let Some(fallback_path) = direct_object_binding_expression_path(fallback) {
                        self.record_direct_object_binding_object_pattern(
                            pattern,
                            &fallback_path,
                            budget,
                        );
                    }
                }
            },
            BindingPattern::AssignmentPattern(assignment) => {
                self.record_direct_object_binding_fallback(&assignment.left, fallback, budget);
                self.record_direct_object_binding_fallback(
                    &assignment.left,
                    &assignment.right,
                    budget,
                );
            }
            BindingPattern::ArrayPattern(_) => {}
        }
    }
}
