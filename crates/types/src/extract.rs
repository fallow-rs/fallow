//! Module extraction types.

use oxc_span::Span;

use crate::discover::FileId;
use crate::suppress::{Suppression, UnknownSuppressionKind};

/// Extracted module information from a single file.
#[derive(Debug, Clone)]
pub struct ModuleInfo {
    /// Unique identifier for this file.
    pub file_id: FileId,
    /// All export declarations in this module.
    pub exports: Vec<ExportInfo>,
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
    /// Static member access expressions (e.g., `Status.Active`).
    pub member_accesses: Vec<MemberAccess>,
    /// Typed semantic facts produced by extraction for cross-layer analysis.
    ///
    /// This carries facts that were previously encoded as synthetic
    /// `member_accesses` strings. Extraction and analysis now use typed facts.
    pub semantic_facts: Box<[SemanticFact]>,
    /// Identifiers used in whole-object access patterns.
    pub whole_object_uses: Box<[String]>,
    /// Whether this module uses CommonJS exports.
    pub has_cjs_exports: bool,
    /// Whether this module declares an Angular component `templateUrl`.
    pub has_angular_component_template_url: bool,
    /// xxh3 hash of the file content for incremental caching.
    pub content_hash: u64,
    /// Inline suppression directives parsed from comments.
    pub suppressions: Vec<Suppression>,
    /// Suppression tokens that did not parse to any known `IssueKind`.
    /// Surfaced as `StaleSuppression` findings via `find_stale` so users see
    /// typos or obsolete kind names instead of having the entire marker
    /// silently discarded. See issue #449.
    pub unknown_suppression_kinds: Vec<UnknownSuppressionKind>,
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
    /// Heritage metadata for exported classes that declare `implements`.
    pub class_heritage: Vec<ClassHeritageInfo>,
    /// Exported free-function factories that provably return one class instance
    /// (`export function useApi() { return new RESTApi() }`). Origin-module proof
    /// that an exported function returns a class instance, so a cross-module
    /// `const x = useApi(); x.member` consumer can credit the returned class.
    /// See issue #1441 (Part A).
    pub exported_factory_returns: Box<[FactoryReturnExport]>,
    /// Named-type property types declared by this module's top-level interfaces
    /// and type-literal aliases (`interface Opts { c: OptDep }`). Names are
    /// local to this module; resolution is deferred to analyze time. Consumed
    /// by the `unused-class-member` typed-property-hop join and the Playwright
    /// fixture-type resolution. See issue #1785.
    pub type_member_types: Box<[TypeMemberTypeEntry]>,
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
    /// Known defensive control call sites found in this module. Consumed only by
    /// the `fallow security --surface` agent JSON path.
    pub security_control_sites: Vec<SecurityControlSite>,
    /// Statically flattenable callee paths invoked in this module, deduped per
    /// unique path (first occurrence wins). Consumed by the
    /// `boundaries.calls.forbidden` detector. Captured unconditionally because
    /// extraction is config-blind; the per-module cost is bounded by the
    /// unique-callee count.
    pub callee_uses: Vec<CalleeUse>,
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
        Self::release_boxed_slice(&mut self.whole_object_uses);
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
}

/// Defensive control family detected on a source to sink path.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    serde::Serialize,
    serde::Deserialize,
    bitcode::Encode,
    bitcode::Decode,
)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum SecurityControlKind {
    /// Sanitization or escaping before a sink.
    Sanitization,
    /// Input validation or schema parsing.
    Validation,
    /// Authentication check or middleware.
    Authentication,
    /// Authorization or permission check.
    Authorization,
}

/// A known defensive control call site.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, bitcode::Encode, bitcode::Decode)]
pub struct SecurityControlSite {
    /// Control family.
    pub kind: SecurityControlKind,
    /// Flattened callee path or a stable synthetic name for guard-derived
    /// controls.
    pub callee_path: String,
    /// Byte offset of the control span start.
    pub span_start: u32,
    /// Byte offset of the control span end.
    pub span_end: u32,
}

/// Sanitizer output domain. Kept intentionally narrow so a sanitizer for one
/// domain cannot suppress a different sink family.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    serde::Serialize,
    serde::Deserialize,
    bitcode::Encode,
    bitcode::Decode,
)]
pub enum SanitizerScope {
    /// HTML markup sanitized by DOMPurify-compatible APIs.
    Html,
    /// URL or redirect target checked against a literal-backed allowlist.
    Url,
    /// Path value checked against a high-confidence containment guard.
    Path,
    /// SQL identifier quoted with a helper that doubles embedded identifier quotes.
    SqlIdentifier,
}

/// A captured sink argument that is itself a recognized sanitizer call.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, bitcode::Encode, bitcode::Decode)]
pub struct SanitizedSinkArg {
    /// Byte offset of the owning sink span start.
    pub span_start: u32,
    /// The positional argument index on the owning sink.
    pub arg_index: u32,
    /// The sanitizer output domain for this argument.
    pub scope: SanitizerScope,
}

/// A local binding tied to the flattened member-access path it was initialized
/// from. The analyze layer matches `source_path` against the data-driven source
/// catalogue; when it matches, `local` is treated as carrying untrusted input.
///
/// Captured for two shapes: a direct assignment (`const id = req.query.id` ->
/// `{ local: "id", source_path: "req.query" }`, the literal-key tail dropped so
/// the path matches a catalogue prefix) and an object destructure
/// (`const { id } = req.query` -> `{ local: "id", source_path: "req.query" }`).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, bitcode::Encode, bitcode::Decode)]
pub struct TaintedBinding {
    /// The local binding name introduced by the declarator.
    pub local: String,
    /// The flattened object member-access path the binding was sourced from.
    pub source_path: String,
    /// Byte offset of the source read (the member-access expression the binding
    /// was sourced from), so the analyze layer can anchor a taint trace's source
    /// node at the real read line instead of the module import line. Stored as a
    /// `u32` (not `Span`) to stay bitcode-encodable for the cache. `0` when no
    /// concrete read expression is available (synthetic framework-param /
    /// helper-return bindings), in which case the analyze layer falls back to the
    /// sink site rather than claiming a spurious line.
    pub source_span_start: u32,
}

/// Why a sink-shaped callee could not be flattened into a static catalogue
/// path.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    serde::Serialize,
    serde::Deserialize,
    bitcode::Encode,
    bitcode::Decode,
)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum SkippedSecurityCalleeReason {
    /// A computed member access such as `client[method](input)`.
    ComputedMember,
    /// A dynamic non-member callee such as `(factory())(input)`.
    DynamicDispatch,
    /// An assignment target whose object could not be flattened.
    UnsupportedAssignmentObject,
}

/// Syntactic expression shape for a skipped security callee.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    serde::Serialize,
    serde::Deserialize,
    bitcode::Encode,
    bitcode::Decode,
)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum SkippedSecurityCalleeExpressionKind {
    /// `obj.prop(...)`.
    StaticMemberExpression,
    /// `obj[prop](...)`.
    ComputedMemberExpression,
    /// A bare identifier or private identifier callee.
    Identifier,
    /// Any other call-like expression that cannot be represented compactly.
    Other,
}

/// Span-only diagnostic for a skipped security callee inside one module.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, bitcode::Encode, bitcode::Decode)]
pub struct SkippedSecurityCalleeSite {
    /// Why the callee was skipped.
    pub reason: SkippedSecurityCalleeReason,
    /// Compact expression shape of the skipped callee.
    pub expression_kind: SkippedSecurityCalleeExpressionKind,
    /// Start byte offset of the skipped callee expression.
    pub span_start: u32,
    /// End byte offset of the skipped callee expression.
    pub span_end: u32,
}

/// The syntactic shape of a captured security sink site. Category-blind: the
/// extractor records the shape and the dotted/bare callee path; the analyze
/// layer matches it against the data-driven catalogue. See
/// `crates/security/data/security_matchers.toml`.
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
pub enum SinkShape {
    /// A call to a bare identifier (e.g. `eval(x)`).
    Call,
    /// A call to a dotted member path (e.g. `child_process.exec(x)`).
    MemberCall,
    /// An assignment to a member target (e.g. `el.innerHTML = x`).
    MemberAssign,
    /// A tagged template expression (e.g. ``sql`...${x}...` ``).
    TaggedTemplate,
    /// A JSX attribute value (e.g. `dangerouslySetInnerHTML={x}`).
    JsxAttr,
    /// A constructor call (e.g. `new Function("return x")`).
    NewExpression,
    /// A static string literal assigned to a secret-shaped identifier or known
    /// provider credential prefix.
    SecretLiteral,
}

/// The shape of the argument captured at a sink site. Category-blind like
/// [`SinkShape`], but finer-grained: it lets the catalogue matcher require or
/// exclude specific argument shapes. The discriminator is what distinguishes an
/// unsafe SQL string concatenation or template-into-`.execute()` from a
/// safely-parameterized `` sql`${x}` `` tagged template, an object-literal
/// `.execute({ sql, args })` argument, or a literal-aware sink argument.
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
pub enum SinkArgKind {
    /// A template literal with at least one `${...}` substitution (e.g.
    /// `` `SELECT ${x}` ``). On a `tagged-template` shape this is the tag's
    /// quasi; on a `call`/`member-call` shape it is the positional argument.
    TemplateWithSubst,
    /// A binary `+` string concatenation (e.g. `"SELECT " + x`).
    Concat,
    /// An object literal (e.g. `.execute({ sql, args })`, the parameterized form).
    Object,
    /// A call expression argument (e.g. `query(buildSql())`).
    Call,
    /// A literal argument admitted by a literal-aware security matcher.
    Literal,
    /// A zero-argument sink captured because the callee itself is the signal.
    NoArg,
    /// Any other non-literal expression (bare identifier, member access, etc.).
    Other,
}

/// Static URL construction shape captured for URL-shaped security sinks.
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
#[serde(rename_all = "kebab-case")]
pub enum SecurityUrlShape {
    /// The sink target has a fixed origin, scheme, or relative root while only
    /// path or query components are dynamic.
    FixedOriginDynamicPath,
    /// The sink target's scheme or origin is dynamic or opaque.
    DynamicOrigin,
}

/// Literal values attached to literal-aware security sink captures.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    serde::Serialize,
    serde::Deserialize,
    bitcode::Encode,
    bitcode::Decode,
)]
pub enum SinkLiteralValue {
    /// A string literal value.
    String(String),
    /// An integer numeric literal value.
    Integer(i64),
    /// A boolean literal value.
    Boolean(bool),
    /// A null literal value.
    Null,
}

/// Static object-literal property metadata attached to a captured sink
/// argument. Nested object paths are flattened with dot-separated keys.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    serde::Serialize,
    serde::Deserialize,
    bitcode::Encode,
    bitcode::Decode,
)]
pub struct SinkObjectProperty {
    /// Static property name. Nested object properties use dot-separated paths.
    pub key: String,
    /// Literal property value when statically knowable.
    pub value: SinkLiteralValue,
}

/// A captured sink site. The visitor records every existing non-literal call /
/// member-assign / member-call / tagged-template / jsx-attr sink site, and a
/// small allowlist of literal-aware sites where the literal value is the signal.
/// It knows nothing about CWE categories.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, bitcode::Encode, bitcode::Decode)]
pub struct SinkSite {
    /// The syntactic shape of the sink site.
    pub sink_shape: SinkShape,
    /// The flattened dotted/bare callee or member path.
    pub callee_path: String,
    /// The positional argument index. For zero-argument captures this is 0.
    pub arg_index: u32,
    /// Whether the relevant argument is non-literal. Existing non-literal
    /// catalogue rows require this to remain true.
    pub arg_is_non_literal: bool,
    /// The finer-grained shape of the captured argument. Lets the catalogue
    /// require unsafe shapes (concat / template-with-substitution / literal /
    /// no-arg) and exclude safe ones (object literal, the parameterized form).
    /// See [`SinkArgKind`].
    pub arg_kind: SinkArgKind,
    /// Literal argument value for literal-aware rows.
    pub arg_literal: Option<SinkLiteralValue>,
    /// Risky regex fragment for structural ReDoS candidates.
    pub regex_pattern: Option<String>,
    /// Static object-literal properties for option-object rows.
    pub object_properties: Vec<SinkObjectProperty>,
    /// Static top-level object-literal keys, including keys whose values are not
    /// literal. Used by missing-option rows that only need key presence.
    pub object_property_keys: Vec<String>,
    /// Whether [`object_property_keys`](Self::object_property_keys) is complete.
    /// False for non-object arguments and object literals with spread or
    /// non-static keys, where a missing-key claim would be speculative.
    pub object_property_keys_complete: bool,
    /// Identifier names referenced anywhere inside the captured non-literal sink
    /// argument, or contextual names for zero-argument captures such as a
    /// token-like `Math.random()` assignment target. Deduped in source order.
    /// Used by the analyze layer to back-trace the sink argument to a known
    /// untrusted source or to apply narrow context gates. Intra-module,
    /// name-based, conservative; it is never a taint proof.
    pub arg_idents: Vec<String>,
    /// Flattened static member paths referenced inside the captured non-literal
    /// sink argument. Includes both the full path and source-object path for
    /// leaf reads (`process.env.SECRET` records `process.env.SECRET` and
    /// `process.env`) so direct source expressions can be matched without an
    /// intermediate local binding.
    pub arg_source_paths: Vec<String>,
    /// Byte offset of the sink span start. Stored as `u32` (not `Span`) so the
    /// struct is bitcode-encodable and can be persisted directly in the cache.
    pub span_start: u32,
    /// Byte offset of the sink span end.
    pub span_end: u32,
    /// The arg-0 URL string literal of a network-shaped call (`fetch`, `axios.*`,
    /// `got`, ...), captured so the `secret-to-network` category (#890) can carry
    /// a destination-host signal on its candidate: `Some(literal)` when the
    /// destination is a static string literal (almost always intended auth, e.g.
    /// the credential's own provider), `None` when it is dynamic (the suspicious
    /// case). `None` for non-call sinks and calls with no arg 0.
    pub url_arg_literal: Option<String>,
    /// URL construction shape for URL-like sink arguments when the extractor can
    /// classify it syntactically. `None` for non-URL sinks and URL expressions
    /// whose shape is not visible at the sink.
    pub url_shape: Option<SecurityUrlShape>,
}

impl SinkSite {
    /// Reconstruct the source span from the stored byte offsets.
    #[must_use]
    pub fn span(&self) -> Span {
        Span::new(self.span_start, self.span_end)
    }
}

/// Env var-name prefixes that frameworks inline into the client bundle by
/// convention. A read of one of these is normal and safe, so it does NOT count
/// as a secret source (issue #890). Shared by the extract layer (so public env
/// vars never become source signals) and the bespoke `client-server-leak` rule.
pub const PUBLIC_ENV_PREFIXES: &[&str] = &[
    "NEXT_PUBLIC_",
    "VITE_",
    "NUXT_PUBLIC_",
    "REACT_APP_",
    "PUBLIC_",
    "GATSBY_",
    "EXPO_PUBLIC_",
    "STORYBOOK_",
];

/// Exact env var names that are public by convention (no prefix).
pub const PUBLIC_ENV_EXACT: &[&str] = &["NODE_ENV"];

/// Env var-name tokens that usually describe public build or deployment
/// metadata rather than secrets. Secret-shaped names win over these tokens.
pub const PUBLIC_ENV_METADATA_TOKENS: &[&str] =
    &["BRANCH", "ENVIRONMENT", "MODE", "REF", "SHA", "TAG"];

/// Env var-name tokens that should keep a variable source-backed even when the
/// name also contains public metadata tokens such as `REF` or `SHA`.
pub const SECRET_ENV_TOKENS: &[&str] = &[
    "AUTH",
    "CREDENTIAL",
    "CREDENTIALS",
    "KEY",
    "PASS",
    "PASSWORD",
    "PRIVATE",
    "SECRET",
    "TOKEN",
];

fn env_name_has_token(name: &str, tokens: &[&str]) -> bool {
    name.split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter(|part| !part.is_empty())
        .any(|part| tokens.contains(&part))
}

/// Whether an env var name is public-by-convention (build-inlined into the
/// client bundle), and therefore not a secret.
#[must_use]
pub fn is_public_env_var(name: &str) -> bool {
    if PUBLIC_ENV_EXACT.contains(&name) || PUBLIC_ENV_PREFIXES.iter().any(|p| name.starts_with(p)) {
        return true;
    }
    env_name_has_token(name, PUBLIC_ENV_METADATA_TOKENS)
        && !env_name_has_token(name, SECRET_ENV_TOKENS)
}

