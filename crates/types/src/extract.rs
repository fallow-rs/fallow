//! Module extraction types.

use std::path::PathBuf;
use std::sync::Arc;

use oxc_span::Span;

pub use crate::complexity_contribution::{
    ComplexityContribution, ComplexityContributionKind, ComplexityMetric,
};
use crate::discover::FileId;
use crate::{inline_threshold::InlineThresholdOverrides, suppress::Suppression};
#[path = "extract_playwright_facts.rs"]
mod playwright_facts;
use playwright_facts::{
    playwright_fixture_alias_facts, playwright_fixture_definition_facts,
    playwright_fixture_type_facts, playwright_fixture_use_facts,
};

/// Extracted module information from a single file.
///
/// The `Arc<[T]>` fields are immutable after extraction and are shared by
/// refcount with the resolver's per-file output, so the resolve/graph path
/// does not deep-copy per-file extraction payloads.
#[derive(Debug, Clone)]
pub struct ModuleInfo {
    /// Unique identifier for this file.
    pub file_id: FileId,
    /// All export declarations in this module.
    pub exports: Arc<[ExportInfo]>,
    /// All import declarations in this module.
    pub imports: Vec<ImportInfo>,
    /// All re-export declarations (e.g., `export { foo } from './bar'`).
    pub re_exports: Vec<ReExportInfo>,
    /// All dynamic `import()` calls with string literal sources.
    pub dynamic_imports: Vec<DynamicImportInfo>,
    /// Dynamic import patterns.
    pub dynamic_import_patterns: Vec<DynamicImportPattern>,
    /// All `require()` calls.
    pub require_calls: Vec<RequireCallInfo>,
    /// Package names statically referenced through package path resolution.
    pub package_path_references: Box<[String]>,
    /// Package names that a module augmentation (`declare module 'pkg'` in a
    /// module file) names. They credit the package as a type-only use.
    pub type_package_references: Box<[String]>,
    /// Binary names found in `node_modules/.bin/<name>` paths in string
    /// literals and template quasis. The analysis maps each name to the
    /// package that declares the binary.
    pub bin_path_references: Box<[String]>,
    /// Package names from direct `require.resolve('pkg')` calls, each with the
    /// byte offset of its call. A name from a resolver function, a loop
    /// binding or a static table has no site, because it is a heuristic
    /// credit. The unlisted-dependency check reports a site like an import.
    pub package_resolve_sites: Box<[(String, u32)]>,
    /// Static member access expressions (e.g., `Status.Active`).
    pub member_accesses: Arc<[MemberAccess]>,
    /// Typed semantic facts produced by extraction for cross-layer analysis.
    pub semantic_facts: Arc<[SemanticFact]>,
    /// Identifiers used in whole-object access patterns.
    pub whole_object_uses: Arc<[String]>,
    /// Whether this module uses CommonJS exports.
    pub has_cjs_exports: bool,
    /// Whether this module declares an Angular component `templateUrl`.
    pub has_angular_component_template_url: bool,
    /// xxh3 hash of the file content for incremental caching.
    pub content_hash: u64,
    /// Number of parser diagnostics the parse of this file produced. Non-zero
    /// means imports, exports and references after the first error may be
    /// missing (`source-parse-degraded`, which never withholds a finding).
    /// Zero for non-JS extraction paths, which do not run the oxc parser.
    pub parse_error_count: u32,
    /// `true` when the parser abandoned the file instead of recovering. The
    /// extracted module is then a fragment of the real one at best.
    pub parse_panicked: bool,
    /// `true` when the file nests too deep to parse, so it was not parsed.
    pub parse_nesting_too_deep: bool,
    /// Inline suppression directives parsed from comments.
    pub suppressions: Vec<Suppression>,
    /// Suppression tokens that parse to no `IssueKind`, reported as stale (issue #449).
    pub unknown_suppression_kinds: Vec<crate::suppress::UnknownSuppressionKind>,
    /// Range suppression markers with a problem. They have no effect.
    pub range_marker_problems: Vec<crate::suppress::RangeMarkerProblem>,
    /// `fallow-override-threshold-next-line` comments and their problems.
    pub inline_threshold_overrides: InlineThresholdOverrides,
    /// Local names of import bindings that are never referenced in this file.
    /// Populated via `oxc_semantic` scope analysis. Used at graph-build time
    /// to skip adding references for imports whose binding is never read,
    /// improving unused-export detection precision.
    pub unused_import_bindings: Vec<String>,
    /// Local import bindings that are referenced from TypeScript type positions.
    /// Used to distinguish value-namespace and type-namespace references when a
    /// module exports both `const X` and `type X`.
    pub type_referenced_import_bindings: Vec<String>,
    /// Local import bindings referenced from runtime/value positions.
    pub value_referenced_import_bindings: Vec<String>,
    /// Pre-computed byte offsets where each line starts.
    pub line_offsets: Vec<u32>,
    /// Per-function complexity metrics.
    pub complexity: Vec<FunctionComplexity>,
    /// Feature flag use sites.
    pub flag_uses: Vec<FlagUse>,
    /// Flag-key registries this module exports, and flag reads that name a
    /// member of an imported registry. `None` for the common module with
    /// neither.
    pub flag_registry_facts: Option<Box<FlagRegistryFacts>>,
    /// Heritage metadata for exported classes that declare `implements`.
    pub class_heritage: Vec<ClassHeritageInfo>,
    /// Exported free-function factories that provably return one class instance
    /// (`export function useApi() { return new RESTApi() }`). Origin-module proof
    /// that an exported function returns a class instance, so a cross-module
    /// `const x = useApi(); x.member` consumer can credit the returned class.
    /// See issue #1441 (Part A).
    pub exported_factory_returns: Arc<[FactoryReturnExport]>,
    /// Exported factories that return an OBJECT LITERAL whose property values are
    /// class instances (`export function createUi() { return { orders: factory.ordersPage } }`).
    /// Each entry maps a dotted property path (`orders`, `invoke.dashboard`) to the
    /// returned class's local name within the factory module, so a cross-module
    /// `const ui = createUi(); ui.orders.member` consumer can credit the class. Names
    /// are local to this module; resolution is deferred to analyze time. See issue #1858.
    pub exported_factory_return_object_shapes: Arc<[FactoryReturnObjectShapeExport]>,
    /// Named-type property types declared by this module's top-level interfaces
    /// and type-literal aliases (`interface Opts { c: OptDep }`). Names are
    /// local to this module; resolution is deferred to analyze time. Consumed
    /// by the `unused-class-member` typed-property-hop join and the Playwright
    /// fixture-type resolution. See issue #1785.
    pub type_member_types: Arc<[TypeMemberTypeEntry]>,
    /// Angular `InjectionToken<Interface>` declarations, as
    /// `(token_export_name, interface_name)` pairs. Recorded only for
    /// `new InjectionToken<I>(...)` initializers whose `InjectionToken` is
    /// imported from `@angular/core`. The analyze layer follows the token's
    /// interface type argument to the classes that `implement` it so a template
    /// member call through `inject(TOKEN)` credits the concrete implementation.
    /// See issue #920 (follow-up to #911 / #913).
    pub injection_tokens: Vec<(String, String)>,
    /// Local type-capable declarations.
    pub local_type_declarations: Vec<LocalTypeDeclaration>,
    /// Type references in exported public signatures.
    pub public_signature_type_references: Vec<PublicSignatureTypeReference>,
    /// Aliases of namespace imports re-exported through an object literal.
    pub namespace_object_aliases: Vec<NamespaceObjectAlias>,
    /// Deduped Iconify collection prefixes found in static icon props.
    pub iconify_prefixes: Vec<String>,
    /// Deduped Nuxt UI `i-<collection>-<icon>` icon class suffixes found in
    /// static script-side icon properties.
    pub iconify_icon_names: Vec<String>,
    /// Bare identifiers that may be resolved by framework auto-imports.
    pub auto_import_candidates: Vec<String>,
    /// File-level string directives in source order (e.g. `"use client"`,
    /// `"use server"`, `"use strict"`). Captured from `Program::directives`.
    /// Consumed by the security `client-server-leak` detector to identify
    /// React Server Component client boundaries.
    pub directives: Vec<String>,
    /// Byte-offset starts of dynamic `import()` expressions wrapped in
    /// `next/dynamic(() => import('./X'), { ssr: false })`. The ssr:false option
    /// is Next.js's sanctioned way to pull a client-only module, so a server-only
    /// module reached ONLY through such an import is NOT a client-server leak. The
    /// security `client-server-leak` BFS resolves each dynamic import to a graph
    /// edge; these span starts let the BFS exclude exactly those edges (matched
    /// against the edge's `import_span`). Empty for files with no ssr:false
    /// dynamic import. Captured only by JS/TS extraction.
    pub client_only_dynamic_import_spans: Vec<u32>,
    /// The non-public env reads in source order, for the `client-server-leak` rule.
    pub env_reads: Vec<EnvRead>,
    /// Captured security sink sites (category-blind). Consumed by the
    /// catalogue-driven `tainted_sink` detector. Captured only by JS/TS
    /// extraction; empty for CSS/MDX/etc. See `security_matchers.toml`.
    pub security_sinks: Vec<SinkSite>,
    /// Count of sink-shaped nodes whose callee could not be flattened to a
    /// static path (dynamic dispatch, computed members, aliased bindings).
    /// Surfaced in-band so an empty catalogue result with a non-zero count is
    /// not a clean bill.
    pub security_sinks_skipped: u32,
    /// Compact span-level diagnostics for skipped security sink callees. Kept
    /// next to `security_sinks_skipped` so warm-cache and cold-cache security
    /// output can explain where the blind spots are concentrated without source
    /// snippets.
    pub security_unresolved_callee_sites: Vec<SkippedSecurityCalleeSite>,
    /// Export names (`default` for a default export) of the module-scope
    /// functions whose body is shallow-safe: it calls only shallow `Object`
    /// builtins and writes only one level deep into a fresh local object. The
    /// security analysis follows a direct import of such a function one hop
    /// and does not report its call as a name-only library sink.
    pub inert_function_exports: Vec<String>,
    /// Local bindings whose initializer (or destructured object) is a flattened
    /// member-access path. Used by the security `tainted_sink` detector to
    /// back-trace a sink argument to a known untrusted source: the analyze layer
    /// matches each binding's `source_path` against the data-driven source
    /// catalogue (`security_matchers.toml` `[[source]]` rows) and treats the
    /// matching `local` names as source-tainted. Intra-module and name-based
    /// (no scope analysis); a conservative association, never a taint proof.
    pub tainted_bindings: Vec<TaintedBinding>,
    /// Sink arguments that were recognized as sanitizer calls at extraction
    /// time. Used for direct sink calls such as
    /// `el.innerHTML = DOMPurify.sanitize(input)`.
    pub sanitized_sink_args: Vec<SanitizedSinkArg>,
    /// Control patterns observed in this module. Surface context only: their
    /// presence does not establish input identity, execution order, or protection.
    pub security_control_sites: Vec<SecurityControlSite>,
    /// Statically flattenable callee paths invoked in this module, one entry
    /// per path and form with the start of every site. Consumed by
    /// `boundaries.calls.forbidden`, `banned-call` and `banned-effect`.
    /// Captured unconditionally because extraction is config-blind; each path
    /// string is stored once, and each extra site costs 4 bytes.
    pub callee_uses: Vec<CalleeUse>,
    /// Literal commands that process-spawning calls run (`spawn("git")`,
    /// `execSync("make build")`, `` $`ffmpeg -i a` ``). Read by the
    /// `unlisted-binaries` detector.
    pub command_invocations: Vec<CommandInvocation>,
    /// Per-call references to actual runtime ESM import bindings.
    pub imported_call_sites: Arc<[ImportedCallSite]>,
    /// Value references to runtime ESM import bindings that are not admitted
    /// call sites, plus admitted calls that initialize a top-level declarator.
    /// Sorted by `span_start`. Read by `fallow trace --dependency`.
    pub import_binding_references: Arc<[ImportBindingReference]>,
    /// `"use client"` / `"use server"` directive strings written as expression
    /// statements in `program.body` (misplaced, NOT in the leading
    /// prologue), so the RSC bundler silently ignores them. One entry per
    /// occurrence. Consumed by the `misplaced-directive` detector. Captured
    /// only by JS/TS extraction.
    pub misplaced_directives: Vec<MisplacedDirectiveSite>,
    /// Export LOCAL NAMES of exported functions / const-arrows whose body has an
    /// inline `"use server"` directive (`export async function f() { "use server"
    /// }`), captured in a NON-`"use server"` file. Consumed by the
    /// `unused-server-action` detector to reclassify an unused inline Server
    /// Action export out of `unused-export`. Captured only by JS/TS extraction.
    pub inline_server_action_exports: Vec<String>,
    /// Vue `provide`/`inject` and Svelte `setContext`/`getContext` call sites
    /// keyed by an identifier symbol. Consumed by the `unprovided-inject`
    /// detector to find an inject/getContext whose key is provided nowhere
    /// project-wide. Only identifier-keyed sites are recorded (string-literal
    /// and computed keys abstain). Captured by JS/TS and SFC extraction.
    pub di_key_sites: Vec<DiKeySite>,
    /// `true` when this module contains a `provide(...)` / `*.provide(...)` /
    /// `setContext(...)` call whose key argument is NOT a plain identifier
    /// (spread, computed, member, loop variable). Such a call can provide an
    /// unknowable key, so the `unprovided-inject` detector abstains on ALL
    /// inject findings project-wide when any reachable module sets this flag.
    /// Mirrors the spread-return whole-object abstain used for Pinia stores.
    pub has_dynamic_provide: bool,
    /// `true` when the prologue holds `"use server"` and every value export
    /// is an async function (a Server Action). A `"use client"` import of
    /// such a module becomes an action reference, so the security
    /// `client-server-leak` BFS stops at the module. `false` for every other
    /// module, including a `"use server"` file with a non-action value
    /// export. Captured only by JS/TS extraction.
    pub is_server_action_module: bool,
    /// `true` when TypeScript reads this file as adding to the global scope:
    /// a script file (no top-level import or export), or a module file with a
    /// top-level `declare global` block or string-named `declare module`
    /// block. Read only for declaration files (`.d.ts`, `.d.mts`, `.d.cts`):
    /// a declaration file without global declarations is seeded as an entry
    /// point only when something points to it. Captured only by JS/TS
    /// extraction; `false` for every other module.
    pub has_global_declarations: bool,
    /// Raw `path` values of `/// <reference path="..." />` directives in this
    /// file, in source order. A declaration file named by one stays an entry
    /// point. Captured only by JS/TS extraction.
    pub triple_slash_reference_paths: Box<[String]>,
    /// `true` when the file has a JSX element or fragment and no
    /// `@jsxImportSource` or `@jsxRuntime classic` pragma. A bundler or test
    /// config can then select the automatic JSX runtime for the file, so the
    /// resolver adds the runtime edge from a plugin rule. Captured only by
    /// JS/TS extraction; `false` for every other module.
    pub jsx_runtime_from_config: bool,
    /// Local names of import bindings that ARE referenced somewhere in this file
    /// (script value/type position OR template/markup). The complement of
    /// `unused_import_bindings` among `imports`. Derived by
    /// `prepare_analysis_facts` while both source vectors are still present, so
    /// it remains readable after the owned release path clears them. It is never
    /// cached and is recomputed on every cache load. Consumed by the
    /// `unrendered-component` detector to credit a
    /// Vue/Svelte SFC that some file actually imports-and-uses, distinguishing it
    /// from a component reachable only through a barrel re-export.
    pub referenced_import_bindings: Vec<String>,
    /// Vue `<script setup>` `defineProps` and Svelte 5 `$props()` declared
    /// props. Consumed by the `unused-component-prop` detector to flag a prop
    /// referenced nowhere in its own SFC. Each entry carries `used_in_script` /
    /// `used_in_template`.
    pub component_props: Vec<ComponentProp>,
    /// Cached framework-neutral component input contracts and inspected callers.
    pub component_contracts: Option<Box<ComponentContractFacts>>,
    /// Effect layer, tagged error and `catchTag` facts. Set only for a module
    /// that imports `effect` or an `@effect/*` package.
    pub effect_facts: Option<Box<EffectFacts>>,
    /// Parameter contracts of module-scope functions, direct calls of
    /// admitted callees and escapes of local functions. `None` when the
    /// module has none of these facts.
    pub function_contracts: Option<Box<FunctionContractFacts>>,
    /// `true` when the template spreads the whole props/attrs object
    /// (`v-bind="$attrs"` / `v-bind="$props"` / `v-bind="props"`) or the props
    /// return is destructured with a rest element. Either form can consume a prop
    /// indirectly, so the detector abstains on the whole file.
    pub has_props_attrs_fallthrough: bool,
    /// `true` when the SFC calls `defineExpose(...)`. A prop may be re-exposed,
    /// so the detector conservatively abstains on the whole file.
    pub has_define_expose: bool,
    /// `true` when the SFC calls `defineModel(...)`. Two-way model props are out
    /// of scope for v1, so the detector abstains on the whole file.
    pub has_define_model: bool,
    /// `true` when props were declared through an unharvestable shape, such as a
    /// Vue type-reference argument or an opaque Svelte `$props()` destructure.
    /// The detector abstains on the whole file so a prop is never falsely
    /// flagged.
    pub has_unharvestable_props: bool,
    /// Vue `<script setup>` `defineEmits` declared events. Consumed by the
    /// `unused-component-emit` detector to flag an event emitted nowhere in its
    /// own SFC. Each entry carries `used`.
    pub component_emits: Vec<ComponentEmit>,
    /// Angular component/directive inputs declared via `@Input()` decorators or
    /// signal `input()` / `input.required()` / `model()` initializers. Consumed
    /// by the `unused-component-input` detector to flag an input read nowhere in
    /// its own component. Empty for every non-Angular class.
    pub angular_inputs: Vec<AngularInputMember>,
    /// Angular component/directive outputs declared via `@Output()` decorators or
    /// signal `output()` / `outputFromObservable()` initializers. Consumed by the
    /// `unused-component-output` detector to flag an output emitted nowhere in its
    /// own component. A `model()` is recorded as an input only (see
    /// `AngularOutputMember`). Empty for every non-Angular class.
    pub angular_outputs: Vec<AngularOutputMember>,
    /// Angular `@Component` declarations with their `selector` value(s), harvested
    /// from `@Component({ selector: '...' })` decorators. Consumed by the Angular
    /// arm of the `unrendered-component` detector. Empty for every non-Angular
    /// class and for `@Directive`. See `AngularComponentSelector`.
    pub angular_component_selectors: Vec<AngularComponentSelector>,
    /// Lit / web-component custom elements REGISTERED in this file via
    /// `@customElement('x-foo')` or `customElements.define('x-foo', C)`. Consumed
    /// by the Lit arm of the `unrendered-component` detector, which flags a
    /// registered element whose tag is rendered in NO `html` template
    /// project-wide. Empty for non-Lit / non-web-component files. See
    /// `RegisteredCustomElement`.
    pub registered_custom_elements: Vec<RegisteredCustomElement>,
    /// Custom-element tag names USED (rendered) in this file's `html` tagged
    /// templates, e.g. `` html`<x-foo></x-foo>` `` -> `x-foo`. Only hyphenated
    /// (custom-element) tags are recorded; native HTML tags are excluded by the
    /// hyphen requirement. The detector unions these project-wide into the
    /// rendered-tag set. Empty for files with no `html` templates.
    pub used_custom_element_tags: Vec<String>,
    /// Custom element selector tag names referenced in this file's Angular
    /// templates (inline `@Component({ template })` and the linked external
    /// `templateUrl` `.html` module), e.g. `<app-foo>` -> `app-foo`. Native HTML
    /// tag names are excluded at harvest. The detector unions these project-wide
    /// into the used-selector set. Empty for non-Angular files.
    pub angular_used_selectors: Vec<String>,
    /// Angular component class names referenced as a route entry or bootstrap
    /// target: a route `component: Foo` / `loadComponent: () => import().then(m =>
    /// m.Foo)` value, a `bootstrapApplication(Foo)` argument, or a
    /// `bootstrap: [Foo]` NgModule entry. These are render-equivalent entry points
    /// (Angular instantiates them without a template `<tag>`), so the Angular
    /// `unrendered-component` detector abstains on a component whose class name is
    /// in the project-wide union. A plain `declarations: [...]` / `imports: [...]`
    /// registration is intentionally NOT harvested here (that is the dead case the
    /// rule catches). Empty for non-Angular files.
    pub angular_entry_component_refs: Vec<String>,
    /// `true` when this file dynamically renders an Angular component fallow
    /// cannot attribute to a literal class reference: a
    /// `ViewContainerRef.createComponent(...)` / `*.createComponent(<ident>)`
    /// call, or an `*ngComponentOutlet` template binding. The Angular
    /// `unrendered-component` detector abstains project-wide when ANY reachable
    /// module sets this (mirroring `unprovided-inject`'s `has_dynamic_provide`),
    /// since a component could be rendered by a non-literal class reference.
    pub has_dynamic_component_render: bool,
    /// `true` when `defineEmits` was called with an unharvestable argument (a
    /// type-reference type argument such as `defineEmits<MyEmits>()`, a
    /// non-literal runtime form, or an unbound `defineEmits([...])`). The
    /// detector abstains on the whole file so an emit is never falsely flagged.
    pub has_unharvestable_emits: bool,
    /// `true` when an `emit(<nonLiteral>)` call was seen (the emitted event name
    /// cannot be known statically). The detector abstains on the whole file.
    pub has_dynamic_emit: bool,
    /// `true` when the `defineEmits` return binding was used as a WHOLE value
    /// (passed to a function, returned, or spread), which can emit any event
    /// opaquely. The detector abstains on the whole file.
    pub has_emit_whole_object_use: bool,
    /// SvelteKit `load()` return-object keys harvested from a
    /// `+page.{ts,server.ts,js,server.js}` file's terminal return literal.
    /// Consumed by the `unused-load-data-key` detector. Empty for every file
    /// that is not a page-load producer (gated by basename at harvest time).
    pub load_return_keys: Vec<LoadReturnKey>,
    /// `true` when this file's `load()` body could not be harvested safely (a
    /// spread return, a non-object/non-literal return, more than one top-level
    /// `return`, a computed key, or a wrapped/re-exported `load`). The detector
    /// abstains on the whole file so a key is never falsely flagged.
    pub has_unharvestable_load: bool,
    /// `true` when this file passes the whole `data` object opaquely (script
    /// `const X = data`, `fn(data)` / `fn(...data)`, or template `data={data}` /
    /// `{...data}` in a route component), so a child can read arbitrary keys the
    /// detector cannot see. Name-gated on the `data` binding. Read ONLY by the
    /// `unused-load-data-key` detector, so capturing it for all files is
    /// byte-identity-safe. See FP-1 in the plan.
    pub has_load_data_whole_use: bool,
    /// `true` when this file uses the whole `page.data` / `$page.data` store
    /// object opaquely (e.g. `Object.values(page.data)`, `{...$page.data}`), so a
    /// reflective read could consume any route's key. Drives the
    /// `unused-load-data-key` detector's project-wide abstain. Derived by
    /// `prepare_analysis_facts` from `whole_object_uses` before the owned release
    /// path clears that vector. It is never cached and is recomputed each run from
    /// the cached `whole_object_uses`. Reassignment forms
    /// (`const all = $page.data`) are not whole-object-tracked and stay out of
    /// scope, matching the syntactic analyzer's conservative posture.
    pub has_page_data_store_whole_use: bool,
    /// `true` when a React Router or Remix route consumes the whole
    /// `useLoaderData()` result opaquely. Derived by `prepare_analysis_facts`
    /// from the synthetic route-loader marker before the owned release path
    /// clears `whole_object_uses`. It is recomputed from cached extraction data.
    pub has_route_loader_data_whole_use: bool,
    /// React/JSX component definitions: functions/arrows whose body returns JSX.
    /// Captured only for `.jsx`/`.tsx` files when a React/Preact dependency is
    /// plausible. Consumed by the React `unused-component-prop` arm and the
    /// complexity-fold phase. Empty for non-React files.
    pub component_functions: Vec<ComponentFunction>,
    /// React component props (reuses the shared `ComponentProp` struct). For
    /// React, `used_in_template` is always false and `used_in_script` means
    /// used-in-body. Empty for non-React files.
    pub react_props: Vec<ComponentProp>,
    /// React hook call sites (`useState` / `useEffect` / `useMemo` /
    /// `useCallback` / custom `use*`). Drives hook-density complexity context.
    /// Empty for non-React files.
    pub hook_uses: Vec<HookUse>,
    /// React render edges: one component rendering another. Captured with the
    /// child's written name; child-to-`FileId` resolution is deferred to graph
    /// build. Empty for non-React files.
    pub render_edges: Vec<RenderEdge>,
    /// Svelte custom events dispatched via `dispatch('<name>')` where `dispatch`
    /// is the binding from `const dispatch = createEventDispatcher()`. Consumed
    /// by the `unused-svelte-event` detector to flag an event dispatched here but
    /// listened to nowhere project-wide. Each entry carries the literal event
    /// name and its span. Empty for every non-Svelte file.
    pub svelte_dispatched_events: Vec<DispatchedEvent>,
    /// Svelte custom-event listener names harvested from template `on:<name>`
    /// bindings on COMPONENT tags (PascalCase tag names). Lowercase DOM-element
    /// `on:click` is a DOM event, not a custom event, and is excluded. Unioned
    /// project-wide by the `unused-svelte-event` detector to build the liberal
    /// "listened" set. Empty for every non-Svelte file.
    pub svelte_listened_events: Vec<String>,
    /// `true` when a `dispatch(<nonLiteral>)` call was seen (the dispatched event
    /// name cannot be known statically), or the `dispatch` binding was used as a
    /// whole value (passed / returned). The `unused-svelte-event` detector
    /// abstains on the whole component so an event is never falsely flagged.
    pub has_dynamic_dispatch: bool,
}

