//! The constructor, the setters, the accessors, and the candidate sets that the
//! semantic pass reads.

use super::*;

impl ModuleInfoExtractor {
    pub(crate) fn subclass_reference_candidates(&self) -> FxHashSet<Span> {
        let mut spans = self.requirement_reference_candidates();
        let add_proof = |spans: &mut FxHashSet<Span>, proof: &BindingClassProof| {
            spans.insert(proof.target_reference_span);
            spans.extend(proof.builtin_containers.iter().copied());
        };
        for access in &self.projected_member_accesses {
            add_proof(&mut spans, &access.proof);
            match self.expand_typed_property_compound(
                &format!("{}.{}", access.proof.target_name, access.property_path),
                Some(access.proof.target_reference_span),
            ) {
                TypedPropertyExpansion::Resolved(
                    target_name,
                    Some(target_reference_span),
                    requirements,
                )
                | TypedPropertyExpansion::CrossModule {
                    type_name: target_name,
                    origin: Some(target_reference_span),
                    requirements,
                    ..
                } => {
                    add_proof(
                        &mut spans,
                        &BindingClassProof {
                            builtin_containers: std::sync::Arc::clone(
                                &access.proof.builtin_containers,
                            ),
                            target_name,
                            target_reference_span,
                            requirements: BindingRequirements::Empty,
                        },
                    );
                    spans.extend(requirements.iter().map(|(reference, _)| *reference));
                }
                _ => {}
            }
        }
        self.add_factory_reference_candidates(&mut spans);
        for value in &self.direct_binding_values {
            if let Some(callback) = &value.callback_result {
                add_proof(&mut spans, &callback.class);
                spans.extend(callback.lazy_reference);
            }
            if let Some(proof) = &value.proof {
                add_proof(&mut spans, proof);
            }
            if let Some(proof) = &value.iteration_proof {
                add_proof(&mut spans, proof);
            }
            if let Some(PendingBindingValue::Call {
                result: Some(proof),
                ..
            }) = &value.pending
            {
                add_proof(&mut spans, proof);
            }
            if let Some(producer) = &value.producer {
                if let Some(proof) = &producer.result {
                    add_proof(&mut spans, proof);
                }
                for (_, proof) in &producer.shape {
                    add_proof(&mut spans, proof);
                }
            }
        }
        for access in &self.pending_subclass_member_accesses {
            if let SubclassAccessProof::Known(proof) = &access.proof {
                add_proof(&mut spans, proof);
            }
        }
        for value in &self.direct_binding_values {
            if let Some(PendingBindingValue::Call { reference, .. }) = &value.pending {
                spans.insert(*reference);
            }
        }

        spans.extend(
            self.subclass_declarations
                .values()
                .filter_map(|class| class.super_reference_span),
        );
        spans
    }

    fn factory_property_proofs(&self) -> impl Iterator<Item = &FactoryPropertyProof> {
        self.factory_property_accesses
            .iter()
            .map(|(proof, _, _)| proof)
            .chain(
                self.direct_binding_values
                    .iter()
                    .filter_map(|value| value.factory_property.as_ref()),
            )
            .chain(
                self.factory_property_observations
                    .values()
                    .filter_map(|(_, _, proof)| proof.as_ref()),
            )
    }

    pub(crate) fn callback_capture_reference_candidates(&self) -> FxHashSet<Span> {
        let mut references = FxHashSet::default();
        for value in &self.direct_binding_values {
            if let Some(callback) = &value.callback_result {
                references.extend(callback.lazy_reference);
            }
        }
        for proof in self.factory_property_proofs() {
            if let FactoryPropertyOrigin::Call { callbacks, .. } = &proof.origin {
                references.extend(
                    callbacks
                        .iter()
                        .filter_map(|(_, callback)| callback.lazy_reference),
                );
            }
        }
        references
    }

    fn add_factory_reference_candidates(&self, spans: &mut FxHashSet<Span>) {
        let add_proof = |spans: &mut FxHashSet<Span>, proof: &BindingClassProof| {
            spans.insert(proof.target_reference_span);
            spans.extend(proof.builtin_containers.iter().copied());
        };
        for (_, _, observation) in self.factory_property_observations.values() {
            if let Some(FactoryPropertyProof {
                origin: FactoryPropertyOrigin::Typed { proof, .. },
                ..
            }) = observation
            {
                add_proof(spans, proof);
            }
        }
        for proof in self.factory_property_proofs() {
            if let Some(class) = &proof.assigned_class {
                add_proof(spans, class);
            }
            match &proof.origin {
                FactoryPropertyOrigin::Call { callbacks, .. } => {
                    for (_, proof) in callbacks.iter() {
                        add_proof(spans, &proof.class);
                        spans.extend(proof.lazy_reference);
                    }
                }
                FactoryPropertyOrigin::CallbackParameter { reference, .. } => {
                    spans.insert(*reference);
                }
                FactoryPropertyOrigin::Typed { .. } => {}
            }
        }
    }

