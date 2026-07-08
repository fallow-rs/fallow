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
    AngularComponentFieldArrayTypeFact, AngularTemplateMemberAccessFact, AngularThisSpreadFact,
    DynamicCustomElementRenderFact, DynamicImportInfo, DynamicImportPattern, ExportInfo,
    ExportName, FactoryCallMemberAccessFact, FactoryFnMemberAccessFact,
    FluentChainMemberAccessFact, FluentChainNewMemberAccessFact, ImportInfo, ImportedName,
    InstanceExportBindingFact, MemberAccess, MemberInfo, MemberKind, ModuleInfo,
    PlaywrightFixtureAliasFact, PlaywrightFixtureDefinitionFact, PlaywrightFixtureTypeFact,
    PlaywrightFixtureUseFact, ReExportInfo, RequireCallInfo, SemanticFact, TypeMemberTypeEntry,
    TypedPropertyMemberAccessFact, VisibilityTag,
};
use fallow_types::extract::{
    AngularComponentSelector, AngularInputMember, AngularOutputMember, CalleeUse,
    ClassHeritageInfo, ComponentFunction, ComponentProp, DiKeySite, DispatchedEvent, HookUse,
    LocalTypeDeclaration, MisplacedDirectiveSite, PublicSignatureTypeReference, RenderEdge,
    SanitizedSinkArg, SanitizerScope, SecurityControlSite, SinkLiteralValue, SinkSite,
    SkippedSecurityCalleeSite, TaintedBinding,
};
use helpers::LitCustomElementDecorator;
use helpers::array_element_type_from_type;

pub(crate) const ROUTE_LOADER_DATA_OBJECT: &str = "$fallow.routeLoaderData";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum RouteLoadHarvestMode {
    #[default]
    None,
    SvelteKitPage,
    ConventionalRoute,
}

/// Infer the element class of a Vue `defineProps` field whose declared type is an
/// array (or nullable array) of a non-builtin class (`items: Util[]` /
/// `Array<Util>` / `readonly Util[]` / `Util[] | null`). Thin crate-visible
/// wrapper over the visitor helper so the SFC props harvest reuses the same
/// inference the `v-for` binding fix uses, keyed by the prop field's `TSType`.
/// Returns a non-builtin class name only; `number[]` / `Map[]` / non-array field
/// types yield `None` (over-credit only, issue #1711).
pub(crate) fn infer_props_field_array_element_type(
    field_type: &oxc_ast::ast::TSType<'_>,
) -> Option<String> {
    array_element_type_from_type(field_type)
}

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

/// `const local = useApi()` where `useApi` is a same-file function whose body
/// returns `new Class()`. Resolved against `factory_return_functions` at finalize
/// time so `local.member` credits the constructed class. See issue #1441.
#[derive(Debug, Clone)]
pub(crate) struct FactoryReturnCandidate {
    pub(crate) local_name: String,
    pub(crate) callee_name: String,
}

/// The classified right-hand side of a module-local assignment, used to build a
/// VALUE proof that an aliased factory's returned local really holds a class
/// instance, not merely a type annotation. See issue #1441.
#[derive(Debug, Clone)]
pub(crate) enum FactoryAssignedValue {
    /// `id = new Class()`, directly a class instance.
    NewClass(String),
    /// `id = callee(...)`, a class instance only if `callee` is a strict
    /// same-file factory (resolved at finalize).
    Call(String),
    /// Anything else (a literal, a mock, `as any`, …), poisons the proof.
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BindingTarget {
    Class(String),
    FactoryCall {
        callee_object: String,
        callee_method: String,
    },
}

impl BindingTarget {
    pub(crate) fn class_name(&self) -> Option<&str> {
        match self {
            Self::Class(name) => Some(name),
            Self::FactoryCall { .. } => None,
        }
    }