/// Whether a flattened member path is a PUBLIC env-secret read
/// (`process.env.NEXT_PUBLIC_X`, `import.meta.env.VITE_Y`), which must not be
/// recorded as a secret source. Non-env paths (`req.query.id`) are never public.
#[must_use]
pub fn is_public_env_path(path: &str) -> bool {
    for object in ["process.env.", "import.meta.env."] {
        if let Some(var) = path.strip_prefix(object) {
            return is_public_env_var(var);
        }
    }
    false
}

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

/// Complexity metrics for a single function/method/arrow.
#[derive(Debug, Clone, serde::Serialize, bitcode::Encode, bitcode::Decode)]
pub struct FunctionComplexity {
    /// Function name (or `"<anonymous>"` for unnamed functions/arrows).
    pub name: String,
    /// 1-based line number where the function starts.
    pub line: u32,
    /// 0-based byte column where the function starts.
    pub col: u32,
    /// `McCabe` cyclomatic complexity (1 + decision points).
    pub cyclomatic: u16,
    /// `SonarSource` cognitive complexity (structural + nesting penalty).
    pub cognitive: u16,
    /// Number of lines in the function body.
    pub line_count: u32,
    /// Number of parameters (excluding TypeScript's `this` parameter).
    pub param_count: u8,
    /// Number of React hook calls (`useState` / `useEffect` / `useMemo` /
    /// `useCallback` / custom `use*`) made directly in this function's body.
    /// Non-zero only for React components/hooks; descriptive context surfaced in
    /// the hotspot drill-down, never a tunable threshold (anti-numerology).
    pub react_hook_count: u16,
    /// Maximum JSX element nesting depth reached in this function's body (the
    /// deepest chain of element-inside-element). `0` when the function renders
    /// no JSX. Descriptive context surfaced in the hotspot drill-down, never a
    /// tunable threshold (anti-numerology).
    pub react_jsx_max_depth: u16,
    /// Number of props destructured from this component's first parameter (the
    /// `{ a, b, c }` props object). `0` for non-component functions and for
    /// components taking a bare `props` identifier (not statically countable).
    /// Descriptive context surfaced in the hotspot drill-down, never a tunable
    /// threshold (anti-numerology).
    pub react_prop_count: u16,
    /// Content digest of the function's full-span source slice.
    pub source_hash: Option<String>,
    /// Per-decision-point breakdown explaining WHICH constructs drove the
    /// cyclomatic and cognitive scores. One entry per increment event (an `if`
    /// emits one cyclomatic and one cognitive entry at the same line, because
    /// the two metrics accrue at different granularities). Always computed and
    /// cached; surfaced in JSON only behind `health --complexity-breakdown`.
    pub contributions: Vec<ComplexityContribution>,
}

/// Structural CSS metrics for a single style rule, computed from the parsed CSS
/// syntax tree. A rule is recorded only when it crosses a structural floor (an
/// id selector, a complex selector, a `!important` declaration, or deep
/// nesting), so the vector stays bounded on normal stylesheets.
///
/// Not persisted in the extraction cache: `fallow health` computes these
/// on demand from the CSS source, so there is no `bitcode` derive.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CssRuleMetric {
    /// 1-based line of the rule's first selector.
    pub line: u32,
    /// 1-based column of the rule's first selector.
    pub col: u32,
    /// Specificity component `a` (id selectors), max across the rule's selectors.
    pub specificity_a: u16,
    /// Specificity component `b` (class / attribute / pseudo-class selectors).
    pub specificity_b: u16,
    /// Specificity component `c` (type / pseudo-element selectors).
    pub specificity_c: u16,
    /// Largest selector component count across the rule's selector list.
    pub complexity: u16,
    /// Declaration count in the rule (normal plus `!important`).
    pub declaration_count: u16,
    /// `!important` declaration count in the rule.
    pub important_count: u16,
    /// Style-rule nesting depth (0 = top level).
    pub nesting_depth: u8,
}

/// A style rule's declaration-block fingerprint and location, for cross-file
/// duplicate-block detection. Only rules with a meaningful number of
/// declarations are recorded (small blocks repeat legitimately). Internal
/// staging only: this is consumed in-process by the health layer to build the
/// grouped `duplicate_declaration_blocks` output and is never serialized.
#[derive(Debug, Clone)]
pub struct CssDeclarationBlock {
    /// xxh3 fingerprint over the rule's normalized (sorted, `!important`-tagged)
    /// declaration set.
    pub fingerprint: u64,
    /// 1-based line of the rule's first selector.
    pub line: u32,
    /// Declaration count in the rule (normal plus `!important`).
    pub declaration_count: u16,
}

/// Located raw styling value authored directly in CSS rather than via a
/// custom property or design-token helper. Internal staging for the health
/// layer; public output adds actions and confidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CssRawStyleValue {
    /// Value axis, e.g. `color`, `font-size`, `line-height`, `radius`, or `shadow`.
    pub axis: String,
    /// CSS property where the value appears.
    pub property: String,
    /// Rendered declaration value.
    pub value: String,
    /// 1-based line of the containing style rule.
    pub line: u32,
}

/// Located CSS custom-property definition with its rendered value. Internal
/// staging for design-token reuse suggestions in the health layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CssCustomPropertyDefinition {
    /// Custom property name, including the leading `--`.
    pub name: String,
    /// Rendered custom property value.
    pub value: String,
    /// 1-based line of the containing style rule.
    pub line: u32,
}

/// Stylesheet-level structural CSS analytics, computed from the parsed CSS
/// syntax tree. Feeds `fallow health` penalty weights and located findings,
/// never a standalone CSS score.
#[derive(Debug, Clone, Default, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CssAnalytics {
    /// Total declarations across every style rule (normal plus `!important`).
    pub total_declarations: u32,
    /// Total `!important` declarations across every style rule.
    pub important_declarations: u32,
    /// Number of style rules.
    pub rule_count: u32,
    /// Number of style rules with no declarations.
    pub empty_rule_count: u32,
    /// Deepest style-rule nesting depth observed (0 = no nesting).
    pub max_nesting_depth: u8,
    /// Rules that crossed the structural floor, in source order. Bounded; see
    /// [`Self::notable_truncated`]. The scalar aggregates above always reflect
    /// the full stylesheet regardless of truncation.
    pub notable_rules: Vec<CssRuleMetric>,
    /// `true` when more rules crossed the structural floor than `notable_rules`
    /// retains (compiled utility CSS can emit thousands of `!important` rules),
    /// so consumers can note that per-rule findings were capped.
    pub notable_truncated: bool,
    /// Distinct color VALUES in the stylesheet, sorted (a palette-size /
    /// design-token-sprawl signal). The parser canonicalizes notation, so the
    /// authored format is NOT preserved: `red`, `#f00`, `#ff0000`, and
    /// `rgb(255,0,0)` all collapse to one entry, and every legacy sRGB notation
    /// renders as hex. Notation-MIXING (hex vs rgb vs hsl) is therefore not
    /// detectable from this set; it would need a separate raw-token pass.
    pub colors: Vec<String>,
    /// Distinct `font-size` declaration values in the stylesheet, sorted.
    pub font_sizes: Vec<String>,
    /// Distinct `z-index` declaration values in the stylesheet, sorted.
    pub z_indexes: Vec<String>,
    /// Distinct `box-shadow` declaration values in the stylesheet, sorted. A
    /// high count signals an uncontrolled shadow scale (design-token sprawl).
    pub box_shadows: Vec<String>,
    /// Distinct `border-radius` declaration values in the stylesheet, sorted.
    pub border_radii: Vec<String>,
    /// Distinct `line-height` declaration values in the stylesheet, sorted.
    pub line_heights: Vec<String>,
    /// Bounded located raw styling values that bypass custom properties or
    /// token helpers. These are conservative declaration-level candidates for
    /// audit introduced-vs-base gating.
    #[serde(skip)]
    #[cfg_attr(feature = "schema", schemars(skip))]
    pub raw_style_values: Vec<CssRawStyleValue>,
    /// Located custom-property definitions with values. Internal staging
    /// consumed by the health layer for nearest-token suggestions.
    #[serde(skip)]
    #[cfg_attr(feature = "schema", schemars(skip))]
    pub custom_property_definitions: Vec<CssCustomPropertyDefinition>,
    /// Distinct custom properties (`--x`) DEFINED in the stylesheet, sorted.
    pub defined_custom_properties: Vec<String>,
    /// Distinct custom properties REFERENCED via `var()` in the stylesheet.
    pub referenced_custom_properties: Vec<String>,
    /// Distinct `@keyframes` names DEFINED in the stylesheet, sorted.
    pub defined_keyframes: Vec<String>,
    /// Distinct `@keyframes` names REFERENCED via `animation` / `animation-name`.
    pub referenced_keyframes: Vec<String>,
    /// Distinct custom properties REGISTERED via an `@property` rule, sorted.
    pub registered_custom_properties: Vec<String>,
    /// Distinct cascade layers DECLARED (via `@layer a, b;` statements or named
    /// `@layer a { }` blocks), sorted.
    pub declared_layers: Vec<String>,
    /// Distinct cascade layers POPULATED by a named `@layer a { }` block, sorted.
    /// A layer declared but never populated (and not imported into) is a
    /// cleanup candidate.
    pub populated_layers: Vec<String>,
    /// Distinct font families DECLARED by an `@font-face` rule in the stylesheet,
    /// sorted. A declared family referenced by no `font-family` anywhere is a
    /// dead web-font payload (cleanup candidate).
    pub defined_font_faces: Vec<String>,
    /// Distinct font families REFERENCED via `font-family` / `font` in the
    /// stylesheet, sorted (generic keywords like `serif` excluded).
    pub referenced_font_families: Vec<String>,
    /// Per-rule declaration-block fingerprints for rules at or above the minimum
    /// block size, used to detect duplicate declaration blocks across the
    /// project. Internal staging consumed by the health layer; never serialized
    /// (the public output is the grouped `duplicate_declaration_blocks`).
    #[serde(skip)]
    #[cfg_attr(feature = "schema", schemars(skip))]
    pub declaration_blocks: Vec<CssDeclarationBlock>,
}

/// Which complexity metric a [`ComplexityContribution`] adds to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, bitcode::Encode, bitcode::Decode)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum ComplexityMetric {
    /// `McCabe` cyclomatic complexity (independent execution paths).
    Cyclomatic,
    /// `SonarSource` cognitive complexity (structural + nesting penalty).
    Cognitive,
}

/// The syntactic construct that produced a single complexity increment.
///
/// Mirrors `SonarSource` cognitive-complexity vocabulary where it overlaps.
/// `Case` means a `case` label carrying a test; a bare `default` adds nothing
/// to cyclomatic complexity and so produces no contribution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, bitcode::Encode, bitcode::Decode)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum ComplexityContributionKind {
    /// An `if` condition.
    If,
    /// A bare `else` branch (cognitive only).
    Else,
    /// An `else if` continuation (both metrics: cyclomatic +1, cognitive flat
    /// +1 with no nesting penalty).
    ElseIf,
    /// A `?:` conditional (ternary) expression.
    Ternary,
    /// A logical `&&` operator.
    LogicalAnd,
    /// A logical `||` operator.
    LogicalOr,
    /// A `??` nullish-coalescing operator.
    NullishCoalescing,
    /// A logical assignment operator (`&&=`, `||=`, `??=`); cyclomatic only.
    LogicalAssignment,
    /// An optional-chaining link (`?.`); cyclomatic only.
    OptionalChain,
    /// A `for` loop.
    For,
    /// A `for...in` loop.
    ForIn,
    /// A `for...of` loop.
    ForOf,
    /// A `while` loop.
    While,
    /// A `do...while` loop.
    DoWhile,
    /// A `switch` statement (cognitive only; each `case` adds cyclomatic).
    Switch,
    /// A `case` label carrying a test (cyclomatic only).
    Case,
    /// A `catch` clause.
    Catch,
    /// A labeled `break` (cognitive only).
    LabeledBreak,
    /// A labeled `continue` (cognitive only).
    LabeledContinue,
    /// Legacy JSX-depth contribution kind kept for schema compatibility. Current
    /// extraction records JSX nesting as descriptive `react_jsx_max_depth`
    /// context and does not emit this kind for layout depth.
    JsxDepth,
    /// React hook density (cognitive only). One contribution per hook call in a
    /// component body (`useState` / `useEffect` / `useMemo` / `useCallback` /
    /// custom `use*`); a hook-heavy component accrues cognitive load the same way
    /// branching does.
    HookDensity,
    /// React prop count past the comfortable floor (cognitive only). A component
    /// destructuring many props is doing many things; the props beyond the floor
    /// fold into cognitive so a wide-interface component surfaces as a hotspot.
    PropCount,
}

/// A single complexity increment, located at its source line/column.
///
/// `weight` is the amount this construct added to `metric`; for nested
/// cognitive increments `weight == 1 + nesting`. Consumers that render inline
/// (the VS Code editor breakdown) group contributions by `line` and sum the
/// weights, deferring the per-kind list to a hover.
#[derive(Debug, Clone, serde::Serialize, bitcode::Encode, bitcode::Decode)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ComplexityContribution {
    /// 1-based line number where the construct begins.
    pub line: u32,
    /// 0-based byte column where the construct begins.
    pub col: u32,
    /// Which metric this increment contributes to.
    pub metric: ComplexityMetric,
    /// The syntactic construct responsible for the increment.
    pub kind: ComplexityContributionKind,
    /// The amount added to `metric` at this site (`1 + nesting` for nested
    /// cognitive increments, otherwise `1`).
    pub weight: u16,
    /// The nesting depth at the increment site (`0` when not nested). Lets a
    /// consumer explain a cognitive `+3` as "+1 base, +2 nesting".
    pub nesting: u16,
}

/// The kind of feature flag pattern detected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, bitcode::Encode, bitcode::Decode)]
pub enum FlagUseKind {
    /// `process.env.FEATURE_X` pattern.
    EnvVar,
    /// SDK function call like `useFlag('name')`.
    SdkCall,
    /// Config object access like `config.features.x`.
    ConfigObject,
}

/// A feature flag use site.
#[derive(Debug, Clone, bitcode::Encode, bitcode::Decode)]
pub struct FlagUse {
    /// Flag identifier.
    pub flag_name: String,
    /// Detection kind.
    pub kind: FlagUseKind,
    /// 1-based line number.
    pub line: u32,
    /// 0-based byte column offset.
    pub col: u32,
    /// Start byte offset of the guarded block.
    pub guard_span_start: Option<u32>,
    /// End byte offset of the guarded block.
    pub guard_span_end: Option<u32>,
    /// SDK/provider name.
    pub sdk_name: Option<String>,
}

const _: () = assert!(std::mem::size_of::<FlagUse>() <= 96);

/// A dynamic import with a partially resolved pattern.
#[derive(Debug, Clone)]
pub struct DynamicImportPattern {
    /// Static prefix of the import path (e.g., "./locales/"). May contain glob characters.
    pub prefix: String,
    /// Static suffix of the import path (e.g., ".json"), if any.
    pub suffix: Option<String>,
    /// Source span in the original file.
    pub span: Span,
}

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

/// Additional heritage metadata for an exported class.
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
pub struct ClassHeritageInfo {
    /// Export name (`default` for default-exported classes).
    pub export_name: String,
    /// Parent class name from the `extends` clause, if any.
    pub super_class: Option<String>,
    /// Interface names from the class `implements` clause.
    pub implements: Vec<String>,
    /// Typed instance bindings used to resolve member-access chains in external templates.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub instance_bindings: Vec<(String, String)>,
}

/// An exported free-function factory proven to return one class instance.
///
/// `export function useApi() { return new RESTApi() }` records
/// `FactoryReturnExport { export_name: "useApi", class_local_name: "RESTApi" }`.
/// The `class_local_name` is the factory module's own LOCAL name, resolved at
/// analyze time through that module's imports/exports to the real class export,
/// so a cross-module `const x = useApi(); x.member` consumer credits the class
/// across the boundary. See issue #1441 (Part A).
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
pub struct FactoryReturnExport {
    /// Public export name (honors `export { useApi as useRestApi }`).
    pub export_name: String,
    /// The returned class's local name within the factory module.
    pub class_local_name: String,
}