    pub(crate) fn set_subclass_binding_provenance(
        &mut self,
        references: FxHashMap<Span, Span>,
        namespace_references: FxHashSet<Span>,
        immutable_references: FxHashSet<Span>,
        builtin_container_references: FxHashSet<Span>,
        binding_writes: FxHashMap<Span, Vec<Span>>,
    ) {
        self.subclass_binding_references = references;
        self.install_requirement_provenance();
        self.subclass_namespace_references = namespace_references;
        self.subclass_immutable_references = immutable_references;
        self.builtin_container_references = builtin_container_references;
        self.subclass_binding_writes = binding_writes;
    }

    /// Store the namespace member uses and holder values that the semantic
    /// pass found.
    pub(crate) fn push_namespace_member_uses(&mut self, facts: Vec<SemanticFact>) {
        self.semantic_facts.extend(facts);
    }

    /// Record the template that a `defineOgImage('Name')` or
    /// `defineOgImageComponent('Name')` call names with a static string.
    pub(super) fn record_og_image_template_call(&mut self, call: &CallExpression<'_>) {
        let Expression::Identifier(callee) = &call.callee else {
            return;
        };
        if !crate::og_image::is_template_call(callee.name.as_str()) {
            return;
        }
        let name = match call.arguments.first().and_then(|arg| arg.as_expression()) {
            Some(Expression::StringLiteral(lit)) => lit.value.text(),
            Some(Expression::TemplateLiteral(lit)) if lit.expressions.is_empty() => {
                match lit
                    .quasis
                    .first()
                    .and_then(|quasi| quasi.value.cooked.as_ref())
                {
                    Some(cooked) => cooked.text(),
                    None => return,
                }
            }
            _ => return,
        };
        if let Some(candidate) = crate::og_image::template_candidate(&name) {
            self.og_image_template_candidates.push(candidate);
        }
    }

    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Every non-destructured CommonJS namespace binding, including ordinary
    /// `const X = require()` and TypeScript import-equals declarations.
    pub(crate) fn require_namespace_bindings(&self) -> Vec<String> {
        let mut bindings: Vec<String> = self
            .require_calls
            .iter()
            .filter_map(|call| call.local_name.clone())
            .collect();
        bindings.sort_unstable();
        bindings.dedup();
        bindings
    }

    /// Seed imported bindings for an isolated framework-template expression.
    /// The snippet contains no import declarations of its own, so its visitor
    /// needs the script-side locals before it can distinguish a dotted member
    /// read from handing the imported value over whole.
    pub(crate) fn register_template_object_locals(
        &mut self,
        locals: impl IntoIterator<Item = String>,
    ) {
        self.template_object_locals.extend(locals);
        self.namespace_import_locals
            .extend(self.template_object_locals.iter().cloned());
    }

    /// Mark the program top level as a component body. A top-level
    /// `await import()` then keeps its dynamic load kind.
    pub(crate) fn set_top_level_is_component_body(&mut self, value: bool) {
        self.top_level_is_component_body = value;
    }

    pub(crate) fn set_route_load_harvest_mode(&mut self, mode: RouteLoadHarvestMode) {
        self.route_load_harvest_mode = mode;
    }

    pub(crate) fn computed_enum_key_reference_spans(&self) -> FxHashSet<Span> {
        self.pending_computed_enum_key_uses
            .iter()
            .map(|usage| usage.key_object_span)
            .collect()
    }

    pub(crate) fn resolve_computed_enum_key_uses(
        &mut self,
        module_binding_references: &FxHashSet<Span>,
    ) {
        self.semantic_facts.extend(
            self.pending_computed_enum_key_uses
                .drain(..)
                .filter(|usage| module_binding_references.contains(&usage.key_object_span))
                .map(|usage| SemanticFact::ComputedEnumKeyUse(usage.fact)),
        );
    }

    pub(super) fn record_local_class_export(&mut self, name: String, info: LocalClassExportInfo) {
        self.local_class_exports.insert(name, info);
    }

    pub(crate) fn binding_target_names(&self) -> &FxHashMap<String, BindingTarget> {
        &self.binding_target_names
    }

    pub(crate) fn array_binding_element_types(&self) -> &FxHashMap<String, String> {
        &self.array_binding_element_types
    }

    /// Mutable accessor so the SFC props harvest can record `props.<field>` ->
    /// element-class entries after the visit but before the template-visible
    /// iterable-types read, typing a Vue `v-for="(util) of props.items"` loop
    /// item to the prop field's array element class (issue #1711).
    pub(crate) fn array_binding_element_types_mut(&mut self) -> &mut FxHashMap<String, String> {
        &mut self.array_binding_element_types
    }

    /// Seed the array/reactive-array element-type map from an already-parsed
    /// scope (the Astro frontmatter). Used by the Astro template-expression pass
    /// so a fresh extractor visiting `{utils.map((util) => util.getter)}` can bind
    /// `util` to the frontmatter's `const utils: Util[]` element class. See issue
    /// #1713.
    pub(crate) fn seed_array_binding_element_types(
        &mut self,
        element_types: &FxHashMap<String, String>,
    ) {
        for (binding, class) in element_types {
            self.array_binding_element_types
                .insert(binding.clone(), class.clone());
        }
    }
}
