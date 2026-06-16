mod declarations;
mod helpers;
mod react;
mod visit_impl;

use oxc_ast::ast::{
    Argument, BindingPattern, CallExpression, Expression, ImportExpression, ObjectPattern,
    ObjectProperty, ObjectPropertyKind, Statement,
};
use oxc_span::Span;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::suppress::ParsedSuppressions;
use crate::{
    DynamicImportInfo, DynamicImportPattern, ExportInfo, ExportName, ImportInfo, ImportedName,
    MemberAccess, MemberInfo, MemberKind, ModuleInfo, ReExportInfo, RequireCallInfo, VisibilityTag,
};
use fallow_types::extract::{
    CalleeUse, ClassHeritageInfo, ComponentFunction, ComponentProp, DiKeySite, HookUse,
    LocalTypeDeclaration, MisplacedDirectiveSite, PublicSignatureTypeReference, RenderEdge,
    SanitizedSinkArg, SanitizerScope, SecurityControlSite, SinkLiteralValue, SinkSite,
    SkippedSecurityCalleeSite, TaintedBinding,
};
use helpers::LitCustomElementDecorator;

#[derive(Debug, Clone)]
struct LocalClassExportInfo {
    members: Vec<MemberInfo>,
    super_class: Option<String>,
    implemented_interfaces: Vec<String>,
    instance_bindings: Vec<(String, String)>,
}

#[derive(Debug, Clone)]
struct LocalSignatureTypeReference {
    owner_name: String,
    type_name: String,
    span: Span,
}

#[derive(Debug, Clone)]
struct ObjectBindingCandidate {
    binding_path: String,
    source_name: String,
}

#[derive(Debug, Clone)]
struct PendingLocalExportSpecifier {
    local_name: String,
    exported_name: String,
    is_type_only: bool,
    span: Span,
}

#[derive(Debug, Clone)]
struct StructuralParameterUse {
    type_name: String,
    members: FxHashSet<String>,
}

#[derive(Debug, Clone, Default)]
struct LocalStructuralFunction {
    params: FxHashMap<usize, StructuralParameterUse>,
}

#[derive(Debug, Clone)]
enum StructuralCallArgument {
    DirectClass(String),
    Binding(String),
}

#[derive(Debug, Clone)]
struct StructuralClassCallCandidate {
    callee_name: String,
    arguments: Vec<Option<StructuralCallArgument>>,
}

#[derive(Debug, Clone)]
pub(crate) struct FactoryCallCandidate {
    pub(crate) local_name: String,
    pub(crate) callee_object: String,
    pub(crate) callee_method: String,
}

#[derive(Debug, Clone)]
pub(crate) struct PendingPlaywrightFactory {
    pub(crate) test_name: String,
    pub(crate) base_name: String,
    pub(crate) type_bindings: Vec<(String, String)>,
}

#[derive(Debug, Clone)]
struct SourceReturnPath {
    arg_index: usize,
    suffixes: Vec<String>,
}

#[derive(Debug, Clone, Default)]
struct SourceReturningHelper {
    paths: Vec<SourceReturnPath>,
}

#[derive(Debug, Clone)]
enum SideEffectRegistrationTarget {
    LocalClass(String),
    AnonymousDefaultExport(usize),
}

#[derive(Debug, Clone)]
struct LitCustomElementCandidate {
    decorator: LitCustomElementDecorator,
    target: SideEffectRegistrationTarget,
}

#[derive(Debug, Clone)]
pub(crate) struct InlineTemplateFinding {
    pub(crate) template_source: String,
    pub(crate) decorator_start: u32,
}

