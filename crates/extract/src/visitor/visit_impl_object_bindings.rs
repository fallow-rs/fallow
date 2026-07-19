#[allow(
    clippy::wildcard_imports,
    reason = "object binding helpers use AST node shapes"
)]
use oxc_ast::ast::*;

use super::super::{BindingTarget, ModuleInfoExtractor, ObjectBindingCandidate};

/// Per-module breadth cap on recorded object-binding candidates (issue #1843
/// follow-up): the companion to `MAX_TAINTED_BINDINGS_PER_MODULE` for the
/// `const obj = { key: ident }` object-binding channel. `object_binding_candidates`
/// grows once per identifier-valued property (recursively through nested object
/// literals) and is resolved by a fixpoint pass whose iteration bound is the
/// candidate count, so an O(n^2) worst case. A dense machine-generated bundle
/// with a huge object literal drove the working set (and that fixpoint) super-
/// linearly. Past the cap no NEW candidate is recorded, degrading an over-cap
/// file to module-level reachability instead of an object-binding member-access
/// claim, matching the false-negative-preferring direction of the taint caps.
/// Deliberately a constant, not a config knob: real hand-written modules stay
/// far below it.
const MAX_OBJECT_BINDING_CANDIDATES: usize = 4096;

impl ModuleInfoExtractor {
    pub(super) fn extract_angular_inject_target(
        &self,
        call: &CallExpression<'_>,
    ) -> Option<String> {
        super::super::helpers::extract_angular_inject_target(
            call,
            &|local_name, source, imported_name| {
                self.is_named_import_from(local_name, source, imported_name)
            },
        )
    }

    pub(super) fn copy_nested_binding_targets(
        &mut self,
        source_binding: &str,
        target_binding: &str,
    ) -> bool {
        // Nothing to copy from an empty map: skip the two `format!` allocations
        // and the no-op scan/collect below.
        if self.binding_target_names.is_empty() {
            return false;
        }
        let source_prefix = format!("{source_binding}.");
        let target_prefix = format!("{target_binding}.");
        // Prefix-index fast-path (issue #1843 follow-up): during the object-binding
        // fixed-point, enumerate the keys under `source_binding.` in O(matches) via
        // the index instead of scanning all of `binding_target_names`, which is
        // what made a real minified bundle full of nested object maps take tens of
        // seconds. Outside the pass (`None`) the map is small and the full scan is
        // used. Both branches produce the same `(binding, target)` set.
        let copied: Vec<(String, BindingTarget)> =
            if let Some(index) = &self.binding_target_prefix_index {
                index
                    .get(source_binding)
                    .into_iter()
                    .flatten()
                    .filter_map(|key| {
                        self.binding_target_names.get(key).map(|target| {
                            let suffix = &key[source_prefix.len()..];
                            (format!("{target_prefix}{suffix}"), target.clone())
                        })
                    })
                    .collect()
            } else {
                self.binding_target_names
                    .iter()
                    .filter_map(|(binding, target)| {
                        binding
                            .strip_prefix(&source_prefix)
                            .map(|suffix| (format!("{target_prefix}{suffix}"), target.clone()))
                    })
                    .collect()
            };

        let mut changed = false;
        for (binding, target) in copied {
            changed |= self.insert_binding_target(binding, target);
        }
        changed
    }

    fn insert_binding_target(&mut self, binding: String, target: BindingTarget) -> bool {
        if self.binding_target_names.get(&binding) == Some(&target) {
            return false;
        }
        // Hard size cap on the object-binding fixed-point's growth (issue #1843
        // follow-up). A pathological minified bundle (huge object maps copied
        // across many bindings) makes the fixed-point multiply
        // `binding_target_names` without bound, taking tens of seconds. Once the
        // map reaches the cap, stop recording NEW keys (an over-cap chain degrades
        // to a false negative, matching the FN-preferring doctrine); updates to an
        // already-present key still apply. Only reached via the fixed-point (the
        // index is `Some`), so the walk-time member crediting is unaffected.
        const MAX_BINDING_TARGET_NAMES: usize = 8192;
        if self.binding_target_prefix_index.is_some()
            && self.binding_target_names.len() >= MAX_BINDING_TARGET_NAMES
            && !self.binding_target_names.contains_key(&binding)
        {
            return false;
        }
        // Keep the ancestor-prefix index current for inserts made during the
        // fixed-point (issue #1843 follow-up), so a key added this pass is visible
        // to a later `copy_nested_binding_targets` call under every prefix.
        if let Some(index) = self.binding_target_prefix_index.as_mut() {
            for (dot, _) in binding.match_indices('.') {
                index
                    .entry(binding[..dot].to_string())
                    .or_default()
                    .push(binding.clone());
            }
        }
        self.binding_target_names.insert(binding, target);
        true
    }

