#[allow(
    clippy::wildcard_imports,
    reason = "object binding helpers use AST node shapes"
)]
use oxc_ast::ast::*;

use super::super::{BindingTarget, ModuleInfoExtractor, ObjectBindingCandidate};

/// Deepest `a.b.c` binding path the resolver materializes. Member-usage
/// analysis reads shallow paths (`obj.member`, at most a few hops through
/// a namespace); deeper paths contribute no findings. The cap also breaks
/// the combinatorial blowup of cyclic object-binding candidates
/// (`const p = { a: q }; const q = { a: p };`), where every fixpoint
/// iteration would otherwise deepen and multiply each copied path.
/// Minified bundles hit that shape in practice.
const MAX_BINDING_PATH_DEPTH: usize = 8;

/// Hard ceiling on resolved binding paths per module: the backstop when
/// many independent candidates stay under the depth cap. Far above any
/// legitimate module; only pathological inputs approach it.
const MAX_BINDING_TARGETS: usize = 65_536;

/// Number of `.`-separated segments in a binding path.
fn binding_path_depth(binding: &str) -> usize {
    binding.matches('.').count()
}

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
        if self.binding_target_names.len() >= MAX_BINDING_TARGETS {
            return false;
        }
        let source_prefix = format!("{source_binding}.");
        let target_prefix = format!("{target_binding}.");
        let target_depth = binding_path_depth(target_binding);
        let copied: Vec<(String, BindingTarget)> = self
            .binding_target_names
            .iter()
            .filter_map(|(binding, target)| {
                let suffix = binding.strip_prefix(&source_prefix)?;
                // Depth check before allocating the joined key: the copy
                // adds the suffix's segments plus the joining dot onto the
                // target path.
                if target_depth + binding_path_depth(suffix) + 1 > MAX_BINDING_PATH_DEPTH {
                    return None;
                }
                Some((format!("{target_prefix}{suffix}"), target.clone()))
            })
            .collect();

        let mut changed = false;
        for (binding, target) in copied {
            changed |= self.insert_binding_target(binding, target);
        }
        changed
    }

    fn insert_binding_target(&mut self, binding: String, target: BindingTarget) -> bool {
        if binding_path_depth(&binding) > MAX_BINDING_PATH_DEPTH {
            return false;
        }
        if self.binding_target_names.get(&binding) == Some(&target) {
            return false;
        }
        if self.binding_target_names.len() >= MAX_BINDING_TARGETS
            && !self.binding_target_names.contains_key(&binding)
        {
            return false;
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
                Expression::Identifier(ident) => {
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