#[derive(Default)]
pub(crate) struct ModuleInfoExtractor {
    pub(crate) exports: Vec<ExportInfo>,
    pub(crate) imports: Vec<ImportInfo>,
    pub(crate) re_exports: Vec<ReExportInfo>,
    pub(crate) dynamic_imports: Vec<DynamicImportInfo>,
    pub(crate) dynamic_import_patterns: Vec<DynamicImportPattern>,
    pub(crate) require_calls: Vec<RequireCallInfo>,
    pub(crate) package_path_references: Vec<String>,
    pub(crate) member_accesses: Vec<MemberAccess>,
    pub(crate) whole_object_uses: Vec<String>,
    pub(crate) has_cjs_exports: bool,
    pub(crate) has_angular_component_template_url: bool,
    handled_require_spans: FxHashSet<Span>,
    handled_import_spans: FxHashSet<Span>,
    namespace_binding_names: Vec<String>,
    binding_target_names: FxHashMap<String, String>,
    interface_property_types: FxHashMap<String, FxHashMap<String, String>>,
    pending_typed_destructures: Vec<(String, String, String)>,
    iterable_element_types: FxHashMap<String, String>,
    object_binding_candidates: Vec<ObjectBindingCandidate>,
    local_declaration_names: FxHashSet<String>,
    pending_local_export_specifiers: Vec<PendingLocalExportSpecifier>,
    local_structural_functions: FxHashMap<String, LocalStructuralFunction>,
    structural_class_call_candidates: Vec<StructuralClassCallCandidate>,
    namespace_depth: u32,
    pending_namespace_members: Vec<MemberInfo>,
    pub(crate) class_heritage: Vec<ClassHeritageInfo>,
    /// `(token_export_name, interface_name)` for `new InjectionToken<I>(...)`
    /// declarations imported from `@angular/core`. See issue #920.
    pub(crate) injection_tokens: Vec<(String, String)>,
    pub(crate) local_type_declarations: Vec<LocalTypeDeclaration>,
    pub(crate) public_signature_type_references: Vec<PublicSignatureTypeReference>,
    local_signature_type_references: Vec<LocalSignatureTypeReference>,
    local_class_exports: FxHashMap<String, LocalClassExportInfo>,
    playwright_fixture_types: FxHashMap<String, Vec<(String, String)>>,
    block_depth: u32,
    function_depth: u32,
    pub(crate) class_super_stack: Vec<Option<String>>,
    pub(crate) inline_template_findings: Vec<InlineTemplateFinding>,
    pub(crate) side_effect_registered_class_names: FxHashSet<String>,
    lit_custom_element_candidates: Vec<LitCustomElementCandidate>,
    pub(crate) factory_call_candidates: Vec<FactoryCallCandidate>,
    pub(crate) node_module_register_url_bindings: FxHashMap<String, Vec<String>>,
    pub(crate) child_process_fork_bindings: FxHashSet<String>,
    pub(crate) child_process_namespace_bindings: FxHashSet<String>,
    pub(crate) node_path_namespace_bindings: FxHashSet<String>,
    pub(crate) node_url_file_url_to_path_bindings: FxHashSet<String>,
    pub(crate) current_module_file_path_bindings: FxHashSet<String>,
    pub(crate) child_process_fork_target_bindings: FxHashMap<String, Vec<String>>,
    pub(crate) static_string_bindings: FxHashMap<String, String>,
    pub(crate) static_string_arrays: FxHashMap<String, Vec<String>>,
    pub(crate) static_object_property_values: FxHashMap<String, FxHashMap<String, Vec<String>>>,
    pub(crate) loop_string_bindings: Vec<FxHashMap<String, Vec<String>>>,
    pub(crate) loop_object_property_values: Vec<FxHashMap<String, FxHashMap<String, Vec<String>>>>,
    pub(crate) package_resolution_function_args: FxHashMap<String, usize>,
    pub(crate) nested_declaration_stack: Vec<FxHashSet<String>>,
    pub(crate) class_type_param_constraints: Vec<FxHashMap<String, Option<String>>>,
    pub(crate) pending_playwright_factory_calls: Vec<PendingPlaywrightFactory>,
    pub(crate) pending_playwright_factory_aliases: Vec<(String, String)>,
    source_returning_helpers: FxHashMap<String, SourceReturningHelper>,
    /// File-level string directives (`"use client"`, `"use server"`) captured
    /// from `Program::directives`. Consumed by the security `client-server-leak`
    /// detector to identify React Server Component client boundaries.
    pub(crate) directives: Vec<String>,
    /// Byte-offset starts of dynamic `import()` expressions wrapped in
    /// `next/dynamic(() => import('./X'), { ssr: false })`. Consumed by the
    /// security `client-server-leak` BFS to exclude the ssr:false client-only
    /// escape hatch.
    pub(crate) client_only_dynamic_import_spans: Vec<u32>,
    /// Captured security sink sites (category-blind). Consumed by the
    /// catalogue-driven `tainted_sink` detector.
    pub(crate) security_sinks: Vec<SinkSite>,
    /// Count of sink-shaped nodes whose callee could not be flattened to a
    /// static path (dynamic dispatch, computed members, aliased bindings).
    pub(crate) security_sinks_skipped: u32,
    /// Span-level diagnostics for skipped security sink callees.
    pub(crate) security_unresolved_callee_sites: Vec<SkippedSecurityCalleeSite>,
    /// Local bindings tied to the member-access path they were sourced from
    /// (e.g. `const id = req.query.id`). Feeds the security `tainted_sink`
    /// source-to-sink association in the analyze layer.
    pub(crate) tainted_bindings: Vec<TaintedBinding>,
    /// Chain-hop depth per recorded tainted binding, aligned 1:1 with
    /// `tainted_bindings` (index `i` describes `tainted_bindings[i]`, so the
    /// depth is tracked per `(local, source_path)` pair, never approximated
    /// across a local's candidate paths). Hop 1 is a direct capture (source
    /// read, framework param, helper return, destructure-from-source); each
    /// #1146 chain step through another local binding adds 1, capped by
    /// `MAX_TAINT_BINDING_HOPS`. Working state only: NOT persisted in the
    /// extract cache and NOT carried across SFC script blocks (`merge_into`
    /// drops it together with this extractor's binding lookup, so a
    /// cross-block chain cannot form and the hop accounting cannot drift from
    /// the bindings it describes).
    pub(crate) tainted_binding_hops: Vec<u8>,
    /// Direct sink arguments recognized as sanitizer calls.
    pub(crate) sanitized_sink_args: Vec<SanitizedSinkArg>,
    /// Defensive control call sites for security surface output.
    pub(crate) security_control_sites: Vec<SecurityControlSite>,
    /// Statically flattenable callee paths, deduped per unique path (first
    /// occurrence wins). Consumed by the `boundaries.calls.forbidden`
    /// detector.
    pub(crate) callee_uses: Vec<CalleeUse>,
    /// Dedup guard for `callee_uses`. Working state only: not persisted and
    /// not merged across SFC script blocks (each block dedups independently;
    /// the detector matches per unique path, so cross-block duplicates only
    /// cost one extra entry).
    pub(crate) seen_callee_paths: FxHashSet<String>,
    /// `"use client"` / `"use server"` directive strings written as expression
    /// statements in `program.body` (misplaced, NOT in the leading
    /// prologue), so the RSC bundler silently ignores them. Captured by
    /// `visit_program` and consumed by the `misplaced-directive` detector.
    pub(crate) misplaced_directives: Vec<MisplacedDirectiveSite>,
    /// Vue `provide`/`inject` and Svelte `setContext`/`getContext` call sites
    /// keyed by a stable identifier symbol. Consumed by the `unprovided-inject`
    /// detector.
    pub(crate) di_key_sites: Vec<DiKeySite>,
    /// `true` when a `provide`/`setContext` keyed by an unknowable key (a
    /// non-identifier, a spread, or a transient nested-scope local) was seen.
    /// Forces the `unprovided-inject` detector to abstain project-wide.
    pub(crate) has_dynamic_provide: bool,
    /// Module-scope `const NAME = "literal"` names: a DI key bound to a string
    /// literal has STRING identity (a provider supplying the literal, often
    /// inside a package, matches it), so its `di_key_sites` are dropped at
    /// finalize. Working state, not persisted.
    pub(crate) string_keyed_di_consts: FxHashSet<String>,
    /// Module-scope default, namespace, or require bindings imported from
    /// DOMPurify-compatible packages.
    pub(crate) dompurify_bindings: FxHashSet<String>,
    /// Module-scope local helpers whose return value is a proven sanitizer
    /// output for a narrow sink domain.
    pub(crate) module_sanitizer_helpers: FxHashMap<String, SanitizerScope>,
    /// Module-scope local sanitizer bindings. `None` means the name is declared
    /// but not sanitizer-backed, shadowing any outer match.
    pub(crate) module_sanitizer_bindings: FxHashMap<String, Option<SanitizerScope>>,
    /// Nested lexical sanitizer binding stack for functions and blocks.
    pub(crate) sanitizer_binding_stack: Vec<FxHashMap<String, Option<SanitizerScope>>>,
    /// Module-scope literal-backed string allowlists. `false` means the name
    /// shadows an outer allowlist but is not trusted itself.
    pub(crate) module_literal_allowlist_bindings: FxHashMap<String, bool>,
    /// Module-scope literal constants that can be propagated into security sink
    /// argument classification.
    pub(crate) module_static_sink_literals: FxHashMap<String, SinkLiteralValue>,
    /// Nested lexical literal allowlist bindings.
    pub(crate) literal_allowlist_binding_stack: Vec<FxHashMap<String, bool>>,
    /// Module-scope locals initialized from risky literal regex patterns.
    /// `None` means the name shadows an outer risky regex but is not itself risky.
    pub(crate) module_risky_regex_bindings: FxHashMap<String, Option<String>>,
    /// Nested lexical risky regex bindings.
    pub(crate) risky_regex_binding_stack: Vec<FxHashMap<String, Option<String>>>,
    /// Module-scope locals initialized from a path sink call.
    pub(crate) module_path_sink_bindings: FxHashMap<String, Option<SecurityPathSinkBinding>>,
    /// Nested lexical locals initialized from a path sink call.
    pub(crate) path_sink_binding_stack: Vec<FxHashMap<String, Option<SecurityPathSinkBinding>>>,
    /// Module-scope `path.relative(base, resolved)` aliases.
    pub(crate) module_path_relative_bindings: FxHashMap<String, Option<String>>,
    /// Nested lexical `path.relative(base, resolved)` aliases.
    pub(crate) path_relative_binding_stack: Vec<FxHashMap<String, Option<String>>>,
    /// Harvested Pinia store members keyed by the store binding's local name
    /// (`export const useFoo = defineStore('foo', {...})` -> `"useFoo"` maps to
    /// its `state` / `getters` / `actions` keys, or setup-store returned keys,
    /// as `MemberKind::StoreMember`). Copied onto the matching `ExportInfo` in
    /// `enrich_store_exports` (the same side-map + finalizer shape as
    /// `enrich_local_class_exports`). Working state only; not persisted.
    store_member_decls: FxHashMap<String, Vec<MemberInfo>>,
    /// Locals bound to a store-factory call (`const s = useFooStore()`).
    /// Gates the store-consumption destructure crediting (`const { count } = s`
    /// / `storeToRefs(s)`) so it never fires on a plain `new`-instance or object
    /// binding, keeping class-member detection drift-free. Working state only.
    store_instance_locals: FxHashSet<String>,
    /// SvelteKit `load()` return-object keys harvested from a `load` export.
    /// Basename-gated to page-load producers in `parse.rs` (cleared for any
    /// non-`+page.{ts,server.ts,js,server.js}` file). Consumed by the
    /// `unused-load-data-key` detector.
    pub(crate) load_return_keys: Vec<fallow_types::extract::LoadReturnKey>,
    /// `true` when a `load` export was seen whose body could not be harvested
    /// safely (spread/non-literal/multi-return/computed-key/wrapped). Forces the
    /// `unused-load-data-key` detector to abstain on the whole file.
    pub(crate) has_unharvestable_load: bool,
    /// `true` when this file passes the whole `data` binding opaquely
    /// (`const X = data`, `fn(data)` / `fn(...data)`). Name-gated on `data`.
    /// Consumed only by the `unused-load-data-key` detector (FP-1).
    pub(crate) has_load_data_whole_use: bool,
    /// `true` when the parse is JSX-capable (`.jsx`/`.tsx`, or a `.js`/`.ts`
    /// file re-parsed through the JSX retry). Gates the React/JSX structural
    /// walk so it is a no-op on non-JSX files (perf: `audit` hot path on
    /// non-React repos must not regress). Set by `parse.rs` after construction.
    pub(crate) jsx_capable: bool,
    /// React component definitions captured during the JSX walk. Empty unless
    /// `jsx_capable`.
    pub(crate) component_functions: Vec<ComponentFunction>,
    /// React component props (reuses `ComponentProp`; `used_in_template` always
    /// false, `used_in_script` = used-in-body). Empty unless `jsx_capable`.
    pub(crate) react_props: Vec<ComponentProp>,
    /// React hook call sites. Empty unless `jsx_capable`.
    pub(crate) hook_uses: Vec<HookUse>,
    /// React render edges (child name captured; resolution deferred to graph
    /// build). Empty unless `jsx_capable`.
    pub(crate) render_edges: Vec<RenderEdge>,
    /// Stack of enclosing React component names, pushed when a component
    /// function/arrow is entered and popped on exit. The top is the
    /// `parent_component` for any render edge or hook captured inside.
    pub(crate) component_stack: Vec<String>,
    /// Pending React-component metadata for a named arrow / function-expression
    /// binding, keyed by the arrow/function span. Populated in
    /// `visit_variable_declaration` BEFORE the walk descends into the init, then
    /// consumed by `visit_arrow_function_expression` /
    /// `visit_function` to push the component stack with the binding name. Working
    /// state only (not persisted, not merged across SFC blocks).
    pub(crate) pending_component_arrows: FxHashMap<Span, PendingComponentArrow>,
}