impl ModuleInfo {
    /// A fully-zeroed `ModuleInfo` for the given file.
    ///
    /// Fixture builders and non-JS extraction paths start from this and set
    /// only the fields they care about via struct-update syntax:
    /// `ModuleInfo { exports: vec![..], ..ModuleInfo::empty(file_id) }`.
    #[must_use]
    pub fn empty(file_id: FileId) -> Self {
        Self {
            file_id,
            exports: Arc::default(),
            imports: Vec::new(),
            re_exports: Vec::new(),
            dynamic_imports: Vec::new(),
            dynamic_import_patterns: Vec::new(),
            require_calls: Vec::new(),
            package_path_references: Box::default(),
            type_package_references: Box::default(),
            bin_path_references: Box::default(),
            package_resolve_sites: Box::default(),
            member_accesses: Arc::default(),
            semantic_facts: Arc::default(),
            whole_object_uses: Arc::default(),
            has_cjs_exports: false,
            has_angular_component_template_url: false,
            content_hash: 0,
            parse_error_count: 0,
            parse_panicked: false,
            parse_nesting_too_deep: false,
            suppressions: Vec::new(),
            unknown_suppression_kinds: Vec::new(),
            range_marker_problems: Vec::new(),
            inline_threshold_overrides: InlineThresholdOverrides::default(),
            unused_import_bindings: Vec::new(),
            type_referenced_import_bindings: Vec::new(),
            value_referenced_import_bindings: Vec::new(),
            line_offsets: Vec::new(),
            complexity: Vec::new(),
            flag_uses: Vec::new(),
            flag_registry_facts: None,
            class_heritage: Vec::new(),
            exported_factory_returns: Arc::default(),
            exported_factory_return_object_shapes: Arc::default(),
            type_member_types: Arc::default(),
            injection_tokens: Vec::new(),
            local_type_declarations: Vec::new(),
            public_signature_type_references: Vec::new(),
            namespace_object_aliases: Vec::new(),
            iconify_prefixes: Vec::new(),
            iconify_icon_names: Vec::new(),
            auto_import_candidates: Vec::new(),
            directives: Vec::new(),
            client_only_dynamic_import_spans: Vec::new(),
            env_reads: Vec::new(),
            security_sinks: Vec::new(),
            security_sinks_skipped: 0,
            security_unresolved_callee_sites: Vec::new(),
            inert_function_exports: Vec::new(),
            tainted_bindings: Vec::new(),
            sanitized_sink_args: Vec::new(),
            security_control_sites: Vec::new(),
            callee_uses: Vec::new(),
            command_invocations: Vec::new(),
            imported_call_sites: Arc::default(),
            import_binding_references: Arc::default(),
            misplaced_directives: Vec::new(),
            inline_server_action_exports: Vec::new(),
            di_key_sites: Vec::new(),
            has_dynamic_provide: false,
            is_server_action_module: false,
            has_global_declarations: false,
            triple_slash_reference_paths: Box::default(),
            jsx_runtime_from_config: false,
            referenced_import_bindings: Vec::new(),
            component_props: Vec::new(),
            component_contracts: None,
            effect_facts: None,
            function_contracts: None,
            has_props_attrs_fallthrough: false,
            has_define_expose: false,
            has_define_model: false,
            has_unharvestable_props: false,
            component_emits: Vec::new(),
            angular_inputs: Vec::new(),
            angular_outputs: Vec::new(),
            has_unharvestable_emits: false,
            has_dynamic_emit: false,
            has_emit_whole_object_use: false,
            load_return_keys: Vec::new(),
            has_unharvestable_load: false,
            has_load_data_whole_use: false,
            has_page_data_store_whole_use: false,
            has_route_loader_data_whole_use: false,
            component_functions: Vec::new(),
            react_props: Vec::new(),
            hook_uses: Vec::new(),
            render_edges: Vec::new(),
            svelte_dispatched_events: Vec::new(),
            svelte_listened_events: Vec::new(),
            angular_component_selectors: Vec::new(),
            registered_custom_elements: Vec::new(),
            used_custom_element_tags: Vec::new(),
            angular_used_selectors: Vec::new(),
            angular_entry_component_refs: Vec::new(),
            has_dynamic_component_render: false,
            has_dynamic_dispatch: false,
        }
    }

