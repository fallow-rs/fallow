//! Typed destructures, bound object names, local subclass accesses, typed
//! property chains, structural calls, object binding candidates, namespace
//! object aliases, and type-only exports.

use super::*;

impl ModuleInfoExtractor {
    pub(crate) fn resolve_typed_destructure_bindings(&mut self) {
        let pending = std::mem::take(&mut self.pending_typed_destructures);
        if pending.is_empty() {
            return;
        }
        for (local, property_key, type_name) in pending {
            let Some(properties) = self.interface_property_types.get(&type_name) else {
                continue;
            };
            let Some(class_name) = properties.get(&property_key) else {
                continue;
            };
            self.insert_class_binding_target_if_absent(local, class_name.name.clone());
        }
    }

    pub(super) fn resolve_bound_object_name(&self, object: &str) -> Option<BindingTarget> {
        if self.class_binding_root_is_shadowed(object) {
            return None;
        }
        if let Some(target_name) = self.binding_target_names.get(object) {
            return Some(target_name.clone());
        }

        self.binding_target_names
            .iter()
            .filter_map(|(binding, target_name)| {
                let suffix = object.strip_prefix(binding.as_str())?.strip_prefix('.')?;
                target_name
                    .class_with_suffix(suffix)
                    .map(|object_name| (binding.len(), BindingTarget::Class(object_name)))
            })
            .max_by_key(|(len, _)| *len)
            .map(|(_, object_name)| object_name)
    }

    /// String-only accesses lose receiver identity when scopes shadow a local.
    /// Propagate only references admitted against the actual class or instance binding.
    pub(super) fn propagate_local_subclass_member_accesses(&mut self) {
        for access in std::mem::take(&mut self.pending_subclass_member_accesses) {
            self.emit_local_subclass_member_access(&access.proof, &access.member);
        }
    }

    pub(super) fn emit_local_subclass_member_access(
        &mut self,
        proof: &SubclassAccessProof,
        member: &str,
    ) -> bool {
        let Some(class_name) = self.subclass_receiver_class(proof) else {
            return false;
        };
        let Some((base, heritage_span)) = self.resolve_local_subclass_base(&class_name) else {
            return false;
        };
        if let Some((namespace_local, class_export_name)) = base.split_once('.') {
            if class_export_name.contains('.')
                || !self.subclass_namespace_references.contains(&heritage_span)
            {
                return false;
            }
            self.semantic_facts
                .push(SemanticFact::QualifiedClassMemberAccess(
                    QualifiedClassMemberAccessFact {
                        namespace_local: namespace_local.to_string(),
                        class_export_name: class_export_name.to_string(),
                        member: member.to_string(),
                    },
                ));
        } else {
            self.member_accesses.push(MemberAccess {
                object: base,
                member: member.to_string(),
            });
        }
        true
    }

    pub(super) fn admitted_binding_containers(&self, proof: &BindingClassProof) -> bool {
        proof
            .builtin_containers
            .iter()
            .all(|span| self.builtin_container_references.contains(span))
    }

    fn subclass_receiver_class(&self, access: &SubclassAccessProof) -> Option<String> {
        let proof = match access {
            SubclassAccessProof::Known(proof) => proof.clone(),
            SubclassAccessProof::Value { id, requirements } => {
                let proof = self.direct_binding_values.get(*id)?.proof.clone()?;
                if !self.admitted_requirements(*requirements) {
                    return None;
                }
                proof
            }
        };
        if !self.admitted_binding_containers(&proof)
            || !self
                .subclass_immutable_references
                .contains(&proof.target_reference_span)
            || !self.admitted_requirements(proof.requirements)
        {
            return None;
        }
        let declaration = self.subclass_declarations.get(&proof.target_name)?;
        (self
            .subclass_binding_references
            .get(&proof.target_reference_span)
            == Some(&declaration.binding_span))
        .then(|| proof.target_name.clone())
    }