    pub(in crate::visitor) fn resolve_object_binding_candidate(
        &mut self,
        candidate: &ObjectBindingCandidate,
    ) -> bool {
        let mut changed = false;
        if self
            .namespace_binding_names
            .iter()
            .any(|name| name == candidate.source_name.as_str())
        {
            changed |= self.insert_binding_target(
                candidate.binding_path.clone(),
                BindingTarget::Class(candidate.source_name.clone()),
            );
        } else if let Some(target_name) = self
            .binding_target_names
            .get(candidate.source_name.as_str())
            .cloned()
        {
            changed |= self.insert_binding_target(candidate.binding_path.clone(), target_name);
        }
        changed | self.copy_nested_binding_targets(&candidate.source_name, &candidate.binding_path)
    }

    pub(super) fn record_object_binding_targets(
        &mut self,
        binding_name: &str,
        obj: &ObjectExpression<'_>,
    ) {
        self.record_object_binding_targets_at_path(binding_name, obj);
    }

    fn record_object_binding_targets_at_path(
        &mut self,
        object_path: &str,
        obj: &ObjectExpression<'_>,
    ) {
        for prop in &obj.properties {
            let ObjectPropertyKind::ObjectProperty(prop) = prop else {
                continue;
            };
            let Some(key_name) = prop.key.static_name() else {
                continue;
            };

            let binding_path = format!("{object_path}.{key_name}");
            match &prop.value {
                // Per-module breadth cap (issue #1843 follow-up): the guard stops
                // recording once at capacity so a pathological object literal
                // cannot grow the candidate set (and its O(n^2) fixpoint resolver)
                // without bound. At capacity the arm falls through to the no-op
                // `_ =>` arm, identical to skipping the push.
                Expression::Identifier(ident)
                    if self.object_binding_candidates.len() < MAX_OBJECT_BINDING_CANDIDATES =>
                {
                    self.object_binding_candidates.push(ObjectBindingCandidate {
                        binding_path,
                        source_name: ident.name.to_string(),
                    });
                }
                Expression::ObjectExpression(child) => {
                    self.record_object_binding_targets_at_path(&binding_path, child);
                }
                _ => {}
            }
        }
    }
}

#[cfg(all(test, not(miri)))]
mod tests {
    use super::MAX_OBJECT_BINDING_CANDIDATES;
    use crate::visitor::ModuleInfoExtractor;
    use oxc_allocator::Allocator;
    use oxc_ast_visit::Visit;
    use oxc_parser::Parser;
    use oxc_span::SourceType;

    /// A single object literal with far more identifier-valued properties than
    /// the per-module cap must not grow `object_binding_candidates` past the cap.
    /// Mirrors `tainted_binding_recording_is_bounded_on_dense_source`: the
    /// object-binding channel has the same super-linear failure mode (an O(n^2)
    /// fixpoint resolver over an unbounded candidate set) on dense machine-
    /// generated source, and the cap degrades over-cap files to module-level
    /// reachability rather than OOMing. See issue #1843 follow-up.
    #[test]
    fn object_binding_candidate_recording_is_bounded_on_dense_source() {
        use std::fmt::Write as _;

        let over_cap = MAX_OBJECT_BINDING_CANDIDATES + 1000;
        let mut props = String::new();
        for k in 0..over_cap {
            // Each identifier-valued property seeds one object-binding candidate.
            let _ = write!(props, "k{k}: v{k}, ");
        }
        let source = format!("const big = {{ {props} }};");

        let allocator = Allocator::default();
        let parser_return = Parser::new(&allocator, &source, SourceType::ts()).parse();
        let mut extractor = ModuleInfoExtractor::new();
        extractor.visit_program(&parser_return.program);

        // The cap must engage (input deterministically exceeds it) but never
        // zero out recording.
        assert!(
            !extractor.object_binding_candidates.is_empty(),
            "the cap must not zero out object-binding recording"
        );
        assert!(
            extractor.object_binding_candidates.len() <= MAX_OBJECT_BINDING_CANDIDATES,
            "object-binding candidate recording must stay bounded at the \
             per-module cap on dense source (got {})",
            extractor.object_binding_candidates.len()
        );
    }
}