/// Metadata for a named arrow / function-expression that may be a React
/// component, captured at the declarator before the function body is walked.
#[derive(Debug, Clone)]
pub(crate) struct PendingComponentArrow {
    /// The binding name.
    pub(crate) name: String,
    /// The component kind (`Arrow`, or a `forwardRef` / `memo` wrapper).
    pub(crate) kind: fallow_types::extract::ComponentFunctionKind,
    /// Whether the binding is exported.
    pub(crate) is_exported: bool,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct SecurityPathSinkBinding {
    pub(crate) span_start: u32,
    pub(crate) arg_index: u32,
}

impl ModuleInfoExtractor {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn record_local_class_export(
        &mut self,
        name: String,
        members: Vec<MemberInfo>,
        super_class: Option<String>,
        implemented_interfaces: Vec<String>,
        instance_bindings: Vec<(String, String)>,
    ) {
        self.local_class_exports.insert(
            name,
            LocalClassExportInfo {
                members,
                super_class,
                implemented_interfaces,
                instance_bindings,
            },
        );
    }

    pub(crate) fn binding_target_names(&self) -> &FxHashMap<String, String> {
        &self.binding_target_names
    }

    pub(crate) fn record_local_declaration_name(&mut self, name: &str) {
        self.local_declaration_names.insert(name.to_string());
    }