    /// Derive compact detector facts from resolution payload before sharing.
    ///
    /// Shared analysis sessions keep the source payload for later graph runs,
    /// but detectors still require the same derived facts that the owned
    /// release path computes before clearing that payload.
    #[doc(hidden)]
    pub fn prepare_analysis_facts(&mut self) {
        // The analyze-layer `unrendered-component` detector needs the compact
        // complement of imports and unused bindings after resolution.
        self.referenced_import_bindings = self
            .imports
            .iter()
            .map(|import| import.local_name.clone())
            .filter(|name| !name.is_empty() && !self.unused_import_bindings.contains(name))
            .collect();
        self.referenced_import_bindings.sort_unstable();
        self.referenced_import_bindings.dedup();

        // The `unused-load-data-key` detector needs the project-wide signal
        // after `whole_object_uses` is released from owned artifacts.
        self.has_page_data_store_whole_use = self
            .whole_object_uses
            .iter()
            .any(|name| name == "page.data" || name == "$page.data");
        self.has_route_loader_data_whole_use = self
            .whole_object_uses
            .iter()
            .any(|name| name == "$fallow.routeLoaderData");
    }

    /// Release extraction payload that resolution has already copied into the graph.
    ///
    /// This keeps fields needed by analysis, health, security, LSP, coverage,
    /// and hash drift checks, while dropping vectors that otherwise duplicate
    /// data owned by `ResolvedModule` or already credited into the module graph.
    pub fn release_resolution_payload(&mut self) {
        self.prepare_analysis_facts();
        Self::release_vec(&mut self.dynamic_imports);
        Self::release_vec(&mut self.require_calls);
        Self::release_boxed_slice(&mut self.package_path_references);
        Self::release_boxed_slice(&mut self.type_package_references);
        Self::release_boxed_slice(&mut self.bin_path_references);
        Self::release_arc_slice(&mut self.whole_object_uses);
        Self::release_arc_slice(&mut self.import_binding_references);
        Self::release_vec(&mut self.unused_import_bindings);
        Self::release_vec(&mut self.type_referenced_import_bindings);
        Self::release_vec(&mut self.value_referenced_import_bindings);
        Self::release_vec(&mut self.namespace_object_aliases);
        Self::release_vec(&mut self.auto_import_candidates);
    }