    fn class_with_suffix(&self, suffix: &str) -> Option<String> {
        self.class_name()
            .map(|class_name| format!("{class_name}.{suffix}"))
    }
}

/// Outcome of expanding a compound binding target (`Opts.c`) through the
/// file's named-type property maps. See issue #1785.
#[derive(Debug, Clone, PartialEq, Eq)]
enum TypedPropertyExpansion {
    /// Every hop resolved locally; the terminal name is an ordinary
    /// local / imported identifier the analyze layer can resolve.
    Resolved(String),
    /// A hop's type name is not declared in this file (imported); the
    /// remainder must be joined cross-module at analyze time.
    CrossModule {
        type_name: String,
        property_path: String,
    },
    /// Not an interface/alias hop (e.g. a local-class compound handled by
    /// `instance_bindings`); leave the compound access untouched.
    Opaque,
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
    /// The `@customElement('x-foo')` tag literal, if statically recoverable.
    /// `None` for a computed / non-literal tag argument (which the Lit
    /// `unrendered-component` arm cannot key on, so no registration is recorded).
    tag: Option<String>,
    /// Start byte offset of the decorated class (anchors the finding).
    span_start: u32,
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
    pub(crate) semantic_facts: Vec<SemanticFact>,
    pub(crate) whole_object_uses: Vec<String>,
    pub(crate) has_cjs_exports: bool,
    pub(crate) has_angular_component_template_url: bool,
    handled_require_spans: FxHashSet<Span>,
    handled_import_spans: FxHashSet<Span>,
    namespace_binding_names: Vec<String>,
    binding_target_names: FxHashMap<String, BindingTarget>,
    interface_property_types: FxHashMap<String, FxHashMap<String, String>>,
    pending_typed_destructures: Vec<(String, String, String)>,
    iterable_element_types: FxHashMap<String, String>,
    /// Module-scope value bindings whose type is an array (or Vue reactive array)
    /// of a non-builtin class, keyed by binding name -> element class name. Read
    /// by the Vue SFC template scanner to type a `v-for` loop variable to its
    /// source iterable's element class so template member accesses on the item
    /// (`{{ util.getter }}`) credit the class. Transient extractor state, not a
    /// cached `ModuleInfo` field (mirrors `binding_target_names`).
    array_binding_element_types: FxHashMap<String, String>,
    /// Block/function-scoped array element types for local iteration receivers.
    /// This lets `const utils: Util[]` inside a function type `.map((util) => …)`
    /// and `for (const util of utils)` in the same lexical scope without leaking
    /// the binding to sibling functions.
    scoped_array_binding_element_types: Vec<FxHashMap<String, String>>,
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
    /// True while walking the immediate quasi of a tagged template. Tagged
    /// templates receive raw values, so interpolation should not credit
    /// `toString` coercion for the quasi itself.
    in_tagged_template_quasi: bool,
    pub(crate) class_super_stack: Vec<Option<String>>,
    pub(crate) inline_template_findings: Vec<InlineTemplateFinding>,
    pub(crate) side_effect_registered_class_names: FxHashSet<String>,
    lit_custom_element_candidates: Vec<LitCustomElementCandidate>,
    pub(crate) registered_custom_elements: Vec<fallow_types::extract::RegisteredCustomElement>,
    pub(crate) used_custom_element_tags: FxHashSet<String>,
    pub(crate) factory_call_candidates: Vec<FactoryCallCandidate>,
    /// Same-file functions whose body returns `new Class()`, mapped to the class
    /// name, plus the `const x = fn()` bindings to resolve against them. See #1441.
    factory_return_functions: FxHashMap<String, String>,
    factory_return_candidates: Vec<FactoryReturnCandidate>,
    /// Same-file functions whose body returns a bare identifier (e.g.
    /// `useApi() { return api }`). Resolved against `binding_target_names` at
    /// finalize: a typed local (`let api: RESTApi`) promotes the function to a
    /// `factory_return_functions` entry, so `const x = useApi()` credits the
    /// class without tracing the assignment chain. See issue #1441 (var-return).
    factory_return_alias_functions: FxHashMap<String, String>,
    /// Subset of factory-return functions (by local name) whose body provably
    /// returns a SINGLE class across ALL static return paths, the all-paths
    /// unanimity proof required before a factory may be exported as cross-module
    /// metadata. Stricter than `factory_return_functions` (which keeps the
    /// same-file last-return leniency). Only entries here become
    /// `ModuleInfo.exported_factory_returns`, bounding the cross-module
    /// over-credit blast radius. See issue #1441 (Part A).
    strict_factory_return_functions: FxHashMap<String, String>,
    /// Alias factory functions (by local name) whose body is eligible for STRICT
    /// (cross-module) promotion: it returns synchronously (not async/generator)
    /// and cannot fall through to `undefined` (terminal last statement). The
    /// same-file (loose) alias promotion does not require this. See #1441 (A).
    strict_alias_eligible: FxHashSet<String>,
    /// Module-scope identifier -> classified initializer right-hand sides, from
    /// MODULE-SCOPE declarators and MODULE-SCOPE assignment expressions
    /// (`let api = new RESTApi()`, `api = init()` at top level). A module-scope
    /// write runs at load and dominates any later call, usable as PROOF and
    /// checked for poison. See #1441 (Part A).
    module_scope_initializers: FxHashMap<String, Vec<FactoryAssignedValue>>,
    /// Identifier -> classified right-hand sides of EVERY assignment expression
    /// to it, in ANY scope (sibling functions, non-dominating branches, …). Used
    /// ONLY as a POISON input for the strict alias proof: a write to the returned
    /// module binding that is `Other`/unresolved/a conflicting class (e.g.
    /// `poison() { api = {} as any }`) makes the strict export abstain, since it
    /// can leave the binding holding a non-class at return time. Declarations
    /// (which introduce a separate binding) are intentionally excluded. See #1441
    /// (Part A).
    identifier_write_values: FxHashMap<String, Vec<FactoryAssignedValue>>,
    /// Alias factory function (by local name) -> classified right-hand sides of
    /// assignments to its RETURNED identifier, collected from that function's OWN
    /// body only (not nested functions). Ties the value-proof to the alias
    /// function itself, so an assignment in an unrelated/sibling function does not
    /// falsely prove it. See #1441 (Part A).
    alias_in_body_assignments: FxHashMap<String, Vec<FactoryAssignedValue>>,
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
    /// Export LOCAL NAMES of exported functions / const-arrows whose body has an
    /// inline `"use server"` directive. Captured by `extract_declaration_exports`
    /// and consumed by the `unused-server-action` reclassifier. Only EXPORTED
    /// declarations are captured (the capture sits on the exported-declaration
    /// path), so a non-exported local function with a use-server body is never
    /// recorded; even if it were, the reclassifier only matches against
    /// unused-EXPORT names, so a stray name is inert.
    pub(crate) inline_server_action_exports: Vec<String>,
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
    /// Local type aliases of the form `type X = ReturnType<typeof useFooStore>`
    /// mapped to the store factory name (`X -> useFooStore`). Lets a param typed
    /// as the store (`(s: X)` or `(props: { store: X })`) bind to the factory so
    /// `s.member` / `props.store.member` credit the store member through the
    /// existing `binding_target_names` remap (issue #1489 Case 2). Working state
    /// only; never persisted.
    ///
    /// Resolution is source-order-dependent (recorded during the walk, the same
    /// constraint as class type-param resolution): an alias declared BELOW its
    /// first consuming param is not in the map yet when the param is processed,
    /// so that one ordering keeps the false positive. The inline
    /// `ReturnType<typeof useStore>` annotation needs no alias and is unaffected.
    type_alias_store_factory: FxHashMap<String, String>,
    /// SvelteKit `load()` return-object keys harvested from a `load` export.
    /// Basename-gated to page-load producers in `parse.rs` (cleared for any
    /// non-`+page.{ts,server.ts,js,server.js}` file). Consumed by the
    /// `unused-load-data-key` detector.
    pub(crate) load_return_keys: Vec<fallow_types::extract::LoadReturnKey>,
    /// Which route-data producer names should be harvested for this file. Set by
    /// `parse.rs` before the AST walk because SvelteKit and conventional route
    /// modules use different export names but share the same cached field.
    route_load_harvest_mode: RouteLoadHarvestMode,
    /// `true` when a `load` export was seen whose body could not be harvested
    /// safely (spread/non-literal/multi-return/computed-key/wrapped). Forces the
    /// `unused-load-data-key` detector to abstain on the whole file.
    pub(crate) has_unharvestable_load: bool,
    /// `true` when this file passes the whole `data` binding opaquely
    /// (`const X = data`, `fn(data)` / `fn(...data)`). Name-gated on `data`.
    /// Consumed only by the `unused-load-data-key` detector (FP-1).
    pub(crate) has_load_data_whole_use: bool,
    /// Locals bound to `useLoaderData()`. Reads on these locals are mirrored to
    /// the reserved route-data marker for React Router and Remix route modules.
    /// Working state only, not persisted.
    route_loader_data_bindings: FxHashSet<String>,
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
    /// Same-file object-type declarations eligible to back a React component's
    /// bare-identifier typed props param (`(props: Props) => ...`). Maps the
    /// type/interface name to its `(prop_name, span_start)` members, populated
    /// ONLY for a plain object shape: an `interface X { ... }` with no `extends`
    /// and no type parameters, or a `type X = { ... }` whose annotation is a bare
    /// object type literal with no type parameters. An `extends` / intersection /
    /// generic / mapped / imported type never enters this map, so a pending typed
    /// props harvest that misses it abstains (ADR-001, zero-FP). Working state,
    /// not persisted.
    pub(crate) react_object_type_props: FxHashMap<String, Vec<(String, u32)>>,
    /// React components whose first param is a bare identifier with a
    /// same-file-resolvable object-type annotation, deferred to finalize because
    /// the backing interface/type may hoist (be declared after the component).
    /// The `props.<name>` member-access usage is computed at capture time (the
    /// body is in hand then); the prop-name SET is resolved in finalize against
    /// `react_object_type_props`. Working state, not persisted.
    pub(crate) pending_typed_react_props: Vec<PendingTypedReactProps>,
    /// Angular component/directive inputs harvested from Angular-decorated
    /// classes (`@Input()` decorators and signal `input()` / `model()`
    /// initializers). Accumulated across every Angular class in the module and
    /// copied onto `ModuleInfo.angular_inputs`. Consumed by the
    /// `unused-component-input` detector.
    pub(crate) angular_inputs: Vec<AngularInputMember>,
    /// Angular component/directive outputs harvested from Angular-decorated
    /// classes (`@Output()` decorators and signal `output()` /
    /// `outputFromObservable()` initializers). Accumulated across every Angular
    /// class in the module and copied onto `ModuleInfo.angular_outputs`. Consumed
    /// by the `unused-component-output` detector.
    pub(crate) angular_outputs: Vec<AngularOutputMember>,
    /// Spans of Angular classes already harvested into `angular_inputs` /
    /// `angular_outputs`. An `export class FooComponent` is visited by both the
    /// named-export declaration path and the top-level class-declaration path, so
    /// this dedups the harvest and prevents one declared input/output from being
    /// flagged twice.
    pub(crate) harvested_angular_class_spans: FxHashSet<Span>,
    /// Angular `@Component` declarations with their `selector` value(s), harvested
    /// from `@Component({ selector })` decorators. Accumulated across every
    /// Angular component class in the module and copied onto
    /// `ModuleInfo.angular_component_selectors`. Consumed by the Angular arm of
    /// the `unrendered-component` detector.
    pub(crate) angular_component_selectors: Vec<AngularComponentSelector>,
    /// Custom element selector tags referenced in this file's Angular templates
    /// (inline `template:` blocks). External `templateUrl` `.html` files are
    /// scanned separately when that file is parsed. Copied onto
    /// `ModuleInfo.angular_used_selectors`.
    pub(crate) angular_used_selectors: Vec<String>,
    /// Angular component class names referenced as a route entry or bootstrap
    /// target (route `component:` / `loadComponent`, `bootstrapApplication` /
    /// `bootstrap: [...]`). Copied onto `ModuleInfo.angular_entry_component_refs`
    /// (the Angular `unrendered-component` entry-point abstain).
    pub(crate) angular_entry_component_refs: Vec<String>,
    /// `true` when a dynamic component render was seen
    /// (`*.createComponent(<ident>)`). Copied onto
    /// `ModuleInfo.has_dynamic_component_render` (the Angular
    /// `unrendered-component` project-wide abstain).
    pub(crate) has_dynamic_component_render: bool,
    /// Local binding names bound from `const dispatch = createEventDispatcher()`
    /// (where `createEventDispatcher` is imported from `svelte`). A
    /// `dispatch('<name>')` call through one of these bindings records a
    /// `DispatchedEvent`. Working state, then copied as the gate for the dispatch
    /// harvest. Not persisted.
    pub(crate) event_dispatch_bindings: FxHashSet<String>,
    /// Svelte custom events dispatched via `dispatch('<name>')` (literal arg).
    /// Copied onto `ModuleInfo.svelte_dispatched_events`. Consumed by the
    /// `unused-svelte-event` detector.
    pub(crate) svelte_dispatched_events: Vec<DispatchedEvent>,
    /// `true` when a `dispatch(<nonLiteral>)` call was seen, or a `dispatch`
    /// binding was used as a whole value (passed / returned). Forces the
    /// `unused-svelte-event` detector to abstain on the whole component.
    pub(crate) has_dynamic_dispatch: bool,
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
    /// For a `forwardRef<Ref, Props>((props, ref) => ...)` wrapper, the bare
    /// single-name SECOND generic type argument (`Props`). The inner render
    /// function's `props` param carries no annotation in this shape, so the props
    /// type lives on the wrapper call's type arguments instead. Resolved against
    /// `react_object_type_props` in finalize, exactly like an inline
    /// `(props: Props)` annotation. `None` for every non-generic / unresolvable
    /// shape (the inner param's own annotation, if any, still wins).
    pub(crate) props_type_name: Option<String>,
}

/// A React component whose first param is a bare identifier carrying a
/// same-file object-type annotation (`(props: Props) => ...`). Captured during
/// the body walk so `props.<name>` member-access usage is recorded against the
/// props local while the body is in hand; the prop-name set is resolved in
/// finalize because the backing interface/type may be declared after the
/// component (TypeScript hoists type declarations).
#[derive(Debug, Clone)]
pub(crate) struct PendingTypedReactProps {
    /// The enclosing component name (`ComponentProp.component`).
    pub(crate) component: String,
    /// The bare-identifier props parameter local (e.g. `props`).
    pub(crate) props_local: String,
    /// The annotation type name to resolve against `react_object_type_props`.
    pub(crate) type_name: String,
    /// Prop names read via `<props_local>.<name>` member access (or via a
    /// `const { name } = props` destructure local) anywhere in the body. A name
    /// in this set is credited `used_in_script = true`.
    pub(crate) member_uses: FxHashSet<String>,
    /// `true` when the props binding is consumed as a whole object (passed to a
    /// call/hook, spread, returned, or assigned). The prop set is then opaque, so
    /// the whole component abstains (`has_unharvestable_props`).
    pub(crate) has_whole_object_use: bool,
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