/// A named-type property whose declared type is a named type reference.
///
/// `interface Opts { c: OptDep }` (or `type Opts = { c: OptDep }`) records
/// `TypeMemberTypeEntry { type_name: "Opts", property: "c", property_type: "OptDep" }`.
/// Both `type_name` and `property_type` are the DECLARING module's own local
/// names; resolution through that module's imports/exports is deferred to
/// analyze time, mirroring `FactoryReturnExport.class_local_name`. Consumed by
/// the `unused-class-member` typed-property-hop join so a consumer's
/// `this.opts.c.optM()` credits `OptDep.optM` across module boundaries.
/// See issue #1785.
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
pub struct TypeMemberTypeEntry {
    /// Local interface or type-alias name declaring the property.
    pub type_name: String,
    /// Property name declared on the type.
    pub property: String,
    /// The property's declared type name (local to the declaring module).
    pub property_type: String,
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
    /// True when this is a static class method that returns a fresh instance
    /// of the same class: either via `return new this()` / `return new
    /// <SameClassName>()` in the body's last statement, or via a declared
    /// return type matching the class name. Consumers calling such a static
    /// method receive an instance, so the call result's member accesses are
    /// credited against the class. See issues #346, #387.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub is_instance_returning_static: bool,
    /// True when this is an instance class method whose call result is an
    /// instance of the same class. Qualifies when the declared return type
    /// matches the class name (`setX(): EventBuilder { ... }`) or when the
    /// body's last statement is `return this`. The analyze layer walks fluent
    /// chains (`Class.factory().setX().setY()`) only through methods carrying
    /// this flag, so the chain stops at a non-self-returning method like
    /// `.build()`. See issue #387.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub is_self_returning: bool,
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

/// A typed extraction fact for cross-layer analysis.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, bitcode::Encode, bitcode::Decode)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SemanticFact {
    /// A class member referenced from an Angular template, host binding, or
    /// component metadata entry.
    AngularTemplateMemberAccess(AngularTemplateMemberAccessFact),
    /// An Angular component field whose value is an array of a class.
    AngularComponentFieldArrayType(AngularComponentFieldArrayTypeFact),
    /// An Angular component spreads `this` into an object literal, so component
    /// input/output usage is opaque.
    AngularThisSpread(AngularThisSpreadFact),
    /// A member access on a value returned by an imported static factory call.
    FactoryCallMemberAccess(FactoryCallMemberAccessFact),
    /// A member access on a value returned by an imported free-function factory
    /// (`const x = importedFactory(); x.member`). See issue #1441 (Part A).
    FactoryFnMemberAccess(FactoryFnMemberAccessFact),
    /// A member access reached through a property of a value whose declared
    /// type is an imported named type (`this.opts.c.optM()` where `opts` is
    /// typed by an imported interface). See issue #1785.
    TypedPropertyMemberAccess(TypedPropertyMemberAccessFact),
    /// A member access on a fluent chain rooted at an imported static factory.
    FluentChainMemberAccess(FluentChainMemberAccessFact),
    /// A member access on a fluent chain rooted at a `new` expression.
    FluentChainNewMemberAccess(FluentChainNewMemberAccessFact),
    /// A member access on a Playwright fixture object inside a test callback.
    PlaywrightFixtureUse(PlaywrightFixtureUseFact),
    /// A Playwright fixture definition declared by a typed `test.extend<T>()`.
    PlaywrightFixtureDefinition(PlaywrightFixtureDefinitionFact),
    /// A Playwright fixture wrapper alias declared by `mergeTests` or `.extend`.
    PlaywrightFixtureAlias(PlaywrightFixtureAliasFact),
    /// A nested Playwright fixture binding declared by a fixture type alias.
    PlaywrightFixtureType(PlaywrightFixtureTypeFact),
    /// An exported value whose runtime instance targets a local class or interface.
    InstanceExportBinding(InstanceExportBindingFact),
    /// A dynamic custom-element tag render that makes static Lit tag credit opaque.
    DynamicCustomElementRender(DynamicCustomElementRenderFact),
}

/// Iterate Angular template member names from typed semantic facts.
fn angular_template_member_names_from_parts(
    semantic_facts: &[SemanticFact],
) -> impl Iterator<Item = &str> {
    semantic_facts.iter().filter_map(|fact| {
        if let SemanticFact::AngularTemplateMemberAccess(access) = fact {
            Some(access.member.as_str())
        } else {
            None
        }
    })
}

/// Iterate Angular template member names from a module's typed facts.
pub fn angular_template_member_names(module: &ModuleInfo) -> impl Iterator<Item = &str> {
    angular_template_member_names_from_parts(&module.semantic_facts)
}

/// Return true when the fact slice contains any Angular template member
/// reference.
#[must_use]
fn has_angular_template_members_from_parts(semantic_facts: &[SemanticFact]) -> bool {
    angular_template_member_names_from_parts(semantic_facts)
        .next()
        .is_some()
}

/// Return true when the module contains any Angular template member reference.
#[must_use]
pub fn has_angular_template_members(module: &ModuleInfo) -> bool {
    has_angular_template_members_from_parts(&module.semantic_facts)
}

/// Return true when a module spreads `this` in Angular template context.
#[must_use]
pub fn has_angular_this_spread(module: &ModuleInfo) -> bool {
    SemanticFactView::new(&module.semantic_facts, &module.member_accesses).has_angular_this_spread()
}

/// Return true when a module contains a dynamic custom-element render.
#[must_use]
pub fn has_dynamic_custom_element_render(module: &ModuleInfo) -> bool {
    module
        .semantic_facts
        .iter()
        .any(|fact| matches!(fact, SemanticFact::DynamicCustomElementRender(_)))
}

/// Typed-first view over semantic extraction facts.
///
/// Extraction populates `semantic_facts` directly. The `member_accesses` slice
/// remains available for consumers that need ordinary source member accesses,
/// but it is no longer decoded as a string protocol for semantic facts.
#[derive(Debug, Clone, Copy)]
pub struct SemanticFactView<'a> {
    semantic_facts: &'a [SemanticFact],
    member_accesses: &'a [MemberAccess],
}

impl<'a> SemanticFactView<'a> {
    /// Create a typed semantic fact view from current semantic facts plus
    /// ordinary source member accesses.
    #[must_use]
    pub const fn new(
        semantic_facts: &'a [SemanticFact],
        member_accesses: &'a [MemberAccess],
    ) -> Self {
        Self {
            semantic_facts,
            member_accesses,
        }
    }

    /// Iterate typed semantic facts.
    pub fn facts(self) -> impl Iterator<Item = &'a SemanticFact> + 'a {
        self.semantic_facts.iter()
    }

    /// Iterate Angular template member references.
    pub fn angular_template_member_names(self) -> impl Iterator<Item = &'a str> + 'a {
        angular_template_member_names_from_parts(self.semantic_facts)
    }

    /// Collect Angular component field array-type facts.
    pub fn angular_component_field_array_types(self) -> Vec<AngularComponentFieldArrayTypeFact> {
        angular_component_field_array_type_facts(self.semantic_facts)
            .cloned()
            .collect()
    }

    /// Return true when any Angular template member reference exists.
    #[must_use]
    pub fn has_angular_template_members(self) -> bool {
        self.angular_template_member_names().next().is_some()
    }

    /// Return true when a module spreads `this` in Angular template context.
    #[must_use]
    pub fn has_angular_this_spread(self) -> bool {
        self.semantic_facts
            .iter()
            .any(|fact| matches!(fact, SemanticFact::AngularThisSpread(_)))
    }

    /// Iterate ordinary source member accesses.
    pub fn ordinary_member_accesses(self) -> impl Iterator<Item = &'a MemberAccess> + 'a {
        self.member_accesses.iter()
    }

    /// Collect instance-export binding facts.
    pub fn instance_export_bindings(self) -> Vec<InstanceExportBindingFact> {
        instance_export_binding_facts(self.semantic_facts)
            .cloned()
            .collect()
    }

    /// Collect static factory call member facts.
    pub fn factory_call_member_accesses(self) -> Vec<FactoryCallMemberAccessFact> {
        factory_call_member_access_facts(self.semantic_facts)
            .cloned()
            .collect()
    }

    /// Collect free-function factory-return member facts.
    pub fn factory_fn_member_accesses(self) -> Vec<FactoryFnMemberAccessFact> {
        factory_fn_member_access_facts(self.semantic_facts)
            .cloned()
            .collect()
    }

    /// Collect typed-property-hop member facts.
    pub fn typed_property_member_accesses(self) -> Vec<TypedPropertyMemberAccessFact> {
        typed_property_member_access_facts(self.semantic_facts)
            .cloned()
            .collect()
    }

    /// Collect static factory fluent-chain member facts.
    pub fn fluent_chain_member_accesses(self) -> Vec<FluentChainMemberAccessFact> {
        fluent_chain_member_access_facts(self.semantic_facts)
            .cloned()
            .collect()
    }

    /// Collect constructor-rooted fluent-chain member facts.
    pub fn fluent_chain_new_member_accesses(self) -> Vec<FluentChainNewMemberAccessFact> {
        fluent_chain_new_member_access_facts(self.semantic_facts)
            .cloned()
            .collect()
    }

    /// Collect Playwright fixture-use facts.
    pub fn playwright_fixture_uses(self) -> Vec<PlaywrightFixtureUseFact> {
        playwright_fixture_use_facts(self.semantic_facts)
            .cloned()
            .collect()
    }

    /// Collect Playwright fixture-definition facts.
    pub fn playwright_fixture_definitions(self) -> Vec<PlaywrightFixtureDefinitionFact> {
        playwright_fixture_definition_facts(self.semantic_facts)
            .cloned()
            .collect()
    }

    /// Collect Playwright fixture-alias facts.
    pub fn playwright_fixture_aliases(self) -> Vec<PlaywrightFixtureAliasFact> {
        playwright_fixture_alias_facts(self.semantic_facts)
            .cloned()
            .collect()
    }

    /// Collect Playwright fixture-type facts.
    pub fn playwright_fixture_types(self) -> Vec<PlaywrightFixtureTypeFact> {
        playwright_fixture_type_facts(self.semantic_facts)
            .cloned()
            .collect()
    }
}

/// Iterate ordinary whole-object uses.
pub fn ordinary_whole_object_uses(whole_object_uses: &[String]) -> impl Iterator<Item = &str> {
    whole_object_uses.iter().map(String::as_str)
}

/// Iterate typed instance-export binding facts.
fn instance_export_binding_facts(
    semantic_facts: &[SemanticFact],
) -> impl Iterator<Item = &InstanceExportBindingFact> {
    semantic_facts.iter().filter_map(|fact| {
        if let SemanticFact::InstanceExportBinding(access) = fact {
            Some(access)
        } else {
            None
        }
    })
}

fn angular_component_field_array_type_facts(
    semantic_facts: &[SemanticFact],
) -> impl Iterator<Item = &AngularComponentFieldArrayTypeFact> {
    semantic_facts.iter().filter_map(|fact| {
        if let SemanticFact::AngularComponentFieldArrayType(access) = fact {
            Some(access)
        } else {
            None
        }
    })
}

/// Iterate typed factory-call member facts.
fn factory_call_member_access_facts(
    semantic_facts: &[SemanticFact],
) -> impl Iterator<Item = &FactoryCallMemberAccessFact> {
    semantic_facts.iter().filter_map(|fact| {
        if let SemanticFact::FactoryCallMemberAccess(access) = fact {
            Some(access)
        } else {
            None
        }
    })
}

/// Iterate typed free-function factory-return member facts.
fn factory_fn_member_access_facts(
    semantic_facts: &[SemanticFact],
) -> impl Iterator<Item = &FactoryFnMemberAccessFact> {
    semantic_facts.iter().filter_map(|fact| {
        if let SemanticFact::FactoryFnMemberAccess(access) = fact {
            Some(access)
        } else {
            None
        }
    })
}

/// Iterate typed fluent-chain member facts.
fn fluent_chain_member_access_facts(
    semantic_facts: &[SemanticFact],
) -> impl Iterator<Item = &FluentChainMemberAccessFact> {
    semantic_facts.iter().filter_map(|fact| {
        if let SemanticFact::FluentChainMemberAccess(access) = fact {
            Some(access)
        } else {
            None
        }
    })
}

/// Iterate typed-property-hop member facts.
fn typed_property_member_access_facts(
    semantic_facts: &[SemanticFact],
) -> impl Iterator<Item = &TypedPropertyMemberAccessFact> {
    semantic_facts.iter().filter_map(|fact| {
        if let SemanticFact::TypedPropertyMemberAccess(access) = fact {
            Some(access)
        } else {
            None
        }
    })
}

/// Iterate typed constructor-rooted fluent-chain member facts.
fn fluent_chain_new_member_access_facts(
    semantic_facts: &[SemanticFact],
) -> impl Iterator<Item = &FluentChainNewMemberAccessFact> {
    semantic_facts.iter().filter_map(|fact| {
        if let SemanticFact::FluentChainNewMemberAccess(access) = fact {
            Some(access)
        } else {
            None
        }
    })
}

/// Iterate typed Playwright fixture-use facts.
fn playwright_fixture_use_facts(
    semantic_facts: &[SemanticFact],
) -> impl Iterator<Item = &PlaywrightFixtureUseFact> {
    semantic_facts.iter().filter_map(|fact| {
        if let SemanticFact::PlaywrightFixtureUse(access) = fact {
            Some(access)
        } else {
            None
        }
    })
}

/// Iterate typed Playwright fixture-definition facts.
fn playwright_fixture_definition_facts(
    semantic_facts: &[SemanticFact],
) -> impl Iterator<Item = &PlaywrightFixtureDefinitionFact> {
    semantic_facts.iter().filter_map(|fact| {
        if let SemanticFact::PlaywrightFixtureDefinition(access) = fact {
            Some(access)
        } else {
            None
        }
    })
}

/// Iterate typed Playwright fixture-alias facts.
fn playwright_fixture_alias_facts(
    semantic_facts: &[SemanticFact],
) -> impl Iterator<Item = &PlaywrightFixtureAliasFact> {
    semantic_facts.iter().filter_map(|fact| {
        if let SemanticFact::PlaywrightFixtureAlias(access) = fact {
            Some(access)
        } else {
            None
        }
    })
}

/// Iterate typed Playwright fixture-type facts.
fn playwright_fixture_type_facts(
    semantic_facts: &[SemanticFact],
) -> impl Iterator<Item = &PlaywrightFixtureTypeFact> {
    semantic_facts.iter().filter_map(|fact| {
        if let SemanticFact::PlaywrightFixtureType(access) = fact {
            Some(access)
        } else {
            None
        }
    })
}

/// A member name referenced from an Angular template surface.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, bitcode::Encode, bitcode::Decode)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct AngularTemplateMemberAccessFact {
    /// Referenced class member name.
    pub member: String,
}

/// A typed Angular component field that exposes array elements to templates.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, bitcode::Encode, bitcode::Decode)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct AngularComponentFieldArrayTypeFact {
    /// Component field name used as the template iterable.
    pub field: String,
    /// Array element class name.
    pub element_class: String,
}

/// Opaque Angular `{ ...this }` forwarding marker.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, bitcode::Encode, bitcode::Decode)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct AngularThisSpreadFact;

/// A member access on a static factory call result.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, bitcode::Encode, bitcode::Decode)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct FactoryCallMemberAccessFact {
    /// Local imported class or namespace object used as the factory callee.
    pub callee_object: String,
    /// Static factory method invoked on the callee object.
    pub callee_method: String,
    /// Member accessed on the returned instance-like object.
    pub member: String,
}

/// A member access on a value returned by an imported free-function factory.
///
/// `const x = importedFactory(); x.member` emits one fact per first-level read
/// on `x`. The analyze layer resolves `callee_name` through the consumer's
/// imports to the factory's origin module, reads that module's
/// `exported_factory_returns` to learn the returned class's local name, resolves
/// THAT through the factory module's own imports to the class export, and
/// credits `member` on the class. See issue #1441 (Part A).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, bitcode::Encode, bitcode::Decode)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct FactoryFnMemberAccessFact {
    /// Local imported function used as the factory callee.
    pub callee_name: String,
    /// Member accessed on the returned instance-like object.
    pub member: String,
}

/// A member access reached through a typed property hop that the extraction
/// layer could not resolve locally.
///
/// `constructor(private opts: Opts) { ... this.opts.c.optM() }` where `Opts`
/// is NOT declared in this file emits
/// `TypedPropertyMemberAccessFact { type_name: "Opts", property_path: "c", member: "optM" }`.
/// The analyze layer resolves `type_name` through the consumer's imports to the
/// declaring module, walks `property_path` through that module's
/// `type_member_types`, resolves the terminal type name through the declaring
/// module's own imports, and credits `member` on the resolved class (gated on
/// the export actually being a class with members). See issue #1785.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, bitcode::Encode, bitcode::Decode)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct TypedPropertyMemberAccessFact {
    /// Local (usually imported) named-type symbol the receiver is typed by.
    pub type_name: String,
    /// Remaining dotted property segments between the typed binding and the
    /// final member (e.g. `"c"` for `this.opts.c.optM()`).
    pub property_path: String,
    /// Member accessed on the terminal property's instance.
    pub member: String,
}

/// A member access on a fluent chain rooted at a static factory call.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, bitcode::Encode, bitcode::Decode)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct FluentChainMemberAccessFact {
    /// Local imported class or namespace object used as the chain root.
    pub root_object: String,
    /// Static factory method that starts the fluent chain.
    pub root_method: String,
    /// Intermediate fluent methods between the root method and final member.
    pub chain: Vec<String>,
    /// Member accessed at this chain step.
    pub member: String,
}