    pub(crate) fn remap_spans_with(&mut self, mut remap: impl FnMut(Span) -> Span) {
        for export in &mut self.exports {
            export.span = remap(export.span);
            for member in &mut export.members {
                member.span = remap(member.span);
            }
        }
        for import in &mut self.imports {
            import.span = remap(import.span);
            import.source_span = remap(import.source_span);
        }
        for re_export in &mut self.re_exports {
            re_export.span = remap(re_export.span);
        }
        for dynamic_import in &mut self.dynamic_imports {
            dynamic_import.span = remap(dynamic_import.span);
        }
        for pattern in &mut self.dynamic_import_patterns {
            pattern.span = remap(pattern.span);
        }
        for require_call in &mut self.require_calls {
            require_call.span = remap(require_call.span);
        }
        for declaration in &mut self.local_type_declarations {
            declaration.span = remap(declaration.span);
        }
        for reference in &mut self.public_signature_type_references {
            reference.span = remap(reference.span);
        }
        for reference in &mut self.local_signature_type_references {
            reference.span = remap(reference.span);
        }
        for specifier in &mut self.pending_local_export_specifiers {
            specifier.span = remap(specifier.span);
        }
        for class in self.local_class_exports.values_mut() {
            for member in &mut class.members {
                member.span = remap(member.span);
            }
        }
        self.handled_require_spans = self
            .handled_require_spans
            .iter()
            .map(|span| remap(*span))
            .collect();
        self.handled_import_spans = self
            .handled_import_spans
            .iter()
            .map(|span| remap(*span))
            .collect();
        for finding in &mut self.inline_template_findings {
            finding.decorator_start =
                remap(Span::new(finding.decorator_start, finding.decorator_start)).start;
        }
        for sink in &mut self.security_sinks {
            let span = remap(Span::new(sink.span_start, sink.span_end));
            sink.span_start = span.start;
            sink.span_end = span.end;
        }
        for skipped in &mut self.security_unresolved_callee_sites {
            let span = remap(Span::new(skipped.span_start, skipped.span_end));
            skipped.span_start = span.start;
            skipped.span_end = span.end;
        }
        for arg in &mut self.sanitized_sink_args {
            arg.span_start = remap(Span::new(arg.span_start, arg.span_start)).start;
        }
        for control in &mut self.security_control_sites {
            let span = remap(Span::new(control.span_start, control.span_end));
            control.span_start = span.start;
            control.span_end = span.end;
        }
    }