    pub(crate) fn set_route_load_harvest_mode(&mut self, mode: RouteLoadHarvestMode) {
        self.route_load_harvest_mode = mode;
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

    /// Run the bound-member resolution finalize and take the member accesses that
    /// resolved onto a SEEDED element class. For the Astro template-expression
    /// pass (issue #1713): a fresh extractor seeded with the frontmatter element
    /// types visits the template `{...}` expression regions,
    /// `bind_iterable_callback_parameter` types each `.map((util) => ...)` callback
    /// param to its element class during the walk, and `resolve_bound_member_accesses`
    /// re-emits the class-qualified access (`Util.getter`). This returns ONLY those
    /// class-qualified accesses (object == a seeded element-class name), dropping
    /// the raw `util.getter` / `utils.map` noise so nothing but the intended
    /// element-class credit reaches the module. Accesses are span-less name pairs,
    /// so no un-remapped template-region span ever leaks into the module.
    pub(crate) fn take_resolved_iteration_member_accesses(&mut self) -> Vec<MemberAccess> {
        self.resolve_typed_destructure_bindings();
        self.resolve_bound_member_accesses();
        let element_classes: FxHashSet<&str> = self
            .array_binding_element_types
            .values()
            .map(String::as_str)
            .collect();
        self.member_accesses
            .drain(..)
            .filter(|access| element_classes.contains(access.object.as_str()))
            .collect()
    }

    fn insert_class_binding_target(&mut self, binding: String, target: String) {
        self.binding_target_names
            .insert(binding, BindingTarget::Class(target));
    }

    fn insert_class_binding_target_if_absent(&mut self, binding: String, target: String) {
        self.binding_target_names
            .entry(binding)
            .or_insert(BindingTarget::Class(target));
    }

    pub(crate) fn record_angular_template_member_fact(&mut self, member: String) {
        self.semantic_facts
            .push(SemanticFact::AngularTemplateMemberAccess(
                AngularTemplateMemberAccessFact { member },
            ));
    }

    pub(crate) fn record_angular_component_field_array_type_fact(
        &mut self,
        field: String,
        element_class: String,
    ) {
        self.semantic_facts
            .push(SemanticFact::AngularComponentFieldArrayType(
                AngularComponentFieldArrayTypeFact {
                    field,
                    element_class,
                },
            ));
    }

    pub(crate) fn record_angular_this_spread_fact(&mut self) {
        self.semantic_facts
            .push(SemanticFact::AngularThisSpread(AngularThisSpreadFact));
    }

    pub(crate) fn record_dynamic_custom_element_render_fact(&mut self) {
        self.semantic_facts
            .push(SemanticFact::DynamicCustomElementRender(
                DynamicCustomElementRenderFact,
            ));
    }

    fn record_instance_export_binding_fact(&mut self, export_name: String, target_name: String) {
        self.semantic_facts
            .push(SemanticFact::InstanceExportBinding(
                InstanceExportBindingFact {
                    export_name,
                    target_name,
                },
            ));
    }

    fn record_factory_call_member_fact(
        &mut self,
        callee_object: String,
        callee_method: String,
        member: String,
    ) {
        self.semantic_facts
            .push(SemanticFact::FactoryCallMemberAccess(
                FactoryCallMemberAccessFact {
                    callee_object,
                    callee_method,
                    member,
                },
            ));
    }

    fn record_factory_fn_member_fact(&mut self, callee_name: String, member: String) {
        self.semantic_facts
            .push(SemanticFact::FactoryFnMemberAccess(
                FactoryFnMemberAccessFact {
                    callee_name,
                    member,
                },
            ));
    }

    fn record_typed_property_member_fact(
        &mut self,
        type_name: String,
        property_path: String,
        member: String,
    ) {
        self.semantic_facts
            .push(SemanticFact::TypedPropertyMemberAccess(
                TypedPropertyMemberAccessFact {
                    type_name,
                    property_path,
                    member,
                },
            ));
    }

    pub(crate) fn record_fluent_chain_member_fact(
        &mut self,
        root_object: String,
        root_method: String,
        chain: Vec<String>,
        member: String,
    ) {
        self.semantic_facts
            .push(SemanticFact::FluentChainMemberAccess(
                FluentChainMemberAccessFact {
                    root_object,
                    root_method,
                    chain,
                    member,
                },
            ));
    }

    pub(crate) fn record_fluent_chain_new_member_fact(
        &mut self,
        class_name: String,
        chain: Vec<String>,
        member: String,
    ) {
        self.semantic_facts
            .push(SemanticFact::FluentChainNewMemberAccess(
                FluentChainNewMemberAccessFact {
                    class_name,
                    chain,
                    member,
                },
            ));
    }

    pub(crate) fn record_playwright_fixture_use_fact(
        &mut self,
        test_name: String,
        fixture_name: String,
        member: String,
    ) {
        self.semantic_facts.push(SemanticFact::PlaywrightFixtureUse(
            PlaywrightFixtureUseFact {
                test_name,
                fixture_name,
                member,
            },
        ));
    }

    pub(crate) fn record_playwright_fixture_definition_fact(
        &mut self,
        test_name: String,
        fixture_name: String,
        type_name: String,
    ) {
        self.semantic_facts
            .push(SemanticFact::PlaywrightFixtureDefinition(
                PlaywrightFixtureDefinitionFact {
                    test_name,
                    fixture_name,
                    type_name,
                },
            ));
    }

    pub(crate) fn record_playwright_fixture_alias_fact(
        &mut self,
        test_name: String,
        base_name: String,
    ) {
        self.semantic_facts
            .push(SemanticFact::PlaywrightFixtureAlias(
                PlaywrightFixtureAliasFact {
                    test_name,
                    base_name,
                },
            ));
    }

    pub(crate) fn record_playwright_fixture_type_fact(
        &mut self,
        alias_name: String,
        fixture_name: String,
        type_name: String,
    ) {
        self.semantic_facts
            .push(SemanticFact::PlaywrightFixtureType(
                PlaywrightFixtureTypeFact {
                    alias_name,
                    fixture_name,
                    type_name,
                },
            ));
    }

    pub(crate) fn record_local_declaration_name(&mut self, name: &str) {
        self.local_declaration_names.insert(name.to_string());
    }

    pub(crate) fn remap_spans_with(&mut self, mut remap: impl FnMut(Span) -> Span) {
        self.remap_graph_spans(&mut remap);
        self.remap_type_and_signature_spans(&mut remap);
        self.remap_handled_spans(&mut remap);
        self.remap_security_spans(&mut remap);
    }

    /// Remap import/export/re-export/dynamic-import/require graph spans.
    fn remap_graph_spans(&mut self, remap: &mut impl FnMut(Span) -> Span) {
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
    }

    /// Remap type declarations, signature references, pending specifiers, and
    /// local class member spans.
    fn remap_type_and_signature_spans(&mut self, remap: &mut impl FnMut(Span) -> Span) {
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
    }

    /// Remap the deduped handled-require / handled-import span sets.
    fn remap_handled_spans(&mut self, remap: &mut impl FnMut(Span) -> Span) {
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
    }

    /// Remap inline-template, security sink, and security control site spans.
    fn remap_security_spans(&mut self, remap: &mut impl FnMut(Span) -> Span) {
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
                    expected_unused_reason: None,
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

    fn record_lit_custom_element_candidate(
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

        for export in self.exports.clone() {
            let Some(local_name) = export.local_name.as_deref() else {
                continue;
            };
            let Some(target_name) = self
                .binding_target_names
                .get(local_name)
                .and_then(BindingTarget::class_name)
            else {
                continue;
            };
            let export_name = export.name.to_string();
            self.record_instance_export_binding_fact(export_name, target_name.to_string());
        }
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
                self.record_playwright_fixture_definition_fact(
                    test_name.clone(),
                    fixture_name,
                    type_name,
                );
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
                self.insert_class_binding_target(local_name, callee_object);
                continue;
            }

            let has_import = self
                .imports
                .iter()
                .any(|import| import.local_name == callee_object);
            if has_import {
                self.binding_target_names.insert(
                    local_name,
                    BindingTarget::FactoryCall {
                        callee_object,
                        callee_method,
                    },
                );
            }
        }
    }