/// A member access on a fluent chain rooted at a `new` expression.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, bitcode::Encode, bitcode::Decode)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct FluentChainNewMemberAccessFact {
    /// Local imported class constructed by the `new` expression.
    pub class_name: String,
    /// Intermediate fluent methods between construction and final member.
    pub chain: Vec<String>,
    /// Member accessed at this chain step.
    pub member: String,
}

/// A member access on a Playwright fixture object inside a test callback.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, bitcode::Encode, bitcode::Decode)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct PlaywrightFixtureUseFact {
    /// Local test function or wrapper used as the callback callee.
    pub test_name: String,
    /// Fixture name or dotted fixture path referenced in the callback.
    pub fixture_name: String,
    /// Member accessed on the fixture target.
    pub member: String,
}

/// A Playwright fixture definition declared by a typed `test.extend<T>()`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, bitcode::Encode, bitcode::Decode)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct PlaywrightFixtureDefinitionFact {
    /// Local test function or wrapper receiving the fixture definition.
    pub test_name: String,
    /// Fixture name or dotted fixture path declared by the fixture type.
    pub fixture_name: String,
    /// Local type symbol used as the fixture target.
    pub type_name: String,
}

/// A Playwright fixture wrapper alias declared by `mergeTests` or `.extend`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, bitcode::Encode, bitcode::Decode)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct PlaywrightFixtureAliasFact {
    /// Local test function or wrapper that inherits fixture definitions.
    pub test_name: String,
    /// Local test function or wrapper inherited by `test_name`.
    pub base_name: String,
}

/// A nested Playwright fixture binding declared by a fixture type alias.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, bitcode::Encode, bitcode::Decode)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct PlaywrightFixtureTypeFact {
    /// Local type alias containing the nested fixture binding.
    pub alias_name: String,
    /// Fixture name or dotted fixture path declared inside the type alias.
    pub fixture_name: String,
    /// Local type symbol used as the nested fixture target.
    pub type_name: String,
}

/// An exported value whose runtime instance targets a local class or interface.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, bitcode::Encode, bitcode::Decode)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct InstanceExportBindingFact {
    /// Exported binding name.
    pub export_name: String,
    /// Local class or interface symbol used as the instance target.
    pub target_name: String,
}

/// Opaque marker for a dynamic custom-element render site.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, bitcode::Encode, bitcode::Decode)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct DynamicCustomElementRenderFact;

/// A statically flattenable callee path invoked in a module (e.g. `execSync`,
/// `child_process.exec`, `console.log`). One entry per unique `callee_path`
/// per module; the span anchors the first occurrence. Consumed by the
/// `boundaries.calls.forbidden` detector.
#[derive(Debug, Clone, bitcode::Encode, bitcode::Decode)]
pub struct CalleeUse {
    /// The dotted or bare callee path as written at the call site.
    pub callee_path: String,
    /// Start byte offset of the first call site using this path.
    pub span_start: u32,
}

/// A `"use client"` / `"use server"` directive string written as an expression
/// statement in `program.body` (NOT the leading prologue), so the RSC bundler
/// silently ignores it. One entry per offending occurrence. Consumed by the
/// `misplaced-directive` detector.
#[derive(Debug, Clone, PartialEq, Eq, bitcode::Encode, bitcode::Decode)]
pub struct MisplacedDirectiveSite {
    /// `true` for `"use server"`, `false` for `"use client"`.
    pub is_server: bool,
    /// Start byte offset of the misplaced directive statement.
    pub span_start: u32,
}

/// Which side of a dependency-injection link a call site represents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, bitcode::Encode, bitcode::Decode)]
pub enum DiRole {
    /// `provide(KEY, value)` / `app.provide(KEY, value)` / `setContext(KEY, value)`.
    Provide,
    /// `inject(KEY)` / `getContext(KEY)`.
    Inject,
}

/// Which framework's DI API a call site came from (drives the finding message).
#[derive(Debug, Clone, Copy, PartialEq, Eq, bitcode::Encode, bitcode::Decode)]
pub enum DiFramework {
    /// Vue `provide` / `inject` (from `vue` / `@vue/runtime-core`).
    Vue,
    /// Svelte `setContext` / `getContext` (from `svelte`).
    Svelte,
    /// Angular `inject(TOKEN)` / `@Inject(TOKEN)` (from `@angular/core`),
    /// matched against `{ provide: TOKEN, ... }` provider objects.
    Angular,
}

/// A Vue `provide`/`inject` or Svelte `setContext`/`getContext` call site keyed
/// by an identifier symbol. The `key_local` is resolved at analyze time through
/// the consuming module's import/export tables to a canonical defining-site
/// export key, so a provide and an inject of the same shared symbol unify even
/// across barrel re-exports. Consumed by the `unprovided-inject` detector.
#[derive(Debug, Clone, PartialEq, Eq, bitcode::Encode, bitcode::Decode)]
pub struct DiKeySite {
    /// The key identifier as written at the call site.
    pub key_local: String,
    /// Whether this is a provide or an inject.
    pub role: DiRole,
    /// Which framework's API this came from.
    pub framework: DiFramework,
    /// Start byte offset of the call expression (anchors the finding).
    pub span_start: u32,
}

/// A component prop declared by Vue `<script setup>` `defineProps` or Svelte 5
/// `$props()`. `used_in_script` / `used_in_template` are set during extraction;
/// the `unused-component-prop` detector flags a prop where neither is true. See
/// `harvest_define_props` and `harvest_svelte_props` in `sfc_props.rs`.
#[derive(Debug, Clone, bitcode::Encode, bitcode::Decode)]
pub struct ComponentProp {
    /// The declared prop name.
    pub name: String,
    /// The template/script-visible local binding name: the destructure alias for
    /// `const { name: alias } = defineProps()` or
    /// `let { name: alias } = $props()`, otherwise the prop name itself. A
    /// renamed prop is read through this local, so usage must be checked against
    /// it, not the declared name.
    pub local: String,
    /// Start byte offset of the prop declaration (anchors the finding).
    pub span_start: u32,
    /// Whether this prop is referenced in the component's `<script>` (a
    /// destructured local binding with a resolved reference, or a `props.<name>`
    /// member access). For React, this is set-in-body: a resolved reference to the
    /// destructured local anywhere in the component function body.
    pub used_in_script: bool,
    /// Whether this prop name is referenced in the component's `<template>`.
    /// Set by `apply_template_usage` when the template scanner credits the name.
    /// Always false for React (no template; React uses `used_in_script`).
    pub used_in_template: bool,
    /// The enclosing component name. Empty for Vue SFCs (one component per file,
    /// the file stem is the component, set by the detector). For React this is the
    /// component function/arrow name a prop was declared on, so the detector can
    /// emit the right `component_name` and apply the per-component abstain ladder
    /// (a file can declare several React components).
    pub component: String,
    /// React-only: `true` when the destructured prop local is referenced at least
    /// once OUTSIDE a child-JSX attribute value expression (a substantive
    /// consumption: a hook arg, a host-element child, a non-JSX-attr read). When
    /// `used_in_script` is true but this is false, the prop is referenced ONLY as
    /// the root of forwarded child attribute values, i.e. a pure pass-through.
    /// Always `false` for Vue (no forward-vs-consume distinction is computed).
    pub used_outside_forward: bool,
}

/// A Vue `<script setup>` `defineEmits` declared event, harvested from the type
/// tuple-call form (`defineEmits<{ (e: 'foo'): void }>()`), the type object form
/// (`defineEmits<{ foo: [x: string] }>()`), or the runtime array form
/// (`defineEmits(['foo'])`). `used` is set during extraction when the bound emit
/// name is called as `emit('<name>')`. The `unused-component-emit` detector flags
/// an event where `used` is false. See `harvest_define_emits` in `sfc_props.rs`.
#[derive(Debug, Clone, bitcode::Encode, bitcode::Decode, PartialEq, Eq)]
pub struct ComponentEmit {
    /// The declared emit event name.
    pub name: String,
    /// Start byte offset of the emit declaration (anchors the finding).
    pub span_start: u32,
    /// Whether this event is emitted via `emit('<name>')` somewhere in the
    /// component's `<script>`.
    pub used: bool,
}

/// A Svelte custom event dispatched via `dispatch('<name>')`, where `dispatch`
/// is the binding from a `const dispatch = createEventDispatcher()` call. Only
/// literal-first-arg dispatches are recorded; a `dispatch(<nonLiteral>)` sets
/// `ModuleInfo::has_dynamic_dispatch` instead. Consumed by the
/// `unused-svelte-event` detector, which flags an event dispatched here but
/// listened to nowhere project-wide (the cross-file dead-output direction). The
/// span is a byte offset (not an `oxc_span::Span`) so the type round-trips
/// through the bitcode cache directly, mirroring `ComponentEmit::span_start`.
#[derive(Debug, Clone, bitcode::Encode, bitcode::Decode, PartialEq, Eq)]
pub struct DispatchedEvent {
    /// The dispatched event name (the literal first argument).
    pub name: String,
    /// Start byte offset of the `dispatch(...)` call (anchors the finding).
    pub span_start: u32,
}

/// A declared Angular component/directive input, harvested from an `@Input()`
/// decorator or a signal `input()` / `input.required()` / `model()` initializer
/// on an Angular-decorated class. Consumed by the `unused-component-input`
/// detector, which flags an input read nowhere in its own component (neither the
/// template nor the class body). The span is stored as a byte offset (not an
/// `oxc_span::Span`) so the type is cheap to mirror onto the cache, matching
/// `ComponentEmit::span_start`. `ModuleInfo` is not serialized, so no serde
/// attrs are derived here. `bitcode` derives let the type be mirrored directly
/// onto `CachedModule` (the same pattern as `ComponentEmit`).
#[derive(Debug, Clone, bitcode::Encode, bitcode::Decode, PartialEq, Eq)]
pub struct AngularInputMember {
    /// The declared input name (the property key).
    pub name: String,
    /// Start byte offset of the property key (anchors the finding).
    pub span_start: u32,
}

/// A declared Angular component/directive output, harvested from an `@Output()`
/// decorator or a signal `output()` / `outputFromObservable()` initializer on an
/// Angular-decorated class. Consumed by the `unused-component-output` detector,
/// which flags an output emitted nowhere in its own component. A `model()` is an
/// input and a framework-driven output, so it is recorded ONLY as an input and
/// never appears here (the implicit `update:` emit is framework-managed). The
/// span is a byte offset for the same reason as `AngularInputMember`.
#[derive(Debug, Clone, bitcode::Encode, bitcode::Decode, PartialEq, Eq)]
pub struct AngularOutputMember {
    /// The declared output name (the property key).
    pub name: String,
    /// Start byte offset of the property key (anchors the finding).
    pub span_start: u32,
}

/// A declared Angular `@Component` and its `selector` value(s), harvested from a
/// `@Component({ selector: '...' })` decorator. Consumed by the Angular arm of
/// the `unrendered-component` detector, which flags a component whose every
/// element selector is used in NO template project-wide (and that is not
/// referenced by class name anywhere, e.g. routed / bootstrapped / dynamically
/// rendered). A multi-selector string (`'app-foo, [appBar]'`) is split into the
/// `selectors` list. The span is stored as a byte offset (not an
/// `oxc_span::Span`) so the type round-trips through the bitcode cache directly,
/// mirroring `AngularInputMember::span_start`. `@Directive` is intentionally NOT
/// harvested here (directives have no template render). `ModuleInfo` is not
/// serialized, so no serde attrs are derived.
#[derive(Debug, Clone, bitcode::Encode, bitcode::Decode, PartialEq, Eq)]
pub struct AngularComponentSelector {
    /// The declared selector strings for this component, split on `,`. A purely
    /// element-selector component has only `app-foo`-shaped entries; attribute
    /// (`[appFoo]`) and class (`.foo`) selectors are retained verbatim so the
    /// detector can abstain when ANY non-element selector is present.
    pub selectors: Vec<String>,
    /// Start byte offset of the component class declaration (anchors the
    /// finding).
    pub span_start: u32,
    /// The component class name (used to credit routed / bootstrapped / dynamic
    /// class-name references project-wide).
    pub class_name: String,
}

/// A Lit / web-component custom element registered in a module via
/// `@customElement('x-foo')` or `customElements.define('x-foo', C)`. Consumed by
/// the Lit arm of the `unrendered-component` detector. The span is stored as a
/// byte offset (not an `oxc_span::Span`) so the type round-trips through the
/// bitcode cache directly, mirroring `AngularComponentSelector::span_start`.
#[derive(Debug, Clone, bitcode::Encode, bitcode::Decode, PartialEq, Eq)]
pub struct RegisteredCustomElement {
    /// The registered custom-element tag name (`x-foo`).
    pub tag: String,
    /// The registering class's local name, used for the public-API / export
    /// abstain (an exported / published element is rendered by a downstream
    /// consumer the scan cannot see). Empty for an anonymous
    /// `export default @customElement('x-foo') class extends LitElement {}`.
    pub class_local_name: String,
    /// Start byte offset of the registering class declaration (anchors the
    /// finding at the element, NOT line 1, since a `.ts` file can register
    /// several custom elements).
    pub span_start: u32,
}

/// A key returned from a SvelteKit route `load()` function's terminal return
/// object literal. Harvested from `+page.{ts,server.ts,js,server.js}` files
/// exporting a `load` function. Consumed by the `unused-load-data-key` detector,
/// which flags a key read by no consumer. The span is stored as byte offsets
/// (not an `oxc_span::Span`) so the type round-trips through the bitcode cache
/// directly, mirroring `DiKeySite::span_start` / `ComponentEmit::span_start`.
#[derive(Debug, Clone, bitcode::Encode, bitcode::Decode, PartialEq, Eq)]
pub struct LoadReturnKey {
    /// The returned-object property key name.
    pub name: String,
    /// Start byte offset of the key (anchors the finding).
    pub span_start: u32,
    /// End byte offset of the key.
    pub span_end: u32,
}

/// The syntactic shape of an identified React component definition. Drives the
/// abstain ladder later phases apply: a `forwardRef` / `memo` wrapper whose
/// props come from an imported interface fallow cannot resolve must abstain
/// (ADR-001), not guess.
#[derive(Debug, Clone, Copy, PartialEq, Eq, bitcode::Encode, bitcode::Decode)]
pub enum ComponentFunctionKind {
    /// A `function Foo() { return <.../> }` declaration.
    FnDecl,
    /// A `const Foo = () => <.../>` arrow (or function-expression) binding.
    Arrow,
    /// A `const Foo = forwardRef((props, ref) => <.../>)` wrapper.
    ForwardRefWrapper,
    /// A `const Foo = memo((props) => <.../>)` wrapper.
    MemoWrapper,
}

/// An identified React component: a function/arrow whose body returns JSX.
/// Captured by `visit_jsx_element`'s enclosing-component tracking. The
/// `unused-component-prop` (React arm) and complexity-fold phases consume this;
/// the abstain flags keep zero-FP on the cases ADR-001 cannot resolve.
#[derive(Debug, Clone, bitcode::Encode, bitcode::Decode)]
pub struct ComponentFunction {
    /// The component name (the binding or declaration identifier).
    pub name: String,
    /// Start byte offset of the component definition (anchors findings).
    pub span_start: u32,
    /// The syntactic shape of the definition.
    pub kind: ComponentFunctionKind,
    /// Whether the component is exported from its module (a named export, a
    /// `export default`, or re-exported in the same module). Public-API
    /// components abstain in the prop phase.
    pub is_exported: bool,
    /// `true` when the component's props are not statically harvestable: a
    /// rest/spread in the signature (`{ ...rest }`), props passed wholesale to a
    /// hook/helper, or a `forwardRef` / `memo` wrapper whose props come from an
    /// imported interface generic fallow cannot resolve (ADR-001). The prop
    /// phase abstains on the whole component when set.
    pub has_unharvestable_props: bool,
    /// `true` when the component body calls `cloneElement` / `React.cloneElement`.
    /// `cloneElement` injects props by reflection, so the static forward-set is
    /// incomplete; the prop-drilling phase abstains on any chain through this
    /// component (ADR-001, zero-FP).
    pub uses_clone_element: bool,
    /// `true` when the component renders a `*.Provider` member-expression tag
    /// (`<FooContext.Provider>`). A context provider in the subtree means the
    /// drilling may be a deliberate non-context choice (or the prop is about to
    /// be provided); the prop-drilling phase downgrades/abstains.
    pub renders_provider: bool,
    /// `true` when the component passes a function as a child render value
    /// (render-props / children-as-function: `<Foo>{() => ...}</Foo>` or
    /// `<Foo render={() => ...}/>`). The forwarded shape is dynamic; the
    /// prop-drilling phase abstains on chains through this component.
    pub has_children_as_function: bool,
    /// `true` when the component body is pure structural indirection: a single
    /// statement returning exactly one capitalized/member-expression JSX element
    /// (no host wrapper, no extra children, optionally a fragment wrapping a
    /// single element) that forwards props via a bare spread of the component's
    /// own props binding / rest local (`<Child {...props}/>`), with NO named
    /// attributes alongside the spread and NO self-render. The cross-component
    /// `thin-wrapper` phase joins this with hook-density / cyclomatic checks and
    /// the resolved single render edge to flag a component that is a candidate
    /// for inlining. Computed from the component's own AST only, so it caches
    /// byte-identity-safe (ADR-001).
    pub is_pure_passthrough: bool,
}