    fn release_vec<T>(values: &mut Vec<T>) {
        *values = Vec::new();
    }

    fn release_boxed_slice<T>(values: &mut Box<[T]>) {
        *values = Box::default();
    }

    /// Drop this module's refcount; the allocation itself is freed only once
    /// the sharing `ResolvedModule` releases its clone too.
    fn release_arc_slice<T>(values: &mut Arc<[T]>) {
        *values = Arc::default();
    }
}

#[path = "extract_sink_values.rs"]
mod sink_values;
pub use sink_values::{SecurityUrlShape, SinkCallOptions, SinkLiteralValue, SinkObjectProperty};
#[path = "extract_env.rs"]
mod env;
pub use env::*;
#[path = "extract_security.rs"]
mod security;
pub use security::*;

/// One alias entry tying an exported object's dotted property path to a namespace import.
#[derive(Debug, Clone)]
pub struct NamespaceObjectAlias {
    /// Canonical export name.
    pub via_export_name: String,
    /// Dotted suffix of the property path relative to the export.
    pub suffix: String,
    /// Local name of the namespace import.
    pub namespace_local: String,
}

/// Compute a table of line-start byte offsets from source text.
#[must_use]
#[expect(
    clippy::cast_possible_truncation,
    reason = "source files are practically < 4GB"
)]
pub fn compute_line_offsets(source: &str) -> Vec<u32> {
    let mut offsets = vec![0u32];
    for (i, byte) in source.bytes().enumerate() {
        if byte == b'\n' {
            debug_assert!(
                u32::try_from(i + 1).is_ok(),
                "source file exceeds u32::MAX bytes: line offsets would overflow"
            );
            offsets.push((i + 1) as u32);
        }
    }
    offsets
}