    /// Promote `useApi() { return api }`-style functions so `const x = useApi()`
    /// credits the class. TWO promotions with different proofs:
    ///
    /// - SAME-FILE (loose) `factory_return_functions`: the returned identifier
    ///   resolves to a class in `binding_target_names` (e.g. a typed `let api:
    ///   RESTApi`). A type annotation is acceptable here, the blast radius is one
    ///   file. This preserves the original var-return behavior.
    /// - CROSS-MODULE (strict) `strict_factory_return_functions`: ALSO requires a
    ///   VALUE proof, the returned local must be assigned `new Class()` or a
    ///   strict same-file factory (`value_prove_alias`), and the function must be
    ///   sync + non-falling-through (`strict_alias_eligible`). A type annotation
    ///   alone (`let api: RESTApi` assigned a mock) must NOT leak into
    ///   cross-module credit. See #1441 (Part A).
    fn resolve_factory_return_aliases(&mut self) {
        if self.factory_return_alias_functions.is_empty() {
            return;
        }
        let aliases = std::mem::take(&mut self.factory_return_alias_functions);
        for (fn_name, returned_id) in aliases {
            if self.factory_return_functions.contains_key(&fn_name) {
                continue;
            }
            let Some(class_name) = self
                .binding_target_names
                .get(&returned_id)
                .and_then(BindingTarget::class_name)
            else {
                continue;
            };
            let class_name = class_name.to_string();
            // Cross-module strict promotion: a sync, terminal body AND a VALUE
            // proof tied to THIS function. Done before the loose insert so a
            // type-only binding never reaches the strict map.
            if self.strict_alias_eligible.contains(&fn_name)
                && let Some(proven_class) = self.value_prove_alias(&fn_name, &returned_id)
            {
                self.strict_factory_return_functions
                    .insert(fn_name.clone(), proven_class);
            }
            self.factory_return_functions.insert(fn_name, class_name);
        }
    }