/// The kind of a React hook call. `Custom` covers any `use*`-named call that is
/// not one of the built-in hooks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, bitcode::Encode, bitcode::Decode)]
pub enum HookUseKind {
    /// `useState(...)`.
    UseState,
    /// `useEffect(...)`.
    UseEffect,
    /// `useMemo(...)`.
    UseMemo,
    /// `useCallback(...)`.
    UseCallback,
    /// Any other `use*`-named call (a custom hook).
    Custom,
}

/// A React hook call site inside a component. Consumed by the complexity-fold
/// phase (hook density) and surfaced as descriptive hotspot context.
#[derive(Debug, Clone, bitcode::Encode, bitcode::Decode)]
pub struct HookUse {
    /// The hook kind.
    pub kind: HookUseKind,
    /// The dependency-array arity, recorded ONLY when a literal array is present
    /// at the dependency-array position (`[a, b]` -> `Some(2)`, `[]` ->
    /// `Some(0)`). `None` when the call has no dependency array argument or the
    /// argument is not a literal array (ADR-001: do not guess).
    pub dep_array_arity: Option<u32>,
    /// Start byte offset of the hook call (anchors findings).
    pub span_start: u32,
    /// The enclosing component name (the top of the visitor's component stack
    /// when the hook call was recorded). Lets the descriptive per-component hook
    /// summary attribute hooks exactly even when a file declares several
    /// components. A hook recorded outside any component carries an empty string
    /// (the visitor only records hooks inside a component, so this is the
    /// rare top-level / unattributed case).
    pub component: String,
}

/// A render edge: one component rendering another (a capitalized or
/// member-expression JSX tag). Captured at extraction time with the child's
/// written name; resolution of `child_component_name` to a `FileId`/export is
/// deferred to graph build via the existing import map.
#[derive(Debug, Clone, bitcode::Encode, bitcode::Decode)]
pub struct RenderEdge {
    /// The name of the component that renders the child (the enclosing
    /// component). Empty when the JSX is not inside an identified component (a
    /// top-level render expression).
    pub parent_component: String,
    /// The rendered child component name as written (`Foo` or the full
    /// member-expression path `Foo.Bar`).
    pub child_component_name: String,
    /// The attribute (prop) names passed at the render site, in source order.
    pub attr_names: Vec<String>,
    /// `true` when the render site contains a JSX spread (`{...x}`), so the
    /// passed-prop set is not statically complete.
    pub has_spread: bool,
    /// The forwarded attributes at this render site: each pairs the child
    /// attribute NAME with the identifier ROOT of its value expression
    /// (`userName={user.name}` -> `{ attr: "userName", root: "user" }`;
    /// `value={x}` -> `{ attr: "value", root: "x" }`). ONLY plain identifier or
    /// member-root access values are recorded (`{x}`, `{x.y}`, `{x.y.z}`); a value
    /// that is a call, an arrow/function, a conditional, a JSX element, or any
    /// other complex expression is NOT recorded here (its root would not be a pure
    /// forward) and sets `has_complex_forward` instead. The prop-drilling chain
    /// walk uses this pairing to map "this component forwards prop P" to "the
    /// child receives it as attribute A".
    pub forward_attrs: Vec<ForwardAttr>,
    /// `true` when at least one attribute value at this render site is a complex
    /// expression (a call, an arrow/function render-prop, a conditional, a JSX
    /// element-as-prop, a template literal, etc.) whose identifier root was NOT
    /// recorded in `forward_attrs`. The prop-drilling phase abstains on a chain
    /// whose forwarded prop flows through such a value (ADR-001, zero-FP).
    pub has_complex_forward: bool,
}

/// One forwarded JSX attribute: the child attribute name plus the identifier
/// root of its value expression. See [`RenderEdge::forward_attrs`].
#[derive(Debug, Clone, bitcode::Encode, bitcode::Decode)]
pub struct ForwardAttr {
    /// The child attribute (prop) name as written (`userName`).
    pub attr: String,
    /// The identifier root of the attribute value expression (`user` for
    /// `userName={user.name}`).
    pub root: String,
}

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
    /// Whether this import originated from a CSS-context.
    pub from_style: bool,
    /// Source span of the import declaration.
    pub span: Span,
    /// Span of the source string literal used by the LSP to highlight the specifier.
    pub source_span: Span,
}

/// How a symbol is imported.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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
const _: () = assert!(std::mem::size_of::<ExportInfo>() == 136);
#[cfg(target_pointer_width = "64")]
const _: () = assert!(std::mem::size_of::<ImportInfo>() == 96);
#[cfg(target_pointer_width = "64")]
const _: () = assert!(std::mem::size_of::<ExportName>() == 24);
#[cfg(target_pointer_width = "64")]
const _: () = assert!(std::mem::size_of::<ImportedName>() == 24);
#[cfg(target_pointer_width = "64")]
const _: () = assert!(std::mem::size_of::<MemberAccess>() == 48);
#[cfg(target_pointer_width = "64")]
const _: () = assert!(std::mem::size_of::<SemanticFact>() == 96);
#[cfg(target_pointer_width = "64")]
const _: () = assert!(std::mem::size_of::<SinkSite>() == 216);
#[cfg(target_pointer_width = "64")]
const _: () = assert!(std::mem::size_of::<ModuleInfo>() == 1336);
#[cfg(target_pointer_width = "64")]
const _: () = assert!(std::mem::size_of::<TypeMemberTypeEntry>() == 72);

/// A re-export declaration.
#[derive(Debug, Clone)]
pub struct ReExportInfo {
    /// The module being re-exported from.
    pub source: String,
    /// The name imported from the source module (or `*` for star re-exports).
    pub imported_name: String,
    /// The name exported from this module.
    pub exported_name: String,
    /// Whether this is a type-only re-export.
    pub is_type_only: bool,
    /// Source span of the re-export declaration on this module.
    pub span: oxc_span::Span,
}

/// A dynamic `import()` call.
#[derive(Debug, Clone)]
pub struct DynamicImportInfo {
    /// The import specifier.
    pub source: String,
    /// Source span of the `import()` expression.
    pub span: Span,
    /// Names destructured from the dynamic import result.
    /// Non-empty means `const { a, b } = await import(...)` -> Named imports.
    /// Empty means simple `import(...)` or `const x = await import(...)` -> Namespace.
    pub destructured_names: Vec<String>,
    /// The local variable name for `const x = await import(...)`.
    /// Used for namespace import narrowing via member access tracking.
    pub local_name: Option<String>,
    /// True when this dynamic import was synthesised by fallow rather than appearing in user source.
    pub is_speculative: bool,
}

/// A `require()` call.
#[derive(Debug, Clone)]
pub struct RequireCallInfo {
    /// The require specifier.
    pub source: String,
    /// Source span of the `require()` call.
    pub span: Span,
    /// Source span of the specifier string-literal argument (including its
    /// quotes), e.g. the `'./x'` in `require('./x')`. Used to anchor an
    /// `unresolved-import` diagnostic squiggly under the specifier rather than
    /// the `require` keyword. `Span::default()` when the argument is not a
    /// plain string literal.
    pub source_span: Span,
    /// Names destructured from the `require()` result.
    pub destructured_names: Vec<String>,
    /// The local variable name for `const x = require(...)`.
    pub local_name: Option<String>,
}

/// Result of parsing all files, including incremental cache statistics.
pub struct ParseResult {
    /// Extracted module information for all successfully parsed files.
    pub modules: Vec<ModuleInfo>,
    /// Number of files whose parse results were loaded from cache (unchanged).
    pub cache_hits: usize,
    /// Number of files that required a full parse (new or changed).
    pub cache_misses: usize,
    /// Summed wall-clock time of the actual AST parses across all rayon workers.
    pub parse_cpu_ms: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span() -> Span {
        Span::new(0, 1)
    }

    macro_rules! assert_released {
        ($values:expr) => {{
            assert!($values.is_empty());
        }};
    }

    #[test]
    fn public_env_var_includes_public_ci_metadata() {
        for name in ["TAG_REF", "GITHUB_SHA", "CI_COMMIT_BRANCH", "APP_MODE"] {
            assert!(is_public_env_var(name), "{name} should be public metadata");
        }
    }

    #[test]
    fn public_env_var_keeps_secret_shaped_names_source_backed() {
        for name in ["GITHUB_TOKEN", "REFRESH_TOKEN", "API_KEY", "SECRET_SHA"] {
            assert!(
                !is_public_env_var(name),
                "{name} should remain secret-shaped"
            );
        }
    }

    #[test]
    fn ordinary_access_helpers_keep_source_accesses() {
        let member_accesses = vec![
            MemberAccess {
                object: "this".to_string(),
                member: "render".to_string(),
            },
            MemberAccess {
                object: "service".to_string(),
                member: "run".to_string(),
            },
        ];
        let ordinary = SemanticFactView::new(&[], &member_accesses)
            .ordinary_member_accesses()
            .map(|access| (access.object.as_str(), access.member.as_str()))
            .collect::<Vec<_>>();

        assert_eq!(ordinary, vec![("this", "render"), ("service", "run")]);

        let whole_object_uses = vec!["model".to_string(), "service".to_string()];

        assert_eq!(
            ordinary_whole_object_uses(&whole_object_uses).collect::<Vec<_>>(),
            vec!["model", "service"]
        );
    }

    #[test]
    fn angular_template_member_names_use_typed_facts() {
        let mut module = minimal_module_info();
        push_semantic_fact(
            &mut module,
            SemanticFact::AngularTemplateMemberAccess(AngularTemplateMemberAccessFact {
                member: "typed".to_string(),
            }),
        );

        let names: Vec<&str> = angular_template_member_names(&module).collect();

        assert_eq!(names, vec!["typed"]);
        assert!(has_angular_template_members(&module));
    }

    #[test]
    fn angular_this_spread_uses_typed_fact() {
        let mut typed = minimal_module_info();
        push_semantic_fact(
            &mut typed,
            SemanticFact::AngularThisSpread(AngularThisSpreadFact),
        );

        assert!(has_angular_this_spread(&typed));
        assert!(!has_angular_this_spread(&minimal_module_info()));
    }

    #[test]
    fn semantic_fact_view_iterates_typed_facts() {
        let mut module = minimal_module_info();
        push_semantic_fact(
            &mut module,
            SemanticFact::FactoryCallMemberAccess(FactoryCallMemberAccessFact {
                callee_object: "Svc".to_string(),
                callee_method: "make".to_string(),
                member: "run".to_string(),
            }),
        );

        let facts = SemanticFactView::new(&module.semantic_facts, &module.member_accesses)
            .facts()
            .collect::<Vec<_>>();

        assert_eq!(
            facts[0],
            &SemanticFact::FactoryCallMemberAccess(FactoryCallMemberAccessFact {
                callee_object: "Svc".to_string(),
                callee_method: "make".to_string(),
                member: "run".to_string(),
            })
        );
    }

    #[test]
    fn typed_fact_helpers_collect_each_family() {
        let mut module = minimal_module_info();
        push_semantic_fact(
            &mut module,
            SemanticFact::InstanceExportBinding(InstanceExportBindingFact {
                export_name: "exported".to_string(),
                target_name: "target".to_string(),
            }),
        );
        push_semantic_fact(
            &mut module,
            SemanticFact::FactoryCallMemberAccess(FactoryCallMemberAccessFact {
                callee_object: "Svc".to_string(),
                callee_method: "create".to_string(),
                member: "run".to_string(),
            }),
        );
        push_semantic_fact(
            &mut module,
            SemanticFact::FluentChainMemberAccess(FluentChainMemberAccessFact {
                root_object: "Builder".to_string(),
                root_method: "start".to_string(),
                chain: vec!["next".to_string()],
                member: "value".to_string(),
            }),
        );
        push_semantic_fact(
            &mut module,
            SemanticFact::FluentChainNewMemberAccess(FluentChainNewMemberAccessFact {
                class_name: "Builder".to_string(),
                chain: vec!["next".to_string(), "finish".to_string()],
                member: "done".to_string(),
            }),
        );

        assert_eq!(
            SemanticFactView::new(&module.semantic_facts, &module.member_accesses)
                .instance_export_bindings(),
            vec![InstanceExportBindingFact {
                export_name: "exported".to_string(),
                target_name: "target".to_string(),
            }]
        );
        assert_eq!(
            SemanticFactView::new(&module.semantic_facts, &module.member_accesses)
                .factory_call_member_accesses(),
            vec![FactoryCallMemberAccessFact {
                callee_object: "Svc".to_string(),
                callee_method: "create".to_string(),
                member: "run".to_string(),
            }]
        );
        assert_eq!(
            SemanticFactView::new(&module.semantic_facts, &module.member_accesses)
                .fluent_chain_member_accesses(),
            vec![FluentChainMemberAccessFact {
                root_object: "Builder".to_string(),
                root_method: "start".to_string(),
                chain: vec!["next".to_string()],
                member: "value".to_string(),
            }]
        );
        assert_eq!(
            SemanticFactView::new(&module.semantic_facts, &module.member_accesses)
                .fluent_chain_new_member_accesses(),
            vec![FluentChainNewMemberAccessFact {
                class_name: "Builder".to_string(),
                chain: vec!["next".to_string(), "finish".to_string()],
                member: "done".to_string(),
            }]
        );
    }

    #[test]
    fn semantic_fact_view_exposes_typed_first_contract() {
        let mut module = minimal_module_info();
        push_semantic_fact(
            &mut module,
            SemanticFact::FactoryCallMemberAccess(FactoryCallMemberAccessFact {
                callee_object: "Svc".to_string(),
                callee_method: "create".to_string(),
                member: "run".to_string(),
            }),
        );
        push_semantic_fact(
            &mut module,
            SemanticFact::PlaywrightFixtureUse(PlaywrightFixtureUseFact {
                test_name: "test".to_string(),
                fixture_name: "page".to_string(),
                member: "goto".to_string(),
            }),
        );
        push_semantic_fact(
            &mut module,
            SemanticFact::InstanceExportBinding(InstanceExportBindingFact {
                export_name: "exported".to_string(),
                target_name: "target".to_string(),
            }),
        );

        let view = SemanticFactView::new(&module.semantic_facts, &module.member_accesses);

        assert_eq!(
            view.factory_call_member_accesses(),
            vec![FactoryCallMemberAccessFact {
                callee_object: "Svc".to_string(),
                callee_method: "create".to_string(),
                member: "run".to_string(),
            }]
        );
        assert_eq!(
            view.playwright_fixture_uses(),
            vec![PlaywrightFixtureUseFact {
                test_name: "test".to_string(),
                fixture_name: "page".to_string(),
                member: "goto".to_string(),
            }]
        );
        assert_eq!(
            view.instance_export_bindings(),
            vec![InstanceExportBindingFact {
                export_name: "exported".to_string(),
                target_name: "target".to_string(),
            }]
        );
    }

    #[test]
    fn playwright_fixture_fact_helpers_select_each_fact_family() {
        let mut module = minimal_module_info();
        push_semantic_fact(
            &mut module,
            SemanticFact::PlaywrightFixtureUse(PlaywrightFixtureUseFact {
                test_name: "test".to_string(),
                fixture_name: "page".to_string(),
                member: "goto".to_string(),
            }),
        );
        push_semantic_fact(
            &mut module,
            SemanticFact::PlaywrightFixtureDefinition(PlaywrightFixtureDefinitionFact {
                test_name: "test".to_string(),
                fixture_name: "adminPage".to_string(),
                type_name: "AdminPage".to_string(),
            }),
        );
        push_semantic_fact(
            &mut module,
            SemanticFact::PlaywrightFixtureAlias(PlaywrightFixtureAliasFact {
                test_name: "mergedTest".to_string(),
                base_name: "test".to_string(),
            }),
        );
        push_semantic_fact(
            &mut module,
            SemanticFact::PlaywrightFixtureType(PlaywrightFixtureTypeFact {
                alias_name: "Pages".to_string(),
                fixture_name: "adminPage".to_string(),
                type_name: "AdminPage".to_string(),
            }),
        );

        assert_eq!(
            playwright_fixture_use_facts(&module.semantic_facts)
                .map(|fact| fact.member.as_str())
                .collect::<Vec<_>>(),
            vec!["goto"]
        );
        assert_eq!(
            playwright_fixture_definition_facts(&module.semantic_facts)
                .map(|fact| fact.type_name.as_str())
                .collect::<Vec<_>>(),
            vec!["AdminPage"]
        );
        assert_eq!(
            playwright_fixture_alias_facts(&module.semantic_facts)
                .map(|fact| fact.base_name.as_str())
                .collect::<Vec<_>>(),
            vec!["test"]
        );
        assert_eq!(
            playwright_fixture_type_facts(&module.semantic_facts)
                .map(|fact| fact.fixture_name.as_str())
                .collect::<Vec<_>>(),
            vec!["adminPage"]
        );
    }