    pub(crate) fn resolve_pending_local_export_specifiers(&mut self) {
        let pending = std::mem::take(&mut self.pending_local_export_specifiers);
        for spec in pending {
            let matching_import = if self.local_declaration_names.contains(&spec.local_name) {
                None
            } else {
                self.imports.iter().find(|import| {
                    import.local_name == spec.local_name
                        && matches!(
                            import.imported_name,
                            ImportedName::Named(_) | ImportedName::Default
                        )
                })
            };

            if let Some(import) = matching_import {
                let imported_name = match &import.imported_name {
                    ImportedName::Named(name) => name.clone(),
                    ImportedName::Default => "default".to_string(),
                    ImportedName::Namespace | ImportedName::SideEffect => {
                        unreachable!("filtered by matches! guard above")
                    }
                };
                self.re_exports.push(ReExportInfo {
                    source: import.source.clone(),
                    imported_name,
                    exported_name: spec.exported_name,
                    is_type_only: spec.is_type_only || import.is_type_only,
                    span: spec.span,
                });
            } else {
                self.exports.push(ExportInfo {
                    name: ExportName::Named(spec.exported_name),
                    local_name: Some(spec.local_name),
                    is_type_only: spec.is_type_only,
                    visibility: VisibilityTag::None,
                    span: spec.span,
                    members: vec![],
                    is_side_effect_used: false,
                    super_class: None,
                });
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

    pub(crate) fn is_node_module_register(&self, local_name: &str, via_namespace: bool) -> bool {
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
        for candidate in &self.lit_custom_element_candidates {
            if !self.is_lit_custom_element_decorator(&candidate.decorator) {
                continue;
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

        self.side_effect_registered_class_names.extend(class_names);
        for index in anonymous_default_indices {
            if let Some(export) = self.exports.get_mut(index) {
                export.is_side_effect_used = true;
            }
        }
    }

    fn record_lit_custom_element_candidate(
        &mut self,
        decorator: LitCustomElementDecorator,
        target: SideEffectRegistrationTarget,
    ) {
        self.lit_custom_element_candidates
            .push(LitCustomElementCandidate { decorator, target });
    }

    fn apply_side_effect_registrations(&mut self) {
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

    fn enrich_local_class_exports(&mut self) {
        if self.local_class_exports.is_empty() {
            return;
        }

        for export in &mut self.exports {
            let Some(local_name) = export.local_name.as_deref() else {
                continue;
            };
            let Some(local_class) = self.local_class_exports.get(local_name) else {
                continue;
            };

            if export.members.is_empty() {
                export.members = local_class.members.clone();
            }
            if export.super_class.is_none() {
                export.super_class = local_class.super_class.clone();
            }

            let export_name = export.name.to_string();
            let already_has_heritage = self
                .class_heritage
                .iter()
                .any(|heritage| heritage.export_name == export_name);
            if !already_has_heritage
                && (local_class.super_class.is_some()
                    || !local_class.implemented_interfaces.is_empty()
                    || !local_class.instance_bindings.is_empty())
            {
                self.class_heritage.push(ClassHeritageInfo {
                    export_name,
                    super_class: local_class.super_class.clone(),
                    implements: local_class.implemented_interfaces.clone(),
                    instance_bindings: local_class.instance_bindings.clone(),
                });
            }
        }
    }

    fn record_exported_instance_bindings(&mut self) {
        if self.binding_target_names.is_empty() {
            return;
        }

        let additional_accesses: Vec<MemberAccess> = self
            .exports
            .iter()
            .filter_map(|export| {
                let local_name = export.local_name.as_deref()?;
                let target_name = self.binding_target_names.get(local_name)?;
                Some(MemberAccess {
                    object: format!("{}{}", crate::INSTANCE_EXPORT_SENTINEL, export.name),
                    member: target_name.clone(),
                })
            })
            .collect();

        self.member_accesses.extend(additional_accesses);
    }

    fn map_local_signature_refs_to_exports(&mut self) {
        if self.local_signature_type_references.is_empty() {
            return;
        }

        for export in &self.exports {
            let export_name = export.name.to_string();
            let Some(local_name) = export.local_name.as_deref().or(Some(export_name.as_str()))
            else {
                continue;
            };
            self.public_signature_type_references.extend(
                self.local_signature_type_references
                    .iter()
                    .filter(|reference| reference.owner_name == local_name)
                    .map(|reference| PublicSignatureTypeReference {
                        export_name: export_name.clone(),
                        type_name: reference.type_name.clone(),
                        span: reference.span,
                    }),
            );
        }
    }

    fn resolve_playwright_factory_call_definitions(&mut self) {
        let pending_calls = std::mem::take(&mut self.pending_playwright_factory_calls);
        let pending_aliases = std::mem::take(&mut self.pending_playwright_factory_aliases);
        if pending_calls.is_empty() && pending_aliases.is_empty() {
            return;
        }

        let mut factory_bindings: FxHashMap<String, Vec<(String, String)>> = FxHashMap::default();
        for entry in pending_calls {
            let base_local_resolves = self.imports.iter().any(|import| {
                import.source == "@playwright/test"
                    && import.local_name == entry.base_name
                    && matches!(
                        &import.imported_name,
                        ImportedName::Named(name) if name == "test"
                    )
            });
            if !base_local_resolves {
                continue;
            }
            factory_bindings
                .entry(entry.test_name)
                .or_default()
                .extend(entry.type_bindings);
        }
        for bindings in factory_bindings.values_mut() {
            bindings.sort();
            bindings.dedup();
        }

        let max_iters = pending_aliases.len() + 1;
        for _ in 0..max_iters {
            let mut changed = false;
            for (caller, callee) in &pending_aliases {
                if factory_bindings.contains_key(caller) {
                    continue;
                }
                if let Some(bindings) = factory_bindings.get(callee).cloned() {
                    factory_bindings.insert(caller.clone(), bindings);
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }

        for (test_name, bindings) in factory_bindings {
            for (fixture_name, type_name) in bindings {
                self.member_accesses.push(MemberAccess {
                    object: format!(
                        "{}{}:{}",
                        crate::PLAYWRIGHT_FIXTURE_DEF_SENTINEL,
                        test_name,
                        fixture_name,
                    ),
                    member: type_name,
                });
            }
        }
    }

    fn resolve_factory_call_candidates(&mut self) {
        if self.factory_call_candidates.is_empty() {
            return;
        }
        let candidates = std::mem::take(&mut self.factory_call_candidates);
        for candidate in candidates {
            let FactoryCallCandidate {
                local_name,
                callee_object,
                callee_method,
            } = candidate;

            if self.binding_target_names.contains_key(&local_name) {
                continue;
            }

            if let Some(local_class) = self.local_class_exports.get(&callee_object)
                && local_class.members.iter().any(|m| {
                    m.is_instance_returning_static
                        && m.kind == MemberKind::ClassMethod
                        && m.name == callee_method
                })
            {
                self.binding_target_names.insert(local_name, callee_object);
                continue;
            }

            let has_import = self
                .imports
                .iter()
                .any(|import| import.local_name == callee_object);
            if has_import {
                let sentinel = format!(
                    "{}{callee_object}:{callee_method}",
                    crate::FACTORY_CALL_SENTINEL,
                );
                self.binding_target_names.insert(local_name, sentinel);
            }
        }
    }

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
            self.binding_target_names
                .entry(local)
                .or_insert_with(|| class_name.clone());
        }
    }

    fn resolve_bound_object_name(&self, object: &str) -> Option<String> {
        if let Some(target_name) = self.binding_target_names.get(object) {
            return Some(target_name.clone());
        }

        self.binding_target_names
            .iter()
            .filter_map(|(binding, target_name)| {
                let suffix = object.strip_prefix(binding.as_str())?.strip_prefix('.')?;
                if target_name.starts_with(crate::FACTORY_CALL_SENTINEL) {
                    return None;
                }
                Some((binding.len(), format!("{target_name}.{suffix}")))
            })
            .max_by_key(|(len, _)| *len)
            .map(|(_, object_name)| object_name)
    }

    fn resolve_bound_member_accesses(&mut self) {
        if self.binding_target_names.is_empty() {
            return;
        }
        let additional_accesses: Vec<MemberAccess> = self
            .member_accesses
            .iter()
            .filter_map(|access| {
                self.resolve_bound_object_name(&access.object)
                    .map(|object| MemberAccess {
                        object,
                        member: access.member.clone(),
                    })
            })
            .collect();
        let additional_whole: Vec<String> = self
            .whole_object_uses
            .iter()
            .filter_map(|name| self.resolve_bound_object_name(name))
            .collect();
        self.member_accesses.extend(additional_accesses);
        self.whole_object_uses.extend(additional_whole);
    }

    fn resolve_structural_class_calls(&mut self) {
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
                if class_name == param_use.type_name {
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
                .filter(|target| !target.starts_with(crate::FACTORY_CALL_SENTINEL))
                .cloned(),
        }
    }

    fn resolve_object_binding_candidates(&mut self) {
        if self.object_binding_candidates.is_empty() {
            return;
        }

        let candidates = self.object_binding_candidates.clone();
        let max_iterations = candidates.len().saturating_add(1);
        for _ in 0..max_iterations {
            let mut changed = false;
            for candidate in &candidates {
                changed |= self.resolve_object_binding_candidate(candidate);
            }
            if !changed {
                break;
            }
        }
    }

    fn collect_namespace_object_aliases(&self) -> Vec<fallow_types::extract::NamespaceObjectAlias> {
        if self.binding_target_names.is_empty() || self.namespace_binding_names.is_empty() {
            return Vec::new();
        }
        let mut aliases = Vec::new();
        for (binding_path, target_name) in &self.binding_target_names {
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
                    namespace_local: target_name.clone(),
                });
            }
        }
        aliases
    }

    fn push_type_export(&mut self, name: &str, span: Span) {
        self.exports.push(ExportInfo {
            name: ExportName::Named(name.to_string()),
            local_name: Some(name.to_string()),
            is_type_only: true,
            visibility: VisibilityTag::None,
            span,
            members: vec![],
            is_side_effect_used: false,
            super_class: None,
        });
    }

    pub(crate) fn into_module_info(
        mut self,
        file_id: fallow_types::discover::FileId,
        content_hash: u64,
        parsed: ParsedSuppressions,
    ) -> ModuleInfo {
        let ParsedSuppressions {
            suppressions,
            unknown_kinds,
        } = parsed;
        self.resolve_typed_destructure_bindings();
        self.resolve_pending_local_export_specifiers();
        self.enrich_local_class_exports();
        self.enrich_store_exports();
        self.finalize_di_key_sites();
        self.record_exported_instance_bindings();
        self.resolve_object_binding_candidates();
        self.resolve_factory_call_candidates();
        self.resolve_playwright_factory_call_definitions();
        self.resolve_structural_class_calls();
        self.resolve_bound_member_accesses();
        self.map_local_signature_refs_to_exports();
        self.apply_side_effect_registrations();
        let namespace_object_aliases = self.collect_namespace_object_aliases();
        ModuleInfo {
            file_id,
            exports: self.exports,
            imports: self.imports,
            re_exports: self.re_exports,
            dynamic_imports: self.dynamic_imports,
            dynamic_import_patterns: self.dynamic_import_patterns,
            require_calls: self.require_calls,
            package_path_references: self.package_path_references,
            member_accesses: self.member_accesses,
            whole_object_uses: self.whole_object_uses,
            has_cjs_exports: self.has_cjs_exports,
            has_angular_component_template_url: self.has_angular_component_template_url,
            content_hash,
            suppressions,
            unknown_suppression_kinds: unknown_kinds,
            unused_import_bindings: Vec::new(),
            type_referenced_import_bindings: Vec::new(),
            value_referenced_import_bindings: Vec::new(),
            line_offsets: Vec::new(),
            complexity: Vec::new(),
            flag_uses: Vec::new(),
            class_heritage: self.class_heritage,
            injection_tokens: self.injection_tokens,
            local_type_declarations: self.local_type_declarations,
            public_signature_type_references: self.public_signature_type_references,
            namespace_object_aliases,
            iconify_prefixes: Vec::new(),
            iconify_icon_names: Vec::new(),
            auto_import_candidates: Vec::new(),
            directives: self.directives,
            client_only_dynamic_import_spans: self.client_only_dynamic_import_spans,
            security_sinks: self.security_sinks,
            security_sinks_skipped: self.security_sinks_skipped,
            security_unresolved_callee_sites: self.security_unresolved_callee_sites,
            tainted_bindings: self.tainted_bindings,
            sanitized_sink_args: self.sanitized_sink_args,
            security_control_sites: self.security_control_sites,
            callee_uses: self.callee_uses,
            misplaced_directives: self.misplaced_directives,
            di_key_sites: self.di_key_sites,
            has_dynamic_provide: self.has_dynamic_provide,
            // Populated in `release_resolution_payload`; empty at construction.
            referenced_import_bindings: Vec::new(),
            component_props: Vec::new(),
            has_props_attrs_fallthrough: false,
            has_define_expose: false,
            has_define_model: false,
            has_unharvestable_props: false,
            component_emits: Vec::new(),
            has_unharvestable_emits: false,
            has_dynamic_emit: false,
            has_emit_whole_object_use: false,
            load_return_keys: self.load_return_keys,
            has_unharvestable_load: self.has_unharvestable_load,
            has_load_data_whole_use: self.has_load_data_whole_use,
            // Derived in `release_resolution_payload` from `whole_object_uses`.
            has_page_data_store_whole_use: false,
            component_functions: self.component_functions,
            react_props: self.react_props,
            hook_uses: self.hook_uses,
            render_edges: self.render_edges,
        }
    }

    pub(crate) fn merge_into(mut self, info: &mut ModuleInfo) {
        debug_assert!(
            self.inline_template_findings.is_empty(),
            "merge_into is the SFC-script path and SFC scripts cannot host \
             Angular @Component decorators; if a future caller routes \
             Angular content here, plumb inline_template_findings into the \
             merge step before relying on this assertion"
        );
        self.resolve_typed_destructure_bindings();
        self.resolve_pending_local_export_specifiers();
        self.enrich_local_class_exports();
        self.enrich_store_exports();
        self.finalize_di_key_sites();
        self.record_exported_instance_bindings();
        self.resolve_object_binding_candidates();
        self.resolve_factory_call_candidates();
        self.resolve_playwright_factory_call_definitions();
        self.resolve_structural_class_calls();
        self.resolve_bound_member_accesses();
        self.map_local_signature_refs_to_exports();
        self.apply_side_effect_registrations();
        let namespace_object_aliases = self.collect_namespace_object_aliases();
        info.imports.extend(self.imports);
        info.exports.extend(self.exports);
        info.re_exports.extend(self.re_exports);
        info.dynamic_imports.extend(self.dynamic_imports);
        info.dynamic_import_patterns
            .extend(self.dynamic_import_patterns);
        info.require_calls.extend(self.require_calls);
        info.package_path_references
            .extend(self.package_path_references);
        info.member_accesses.extend(self.member_accesses);
        info.whole_object_uses.extend(self.whole_object_uses);
        info.has_cjs_exports |= self.has_cjs_exports;
        info.has_angular_component_template_url |= self.has_angular_component_template_url;
        info.class_heritage.extend(self.class_heritage);
        info.injection_tokens.extend(self.injection_tokens);
        info.local_type_declarations
            .extend(self.local_type_declarations);
        info.public_signature_type_references
            .extend(self.public_signature_type_references);
        info.namespace_object_aliases
            .extend(namespace_object_aliases);
        info.directives.extend(self.directives);
        info.client_only_dynamic_import_spans
            .extend(self.client_only_dynamic_import_spans);
        info.security_sinks.extend(self.security_sinks);
        info.security_sinks_skipped += self.security_sinks_skipped;
        info.security_unresolved_callee_sites
            .extend(self.security_unresolved_callee_sites);
        info.tainted_bindings.extend(self.tainted_bindings);
        info.sanitized_sink_args.extend(self.sanitized_sink_args);
        info.security_control_sites
            .extend(self.security_control_sites);
        info.callee_uses.extend(self.callee_uses);
        info.misplaced_directives.extend(self.misplaced_directives);
        info.di_key_sites.extend(self.di_key_sites);
        info.has_dynamic_provide = info.has_dynamic_provide || self.has_dynamic_provide;
        info.load_return_keys.extend(self.load_return_keys);
        info.has_unharvestable_load = info.has_unharvestable_load || self.has_unharvestable_load;
        info.has_load_data_whole_use = info.has_load_data_whole_use || self.has_load_data_whole_use;
    }
}

pub(super) fn extract_destructured_names(obj_pat: &ObjectPattern<'_>) -> Vec<String> {
    if obj_pat.rest.is_some() {
        return Vec::new();
    }
    obj_pat
        .properties
        .iter()
        .filter_map(|prop| prop.key.static_name().map(|n| n.to_string()))
        .collect()
}

fn try_extract_require<'a, 'b>(
    init: &'b Expression<'a>,
) -> Option<(&'b CallExpression<'a>, &'b str)> {
    let Expression::CallExpression(call) = init else {
        return None;
    };
    let Expression::Identifier(callee) = &call.callee else {
        return None;
    };
    if callee.name != "require" {
        return None;
    }
    let Some(Argument::StringLiteral(lit)) = call.arguments.first() else {
        return None;
    };
    Some((call, &lit.value))
}

fn try_extract_dynamic_import<'a, 'b>(
    init: &'b Expression<'a>,
) -> Option<(&'b ImportExpression<'a>, &'b str)> {
    let import_expr = extract_import_expression(init)?;
    let Expression::StringLiteral(lit) = &import_expr.source else {
        return None;
    };
    Some((import_expr, &lit.value))
}

fn try_extract_property_callback_import<'a, 'b>(
    prop: &'b ObjectProperty<'a>,
) -> Option<(&'b ImportExpression<'a>, &'b str)> {
    let property_name = prop.key.static_name()?;
    if !matches!(
        property_name.as_ref(),
        "component" | "loadChildren" | "loadComponent"
    ) {
        return None;
    }

    let import_expr = extract_import_from_callable(&prop.value)?;
    let Expression::StringLiteral(lit) = &import_expr.source else {
        return None;
    };
    Some((import_expr, &lit.value))
}

#[must_use]
/// Recursively unwrap an expression until it reaches an import expression.
pub fn extract_import_expression<'a, 'b>(
    expr: &'b Expression<'a>,
) -> Option<&'b ImportExpression<'a>> {
    match expr {
        Expression::AwaitExpression(await_expr) => extract_import_expression(&await_expr.argument),
        Expression::ImportExpression(imp) => Some(imp),
        Expression::ParenthesizedExpression(paren) => extract_import_expression(&paren.expression),
        _ => None,
    }
}

fn try_extract_arrow_wrapped_import<'a, 'b>(
    arguments: &'b [Argument<'a>],
) -> Option<(&'b ImportExpression<'a>, &'b str)> {
    for arg in arguments {
        let Some(expr) = arg.as_expression() else {
            continue;
        };
        let Some(import_expr) = extract_import_from_callable(expr) else {
            continue;
        };
        let Expression::StringLiteral(lit) = &import_expr.source else {
            continue;
        };
        return Some((import_expr, &lit.value));
    }
    None
}

#[must_use]
/// Extract an import expression from a return statement body.
pub fn extract_import_from_return_body<'a, 'b>(
    stmts: &'b [Statement<'a>],
) -> Option<&'b ImportExpression<'a>> {
    for stmt in stmts.iter().rev() {
        if let Statement::ReturnStatement(ret) = stmt
            && let Some(argument) = &ret.argument
            && let Some(imp) = extract_import_expression(argument)
        {
            return Some(imp);
        }
    }
    None
}

#[must_use]
/// Extract an import expression from a callable expression body.
pub fn extract_import_from_callable<'a, 'b>(
    expr: &'b Expression<'a>,
) -> Option<&'b ImportExpression<'a>> {
    match expr {
        Expression::ArrowFunctionExpression(arrow) => {
            if arrow.expression {
                let Statement::ExpressionStatement(expr_stmt) = arrow.body.statements.first()?
                else {
                    return None;
                };
                extract_import_expression(&expr_stmt.expression)
            } else {
                extract_import_from_return_body(&arrow.body.statements)
            }
        }
        Expression::FunctionExpression(func) => {
            let body = func.body.as_ref()?;
            extract_import_from_return_body(&body.statements)
        }
        _ => None,
    }
}

struct ImportThenCallback {
    source: String,
    import_span: oxc_span::Span,
    destructured_names: Vec<String>,
    local_name: Option<String>,
}

fn try_extract_import_then_callback(expr: &CallExpression<'_>) -> Option<ImportThenCallback> {
    let Expression::StaticMemberExpression(member) = &expr.callee else {
        return None;
    };
    if member.property.name != "then" {
        return None;
    }

    let Expression::ImportExpression(import_expr) = &member.object else {
        return None;
    };
    let Expression::StringLiteral(lit) = &import_expr.source else {
        return None;
    };
    let source = lit.value.to_string();
    let import_span = import_expr.span;

    let first_arg = expr.arguments.first()?;

    match first_arg {
        Argument::ArrowFunctionExpression(arrow) => {
            let param = arrow.params.items.first()?;
            match &param.pattern {
                BindingPattern::ObjectPattern(obj_pat) => Some(ImportThenCallback {
                    source,
                    import_span,
                    destructured_names: extract_destructured_names(obj_pat),
                    local_name: None,
                }),
                BindingPattern::BindingIdentifier(id) => {
                    let param_name = id.name.to_string();

                    if arrow.expression
                        && let Some(Statement::ExpressionStatement(expr_stmt)) =
                            arrow.body.statements.first()
                        && let Some(names) =
                            extract_member_names_from_expr(&expr_stmt.expression, &param_name)
                    {
                        return Some(ImportThenCallback {
                            source,
                            import_span,
                            destructured_names: names,
                            local_name: None,
                        });
                    }

                    Some(ImportThenCallback {
                        source,
                        import_span,
                        destructured_names: Vec::new(),
                        local_name: Some(param_name),
                    })
                }
                _ => None,
            }
        }
        Argument::FunctionExpression(func) => {
            let param = func.params.items.first()?;
            match &param.pattern {
                BindingPattern::ObjectPattern(obj_pat) => Some(ImportThenCallback {
                    source,
                    import_span,
                    destructured_names: extract_destructured_names(obj_pat),
                    local_name: None,
                }),
                BindingPattern::BindingIdentifier(id) => Some(ImportThenCallback {
                    source,
                    import_span,
                    destructured_names: Vec::new(),
                    local_name: Some(id.name.to_string()),
                }),
                _ => None,
            }
        }
        _ => None,
    }
}

fn extract_member_names_from_expr(expr: &Expression<'_>, param_name: &str) -> Option<Vec<String>> {
    match expr {
        Expression::StaticMemberExpression(member) => {
            if let Expression::Identifier(obj) = &member.object
                && obj.name == param_name
            {
                Some(vec![member.property.name.to_string()])
            } else {
                None
            }
        }
        Expression::ObjectExpression(obj) => extract_member_names_from_object(obj, param_name),
        Expression::ParenthesizedExpression(paren) => {
            extract_member_names_from_expr(&paren.expression, param_name)
        }
        _ => None,
    }
}

fn extract_member_names_from_object(
    obj: &oxc_ast::ast::ObjectExpression<'_>,
    param_name: &str,
) -> Option<Vec<String>> {
    let mut names = Vec::new();
    for prop in &obj.properties {
        if let ObjectPropertyKind::ObjectProperty(p) = prop
            && let Expression::StaticMemberExpression(member) = &p.value
            && let Expression::Identifier(obj) = &member.object
            && obj.name == param_name
        {
            names.push(member.property.name.to_string());
        }
    }
    if names.is_empty() { None } else { Some(names) }
}

#[cfg(all(test, not(miri)))]
mod tests;