/// Convert a byte offset to a 1-based line number and 0-based byte column.
#[must_use]
#[expect(
    clippy::cast_possible_truncation,
    reason = "line count is bounded by source size"
)]
pub fn byte_offset_to_line_col(line_offsets: &[u32], byte_offset: u32) -> (u32, u32) {
    let line_idx = match line_offsets.binary_search(&byte_offset) {
        Ok(idx) => idx,
        Err(idx) => idx.saturating_sub(1),
    };
    let line = line_idx as u32 + 1;
    let col = byte_offset - line_offsets[line_idx];
    (line, col)
}

/// True when `name` identifies a synthetic template-family complexity unit:
/// the per-file `<template>` unit every framework template scanner emits, or a
/// Svelte `<snippet:NAME>` unit. These units are exercised only through their
/// component, carry no directly measurable test coverage, and are therefore
/// excluded from the CRAP dimension. The `<component>` rollup is NOT part of
/// this family: it is an aggregate over class + template findings, not an
/// extracted unit.
#[must_use]
pub fn is_synthetic_template_unit(name: &str) -> bool {
    name == "<template>" || name.starts_with("<snippet:")
}

/// Placeholder name of a function expression that has no name from its
/// declaration, its binding, a key, or its call context.
pub const ANONYMOUS_FUNCTION_NAME: &str = "<anonymous>";

/// Placeholder name of an arrow function that has no name from its binding, a
/// key, or its call context.
pub const ANONYMOUS_ARROW_NAME: &str = "<arrow>";

/// True when `name` is the placeholder of a function without a name. Two
/// functions that share a placeholder are not the same function.
#[must_use]
pub fn is_anonymous_function_name(name: &str) -> bool {
    name == ANONYMOUS_FUNCTION_NAME || name == ANONYMOUS_ARROW_NAME
}

/// Emitted name of the synthetic module-scope complexity unit.
pub const MODULE_UNIT_NAME: &str = "<module>";

/// True when `name` identifies the synthetic per-file module-scope unit.
///
/// Extraction pushes one root frame per program so decision points outside
/// every function (an environment guard, a top-level `??` / `||` default, an
/// `?.` access on a config object) are counted instead of silently dropped.
/// The unit is emitted only when it actually branches, and it is
/// aggregate-only: it feeds vital signs, file scores, and branching
/// conservation, and never becomes a user-facing finding. "Extract a helper"
/// is not advice that applies to module scope, so the unit reports the
/// quantity without asking anyone to act on it.
///
/// Deliberately NOT part of [`is_synthetic_template_unit`]. That predicate
/// carries template-family CRAP, suppression, and display semantics, and
/// [`FileBranching::from_units`] filters on it: folding `<module>` in would
/// exclude module-scope branching from the branching totals, which is the
/// blind spot this unit exists to close.
#[must_use]
pub fn is_synthetic_module_unit(name: &str) -> bool {
    name == MODULE_UNIT_NAME
}

#[path = "extract_function_metrics.rs"]
mod function_metrics;
pub use function_metrics::*;

#[path = "extract_css_analytics.rs"]
mod css_analytics;
pub use css_analytics::*;

#[path = "extract_flags.rs"]
mod flags;
pub use flags::*;