    /// Follow proven local heritage bindings without a depth cap. Cycles abstain.
    fn resolve_local_subclass_base(&self, name: &str) -> Option<(String, Span)> {
        let mut visited: FxHashSet<&str> = FxHashSet::default();
        let mut current = name;
        loop {
            if !visited.insert(current) {
                return None;
            }
            let base = self
                .local_class_exports
                .get(current)?
                .super_class
                .as_deref()?;
            let heritage_span = self
                .subclass_declarations
                .get(current)?
                .super_reference_span?;
            if !self.subclass_immutable_references.contains(&heritage_span) {
                return None;
            }
            let actual_binding = self.subclass_binding_references.get(&heritage_span)?;
            let Some(declaration) = self.subclass_declarations.get(base) else {
                return Some((base.to_string(), heritage_span));
            };
            if *actual_binding != declaration.binding_span {
                return None;
            }
            current = base;
        }
    }

    /// Expand a compound binding target (`Opts.c[.d...]`) through this file's
    /// named-type property maps: `interface_property_types` for interfaces and
    /// type-literal aliases, and a locally-declared class's own typed-property
    /// bindings (`local_class_exports[..].instance_bindings`, issue #1788, so
    /// an UNEXPORTED options class resolves; exported classes keep their
    /// analyze-side `instance_bindings` path too, the extract-side credit is
    /// additive and gated identically downstream).
    ///
    /// Each hop consumes one path segment, so the walk terminates after at most
    /// `segments` iterations even for self-referential types. A hop through a
    /// locally-declared non-class, non-literal type (an enum, a const) returns
    /// `Opaque` at the root; a hop that leaves the file (the type name is not
    /// declared here, i.e. imported) returns `CrossModule` so the caller can
    /// emit a `TypedPropertyMemberAccess` fact for the analyze-layer join. See
    /// issue #1785.
    pub(super) fn expand_typed_property_compound(
        &self,
        compound: &str,
        origin: Option<Span>,
    ) -> TypedPropertyExpansion {
        let mut segments = compound.split('.');
        let Some(root) = segments.next() else {
            return TypedPropertyExpansion::Opaque;
        };
        let remaining: Vec<&str> = segments.collect();
        if remaining.is_empty() {
            return TypedPropertyExpansion::Opaque;
        }
        let mut current = root.to_string();
        let mut current_origin = origin;
        let mut requirements = Vec::new();
        let mut idx = 0;
        while idx < remaining.len() {
            let next = if let Some(properties) = self.interface_property_types.get(&current) {
                let Some(next) = properties.get(remaining[idx]) else {
                    // The property is not a named-reference-typed member of
                    // this type (union / generic / unharvested shape): abstain.
                    return TypedPropertyExpansion::Opaque;
                };
                if let (Some(reference), Some(owner)) = (current_origin, next.owner_span) {
                    requirements.push((reference, owner));
                    current_origin = next.reference_span;
                } else {
                    current_origin = None;
                }
                next.name.clone()
            } else if let Some(local_class) = self.local_class_exports.get(&current) {
                let Some((_, bound_type)) = local_class
                    .instance_bindings
                    .iter()
                    .find(|(name, _)| name == remaining[idx])
                else {
                    // A known local class whose property is not a typed
                    // binding (untyped, private, method): abstain.
                    return TypedPropertyExpansion::Opaque;
                };
                if let Some(reference) = current_origin {
                    requirements.push((reference, local_class.binding_span));
                    current_origin = local_class.binding_origins.get(remaining[idx]).copied();
                }
                bound_type.clone()
            } else {
                if idx == 0 && self.local_declaration_names.contains(&current) {
                    // A local non-class root (enum, function, const): the
                    // compound is not a typed-property hop; leave it alone.
                    return TypedPropertyExpansion::Opaque;
                }
                return TypedPropertyExpansion::CrossModule {
                    type_name: current,
                    property_path: remaining[idx..].join("."),
                    origin: current_origin,
                    requirements,
                };
            };
            current = next;
            idx += 1;
        }
        TypedPropertyExpansion::Resolved(current, current_origin, requirements)
    }

    pub(super) fn resolve_structural_class_calls(&mut self) {
        if self.local_structural_functions.is_empty()
            || self.structural_class_call_candidates.is_empty()
        {
            return;
        }

        let candidates = std::mem::take(&mut self.structural_class_call_candidates);
        let mut additional_accesses = Vec::new();
        for candidate in candidates {
            let Some(function) = self.local_structural_functions.get(&candidate.callee_name) else {
                continue;
            };

            for (arg_index, arg) in candidate.arguments.iter().enumerate() {
                let Some(param_use) = function.params.get(&arg_index) else {
                    continue;
                };
                let Some(arg) = arg else {
                    continue;
                };
                let Some(class_name) = self.resolve_structural_call_argument(arg) else {
                    continue;
                };
                if param_use.type_name.as_deref() == Some(class_name.as_str()) {
                    continue;
                }
                for member in &param_use.members {
                    additional_accesses.push(MemberAccess {
                        object: class_name.clone(),
                        member: member.clone(),
                    });
                }
            }
        }
        self.member_accesses.extend(additional_accesses);
    }