    /// VALUE-prove the class an alias factory `fn_name` returns through its
    /// returned identifier `returned_id`. The proof is tied to the function:
    /// assignments to `returned_id` inside `fn_name`'s OWN body
    /// (`alias_in_body_assignments`) plus a MODULE-SCOPE initializer of
    /// `returned_id` (`module_scope_initializers`), never an assignment in an
    /// unrelated/sibling function. `new Class()` proves directly; `factory()`
    /// proves only when `factory` is a strict same-file factory. Proven ONLY with
    /// at least one resolvable source and ALL sources agreeing on one class; any
    /// `Other`/unresolved/conflicting source abstains. So `let api: RESTApi` with
    /// no class-proven write is not proven. See #1441 (Part A).
    fn value_prove_alias(&self, fn_name: &str, returned_id: &str) -> Option<String> {
        // PROOF: dominating writes, the alias's own dominating in-body
        // assignments and module-scope initializers. Must be unanimous on one
        // class, with at least one source.
        let mut class: Option<String> = None;
        let mut saw_source = false;
        let proof_sources = self
            .alias_in_body_assignments
            .get(fn_name)
            .into_iter()
            .flatten()
            .chain(
                self.module_scope_initializers
                    .get(returned_id)
                    .into_iter()
                    .flatten(),
            );
        for value in proof_sources {
            saw_source = true;
            let resolved = self.resolve_factory_assigned_value(value)?;
            match &class {
                None => class = Some(resolved),
                Some(existing) if *existing == resolved => {}
                Some(_) => return None,
            }
        }
        let class = if saw_source { class? } else { return None };

        // POISON: ANY write to the binding (any scope, incl. sibling functions
        // and non-dominating branches) that is `Other`/unresolved or a CONFLICTING
        // class means the binding can hold a non-`class` value at return time
        // (e.g. `poison() { api = {} as any }`). Abstain. A write that resolves to
        // the same class is harmless. See #1441 (Part A).
        let poison_sources = self
            .identifier_write_values
            .get(returned_id)
            .into_iter()
            .flatten()
            .chain(
                self.module_scope_initializers
                    .get(returned_id)
                    .into_iter()
                    .flatten(),
            );
        for value in poison_sources {
            match self.resolve_factory_assigned_value(value) {
                Some(resolved) if resolved == class => {}
                _ => return None,
            }
        }
        Some(class)
    }

    /// Resolve a classified assignment value to the class it produces: a direct
    /// `new Class()`, or a `factory()` call only when the callee is a strict
    /// same-file factory. `Other` and unresolved calls yield `None`. #1441 (A).
    fn resolve_factory_assigned_value(&self, value: &FactoryAssignedValue) -> Option<String> {
        match value {
            FactoryAssignedValue::NewClass(name) => Some(name.clone()),
            FactoryAssignedValue::Call(callee) => {
                self.strict_factory_return_functions.get(callee).cloned()
            }
            FactoryAssignedValue::Other => None,
        }
    }

    /// Resolve `const x = useApi()` bindings. A same-file factory whose body
    /// returns `new Class()` binds `x` directly to the class so `x.member`
    /// credits it. An IMPORTED factory callee instead emits a typed
    /// `FactoryFnMemberAccess` fact so the analyze layer resolves the returned
    /// class across the module boundary via `exported_factory_returns`. See issue
    /// #1441 (Part A).
    fn resolve_factory_return_candidates(&mut self) {
        if self.factory_return_candidates.is_empty() {
            return;
        }
        let candidates = std::mem::take(&mut self.factory_return_candidates);
        let mut deferred_factory_facts: Vec<(String, String)> = Vec::new();
        for candidate in candidates {
            // Same-file factory returning `new Class()`: bind the local to the
            // class so `resolve_bound_member_accesses` credits `x.member` directly.
            if let Some(class_name) = self.factory_return_functions.get(&candidate.callee_name) {
                let class_name = class_name.clone();
                self.binding_target_names
                    .entry(candidate.local_name)
                    .or_insert(BindingTarget::Class(class_name));
                continue;
            }
            // Cross-module: `const x = importedFactory()`. We do NOT route through
            // `binding_target_names` here: the Pinia store-consumption heuristic
            // (`is_store_factory_call`) already weakly binds every imported-call
            // local to its bare callee name, which would shadow a fact binding.
            // Instead emit the factory-fn member facts directly for the local's
            // first-level reads. The analyze layer credits a class only when the
            // callee resolves to a proven exported factory return; for any other
            // callee (a real store, a plain helper) it is a harmless no-op.
            // See issue #1441 (Part A).
            let callee_is_imported = self
                .imports
                .iter()
                .any(|import| import.local_name == candidate.callee_name);
            if !callee_is_imported {
                continue;
            }
            for access in &self.member_accesses {
                if access.object == candidate.local_name {
                    deferred_factory_facts
                        .push((candidate.callee_name.clone(), access.member.clone()));
                }
            }
        }
        for (callee_name, member) in deferred_factory_facts {
            self.record_factory_fn_member_fact(callee_name, member);
        }
    }