/// The runtime mechanism used to load a module.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    bitcode::Encode,
    bitcode::Decode,
)]
#[repr(u8)]
pub enum ModuleLoadMechanism {
    /// ECMAScript module loading through imports, re-exports, or import globs.
    EsModule = 0,
    /// CommonJS module loading through `require()` or `require.context`.
    CommonJsRequire = 1,
}

#[path = "extract_load_kind.rs"]
mod load_kind;
pub use load_kind::ImportLoadKind;

/// Visibility tag from JSDoc/TSDoc comments that suppresses unused-export detection.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
#[repr(u8)]
pub enum VisibilityTag {
    /// No visibility tag present.
    #[default]
    None = 0,
    /// `@public` or `@api public` -- part of the public API surface.
    Public = 1,
    /// `@internal` -- exported for internal use (sister packages, build tools).
    Internal = 2,
    /// `@beta` -- public but unstable, may change without notice.
    Beta = 3,
    /// `@alpha` -- early preview, may change drastically without notice.
    Alpha = 4,
    /// `@expected-unused` -- intentionally unused, should warn when it becomes used.
    ExpectedUnused = 5,
}

impl VisibilityTag {
    /// Whether this tag permanently suppresses unused-export detection.
    /// `ExpectedUnused` is handled separately (conditionally suppresses,
    /// reports stale when the export becomes used).
    pub const fn suppresses_unused(self) -> bool {
        matches!(
            self,
            Self::Public | Self::Internal | Self::Beta | Self::Alpha
        )
    }

    /// For serde `skip_serializing_if`.
    pub fn is_none(&self) -> bool {
        matches!(self, Self::None)
    }
}

/// An export declaration.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ExportInfo {
    /// The exported name (named or default).
    pub name: ExportName,
    /// The local binding name, if different from the exported name.
    pub local_name: Option<String>,
    /// Whether this is a type-only export (`export type`).
    pub is_type_only: bool,
    /// Whether this export is registered through a runtime side effect at module load time.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub is_side_effect_used: bool,
    /// Visibility tag from JSDoc/TSDoc comment.
    #[serde(default, skip_serializing_if = "VisibilityTag::is_none")]
    pub visibility: VisibilityTag,
    /// Human-authored reason on `@expected-unused -- <reason>`, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_unused_reason: Option<String>,
    /// Whether the leading JSDoc carries a `@deprecated` tag. Orthogonal to
    /// `visibility`: `@public @deprecated` is a normal combination.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub deprecated: bool,
    /// Plain-text `@deprecated` message, capped at
    /// `DEPRECATED_REASON_MAX_CHARS` characters. `None` for a bare tag.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deprecated_reason: Option<Box<str>>,
    /// Source span of the export declaration.
    #[serde(serialize_with = "serialize_span")]
    pub span: Span,
    /// Members of this export (for enums, classes, and namespaces).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub members: Vec<MemberInfo>,
    /// The local name of the parent class from `extends` clause, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub super_class: Option<String>,
}

#[path = "extract_class_heritage.rs"]
mod class_heritage;
pub use class_heritage::ClassHeritageInfo;

/// One resolved property of an object-literal factory return: a dotted property
/// path mapped to the class the value at that path is an instance of.
///
/// `return { invoke: { orders: factory.ordersPage } }` records
/// `{ property_path: "invoke.orders", class_local_name: "OrdersPage" }`. The class
/// name is the factory module's own LOCAL name, resolved at analyze time through the
/// factory module's imports to the real class export. See issue #1858.
#[derive(
    Debug,
    Clone,
    serde::Serialize,
    serde::Deserialize,
    bitcode::Encode,
    bitcode::Decode,
    PartialEq,
    Eq,
)]
pub struct FactoryReturnObjectProperty {
    /// Dotted property path from the returned object literal (`orders`, `invoke.orders`).
    pub property_path: String,
    /// The property value's class local name within the factory module.
    pub class_local_name: String,
}

/// An exported factory function that returns an object literal whose property
/// values are class instances, joined to its public export name.
///
/// A cross-module `const ui = createUi(); ui.orders.member` consumer emits a
/// `FactoryReturnObjectPropertyAccess` fact; the analyze layer resolves `export_name`
/// through the consumer's imports to this module, matches `property_path`, and credits
/// `member` on the resolved class (gated on it being a class with members). See issue #1858.
#[derive(
    Debug,
    Clone,
    serde::Serialize,
    serde::Deserialize,
    bitcode::Encode,
    bitcode::Decode,
    PartialEq,
    Eq,
)]
pub struct FactoryReturnObjectShapeExport {
    /// Public export name (honors `export { createUi as createOrdersUi }`).
    pub export_name: String,
    /// Resolved `(property_path -> class_local_name)` entries for the returned literal.
    pub properties: Box<[FactoryReturnObjectProperty]>,
}

/// A module-scope declaration that can be used as a TypeScript type.
#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
pub struct LocalTypeDeclaration {
    /// Local declaration name.
    pub name: String,
    /// Declaration identifier span.
    #[serde(serialize_with = "serialize_span")]
    pub span: Span,
}

/// A reference from an exported symbol's public signature to a type name.
#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
pub struct PublicSignatureTypeReference {
    /// Exported symbol whose signature contains the reference.
    pub export_name: String,
    /// Referenced type name. Qualified names are reduced to their root identifier.
    pub type_name: String,
    /// Reference span.
    #[serde(serialize_with = "serialize_span")]
    pub span: Span,
    /// True when the reference comes from a `satisfies` clause. The clause
    /// checks the value but does not change the exported type, so the type is
    /// in use but is not a private type leak.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub from_satisfies: bool,
}

/// A member of an enum, class, or namespace.
#[derive(Debug, Clone, serde::Serialize)]
pub struct MemberInfo {
    /// Member name.
    pub name: String,
    /// The kind of member (enum, class method/property, or namespace member).
    pub kind: MemberKind,
    /// Source span of the member declaration.
    #[serde(serialize_with = "serialize_span")]
    pub span: Span,
    /// Whether this member has decorators (e.g., `@Column()`, `@Inject()`).
    /// Decorated members are used by frameworks at runtime and should not be
    /// flagged as unused class members, unless every decorator on the member
    /// is opted out via `FallowConfig.ignore_decorators`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub has_decorator: bool,
    /// Full dotted path of each decorator on this member, in source order.
    /// `@step("x")` stores `"step"`; `@ns.foo` stores `"ns.foo"`. Empty for
    /// undecorated members, Angular signal-initializer properties (which set
    /// `has_decorator` without a literal decorator AST node), and decorators
    /// whose expression is not an identifier ladder (the entry is the empty
    /// string in that case, treated as never-matching by the predicate).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub decorator_names: Vec<String>,
    /// True for a static method that returns a new instance of its class
    /// (`return new this()`, `return new C()`, or a declared return type `C`).
    /// Member accesses on the call result credit the class (#346, #387).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub is_instance_returning_static: bool,
    /// True for an instance method whose call result is an instance of the
    /// same class: the declared return type names the class, or the body ends
    /// with `return this`. The analysis walks fluent chains
    /// (`Class.factory().setX().setY()`) only through these methods (#387).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub is_self_returning: bool,
    /// Root identifier of the class `extends` expression, set only when the
    /// member has the `override` modifier: `Container` in
    /// `extends Container<Env>`, `Schema` in `extends Schema.TaggedError<E>()(...)`.
    /// The analysis credits the member when this root names code outside the
    /// project, because that code calls the overridden member.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub override_base: Option<String>,
    /// True for a class member with the TypeScript `protected` modifier. Only
    /// the `unused-protected-class-members` rule checks these members. Every
    /// other consumer that checks class members skips them.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub is_protected: bool,
    /// True for a `static` class member, which code with the class value reads.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub is_static: bool,
    /// True for `dispose` when the class declares `[Symbol.dispose]` or `[Symbol.asyncDispose]`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pairs_with_symbol_dispose: bool,
}