    fn resolve_structural_call_argument(&self, arg: &StructuralCallArgument) -> Option<String> {
        match arg {
            StructuralCallArgument::DirectClass(class_name) => Some(class_name.clone()),
            StructuralCallArgument::Binding(binding) => self
                .binding_target_names
                .get(binding.as_str())
                .and_then(BindingTarget::class_name)
                .map(str::to_string),
        }
    }

    pub(super) fn resolve_object_binding_candidates(&mut self) {
        if self.object_binding_candidates.is_empty() {
            return;
        }

        // Bound the fixed-point (issue #1843 follow-up). Two costs explode on a
        // pathological minified bundle: per-pass copy work (bounded by the prefix
        // index below plus the `MAX_BINDING_TARGET_NAMES` size cap in
        // `insert_binding_target`) and the PASS COUNT. The loop otherwise runs up
        // to `candidates.len()` passes to resolve a rebinding chain, but real
        // chains are a few levels deep, so a small pass cap keeps every real case
        // while turning the crafted / minified worst case into K passes. A chain
        // deeper than the cap stops propagating (a false negative, matching the
        // FN-preferring `MAX_BINDING_PATH_DEPTH` / `MAX_TAINT_BINDING_HOPS`
        // doctrine).
        const MAX_RESOLUTION_PASSES: usize = 8;

        // Build the ancestor-prefix index once so `copy_nested_binding_targets`
        // enumerates keys under a source prefix in O(matches) rather than scanning
        // all of `binding_target_names`. Kept current by `insert_binding_target`.
        let mut index: FxHashMap<String, Vec<String>> = FxHashMap::default();
        for key in self.binding_target_names.keys() {
            for (dot, _) in key.match_indices('.') {
                index
                    .entry(key[..dot].to_string())
                    .or_default()
                    .push(key.clone());
            }
        }
        self.binding_target_prefix_index = Some(index);

        let candidates = self.object_binding_candidates.clone();
        let max_iterations = candidates
            .len()
            .saturating_add(1)
            .min(MAX_RESOLUTION_PASSES);
        for _ in 0..max_iterations {
            let mut changed = false;
            for candidate in &candidates {
                changed |= self.resolve_object_binding_candidate(candidate);
            }
            if !changed {
                break;
            }
        }

        self.binding_target_prefix_index = None;
    }

    pub(super) fn collect_namespace_object_aliases(
        &self,
    ) -> Vec<fallow_types::extract::NamespaceObjectAlias> {
        if self.binding_target_names.is_empty() || self.namespace_binding_names.is_empty() {
            return Vec::new();
        }
        let mut aliases = Vec::new();
        for (binding_path, target_name) in &self.binding_target_names {
            let Some(target_name) = target_name.class_name() else {
                continue;
            };
            if !self
                .namespace_binding_names
                .iter()
                .any(|name| name == target_name)
            {
                continue;
            }
            let Some((root_local, suffix)) = binding_path.split_once('.') else {
                continue;
            };
            for export in &self.exports {
                if export.local_name.as_deref() != Some(root_local) {
                    continue;
                }
                let canonical_name = match &export.name {
                    ExportName::Named(name) => name.clone(),
                    ExportName::Default => "default".to_string(),
                };
                aliases.push(fallow_types::extract::NamespaceObjectAlias {
                    via_export_name: canonical_name,
                    suffix: suffix.to_string(),
                    namespace_local: target_name.to_string(),
                });
            }
        }
        aliases
    }

    pub(super) fn push_type_export(&mut self, name: &str, span: Span) {
        self.exports.push(ExportInfo {
            name: ExportName::Named(name.to_string()),
            local_name: Some(name.to_string()),
            is_type_only: true,
            visibility: VisibilityTag::None,
            expected_unused_reason: None,
            span,
            members: vec![],
            is_side_effect_used: false,
            super_class: None,
            deprecated: false,
            deprecated_reason: None,
        });
    }
}