    #[test]
    fn line_offsets_empty_string() {
        assert_eq!(compute_line_offsets(""), vec![0]);
    }

    #[test]
    #[expect(
        clippy::too_many_lines,
        reason = "exhaustive field-by-field construction + release assertions for every ModuleInfo field"
    )]
    fn release_resolution_payload_drops_copied_vectors_only() {
        let mut module = ModuleInfo {
            file_id: FileId(7),
            exports: vec![ExportInfo {
                name: ExportName::Named("kept".to_string()),
                local_name: None,
                is_type_only: false,
                is_side_effect_used: false,
                visibility: VisibilityTag::None,
                expected_unused_reason: None,
                span: span(),
                members: Vec::new(),
                super_class: None,
            }],
            imports: vec![ImportInfo {
                source: "node:child_process".to_string(),
                imported_name: ImportedName::Default,
                local_name: "childProcess".to_string(),
                is_type_only: false,
                from_style: false,
                span: span(),
                source_span: span(),
            }],
            re_exports: vec![ReExportInfo {
                source: "./kept".to_string(),
                imported_name: "kept".to_string(),
                exported_name: "kept".to_string(),
                is_type_only: false,
                span: span(),
            }],
            dynamic_imports: vec![DynamicImportInfo {
                source: "./dynamic".to_string(),
                span: span(),
                destructured_names: vec!["value".to_string()],
                local_name: None,
                is_speculative: false,
            }],
            dynamic_import_patterns: vec![DynamicImportPattern {
                prefix: "./pages/".to_string(),
                suffix: Some(".tsx".to_string()),
                span: span(),
            }],
            require_calls: vec![RequireCallInfo {
                source: "./required".to_string(),
                span: span(),
                source_span: span(),
                destructured_names: Vec::new(),
                local_name: Some("required".to_string()),
            }],
            package_path_references: vec!["react".to_string()].into(),
            member_accesses: vec![MemberAccess {
                object: "Status".to_string(),
                member: "Active".to_string(),
            }],
            semantic_facts: Box::default(),
            whole_object_uses: vec!["Status".to_string()].into(),
            has_cjs_exports: true,
            has_angular_component_template_url: true,
            content_hash: 42,
            suppressions: Vec::new(),
            unknown_suppression_kinds: Vec::new(),
            unused_import_bindings: vec!["unused".to_string()],
            type_referenced_import_bindings: vec!["TypeOnly".to_string()],
            value_referenced_import_bindings: vec!["Value".to_string()],
            line_offsets: vec![0, 8],
            complexity: vec![FunctionComplexity {
                name: "work".to_string(),
                line: 1,
                col: 0,
                cyclomatic: 2,
                cognitive: 3,
                line_count: 4,
                param_count: 1,
                react_hook_count: 0,
                react_jsx_max_depth: 0,
                react_prop_count: 0,
                source_hash: Some("hash".to_string()),
                contributions: Vec::new(),
            }],
            flag_uses: vec![FlagUse {
                flag_name: "FEATURE_X".to_string(),
                kind: FlagUseKind::EnvVar,
                line: 1,
                col: 0,
                guard_span_start: None,
                guard_span_end: None,
                sdk_name: None,
            }],
            class_heritage: vec![ClassHeritageInfo {
                export_name: "Child".to_string(),
                super_class: Some("Parent".to_string()),
                implements: vec!["Contract".to_string()],
                instance_bindings: Vec::new(),
            }],
            exported_factory_returns: Box::from([FactoryReturnExport {
                export_name: "useApi".to_string(),
                class_local_name: "RESTApi".to_string(),
            }]),
            type_member_types: Box::from([TypeMemberTypeEntry {
                type_name: "Opts".to_string(),
                property: "c".to_string(),
                property_type: "OptDep".to_string(),
            }]),
            injection_tokens: vec![("TOKEN".to_string(), "Contract".to_string())],
            local_type_declarations: vec![LocalTypeDeclaration {
                name: "Contract".to_string(),
                span: span(),
            }],
            public_signature_type_references: vec![PublicSignatureTypeReference {
                export_name: "kept".to_string(),
                type_name: "Contract".to_string(),
                span: span(),
            }],
            namespace_object_aliases: vec![NamespaceObjectAlias {
                via_export_name: "api".to_string(),
                suffix: "read".to_string(),
                namespace_local: "ns".to_string(),
            }],
            iconify_prefixes: vec!["hero".to_string()],
            iconify_icon_names: vec!["hero-home".to_string()],
            auto_import_candidates: vec!["useState".to_string()],
            directives: vec!["use client".to_string()],
            client_only_dynamic_import_spans: Vec::new(),
            security_sinks: Vec::new(),
            security_sinks_skipped: 1,
            security_unresolved_callee_sites: Vec::new(),
            tainted_bindings: Vec::new(),
            sanitized_sink_args: Vec::new(),
            security_control_sites: Vec::new(),
            callee_uses: Vec::new(),
            misplaced_directives: Vec::new(),
            inline_server_action_exports: Vec::new(),
            di_key_sites: Vec::new(),
            has_dynamic_provide: false,
            referenced_import_bindings: Vec::new(),
            component_props: Vec::new(),
            has_props_attrs_fallthrough: false,
            has_define_expose: false,
            has_define_model: false,
            has_unharvestable_props: false,
            component_emits: Vec::new(),
            angular_inputs: Vec::new(),
            angular_outputs: Vec::new(),
            angular_component_selectors: Vec::new(),
            registered_custom_elements: Vec::new(),
            used_custom_element_tags: Vec::new(),
            angular_used_selectors: Vec::new(),
            angular_entry_component_refs: Vec::new(),
            has_dynamic_component_render: false,
            has_unharvestable_emits: false,
            has_dynamic_emit: false,
            has_emit_whole_object_use: false,
            load_return_keys: Vec::new(),
            has_unharvestable_load: false,
            has_load_data_whole_use: false,
            has_page_data_store_whole_use: false,
            component_functions: Vec::new(),
            react_props: Vec::new(),
            hook_uses: Vec::new(),
            render_edges: Vec::new(),
            svelte_dispatched_events: Vec::new(),
            svelte_listened_events: Vec::new(),
            has_dynamic_dispatch: false,
        };

        module.release_resolution_payload();

        assert_eq!(module.file_id, FileId(7));
        assert_eq!(module.content_hash, 42);
        assert_eq!(module.line_offsets, vec![0, 8]);
        assert_eq!(module.imports.len(), 1);
        assert_eq!(module.exports.len(), 1);
        assert_eq!(module.re_exports.len(), 1);
        assert_eq!(module.dynamic_import_patterns.len(), 1);
        assert_eq!(module.member_accesses.len(), 1);
        assert_eq!(module.complexity.len(), 1);
        assert_eq!(module.flag_uses.len(), 1);
        assert_eq!(module.class_heritage.len(), 1);
        assert_eq!(module.exported_factory_returns.len(), 1);
        assert_eq!(module.injection_tokens.len(), 1);
        assert_eq!(module.local_type_declarations.len(), 1);
        assert_eq!(module.public_signature_type_references.len(), 1);
        assert_eq!(module.iconify_prefixes.len(), 1);
        assert_eq!(module.iconify_icon_names.len(), 1);
        assert_eq!(module.directives.len(), 1);
        assert_eq!(module.security_sinks_skipped, 1);
        assert_released!(module.dynamic_imports);
        assert_released!(module.require_calls);
        assert_released!(module.package_path_references);
        assert_released!(module.whole_object_uses);
        assert_released!(module.unused_import_bindings);
        assert_released!(module.type_referenced_import_bindings);
        assert_released!(module.value_referenced_import_bindings);
        assert_released!(module.namespace_object_aliases);
        assert_released!(module.auto_import_candidates);
        assert_eq!(
            module.referenced_import_bindings,
            vec!["childProcess".to_string()]
        );
    }

    #[test]
    fn sink_shape_bitcode_roundtrip() {
        for shape in [
            SinkShape::Call,
            SinkShape::MemberCall,
            SinkShape::MemberAssign,
            SinkShape::TaggedTemplate,
            SinkShape::JsxAttr,
            SinkShape::NewExpression,
            SinkShape::SecretLiteral,
        ] {
            let encoded = bitcode::encode(&shape);
            let decoded: SinkShape = bitcode::decode(&encoded).expect("decode sink shape");
            assert_eq!(shape, decoded);
        }
    }

    #[test]
    fn sink_arg_kind_bitcode_roundtrip() {
        for kind in [
            SinkArgKind::TemplateWithSubst,
            SinkArgKind::Concat,
            SinkArgKind::Object,
            SinkArgKind::Call,
            SinkArgKind::Literal,
            SinkArgKind::NoArg,
            SinkArgKind::Other,
        ] {
            let encoded = bitcode::encode(&kind);
            let decoded: SinkArgKind = bitcode::decode(&encoded).expect("decode sink arg kind");
            assert_eq!(kind, decoded);
        }
    }

    #[test]
    fn security_url_shape_bitcode_roundtrip() {
        for shape in [
            SecurityUrlShape::FixedOriginDynamicPath,
            SecurityUrlShape::DynamicOrigin,
        ] {
            let encoded = bitcode::encode(&shape);
            let decoded: SecurityUrlShape =
                bitcode::decode(&encoded).expect("decode security url shape");
            assert_eq!(shape, decoded);
        }
    }

    #[test]
    fn sink_site_bitcode_roundtrip() {
        let site = SinkSite {
            sink_shape: SinkShape::MemberAssign,
            callee_path: "el.innerHTML".to_string(),
            arg_index: 0,
            arg_is_non_literal: true,
            arg_kind: SinkArgKind::Other,
            arg_literal: Some(SinkLiteralValue::Integer(511)),
            regex_pattern: None,
            object_properties: vec![SinkObjectProperty {
                key: "origin".to_string(),
                value: SinkLiteralValue::String("*".to_string()),
            }],
            object_property_keys: vec!["origin".to_string()],
            object_property_keys_complete: true,
            arg_idents: vec!["userInput".to_string()],
            arg_source_paths: vec!["req.body.email".to_string(), "req.body".to_string()],
            span_start: 10,
            span_end: 20,
            url_arg_literal: Some("https://api.example.com".to_string()),
            url_shape: Some(SecurityUrlShape::FixedOriginDynamicPath),
        };
        let encoded = bitcode::encode(&site);
        let decoded: SinkSite = bitcode::decode(&encoded).expect("decode sink site");
        assert_eq!(decoded.sink_shape, site.sink_shape);
        assert_eq!(decoded.callee_path, site.callee_path);
        assert_eq!(decoded.arg_index, site.arg_index);
        assert_eq!(decoded.arg_is_non_literal, site.arg_is_non_literal);
        assert_eq!(decoded.arg_kind, site.arg_kind);
        assert_eq!(decoded.arg_literal, site.arg_literal);
        assert_eq!(decoded.object_properties, site.object_properties);
        assert_eq!(decoded.object_property_keys, site.object_property_keys);
        assert_eq!(
            decoded.object_property_keys_complete,
            site.object_property_keys_complete
        );
        assert_eq!(decoded.arg_idents, site.arg_idents);
        assert_eq!(decoded.arg_source_paths, site.arg_source_paths);
        assert_eq!(decoded.url_shape, site.url_shape);
        assert_eq!(decoded.span(), site.span());
    }

    #[test]
    fn line_offsets_single_line_no_newline() {
        assert_eq!(compute_line_offsets("hello"), vec![0]);
    }

    #[test]
    fn line_offsets_single_line_with_newline() {
        assert_eq!(compute_line_offsets("hello\n"), vec![0, 6]);
    }

    #[test]
    fn line_offsets_multiple_lines() {
        assert_eq!(compute_line_offsets("abc\ndef\nghi"), vec![0, 4, 8]);
    }

    #[test]
    fn line_offsets_trailing_newline() {
        assert_eq!(compute_line_offsets("abc\ndef\n"), vec![0, 4, 8]);
    }

    #[test]
    fn line_offsets_consecutive_newlines() {
        assert_eq!(compute_line_offsets("\n\n\n"), vec![0, 1, 2, 3]);
    }

    #[test]
    fn line_offsets_multibyte_utf8() {
        assert_eq!(compute_line_offsets("á\n"), vec![0, 3]);
    }

    #[test]
    fn line_col_offset_zero() {
        let offsets = compute_line_offsets("abc\ndef\nghi");
        let (line, col) = byte_offset_to_line_col(&offsets, 0);
        assert_eq!((line, col), (1, 0));
    }

    #[test]
    fn line_col_middle_of_first_line() {
        let offsets = compute_line_offsets("abc\ndef\nghi");
        let (line, col) = byte_offset_to_line_col(&offsets, 2);
        assert_eq!((line, col), (1, 2));
    }

    #[test]
    fn line_col_start_of_second_line() {
        let offsets = compute_line_offsets("abc\ndef\nghi");
        let (line, col) = byte_offset_to_line_col(&offsets, 4);
        assert_eq!((line, col), (2, 0));
    }

    #[test]
    fn line_col_middle_of_second_line() {
        let offsets = compute_line_offsets("abc\ndef\nghi");
        let (line, col) = byte_offset_to_line_col(&offsets, 5);
        assert_eq!((line, col), (2, 1));
    }

    #[test]
    fn line_col_start_of_third_line() {
        let offsets = compute_line_offsets("abc\ndef\nghi");
        let (line, col) = byte_offset_to_line_col(&offsets, 8);
        assert_eq!((line, col), (3, 0));
    }

    #[test]
    fn line_col_end_of_file() {
        let offsets = compute_line_offsets("abc\ndef\nghi");
        let (line, col) = byte_offset_to_line_col(&offsets, 10);
        assert_eq!((line, col), (3, 2));
    }

    #[test]
    fn line_col_single_line() {
        let offsets = compute_line_offsets("hello");
        let (line, col) = byte_offset_to_line_col(&offsets, 3);
        assert_eq!((line, col), (1, 3));
    }

    #[test]
    fn line_col_at_newline_byte() {
        let offsets = compute_line_offsets("abc\ndef");
        let (line, col) = byte_offset_to_line_col(&offsets, 3);
        assert_eq!((line, col), (1, 3));
    }

    #[test]
    fn export_name_matches_str_named() {
        let name = ExportName::Named("foo".to_string());
        assert!(name.matches_str("foo"));
        assert!(!name.matches_str("bar"));
        assert!(!name.matches_str("default"));
    }

    #[test]
    fn export_name_matches_str_default() {
        let name = ExportName::Default;
        assert!(name.matches_str("default"));
        assert!(!name.matches_str("foo"));
    }

    #[test]
    fn export_name_display_named() {
        let name = ExportName::Named("myExport".to_string());
        assert_eq!(name.to_string(), "myExport");
    }

    #[test]
    fn export_name_display_default() {
        let name = ExportName::Default;
        assert_eq!(name.to_string(), "default");
    }

    #[test]
    fn export_name_equality_named() {
        let a = ExportName::Named("foo".to_string());
        let b = ExportName::Named("foo".to_string());
        let c = ExportName::Named("bar".to_string());
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn export_name_equality_default() {
        let a = ExportName::Default;
        let b = ExportName::Default;
        assert_eq!(a, b);
    }

    #[test]
    fn export_name_named_not_equal_to_default() {
        let named = ExportName::Named("default".to_string());
        let default = ExportName::Default;
        assert_ne!(named, default);
    }

    #[test]
    fn export_name_hash_consistency() {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut h1 = DefaultHasher::new();
        let mut h2 = DefaultHasher::new();
        ExportName::Named("foo".to_string()).hash(&mut h1);
        ExportName::Named("foo".to_string()).hash(&mut h2);
        assert_eq!(h1.finish(), h2.finish());
    }

    #[test]
    fn export_name_matches_str_empty_string() {
        let name = ExportName::Named(String::new());
        assert!(name.matches_str(""));
        assert!(!name.matches_str("foo"));
    }

    #[test]
    fn export_name_default_does_not_match_empty() {
        let name = ExportName::Default;
        assert!(!name.matches_str(""));
    }

    #[test]
    fn imported_name_equality() {
        assert_eq!(
            ImportedName::Named("foo".to_string()),
            ImportedName::Named("foo".to_string())
        );
        assert_ne!(
            ImportedName::Named("foo".to_string()),
            ImportedName::Named("bar".to_string())
        );
        assert_eq!(ImportedName::Default, ImportedName::Default);
        assert_eq!(ImportedName::Namespace, ImportedName::Namespace);
        assert_eq!(ImportedName::SideEffect, ImportedName::SideEffect);
        assert_ne!(ImportedName::Default, ImportedName::Namespace);
        assert_ne!(
            ImportedName::Named("default".to_string()),
            ImportedName::Default
        );
    }

    #[test]
    fn member_kind_equality() {
        assert_eq!(MemberKind::EnumMember, MemberKind::EnumMember);
        assert_eq!(MemberKind::ClassMethod, MemberKind::ClassMethod);
        assert_eq!(MemberKind::ClassProperty, MemberKind::ClassProperty);
        assert_eq!(MemberKind::NamespaceMember, MemberKind::NamespaceMember);
        assert_ne!(MemberKind::EnumMember, MemberKind::ClassMethod);
        assert_ne!(MemberKind::ClassMethod, MemberKind::ClassProperty);
        assert_ne!(MemberKind::NamespaceMember, MemberKind::EnumMember);
    }

    #[test]
    fn member_kind_bitcode_roundtrip() {
        let kinds = [
            MemberKind::EnumMember,
            MemberKind::ClassMethod,
            MemberKind::ClassProperty,
            MemberKind::NamespaceMember,
        ];
        for kind in &kinds {
            let bytes = bitcode::encode(kind);
            let decoded: MemberKind = bitcode::decode(&bytes).unwrap();
            assert_eq!(&decoded, kind);
        }
    }

    #[test]
    fn member_access_bitcode_roundtrip() {
        let access = MemberAccess {
            object: "Status".to_string(),
            member: "Active".to_string(),
        };
        let bytes = bitcode::encode(&access);
        let decoded: MemberAccess = bitcode::decode(&bytes).unwrap();
        assert_eq!(decoded.object, "Status");
        assert_eq!(decoded.member, "Active");
    }

    #[test]
    fn line_offsets_crlf_only_counts_lf() {
        let offsets = compute_line_offsets("ab\r\ncd");
        assert_eq!(offsets, vec![0, 4]);
    }

    #[test]
    fn line_col_empty_file_offset_zero() {
        let offsets = compute_line_offsets("");
        let (line, col) = byte_offset_to_line_col(&offsets, 0);
        assert_eq!((line, col), (1, 0));
    }

    // --- VisibilityTag ---

    #[test]
    fn visibility_tag_default_is_none_variant() {
        assert_eq!(VisibilityTag::default(), VisibilityTag::None);
    }

    #[test]
    fn visibility_tag_is_none_only_for_none_variant() {
        assert!(VisibilityTag::None.is_none());
        assert!(!VisibilityTag::Public.is_none());
        assert!(!VisibilityTag::Internal.is_none());
        assert!(!VisibilityTag::Beta.is_none());
        assert!(!VisibilityTag::Alpha.is_none());
        assert!(!VisibilityTag::ExpectedUnused.is_none());
    }

    #[test]
    fn visibility_tag_suppresses_unused_for_api_tags() {
        assert!(VisibilityTag::Public.suppresses_unused());
        assert!(VisibilityTag::Internal.suppresses_unused());
        assert!(VisibilityTag::Beta.suppresses_unused());
        assert!(VisibilityTag::Alpha.suppresses_unused());
    }

    #[test]
    fn visibility_tag_does_not_suppress_none_or_expected_unused() {
        assert!(!VisibilityTag::None.suppresses_unused());
        assert!(!VisibilityTag::ExpectedUnused.suppresses_unused());
    }

    // --- is_public_env_path ---

    #[test]
    fn is_public_env_path_process_env_public_prefix() {
        assert!(is_public_env_path("process.env.NEXT_PUBLIC_API_URL"));
        assert!(is_public_env_path("process.env.VITE_APP_KEY"));
        assert!(is_public_env_path("process.env.REACT_APP_TITLE"));
        assert!(is_public_env_path("process.env.NODE_ENV"));
    }

    #[test]
    fn is_public_env_path_import_meta_env_public_prefix() {
        assert!(is_public_env_path("import.meta.env.VITE_BASE_URL"));
        assert!(is_public_env_path("import.meta.env.PUBLIC_API"));
    }

    #[test]
    fn is_public_env_path_secret_env_vars_are_not_public() {
        assert!(!is_public_env_path("process.env.SECRET_KEY"));
        assert!(!is_public_env_path("process.env.DATABASE_PASSWORD"));
        assert!(!is_public_env_path("import.meta.env.API_TOKEN"));
    }

    #[test]
    fn is_public_env_path_non_env_paths_are_not_public() {
        assert!(!is_public_env_path("req.query.id"));
        assert!(!is_public_env_path("process.argv"));
        assert!(!is_public_env_path("window.location.href"));
    }

    // --- is_public_env_var edge cases ---

    #[test]
    fn is_public_env_var_exact_matches() {
        assert!(is_public_env_var("NODE_ENV"));
    }

    #[test]
    fn is_public_env_var_all_known_prefixes() {
        assert!(is_public_env_var("NUXT_PUBLIC_API_URL"));
        assert!(is_public_env_var("PUBLIC_API_KEY"));
        assert!(is_public_env_var("GATSBY_APP_ID"));
        assert!(is_public_env_var("EXPO_PUBLIC_SENTRY_DSN"));
        assert!(is_public_env_var("STORYBOOK_ENV"));
    }

    #[test]
    fn is_public_env_var_secret_token_beats_metadata_token() {
        // "SECRET_SHA": has SECRET (wins) and SHA (metadata); should NOT be public
        assert!(!is_public_env_var("SECRET_SHA"));
        // "REF_TOKEN": has TOKEN (secret) and REF (metadata); should NOT be public
        assert!(!is_public_env_var("REF_TOKEN"));
    }

    #[test]
    fn is_public_env_var_plain_unknown_names_are_not_public() {
        assert!(!is_public_env_var("MY_SERVICE_URL"));
        assert!(!is_public_env_var("FEATURE_FLAG"));
        assert!(!is_public_env_var("DATABASE_URL"));
    }

    // --- SinkSite::span ---

    #[test]
    fn sink_site_span_reconstructs_from_offsets() {
        let site = SinkSite {
            sink_shape: SinkShape::Call,
            callee_path: "eval".to_string(),
            arg_index: 0,
            arg_is_non_literal: true,
            arg_kind: SinkArgKind::Other,
            arg_literal: None,
            regex_pattern: None,
            object_properties: Vec::new(),
            object_property_keys: Vec::new(),
            object_property_keys_complete: false,
            arg_idents: Vec::new(),
            arg_source_paths: Vec::new(),
            span_start: 5,
            span_end: 15,
            url_arg_literal: None,
            url_shape: None,
        };
        let s = site.span();
        assert_eq!(s.start, 5);
        assert_eq!(s.end, 15);
    }

    // --- SecurityControlKind ---

    #[test]
    fn security_control_kind_equality_and_ordering() {
        assert_eq!(
            SecurityControlKind::Sanitization,
            SecurityControlKind::Sanitization
        );
        assert_eq!(
            SecurityControlKind::Validation,
            SecurityControlKind::Validation
        );
        assert_ne!(
            SecurityControlKind::Sanitization,
            SecurityControlKind::Validation
        );
        assert!(SecurityControlKind::Sanitization < SecurityControlKind::Validation);
        assert!(SecurityControlKind::Authentication < SecurityControlKind::Authorization);
    }

    // --- SanitizerScope ---

    #[test]
    fn sanitizer_scope_equality_and_ordering() {
        assert_eq!(SanitizerScope::Html, SanitizerScope::Html);
        assert_eq!(SanitizerScope::Url, SanitizerScope::Url);
        assert_eq!(SanitizerScope::Path, SanitizerScope::Path);
        assert_eq!(SanitizerScope::SqlIdentifier, SanitizerScope::SqlIdentifier);
        assert_ne!(SanitizerScope::Html, SanitizerScope::Url);
        assert!(SanitizerScope::Html < SanitizerScope::Url);
    }

    // --- SkippedSecurityCalleeReason ---

    #[test]
    fn skipped_security_callee_reason_equality() {
        assert_eq!(
            SkippedSecurityCalleeReason::ComputedMember,
            SkippedSecurityCalleeReason::ComputedMember
        );
        assert_ne!(
            SkippedSecurityCalleeReason::ComputedMember,
            SkippedSecurityCalleeReason::DynamicDispatch
        );
        assert_ne!(
            SkippedSecurityCalleeReason::DynamicDispatch,
            SkippedSecurityCalleeReason::UnsupportedAssignmentObject
        );
    }

    // --- SkippedSecurityCalleeExpressionKind ---

    #[test]
    fn skipped_security_callee_expression_kind_equality() {
        use SkippedSecurityCalleeExpressionKind as K;
        assert_eq!(K::StaticMemberExpression, K::StaticMemberExpression);
        assert_eq!(K::ComputedMemberExpression, K::ComputedMemberExpression);
        assert_eq!(K::Identifier, K::Identifier);
        assert_eq!(K::Other, K::Other);
        assert_ne!(K::StaticMemberExpression, K::ComputedMemberExpression);
        assert_ne!(K::Identifier, K::Other);
    }

    // --- SinkLiteralValue ---

    #[test]
    fn sink_literal_value_equality() {
        assert_eq!(
            SinkLiteralValue::String("x".to_string()),
            SinkLiteralValue::String("x".to_string())
        );
        assert_ne!(
            SinkLiteralValue::String("x".to_string()),
            SinkLiteralValue::String("y".to_string())
        );
        assert_eq!(SinkLiteralValue::Integer(42), SinkLiteralValue::Integer(42));
        assert_ne!(SinkLiteralValue::Integer(1), SinkLiteralValue::Integer(2));
        assert_eq!(
            SinkLiteralValue::Boolean(true),
            SinkLiteralValue::Boolean(true)
        );
        assert_ne!(
            SinkLiteralValue::Boolean(true),
            SinkLiteralValue::Boolean(false)
        );
        assert_eq!(SinkLiteralValue::Null, SinkLiteralValue::Null);
        assert_ne!(SinkLiteralValue::Null, SinkLiteralValue::Boolean(false));
    }

    // --- SecurityUrlShape ---

    #[test]
    fn security_url_shape_equality() {
        assert_eq!(
            SecurityUrlShape::FixedOriginDynamicPath,
            SecurityUrlShape::FixedOriginDynamicPath
        );
        assert_eq!(
            SecurityUrlShape::DynamicOrigin,
            SecurityUrlShape::DynamicOrigin
        );
        assert_ne!(
            SecurityUrlShape::FixedOriginDynamicPath,
            SecurityUrlShape::DynamicOrigin
        );
    }

    // --- FlagUseKind ---

    #[test]
    fn flag_use_kind_equality() {
        assert_eq!(FlagUseKind::EnvVar, FlagUseKind::EnvVar);
        assert_eq!(FlagUseKind::SdkCall, FlagUseKind::SdkCall);
        assert_eq!(FlagUseKind::ConfigObject, FlagUseKind::ConfigObject);
        assert_ne!(FlagUseKind::EnvVar, FlagUseKind::SdkCall);
        assert_ne!(FlagUseKind::SdkCall, FlagUseKind::ConfigObject);
    }

    // --- ComplexityMetric ---

    #[test]
    fn complexity_metric_equality() {
        assert_eq!(ComplexityMetric::Cyclomatic, ComplexityMetric::Cyclomatic);
        assert_eq!(ComplexityMetric::Cognitive, ComplexityMetric::Cognitive);
        assert_ne!(ComplexityMetric::Cyclomatic, ComplexityMetric::Cognitive);
    }

    // --- ComplexityContributionKind ---

    #[test]
    fn complexity_contribution_kind_equality_spot_check() {
        use ComplexityContributionKind as K;
        assert_eq!(K::If, K::If);
        assert_eq!(K::Else, K::Else);
        assert_eq!(K::ElseIf, K::ElseIf);
        assert_eq!(K::Ternary, K::Ternary);
        assert_eq!(K::LogicalAnd, K::LogicalAnd);
        assert_eq!(K::LogicalOr, K::LogicalOr);
        assert_eq!(K::NullishCoalescing, K::NullishCoalescing);
        assert_eq!(K::LogicalAssignment, K::LogicalAssignment);
        assert_eq!(K::OptionalChain, K::OptionalChain);
        assert_eq!(K::For, K::For);
        assert_eq!(K::ForIn, K::ForIn);
        assert_eq!(K::ForOf, K::ForOf);
        assert_eq!(K::While, K::While);
        assert_eq!(K::DoWhile, K::DoWhile);
        assert_eq!(K::Switch, K::Switch);
        assert_eq!(K::Case, K::Case);
        assert_eq!(K::Catch, K::Catch);
        assert_eq!(K::LabeledBreak, K::LabeledBreak);
        assert_eq!(K::LabeledContinue, K::LabeledContinue);
        assert_eq!(K::JsxDepth, K::JsxDepth);
        assert_eq!(K::HookDensity, K::HookDensity);
        assert_eq!(K::PropCount, K::PropCount);
        assert_ne!(K::If, K::Else);
        assert_ne!(K::For, K::While);
        assert_ne!(K::Switch, K::Case);
    }

    // --- MisplacedDirectiveSite ---

    #[test]
    fn misplaced_directive_site_equality() {
        let client = MisplacedDirectiveSite {
            is_server: false,
            span_start: 10,
        };
        let server = MisplacedDirectiveSite {
            is_server: true,
            span_start: 10,
        };
        let client2 = MisplacedDirectiveSite {
            is_server: false,
            span_start: 10,
        };
        assert_eq!(client, client2);
        assert_ne!(client, server);
    }

    #[test]
    fn misplaced_directive_site_is_server_flag() {
        let site = MisplacedDirectiveSite {
            is_server: true,
            span_start: 42,
        };
        assert!(site.is_server);
        assert_eq!(site.span_start, 42);

        let client_site = MisplacedDirectiveSite {
            is_server: false,
            span_start: 0,
        };
        assert!(!client_site.is_server);
    }

    // --- DiRole / DiFramework ---

    #[test]
    fn di_role_equality() {
        assert_eq!(DiRole::Provide, DiRole::Provide);
        assert_eq!(DiRole::Inject, DiRole::Inject);
        assert_ne!(DiRole::Provide, DiRole::Inject);
    }

    #[test]
    fn di_framework_equality() {
        assert_eq!(DiFramework::Vue, DiFramework::Vue);
        assert_eq!(DiFramework::Svelte, DiFramework::Svelte);
        assert_eq!(DiFramework::Angular, DiFramework::Angular);
        assert_ne!(DiFramework::Vue, DiFramework::Svelte);
        assert_ne!(DiFramework::Svelte, DiFramework::Angular);
    }

    // --- ComponentEmit ---

    #[test]
    fn component_emit_equality() {
        let a = ComponentEmit {
            name: "close".to_string(),
            span_start: 10,
            used: true,
        };
        let b = ComponentEmit {
            name: "close".to_string(),
            span_start: 10,
            used: true,
        };
        let different_used = ComponentEmit {
            name: "close".to_string(),
            span_start: 10,
            used: false,
        };
        let different_name = ComponentEmit {
            name: "open".to_string(),
            span_start: 10,
            used: true,
        };
        assert_eq!(a, b);
        assert_ne!(a, different_used);
        assert_ne!(a, different_name);
    }

    // --- DispatchedEvent ---

    #[test]
    fn dispatched_event_equality() {
        let a = DispatchedEvent {
            name: "myEvent".to_string(),
            span_start: 20,
        };
        let b = DispatchedEvent {
            name: "myEvent".to_string(),
            span_start: 20,
        };
        let c = DispatchedEvent {
            name: "otherEvent".to_string(),
            span_start: 20,
        };
        let d = DispatchedEvent {
            name: "myEvent".to_string(),
            span_start: 99,
        };
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_ne!(a, d);
    }

    // --- AngularInputMember / AngularOutputMember ---

    #[test]
    fn angular_input_member_equality() {
        let a = AngularInputMember {
            name: "title".to_string(),
            span_start: 5,
        };
        let b = AngularInputMember {
            name: "title".to_string(),
            span_start: 5,
        };
        let c = AngularInputMember {
            name: "label".to_string(),
            span_start: 5,
        };
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn angular_output_member_equality() {
        let a = AngularOutputMember {
            name: "clicked".to_string(),
            span_start: 8,
        };
        let b = AngularOutputMember {
            name: "clicked".to_string(),
            span_start: 8,
        };
        let c = AngularOutputMember {
            name: "hovered".to_string(),
            span_start: 8,
        };
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    // --- AngularComponentSelector ---

    #[test]
    fn angular_component_selector_fields() {
        let s = AngularComponentSelector {
            selectors: vec!["app-foo".to_string(), "[appFoo]".to_string()],
            span_start: 100,
            class_name: "FooComponent".to_string(),
        };
        assert_eq!(s.selectors.len(), 2);
        assert_eq!(s.selectors[0], "app-foo");
        assert_eq!(s.selectors[1], "[appFoo]");
        assert_eq!(s.class_name, "FooComponent");
    }

    #[test]
    fn angular_component_selector_equality() {
        let a = AngularComponentSelector {
            selectors: vec!["app-bar".to_string()],
            span_start: 0,
            class_name: "BarComponent".to_string(),
        };
        let b = AngularComponentSelector {
            selectors: vec!["app-bar".to_string()],
            span_start: 0,
            class_name: "BarComponent".to_string(),
        };
        let c = AngularComponentSelector {
            selectors: vec!["app-baz".to_string()],
            span_start: 0,
            class_name: "BazComponent".to_string(),
        };
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    // --- LoadReturnKey ---

    #[test]
    fn load_return_key_equality() {
        let a = LoadReturnKey {
            name: "user".to_string(),
            span_start: 50,
            span_end: 54,
        };
        let b = LoadReturnKey {
            name: "user".to_string(),
            span_start: 50,
            span_end: 54,
        };
        let c = LoadReturnKey {
            name: "posts".to_string(),
            span_start: 50,
            span_end: 55,
        };
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn load_return_key_span_fields() {
        let key = LoadReturnKey {
            name: "data".to_string(),
            span_start: 10,
            span_end: 14,
        };
        assert_eq!(key.span_start, 10);
        assert_eq!(key.span_end, 14);
        assert_eq!(key.name, "data");
    }

    // --- ComponentFunctionKind ---

    #[test]
    fn component_function_kind_equality() {
        assert_eq!(ComponentFunctionKind::FnDecl, ComponentFunctionKind::FnDecl);
        assert_eq!(ComponentFunctionKind::Arrow, ComponentFunctionKind::Arrow);
        assert_eq!(
            ComponentFunctionKind::ForwardRefWrapper,
            ComponentFunctionKind::ForwardRefWrapper
        );
        assert_eq!(
            ComponentFunctionKind::MemoWrapper,
            ComponentFunctionKind::MemoWrapper
        );
        assert_ne!(ComponentFunctionKind::FnDecl, ComponentFunctionKind::Arrow);
        assert_ne!(
            ComponentFunctionKind::ForwardRefWrapper,
            ComponentFunctionKind::MemoWrapper
        );
    }

    // --- HookUseKind ---

    #[test]
    fn hook_use_kind_equality() {
        assert_eq!(HookUseKind::UseState, HookUseKind::UseState);
        assert_eq!(HookUseKind::UseEffect, HookUseKind::UseEffect);
        assert_eq!(HookUseKind::UseMemo, HookUseKind::UseMemo);
        assert_eq!(HookUseKind::UseCallback, HookUseKind::UseCallback);
        assert_eq!(HookUseKind::Custom, HookUseKind::Custom);
        assert_ne!(HookUseKind::UseState, HookUseKind::UseEffect);
        assert_ne!(HookUseKind::UseMemo, HookUseKind::Custom);
    }

    // --- HookUse ---

    #[test]
    fn hook_use_fields() {
        let h = HookUse {
            kind: HookUseKind::UseEffect,
            dep_array_arity: Some(2),
            span_start: 30,
            component: "Widget".to_string(),
        };
        assert_eq!(h.kind, HookUseKind::UseEffect);
        assert_eq!(h.dep_array_arity, Some(2));
        assert_eq!(h.span_start, 30);
        assert_eq!(h.component, "Widget");
    }

    #[test]
    fn hook_use_no_dep_array() {
        let h = HookUse {
            kind: HookUseKind::UseCallback,
            dep_array_arity: None,
            span_start: 0,
            component: String::new(),
        };
        assert!(h.dep_array_arity.is_none());
    }

    // --- MemberKind::StoreMember (missed in existing bitcode test) ---

    #[test]
    fn member_kind_store_member_bitcode_roundtrip() {
        let kind = MemberKind::StoreMember;
        let bytes = bitcode::encode(&kind);
        let decoded: MemberKind = bitcode::decode(&bytes).unwrap();
        assert_eq!(decoded, kind);
    }

    // --- RenderEdge / ForwardAttr ---

    #[test]
    fn render_edge_fields() {
        let edge = RenderEdge {
            parent_component: "Parent".to_string(),
            child_component_name: "Child".to_string(),
            attr_names: vec!["title".to_string(), "onClick".to_string()],
            has_spread: false,
            forward_attrs: vec![ForwardAttr {
                attr: "title".to_string(),
                root: "props".to_string(),
            }],
            has_complex_forward: false,
        };
        assert_eq!(edge.parent_component, "Parent");
        assert_eq!(edge.child_component_name, "Child");
        assert_eq!(edge.attr_names.len(), 2);
        assert!(!edge.has_spread);
        assert_eq!(edge.forward_attrs.len(), 1);
        assert_eq!(edge.forward_attrs[0].attr, "title");
        assert_eq!(edge.forward_attrs[0].root, "props");
        assert!(!edge.has_complex_forward);
    }

    #[test]
    fn render_edge_with_spread() {
        let edge = RenderEdge {
            parent_component: "Wrapper".to_string(),
            child_component_name: "Inner".to_string(),
            attr_names: Vec::new(),
            has_spread: true,
            forward_attrs: Vec::new(),
            has_complex_forward: true,
        };
        assert!(edge.has_spread);
        assert!(edge.has_complex_forward);
    }

    // --- ComponentFunction ---

    #[test]
    fn component_function_fields() {
        let cf = ComponentFunction {
            name: "MyButton".to_string(),
            span_start: 0,
            kind: ComponentFunctionKind::Arrow,
            is_exported: true,
            has_unharvestable_props: false,
            uses_clone_element: false,
            renders_provider: false,
            has_children_as_function: false,
            is_pure_passthrough: false,
        };
        assert_eq!(cf.name, "MyButton");
        assert_eq!(cf.kind, ComponentFunctionKind::Arrow);
        assert!(cf.is_exported);
        assert!(!cf.has_unharvestable_props);
        assert!(!cf.is_pure_passthrough);
    }

    #[test]
    fn component_function_passthrough_flag() {
        let cf = ComponentFunction {
            name: "Passthrough".to_string(),
            span_start: 5,
            kind: ComponentFunctionKind::FnDecl,
            is_exported: false,
            has_unharvestable_props: false,
            uses_clone_element: false,
            renders_provider: false,
            has_children_as_function: false,
            is_pure_passthrough: true,
        };
        assert!(cf.is_pure_passthrough);
        assert!(!cf.is_exported);
    }

    // --- DiKeySite ---

    #[test]
    fn di_key_site_fields() {
        let site = DiKeySite {
            key_local: "MY_KEY".to_string(),
            role: DiRole::Provide,
            framework: DiFramework::Vue,
            span_start: 77,
        };
        assert_eq!(site.key_local, "MY_KEY");
        assert_eq!(site.role, DiRole::Provide);
        assert_eq!(site.framework, DiFramework::Vue);
        assert_eq!(site.span_start, 77);
    }

    #[test]
    fn di_key_site_inject_svelte() {
        let site = DiKeySite {
            key_local: "ctx_key".to_string(),
            role: DiRole::Inject,
            framework: DiFramework::Svelte,
            span_start: 0,
        };
        assert_eq!(site.role, DiRole::Inject);
        assert_eq!(site.framework, DiFramework::Svelte);
    }

    // --- release_resolution_payload: page data store whole-use derivation ---

    #[test]
    fn release_payload_derives_page_data_store_whole_use_from_page_data() {
        let mut m = minimal_module_info();
        m.whole_object_uses = vec!["page.data".to_string()].into();
        m.release_resolution_payload();
        assert!(m.has_page_data_store_whole_use);
    }

    #[test]
    fn release_payload_derives_page_data_store_whole_use_from_dollar_page_data() {
        let mut m = minimal_module_info();
        m.whole_object_uses = vec!["$page.data".to_string()].into();
        m.release_resolution_payload();
        assert!(m.has_page_data_store_whole_use);
    }

    #[test]
    fn release_payload_does_not_set_page_data_store_whole_use_for_other_names() {
        let mut m = minimal_module_info();
        m.whole_object_uses = vec!["data".to_string(), "page".to_string()].into();
        m.release_resolution_payload();
        assert!(!m.has_page_data_store_whole_use);
    }

    // --- release_resolution_payload: referenced_import_bindings derivation ---

    #[test]
    fn release_payload_referenced_bindings_excludes_empty_local_names() {
        let mut m = minimal_module_info();
        m.imports = vec![
            ImportInfo {
                source: "./styles.css".to_string(),
                imported_name: ImportedName::SideEffect,
                local_name: String::new(), // empty = side-effect import
                is_type_only: false,
                from_style: true,
                span: span(),
                source_span: span(),
            },
            ImportInfo {
                source: "react".to_string(),
                imported_name: ImportedName::Default,
                local_name: "React".to_string(),
                is_type_only: false,
                from_style: false,
                span: span(),
                source_span: span(),
            },
        ];
        m.unused_import_bindings = vec!["React".to_string()];
        m.release_resolution_payload();
        // "React" was unused, empty local is filtered; result should be empty
        assert!(m.referenced_import_bindings.is_empty());
    }

    #[test]
    fn release_payload_referenced_bindings_sorted_and_deduped() {
        let mut m = minimal_module_info();
        // Two imports with the same local name (unusual but possible via re-exports)
        m.imports = vec![
            ImportInfo {
                source: "a".to_string(),
                imported_name: ImportedName::Named("foo".to_string()),
                local_name: "foo".to_string(),
                is_type_only: false,
                from_style: false,
                span: span(),
                source_span: span(),
            },
            ImportInfo {
                source: "b".to_string(),
                imported_name: ImportedName::Named("bar".to_string()),
                local_name: "bar".to_string(),
                is_type_only: false,
                from_style: false,
                span: span(),
                source_span: span(),
            },
            ImportInfo {
                source: "c".to_string(),
                imported_name: ImportedName::Named("foo".to_string()),
                local_name: "foo".to_string(),
                is_type_only: false,
                from_style: false,
                span: span(),
                source_span: span(),
            },
        ];
        m.unused_import_bindings = Vec::new();
        m.release_resolution_payload();
        // sorted: ["bar", "foo"] with "foo" deduped
        assert_eq!(
            m.referenced_import_bindings,
            vec!["bar".to_string(), "foo".to_string()]
        );
    }

    // --- CalleeUse ---

    #[test]
    fn callee_use_fields() {
        let cu = CalleeUse {
            callee_path: "child_process.exec".to_string(),
            span_start: 100,
        };
        assert_eq!(cu.callee_path, "child_process.exec");
        assert_eq!(cu.span_start, 100);
    }

    // --- Helper to build a minimal ModuleInfo for targeted tests ---

    fn minimal_module_info() -> ModuleInfo {
        ModuleInfo {
            file_id: FileId(0),
            exports: Vec::new(),
            imports: Vec::new(),
            re_exports: Vec::new(),
            dynamic_imports: Vec::new(),
            dynamic_import_patterns: Vec::new(),
            require_calls: Vec::new(),
            package_path_references: Box::default(),
            member_accesses: Vec::new(),
            semantic_facts: Box::default(),
            whole_object_uses: Box::default(),
            has_cjs_exports: false,
            has_angular_component_template_url: false,
            content_hash: 0,
            suppressions: Vec::new(),
            unknown_suppression_kinds: Vec::new(),
            unused_import_bindings: Vec::new(),
            type_referenced_import_bindings: Vec::new(),
            value_referenced_import_bindings: Vec::new(),
            line_offsets: Vec::new(),
            complexity: Vec::new(),
            flag_uses: Vec::new(),
            class_heritage: Vec::new(),
            exported_factory_returns: Box::default(),
            type_member_types: Box::default(),
            injection_tokens: Vec::new(),
            local_type_declarations: Vec::new(),
            public_signature_type_references: Vec::new(),
            namespace_object_aliases: Vec::new(),
            iconify_prefixes: Vec::new(),
            iconify_icon_names: Vec::new(),
            auto_import_candidates: Vec::new(),
            directives: Vec::new(),
            client_only_dynamic_import_spans: Vec::new(),
            security_sinks: Vec::new(),
            security_sinks_skipped: 0,
            security_unresolved_callee_sites: Vec::new(),
            tainted_bindings: Vec::new(),
            sanitized_sink_args: Vec::new(),
            security_control_sites: Vec::new(),
            callee_uses: Vec::new(),
            misplaced_directives: Vec::new(),
            inline_server_action_exports: Vec::new(),
            di_key_sites: Vec::new(),
            has_dynamic_provide: false,
            referenced_import_bindings: Vec::new(),
            component_props: Vec::new(),
            has_props_attrs_fallthrough: false,
            has_define_expose: false,
            has_define_model: false,
            has_unharvestable_props: false,
            component_emits: Vec::new(),
            angular_inputs: Vec::new(),
            angular_outputs: Vec::new(),
            angular_component_selectors: Vec::new(),
            registered_custom_elements: Vec::new(),
            used_custom_element_tags: Vec::new(),
            angular_used_selectors: Vec::new(),
            angular_entry_component_refs: Vec::new(),
            has_dynamic_component_render: false,
            has_unharvestable_emits: false,
            has_dynamic_emit: false,
            has_emit_whole_object_use: false,
            load_return_keys: Vec::new(),
            has_unharvestable_load: false,
            has_load_data_whole_use: false,
            has_page_data_store_whole_use: false,
            component_functions: Vec::new(),
            react_props: Vec::new(),
            hook_uses: Vec::new(),
            render_edges: Vec::new(),
            svelte_dispatched_events: Vec::new(),
            svelte_listened_events: Vec::new(),
            has_dynamic_dispatch: false,
        }
    }

    fn push_semantic_fact(module: &mut ModuleInfo, fact: SemanticFact) {
        let mut facts = std::mem::take(&mut module.semantic_facts).into_vec();
        facts.push(fact);
        module.semantic_facts = facts.into_boxed_slice();
    }

    #[test]
    fn dynamic_custom_element_render_helper_prefers_typed_fact() {
        let mut module = minimal_module_info();
        push_semantic_fact(
            &mut module,
            SemanticFact::DynamicCustomElementRender(DynamicCustomElementRenderFact),
        );

        assert!(has_dynamic_custom_element_render(&module));
    }

    #[test]
    fn function_complexity_bitcode_roundtrip() {
        let fc = FunctionComplexity {
            name: "processData".to_string(),
            line: 42,
            col: 4,
            cyclomatic: 15,
            cognitive: 25,
            line_count: 80,
            param_count: 3,
            react_hook_count: 0,
            react_jsx_max_depth: 0,
            react_prop_count: 0,
            source_hash: Some("0123456789abcdef".to_string()),
            contributions: vec![
                ComplexityContribution {
                    line: 43,
                    col: 8,
                    metric: ComplexityMetric::Cyclomatic,
                    kind: ComplexityContributionKind::If,
                    weight: 1,
                    nesting: 0,
                },
                ComplexityContribution {
                    line: 45,
                    col: 12,
                    metric: ComplexityMetric::Cognitive,
                    kind: ComplexityContributionKind::ElseIf,
                    weight: 3,
                    nesting: 2,
                },
            ],
        };
        let bytes = bitcode::encode(&fc);
        let decoded: FunctionComplexity = bitcode::decode(&bytes).unwrap();
        assert_eq!(decoded.name, "processData");
        assert_eq!(decoded.line, 42);
        assert_eq!(decoded.col, 4);
        assert_eq!(decoded.cyclomatic, 15);
        assert_eq!(decoded.cognitive, 25);
        assert_eq!(decoded.line_count, 80);
        assert_eq!(decoded.source_hash.as_deref(), Some("0123456789abcdef"));
        assert_eq!(decoded.contributions.len(), 2);
        assert_eq!(
            decoded.contributions[1].kind,
            ComplexityContributionKind::ElseIf
        );
        assert_eq!(decoded.contributions[1].weight, 3);
        assert_eq!(decoded.contributions[1].nesting, 2);
        assert_eq!(decoded.contributions[1].metric, ComplexityMetric::Cognitive);
    }
}