/// The kind of member.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    serde::Serialize,
    serde::Deserialize,
    bitcode::Encode,
    bitcode::Decode,
)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum MemberKind {
    /// A TypeScript enum member.
    EnumMember,
    /// A class method.
    ClassMethod,
    /// A class property.
    ClassProperty,
    /// A member exported from a TypeScript namespace.
    NamespaceMember,
    /// A member declared by a store object (Pinia `state` / `getters` /
    /// `actions` key, or a setup-store returned key). Cross-graph dead-member
    /// detection: a store member never accessed by any consumer project-wide.
    StoreMember,
}

/// A static member access expression (e.g., `Status.Active`, `MyClass.create()`).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, bitcode::Encode, bitcode::Decode)]
pub struct MemberAccess {
    /// The identifier being accessed (the import name).
    pub object: String,
    /// The member being accessed.
    pub member: String,
}

/// Direct export declarations that TypeScript treats as one merged symbol.
///
/// Spans identify the exact declaration slots without conflating unrelated
/// type/value declarations that happen to share a name.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, bitcode::Encode, bitcode::Decode)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct DeclarationMergeFact {
    /// Identifier spans for the declarations in this merge group.
    pub export_spans: Vec<(u32, u32)>,
}

/// A default import binding consumed outside a statically known member access.
///
/// Resolution decides whether the target is an object-shaped module such as a
/// CSS Module or a proven static CommonJS object map. Keeping this fact
/// target-agnostic lets aliases and extensionless specifiers use the resolved
/// target path without broadening ordinary default-import behavior.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, bitcode::Encode, bitcode::Decode)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct DefaultImportWholeObjectUseFact {
    /// Local binding name in the importing module.
    pub local_name: String,
}

#[path = "extract_semantic.rs"]
mod semantic;
pub use semantic::SemanticFact;

pub use crate::extract_namespace_member_uses::{HolderValueFact, NamespaceMemberUseFact};

/// A static member name that a runtime probe checks on an untyped value.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, bitcode::Encode, bitcode::Decode)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MemberNameProbeFact {
    /// The probed member name.
    pub name: String,
}

/// Deferred class provenance owned by the module declaring an object factory.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, bitcode::Encode, bitcode::Decode)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum FactoryPropertySource {
    /// A property path through a named type in the factory's module.
    TypedProperty {
        /// Named receiver type in the producer module.
        type_name: String,
        /// Remaining path through the receiver type.
        property_path: String,
    },
    /// A synchronous function call in the factory's module.
    FunctionReturn {
        /// Function binding in the producer module.
        callee_name: String,
    },
    /// A proven instance-returning static class method.
    StaticFactoryReturn {
        /// Class binding in the producer module.
        callee_object: String,
        /// Statically named instance-returning method.
        callee_method: String,
    },
    /// A class proven in the object factory's own module.
    LocalClass {
        /// Local class binding resolved relative to the factory module.
        class_local_name: String,
    },
    /// Return of the callback at this ordinal in the proven helper declaration.
    CallbackParameterReturn {
        /// Zero-based formal parameter ordinal.
        parameter_index: u32,
    },
}

/// Callback-origin member use, joined with the callee module's property source.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, bitcode::Encode, bitcode::Decode)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CallbackFactoryPropertyAccessFact {
    /// Callee import binding in the caller module.
    pub callee_name: String,
    /// Static export selected from a namespace import, if present.
    pub callee_member: Option<String>,
    /// Requested path in the wrapper's returned value.
    pub property_path: String,
    /// Requested member on the callback result.
    pub member: String,
    /// A property write permits only the canonical Vue getter contract.
    pub vue_only: bool,
    /// Ordinary assigned value, ignored by a canonical Vue setter contract.
    pub assigned_class: Option<String>,
    /// Callback ordinal and class origin interpreted in the caller module.
    pub arguments: Vec<CallbackFactoryArgument>,
}

/// Proven callback result owned by a factory consumer's module.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, bitcode::Encode, bitcode::Decode)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CallbackFactoryArgument {
    /// Zero-based actual argument ordinal.
    pub parameter_index: u32,
    /// Class binding resolved in the consumer module.
    pub class_local_name: String,
    /// This argument is an options object with a proven `get` callback.
    pub getter: bool,
    /// No mutable capture makes a lazy invocation select another class.
    pub lazy_safe: bool,
}

/// An exported object factory's deferred property receiver.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, bitcode::Encode, bitcode::Decode)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct FactoryReturnObjectPropertySourceFact {
    /// Public factory export name.
    pub factory_export_name: String,
    /// Path within the returned object, separate from any source property path.
    pub property_path: String,
    /// Names interpreted relative to this fact's owning module.
    pub source: FactoryPropertySource,
}

/// The load kind of the dynamic imports or patterns that start at `span_start`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, bitcode::Encode, bitcode::Decode)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ImportLoadKindOverrideFact {
    /// Byte offset of the call or `new` expression that records the edge.
    pub span_start: u32,
    /// The load kind of every edge that the expression records.
    pub kind: ImportLoadKind,
}

/// The Module Federation runtime function that a call names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, bitcode::Encode, bitcode::Decode)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub enum FederationRuntimeCall {
    /// `registerRemotes([{ name, entry }])`.
    RegisterRemotes,
    /// `loadRemote('remote/module')`.
    LoadRemote,
    /// `init({ remotes: [{ name, entry }] })`.
    /// Appended because bitcode encodes enum variants by ordinal.
    Init,
    /// `createInstance({ remotes: [{ name, entry }] })`.
    CreateInstance,
}

impl FederationRuntimeCall {
    /// The function name as the source writes it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::RegisterRemotes => "registerRemotes",
            Self::LoadRemote => "loadRemote",
            Self::Init => "init",
            Self::CreateInstance => "createInstance",
        }
    }
}

/// One remote that a Module Federation runtime call names.
///
/// `remote` is the remote alias when the argument is a static literal, and
/// `None` when the call receives a value that static analysis cannot read.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, bitcode::Encode, bitcode::Decode)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct FederationRuntimeRemoteFact {
    /// The runtime function the call names.
    pub call: FederationRuntimeCall,
    /// The remote alias the call names, or `None` for a non-literal argument.
    pub remote: Option<String>,
}

#[path = "extract_semantic_view.rs"]
mod semantic_view;
pub use semantic_view::*;

#[path = "extract_member_facts.rs"]
mod member_facts;
pub use member_facts::*;

#[path = "extract_components.rs"]
mod components;
pub use components::*;

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde serialize_with requires &T"
)]
fn serialize_span<S: serde::Serializer>(span: &Span, serializer: S) -> Result<S::Ok, S::Error> {
    use serde::ser::SerializeMap;
    let mut map = serializer.serialize_map(Some(2))?;
    map.serialize_entry("start", &span.start)?;
    map.serialize_entry("end", &span.end)?;
    map.end()
}

/// Export identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ExportName {
    /// A named export (e.g., `export const foo`).
    Named(String),
    /// The default export.
    Default,
}

impl ExportName {
    /// Compare against a string without allocating (avoids `to_string()`).
    #[must_use]
    pub fn matches_str(&self, s: &str) -> bool {
        match self {
            Self::Named(n) => n == s,
            Self::Default => s == "default",
        }
    }
}