    /// Build the cross-module `exported_factory_returns` metadata: join the
    /// strict (all-paths-unanimous) factory map against this module's exports, so
    /// a `const x = useApi()` consumer can credit the returned class across the
    /// boundary. The stored class name is the factory module's own LOCAL name,
    /// resolved at analyze time through this module's imports to the real class
    /// export. Only strict entries qualify, bounding over-credit. Must run after
    /// `resolve_factory_return_aliases` has populated the strict map. See issue
    /// #1441 (Part A).
    fn collect_exported_factory_returns(&self) -> Vec<fallow_types::extract::FactoryReturnExport> {
        if self.strict_factory_return_functions.is_empty() {
            return Vec::new();
        }
        let mut out = Vec::new();
        for export in &self.exports {
            if export.is_type_only {
                continue;
            }
            let local_name = match (export.local_name.as_deref(), &export.name) {
                (Some(local), _) => local,
                (None, ExportName::Named(name)) => name.as_str(),
                (None, ExportName::Default) => continue,
            };
            if let Some(class_local_name) = self.strict_factory_return_functions.get(local_name) {
                out.push(fallow_types::extract::FactoryReturnExport {
                    export_name: export.name.to_string(),
                    class_local_name: class_local_name.clone(),
                });
            }
        }
        out
    }

    /// Flatten this file's named-type property maps into the persisted
    /// `ModuleInfo.type_member_types` entries, sorted for deterministic
    /// output. Names stay local to this module; resolution is deferred to
    /// the analyze-layer join. See issue #1785.
    fn collect_type_member_types(&self) -> Vec<TypeMemberTypeEntry> {
        let mut entries: Vec<TypeMemberTypeEntry> = self
            .interface_property_types
            .iter()
            .flat_map(|(type_name, properties)| {
                properties
                    .iter()
                    .map(|(property, property_type)| TypeMemberTypeEntry {
                        type_name: type_name.clone(),
                        property: property.clone(),
                        property_type: property_type.clone(),
                    })
            })
            .collect();
        entries
            .sort_unstable_by(|a, b| (&a.type_name, &a.property).cmp(&(&b.type_name, &b.property)));
        entries
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
            self.insert_class_binding_target_if_absent(local, class_name.clone());
        }
    }

    fn resolve_bound_object_name(&self, object: &str) -> Option<BindingTarget> {
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

    fn resolve_bound_member_accesses(&mut self) {
        if self.binding_target_names.is_empty() {
            return;
        }
        let mut additional_accesses = Vec::new();
        let mut additional_facts = Vec::new();
        let mut additional_typed_property_facts = Vec::new();
        for access in &self.member_accesses {
            let Some(target) = self.resolve_bound_object_name(&access.object) else {
                continue;
            };
            match target {
                BindingTarget::Class(object) => {
                    // A compound target (`Opts.c` from `this.opts.c` with binding
                    // `this.opts -> Opts`) may hop through a named interface /
                    // type-literal alias; expand it so the terminal property type
                    // is credited (issue #1785). The compound access itself is
                    // still pushed: local-class compounds resolve downstream via
                    // `instance_bindings`, and interface compounds are inert.
                    match self.expand_typed_property_compound(&object) {
                        TypedPropertyExpansion::Resolved(terminal) => {
                            additional_accesses.push(MemberAccess {
                                object: terminal,
                                member: access.member.clone(),
                            });
                        }
                        TypedPropertyExpansion::CrossModule {
                            type_name,
                            property_path,
                        } => additional_typed_property_facts.push((
                            type_name,
                            property_path,
                            access.member.clone(),
                        )),
                        TypedPropertyExpansion::Opaque => {}
                    }
                    additional_accesses.push(MemberAccess {
                        object,
                        member: access.member.clone(),
                    });
                }
                BindingTarget::FactoryCall {
                    callee_object,
                    callee_method,
                } => additional_facts.push((callee_object, callee_method, access.member.clone())),
            }
        }
        let additional_whole: Vec<String> =
            self.whole_object_uses
                .iter()
                .filter_map(|name| self.resolve_bound_object_name(name))
                .filter_map(|target| {
                    if let BindingTarget::Class(name) = target {
                        Some(name)
                    } else {
                        None
                    }
                })
                .flat_map(|name| {
                    // Mirror the member-access expansion for whole-object uses:
                    // `use(this.opts.c)` through a local interface hop credits the
                    // terminal class wholesale. Cross-module whole-object hops are
                    // out of scope (issue #1785 covers member accesses).
                    let expanded = match self.expand_typed_property_compound(&name) {
                        TypedPropertyExpansion::Resolved(terminal) => Some(terminal),
                        TypedPropertyExpansion::CrossModule { .. }
                        | TypedPropertyExpansion::Opaque => None,
                    };
                    std::iter::once(name).chain(expanded)
                })
                .collect();
        self.member_accesses.extend(additional_accesses);
        for (callee_object, callee_method, member) in additional_facts {
            self.record_factory_call_member_fact(callee_object, callee_method, member);
        }
        for (type_name, property_path, member) in additional_typed_property_facts {
            self.record_typed_property_member_fact(type_name, property_path, member);
        }
        self.whole_object_uses.extend(additional_whole);
    }

    /// Expand a compound binding target (`Opts.c[.d...]`) through this file's
    /// named-type property maps (`interface_property_types`).
    ///
    /// Each hop consumes one path segment, so the walk terminates after at most
    /// `segments` iterations even for self-referential types. A hop through a
    /// locally-declared non-literal type (a class, an enum) returns `Opaque` at
    /// the root (the `instance_bindings` machinery owns local-class compounds);
    /// a hop that leaves the file (the type name is not declared here, i.e.
    /// imported) returns `CrossModule` so the caller can emit a
    /// `TypedPropertyMemberAccess` fact for the analyze-layer join. See issue
    /// #1785.
    fn expand_typed_property_compound(&self, compound: &str) -> TypedPropertyExpansion {
        let mut segments = compound.split('.');
        let Some(root) = segments.next() else {
            return TypedPropertyExpansion::Opaque;
        };
        let remaining: Vec<&str> = segments.collect();
        if remaining.is_empty() {
            return TypedPropertyExpansion::Opaque;
        }
        let mut current = root.to_string();
        let mut idx = 0;
        while idx < remaining.len() {
            let Some(properties) = self.interface_property_types.get(&current) else {
                if idx == 0 && self.local_declaration_names.contains(&current) {
                    // A local class / enum root: the compound access resolves
                    // through `instance_bindings` downstream; leave it alone.
                    return TypedPropertyExpansion::Opaque;
                }
                return TypedPropertyExpansion::CrossModule {
                    type_name: current,
                    property_path: remaining[idx..].join("."),
                };
            };
            let Some(next) = properties.get(remaining[idx]) else {
                // The property is not a named-reference-typed member of this
                // type (union / generic / unharvested shape): abstain.
                return TypedPropertyExpansion::Opaque;
            };
            current.clone_from(next);
            idx += 1;
        }
        TypedPropertyExpansion::Resolved(current)
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
                .and_then(BindingTarget::class_name)
                .map(str::to_string),
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

    fn push_type_export(&mut self, name: &str, span: Span) {
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
        });
    }

