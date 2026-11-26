//! Import forwards, local export specifiers, Lit custom elements, and side-effect
//! registrations that the extractor resolves after the walk.

use super::*;

impl ModuleInfoExtractor {
    /// The named or default import that a top-level `local_name` binds, unless
    /// a local declaration of the same name owns the binding.
    fn forwardable_import(&self, local_name: &str) -> Option<ForwardableImport> {
        if self.local_declaration_names.contains(local_name) {
            return None;
        }
        self.imports.iter().find_map(|import| {
            if import.local_name != local_name {
                return None;
            }
            let imported_name = match &import.imported_name {
                ImportedName::Named(name) => name.clone(),
                ImportedName::Default => "default".to_string(),
                ImportedName::Namespace | ImportedName::SideEffect => return None,
            };
            Some(ForwardableImport {
                source: import.source.clone(),
                imported_name,
                is_type_only: import.is_type_only,
            })
        })
    }

    /// Record a re-export of `import` under `exported_name`.
    ///
    /// The unresolvable literal lives in the matching import statement, not in
    /// the export statement, so a cross-statement source anchor would smear
    /// suppression coverage across unrelated lines. Both extra spans stay
    /// empty and consumers fall back to the export span.
    fn push_import_forward(
        &mut self,
        import: ForwardableImport,
        exported_name: String,
        is_type_only: bool,
        span: Span,
    ) {
        self.re_exports.push(ReExportInfo {
            source: import.source,
            imported_name: import.imported_name,
            exported_name,
            is_type_only: is_type_only || import.is_type_only,
            span,
            ..Default::default()
        });
    }

    pub(crate) fn resolve_pending_local_export_specifiers(&mut self) {
        self.resolve_pending_binding_forwards();
        let pending = std::mem::take(&mut self.pending_local_export_specifiers);
        for spec in pending {
            if let Some(import) = self.forwardable_import(&spec.local_name) {
                self.push_import_forward(import, spec.exported_name, spec.is_type_only, spec.span);
            } else {
                self.exports.push(ExportInfo {
                    name: ExportName::Named(spec.exported_name),
                    local_name: Some(spec.local_name),
                    is_type_only: spec.is_type_only,
                    visibility: VisibilityTag::None,
                    expected_unused_reason: None,
                    span: spec.span,
                    members: vec![],
                    is_side_effect_used: false,
                    super_class: None,
                    deprecated: false,
                    deprecated_reason: None,
                });
            }
        }
    }

    /// Turn `export default x` and `export = x` of an imported binding into a
    /// re-export of `default`, the same record that `export { x as default }`
    /// makes. The graph then credits the importers of this file to the
    /// original export (issue #3392).
    ///
    /// `export = x` also flags the file as carrying CommonJS exports: the
    /// whole module object is `x`, so a member read on a require binding of
    /// this file reads a member of the forwarded default export.
    fn resolve_pending_binding_forwards(&mut self) {
        let pending = std::mem::take(&mut self.pending_binding_forwards);
        for forward in pending {
            match forward {
                PendingBindingForward::ExportDefault { local_name } => {
                    let Some(import) = self.forwardable_import(&local_name) else {
                        continue;
                    };
                    let Some(index) = self.exports.iter().position(|export| {
                        matches!(export.name, ExportName::Default)
                            && export.local_name.as_deref() == Some(local_name.as_str())
                    }) else {
                        continue;
                    };
                    let export = self.exports.remove(index);
                    self.push_import_forward(
                        import,
                        "default".to_string(),
                        export.is_type_only,
                        export.span,
                    );
                }
                PendingBindingForward::ExportAssignment { local_name, span } => {
                    let Some(import) = self.forwardable_import(&local_name) else {
                        continue;
                    };
                    self.has_cjs_exports = true;
                    self.push_import_forward(import, "default".to_string(), false, span);
                }
            }
        }
    }

    fn is_lit_custom_element_decorator(&self, decorator: &LitCustomElementDecorator) -> bool {
        const LIT_DECORATOR_SOURCES: &[&str] =
            &["lit/decorators.js", "lit/decorators/custom-element.js"];

        self.imports.iter().any(|import| {
            LIT_DECORATOR_SOURCES.contains(&import.source.as_str())
                && match decorator {
                    LitCustomElementDecorator::Named { local_name } => {
                        import.local_name == *local_name
                            && matches!(
                                &import.imported_name,
                                ImportedName::Named(name) if name == "customElement"
                            )
                    }
                    LitCustomElementDecorator::Namespace { local_name } => {
                        import.local_name == *local_name
                            && matches!(import.imported_name, ImportedName::Namespace)
                    }
                }
        })
    }

    pub(super) fn is_node_module_register(&self, local_name: &str, via_namespace: bool) -> bool {
        const NODE_MODULE_SOURCES: &[&str] = &["node:module", "module"];

        self.imports.iter().any(|import| {
            NODE_MODULE_SOURCES.contains(&import.source.as_str())
                && import.local_name == local_name
                && if via_namespace {
                    matches!(import.imported_name, ImportedName::Namespace)
                } else {
                    matches!(
                        &import.imported_name,
                        ImportedName::Named(name) if name == "register"
                    )
                }
        })
    }

    fn apply_lit_custom_element_candidates(&mut self) {
        if self.lit_custom_element_candidates.is_empty() {
            return;
        }

        let mut class_names = Vec::new();
        let mut anonymous_default_indices = Vec::new();
        let mut registrations = Vec::new();
        for candidate in &self.lit_custom_element_candidates {
            if !self.is_lit_custom_element_decorator(&candidate.decorator) {
                continue;
            }
            // Record the registration for the Lit `unrendered-component` arm, but
            // only when a static tag literal was recoverable (a computed tag is
            // not flaggable). The class-local name drives the public-API abstain.
            if let Some(tag) = &candidate.tag {
                let class_local_name = match &candidate.target {
                    SideEffectRegistrationTarget::LocalClass(name) => name.clone(),
                    SideEffectRegistrationTarget::AnonymousDefaultExport(_) => String::new(),
                };
                registrations.push(fallow_types::extract::RegisteredCustomElement {
                    tag: tag.clone(),
                    class_local_name,
                    span_start: candidate.span_start,
                });
            }
            match &candidate.target {
                SideEffectRegistrationTarget::LocalClass(class_name) => {
                    class_names.push(class_name.clone());
                }
                SideEffectRegistrationTarget::AnonymousDefaultExport(index) => {
                    anonymous_default_indices.push(*index);
                }
            }
        }

        self.registered_custom_elements.extend(registrations);
        self.side_effect_registered_class_names.extend(class_names);
        for index in anonymous_default_indices {
            if let Some(export) = self.exports.get_mut(index) {
                export.is_side_effect_used = true;
            }
        }
    }

    pub(super) fn record_lit_custom_element_candidate(
        &mut self,
        decorator: LitCustomElementDecorator,
        target: SideEffectRegistrationTarget,
        tag: Option<String>,
        span_start: u32,
    ) {
        self.lit_custom_element_candidates
            .push(LitCustomElementCandidate {
                decorator,
                target,
                tag,
                span_start,
            });
    }

    pub(super) fn apply_side_effect_registrations(&mut self) {
        self.apply_lit_custom_element_candidates();
        if self.side_effect_registered_class_names.is_empty() {
            return;
        }
        for export in &mut self.exports {
            let Some(local_name) = export.local_name.as_deref() else {
                continue;
            };
            if self.side_effect_registered_class_names.contains(local_name) {
                export.is_side_effect_used = true;
            }
        }
    }
}