impl std::fmt::Display for ExportName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Named(n) => write!(f, "{n}"),
            Self::Default => write!(f, "default"),
        }
    }
}

/// An import declaration.
#[derive(Debug, Clone)]
pub struct ImportInfo {
    /// The import specifier (e.g., `./utils` or `react`).
    pub source: String,
    /// How the symbol is imported (named, default, namespace, or side-effect).
    pub imported_name: ImportedName,
    /// The local binding name in the importing module.
    pub local_name: String,
    /// Whether this is a type-only import (`import type`).
    pub is_type_only: bool,
    /// Whether this whole-module import forwards type meanings only.
    ///
    /// Set for `export type *` and `export type * as ns` inside a
    /// `declare module '...'` body (issue #2375). Those forms record the same
    /// bindingless whole-module shape the plain ambient star records
    /// (issue #2357), but the star they stand for erases every value meaning,
    /// so the graph credits the target's star surface in the type namespace
    /// alone. Every other import leaves this false: `is_type_only` already
    /// decides their namespace on its own.
    pub is_type_only_star: bool,
    /// Whether this import originated from a CSS-context.
    pub from_style: bool,
    /// Source span of the import declaration.
    pub span: Span,
    /// Span of the source string literal used by the LSP to highlight the specifier.
    pub source_span: Span,
}

/// How a symbol is imported.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ImportedName {
    /// A named import (e.g., `import { foo }`).
    Named(String),
    /// A default import (e.g., `import React`).
    Default,
    /// A namespace import (e.g., `import * as utils`).
    Namespace,
    /// A side-effect import (e.g., `import './styles.css'`).
    SideEffect,
}

#[cfg(target_pointer_width = "64")]
const _: () = assert!(std::mem::size_of::<ExportInfo>() == 152);
#[cfg(target_pointer_width = "64")]
const _: () = assert!(std::mem::size_of::<ImportInfo>() == 96);
#[cfg(target_pointer_width = "64")]
const _: () = assert!(std::mem::size_of::<ExportName>() == 24);
#[cfg(target_pointer_width = "64")]
const _: () = assert!(std::mem::size_of::<ImportedName>() == 24);
#[cfg(target_pointer_width = "64")]
const _: () = assert!(std::mem::size_of::<MemberAccess>() == 48);
#[cfg(target_pointer_width = "64")]
const _: () = assert!(std::mem::size_of::<SemanticFact>() == 80);
#[cfg(target_pointer_width = "64")]
const _: () = assert!(std::mem::size_of::<SinkSite>() == 224);
#[cfg(target_pointer_width = "64")]
const _: () = assert!(std::mem::size_of::<ModuleInfo>() == 1624);
#[cfg(target_pointer_width = "64")]
const _: () = assert!(std::mem::size_of::<ImportBindingReference>() == 64);
#[cfg(target_pointer_width = "64")]
const _: () = assert!(std::mem::size_of::<TypeMemberTypeEntry>() == 72);

#[path = "extract_re_export.rs"]
mod re_export;
pub use re_export::ReExportInfo;

#[path = "extract_dynamic_pattern.rs"]
mod dynamic_pattern;
pub use dynamic_pattern::DynamicImportPattern;

#[path = "extract_dynamic_import.rs"]
mod dynamic_import;
pub use dynamic_import::DynamicImportInfo;

/// A `require()` call.
#[derive(Debug, Clone)]
pub struct RequireCallInfo {
    /// The require specifier.
    pub source: String,
    /// Source span of the `require()` call.
    pub span: Span,
    /// Span of the string-literal argument with quotes (`'./x'`), which anchors an
    /// `unresolved-import` diagnostic. `Span::default()` for a non-literal argument.
    pub source_span: Span,
    /// Names destructured from the `require()` result.
    pub destructured_names: Vec<String>,
    /// The `(key, local)` pair of each destructured identifier property:
    /// `const { exec: run } = require(...)` gives `("exec", "run")`.
    pub destructured_locals: Vec<(String, String)>,
    /// The local variable name for `const x = require(...)`.
    pub local_name: Option<String>,
    /// `true` for `import type X = require('...')`, the one require spelling
    /// TypeScript erases entirely: the emitted JavaScript contains no
    /// `require` call, so the target is a type-space reference and never a
    /// runtime dependency. Always `false` for `const x = require(...)`, which
    /// has no type-only spelling. Read by dependency classification so a
    /// type-only devDependency is not reported as production usage.
    pub is_type_only: bool,
}

/// Result of parsing all files, including incremental cache statistics.
pub struct ParseResult {
    /// Extracted module information for all successfully parsed files.
    pub modules: Vec<ModuleInfo>,
    /// Files discovered with stable IDs but unreadable by the parser.
    pub read_failures: Vec<SourceReadFailure>,
    /// Files whose extracted modules may be incomplete because of parse diagnostics.
    /// Analysis keeps findings visible and attaches caveats to affected findings.
    pub parse_degradations: Vec<SourceParseDegradation>,
    /// Number of files whose parse results were loaded from cache (unchanged).
    pub cache_hits: usize,
    /// Number of files that required a full parse (new or changed).
    pub cache_misses: usize,
    /// Summed wall-clock time of the actual AST parses across all rayon workers.
    pub parse_cpu_ms: f64,
    /// Files whose bytes were read from disk: every parse, plus every cache
    /// hit that had to compare the content hash.
    pub files_read: u64,
    /// Bytes of source read from disk across all files.
    pub source_bytes_read: u64,
    /// Source bytes that the CSS comment mask read across all parsed files.
    pub css_masked_bytes: u64,
    /// Files that the cache served from their metadata alone, without a read
    /// of the source, sorted by id. The stored fingerprint of each equals the
    /// metadata that the parse read, so a cache writer can keep it.
    pub metadata_hits: Vec<FileId>,
}

/// A discovered source that could not be read as UTF-8 text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceReadFailure {
    /// Stable discovery identity retained even though no module was produced.
    pub file_id: FileId,
    /// Absolute discovered source path.
    pub path: PathBuf,
    /// Underlying filesystem or UTF-8 decoding error.
    pub error: String,
}

/// A discovered source that was read but did not parse cleanly.
///
/// The module it produced is still analyzed: dropping it would turn one broken
/// file into project-wide silence. The point of carrying the degradation is
/// that the imports the file failed to parse credited nothing, so its targets
/// can be reported as unused with full confidence unless a consumer is told the
/// parse was partial.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceParseDegradation {
    /// Stable discovery identity of the degraded source.
    pub file_id: FileId,
    /// Absolute discovered source path.
    pub path: PathBuf,
    /// Number of parser diagnostics reported for the file.
    pub error_count: u32,
    /// `true` when the parser abandoned the file instead of recovering.
    pub panicked: bool,
    /// The nesting cap when the file nests too deep to parse.
    pub nesting_limit: Option<u32>,
}

#[path = "extract_effect.rs"]
mod effect;
pub use effect::*;
#[path = "extract_calls.rs"]
mod calls;
pub use calls::*;
#[path = "extract_function_contracts.rs"]
mod function_contracts;
pub use function_contracts::*;
#[path = "extract_factory.rs"]
mod factory;
pub use factory::*;
#[path = "extract_component_contracts.rs"]
mod component_contracts;
pub use component_contracts::*;
#[path = "extract_type_members.rs"]
mod type_members;
pub use type_members::*;

#[path = "extract_template_members.rs"]
mod extract_template_members;
use extract_template_members::angular_template_member_names_from_parts;

#[cfg(test)]
#[path = "extract_tests.rs"]
mod tests;