    /// Run every finalize/resolve pass shared by `into_module_info` and
    /// `merge_into`, returning the collected namespace object aliases.
    fn finalize_resolution_phase(&mut self) -> Vec<fallow_types::extract::NamespaceObjectAlias> {
        self.resolve_typed_destructure_bindings();
        self.resolve_pending_local_export_specifiers();
        self.enrich_local_class_exports();
        self.enrich_store_exports();
        self.finalize_di_key_sites();
        // Before `record_exported_instance_bindings` / `resolve_object_binding_candidates`,
        // which read `binding_target_names`, so a factory-return-bound local also
        // propagates through object literals and exported-instance bindings (parity
        // with the during-the-walk `new Class()` binding). See issue #1441.
        // Aliases first: promote `useApi(){ return api }` to a factory-return via
        // the typed local, so the `const x = useApi()` candidate below resolves.
        self.resolve_factory_return_aliases();
        self.resolve_factory_return_candidates();
        self.record_exported_instance_bindings();
        self.resolve_object_binding_candidates();
        self.resolve_factory_call_candidates();
        self.resolve_playwright_factory_call_definitions();
        self.resolve_structural_class_calls();
        self.resolve_bound_member_accesses();
        self.map_local_signature_refs_to_exports();
        self.apply_side_effect_registrations();
        self.resolve_typed_react_props();
        self.collect_namespace_object_aliases()
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
        let namespace_object_aliases = self.finalize_resolution_phase();
        let exported_factory_returns = self.collect_exported_factory_returns();
        let type_member_types = self.collect_type_member_types();
        ModuleInfo {
            file_id,
            exports: self.exports,
            imports: self.imports,
            re_exports: self.re_exports,
            dynamic_imports: self.dynamic_imports,
            dynamic_import_patterns: self.dynamic_import_patterns,
            require_calls: self.require_calls,
            package_path_references: self.package_path_references.into_boxed_slice(),
            member_accesses: self.member_accesses,
            semantic_facts: self.semantic_facts.into_boxed_slice(),
            whole_object_uses: self.whole_object_uses.into_boxed_slice(),
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
            exported_factory_returns: exported_factory_returns.into_boxed_slice(),
            type_member_types: type_member_types.into_boxed_slice(),
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
            inline_server_action_exports: self.inline_server_action_exports,
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
            angular_inputs: self.angular_inputs,
            angular_outputs: self.angular_outputs,
            angular_component_selectors: self.angular_component_selectors,
            registered_custom_elements: self.registered_custom_elements,
            used_custom_element_tags: {
                let mut tags: Vec<String> = self.used_custom_element_tags.into_iter().collect();
                tags.sort_unstable();
                tags
            },
            angular_used_selectors: self.angular_used_selectors,
            angular_entry_component_refs: self.angular_entry_component_refs,
            has_dynamic_component_render: self.has_dynamic_component_render,
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
            svelte_dispatched_events: self.svelte_dispatched_events,
            svelte_listened_events: Vec::new(),
            has_dynamic_dispatch: self.has_dynamic_dispatch,
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
        let namespace_object_aliases = self.finalize_resolution_phase();
        self.merge_module_graph(info, namespace_object_aliases);
        self.merge_security_info(info);
        self.merge_framework_info(info);
    }

    fn merge_module_graph(
        &mut self,
        info: &mut ModuleInfo,
        mut namespace_object_aliases: Vec<fallow_types::extract::NamespaceObjectAlias>,
    ) {
        // Compute before `self.exports` is drained below; the join reads exports.
        let mut exported_factory_returns = self.collect_exported_factory_returns();
        let mut type_member_types = self.collect_type_member_types();
        info.imports.append(&mut self.imports);
        info.exports.append(&mut self.exports);
        info.re_exports.append(&mut self.re_exports);
        info.dynamic_imports.append(&mut self.dynamic_imports);
        info.dynamic_import_patterns
            .append(&mut self.dynamic_import_patterns);
        info.require_calls.append(&mut self.require_calls);
        let mut package_path_references =
            std::mem::take(&mut info.package_path_references).into_vec();
        package_path_references.append(&mut self.package_path_references);
        info.package_path_references = package_path_references.into_boxed_slice();
        info.member_accesses.append(&mut self.member_accesses);
        let mut whole_object_uses = std::mem::take(&mut info.whole_object_uses).into_vec();
        whole_object_uses.append(&mut self.whole_object_uses);
        info.whole_object_uses = whole_object_uses.into_boxed_slice();
        // Carry typed semantic facts through the SFC merge path; without this
        // every fact kind (factory-fn, fluent-chain, typed-property-hop, ...)
        // was silently dropped for Vue/Svelte `<script>` blocks, so the
        // analyze-layer cross-module joins never saw SFC consumers. See issue
        // #1785 (review finding).
        let mut semantic_facts = std::mem::take(&mut info.semantic_facts).into_vec();
        semantic_facts.append(&mut self.semantic_facts);
        info.semantic_facts = semantic_facts.into_boxed_slice();
        info.has_cjs_exports |= self.has_cjs_exports;
        info.has_angular_component_template_url |= self.has_angular_component_template_url;
        info.class_heritage.append(&mut self.class_heritage);
        if !exported_factory_returns.is_empty() {
            let mut merged = std::mem::take(&mut info.exported_factory_returns).into_vec();
            merged.append(&mut exported_factory_returns);
            info.exported_factory_returns = merged.into_boxed_slice();
        }
        if !type_member_types.is_empty() {
            let mut merged = std::mem::take(&mut info.type_member_types).into_vec();
            merged.append(&mut type_member_types);
            info.type_member_types = merged.into_boxed_slice();
        }
        info.injection_tokens.append(&mut self.injection_tokens);
        info.local_type_declarations
            .append(&mut self.local_type_declarations);
        info.public_signature_type_references
            .append(&mut self.public_signature_type_references);
        info.namespace_object_aliases
            .append(&mut namespace_object_aliases);
        info.directives.append(&mut self.directives);
        info.client_only_dynamic_import_spans
            .append(&mut self.client_only_dynamic_import_spans);
        info.callee_uses.append(&mut self.callee_uses);
    }

    fn merge_security_info(&mut self, info: &mut ModuleInfo) {
        info.security_sinks.append(&mut self.security_sinks);
        info.security_sinks_skipped += self.security_sinks_skipped;
        info.security_unresolved_callee_sites
            .append(&mut self.security_unresolved_callee_sites);
        info.tainted_bindings.append(&mut self.tainted_bindings);
        info.sanitized_sink_args
            .append(&mut self.sanitized_sink_args);
        info.security_control_sites
            .append(&mut self.security_control_sites);
    }

    fn merge_framework_info(&mut self, info: &mut ModuleInfo) {
        info.misplaced_directives
            .append(&mut self.misplaced_directives);
        info.inline_server_action_exports
            .append(&mut self.inline_server_action_exports);
        info.di_key_sites.append(&mut self.di_key_sites);
        info.has_dynamic_provide |= self.has_dynamic_provide;
        info.load_return_keys.append(&mut self.load_return_keys);
        info.has_unharvestable_load |= self.has_unharvestable_load;
        info.has_load_data_whole_use |= self.has_load_data_whole_use;
        info.angular_inputs.append(&mut self.angular_inputs);
        info.angular_outputs.append(&mut self.angular_outputs);
        info.angular_component_selectors
            .append(&mut self.angular_component_selectors);
        info.angular_used_selectors
            .append(&mut self.angular_used_selectors);
        info.angular_entry_component_refs
            .append(&mut self.angular_entry_component_refs);
        info.has_dynamic_component_render |= self.has_dynamic_component_render;
        info.svelte_dispatched_events
            .append(&mut self.svelte_dispatched_events);
        info.has_dynamic_dispatch |= self.has_dynamic_dispatch;
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

/// Collect every statically-resolvable module specifier from a dynamic
/// `import()` source expression, following conditional and logical branches so
/// `import(cond ? './a' : './b')` yields both `./a` and `./b`. String literals
/// and no-substitution template literals resolve; genuinely runtime branches (a
/// bare identifier, a call) are skipped, so a mixed
/// `import(cond ? './a' : runtimeVar)` still credits the literal branch while
/// `import(runtimeVar)` yields nothing (correctly left unresolvable). Repeated
/// literals across branches are deduplicated so one call site never yields two
/// identical edges (and thus duplicate unresolved-import findings).
fn collect_static_import_specifiers(source: &Expression<'_>, out: &mut Vec<String>) {
    match source {
        Expression::StringLiteral(lit) => {
            let value = lit.value.to_string();
            if !out.contains(&value) {
                out.push(value);
            }
        }
        Expression::TemplateLiteral(tpl)
            if tpl.expressions.is_empty() && !tpl.quasis.is_empty() =>
        {
            let value = tpl.quasis[0].value.raw.to_string();
            if !value.is_empty() && !out.contains(&value) {
                out.push(value);
            }
        }
        Expression::ParenthesizedExpression(paren) => {
            collect_static_import_specifiers(&paren.expression, out);
        }
        Expression::ConditionalExpression(cond) => {
            collect_static_import_specifiers(&cond.consequent, out);
            collect_static_import_specifiers(&cond.alternate, out);
        }
        Expression::LogicalExpression(logical) => {
            collect_static_import_specifiers(&logical.left, out);
            collect_static_import_specifiers(&logical.right, out);
        }
        _ => {}
    }
}

fn try_extract_property_callback_import<'a, 'b>(
    prop: &'b ObjectProperty<'a>,
) -> Option<(&'b ImportExpression<'a>, Vec<String>)> {
    let property_name = prop.key.static_name()?;
    if !matches!(
        property_name.as_ref(),
        "component" | "loadChildren" | "loadComponent"
    ) {
        return None;
    }

    let import_expr = extract_import_from_callable(&prop.value)?;
    let mut sources = Vec::new();
    collect_static_import_specifiers(&import_expr.source, &mut sources);
    if sources.is_empty() {
        return None;
    }
    Some((import_expr, sources))
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
) -> Option<(&'b ImportExpression<'a>, Vec<String>)> {
    for arg in arguments {
        let Some(expr) = arg.as_expression() else {
            continue;
        };
        let Some(import_expr) = extract_import_from_callable(expr) else {
            continue;
        };
        let mut sources = Vec::new();
        collect_static_import_specifiers(&import_expr.source, &mut sources);
        if !sources.is_empty() {
            return Some((import_expr, sources));
        }
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
    sources: Vec<String>,
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
    let mut sources = Vec::new();
    collect_static_import_specifiers(&import_expr.source, &mut sources);
    if sources.is_empty() {
        return None;
    }
    let import_span = import_expr.span;

    match expr.arguments.first()? {
        Argument::ArrowFunctionExpression(arrow) => {
            arrow_then_callback(arrow, sources, import_span)
        }
        Argument::FunctionExpression(func) => {
            let param = func.params.items.first()?;
            then_callback_from_pattern(&param.pattern, sources, import_span)
        }
        _ => None,
    }
}

/// Build an `ImportThenCallback` from a `.then()` arrow callback, handling the
/// expression-body member-access shape before falling back to the bare param.
fn arrow_then_callback(
    arrow: &oxc_ast::ast::ArrowFunctionExpression<'_>,
    sources: Vec<String>,
    import_span: Span,
) -> Option<ImportThenCallback> {
    let param = arrow.params.items.first()?;
    if let BindingPattern::BindingIdentifier(id) = &param.pattern {
        let param_name = id.name.to_string();
        if arrow.expression
            && let Some(Statement::ExpressionStatement(expr_stmt)) = arrow.body.statements.first()
            && let Some(names) = extract_member_names_from_expr(&expr_stmt.expression, &param_name)
        {
            return Some(ImportThenCallback {
                sources,
                import_span,
                destructured_names: names,
                local_name: None,
            });
        }
        return Some(ImportThenCallback {
            sources,
            import_span,
            destructured_names: Vec::new(),
            local_name: Some(param_name),
        });
    }
    then_callback_from_pattern(&param.pattern, sources, import_span)
}

/// Build an `ImportThenCallback` from a callback param pattern: object pattern
/// yields destructured names, a bare identifier yields a namespace local.
fn then_callback_from_pattern(
    pattern: &BindingPattern<'_>,
    sources: Vec<String>,
    import_span: Span,
) -> Option<ImportThenCallback> {
    match pattern {
        BindingPattern::ObjectPattern(obj_pat) => Some(ImportThenCallback {
            sources,
            import_span,
            destructured_names: extract_destructured_names(obj_pat),
            local_name: None,
        }),
        BindingPattern::BindingIdentifier(id) => Some(ImportThenCallback {
            sources,
            import_span,
            destructured_names: Vec::new(),
            local_name: Some(id.name.to_string()),
        }),
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
