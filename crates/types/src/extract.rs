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
    pub package_path_references: Vec<String>,
    /// Static member access expressions (e.g., `Status.Active`).
    pub member_accesses: Vec<MemberAccess>,
    /// Identifiers used in whole-object access patterns.
    pub whole_object_uses: Vec<String>,
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
    /// `unused_import_bindings` among `imports`. Derived in
    /// `release_resolution_payload` (where both `imports` and
    /// `unused_import_bindings` are still present) so it survives the release and
    /// is readable by the analyze layer; it is never cached (recomputed on every
    /// cache load). Consumed by the `unrendered-component` detector to credit a
    /// Vue/Svelte SFC that some file actually imports-and-uses, distinguishing it
    /// from a component reachable only through a barrel re-export.
    pub referenced_import_bindings: Vec<String>,
    /// Vue `<script setup>` `defineProps` declared props. Consumed by the
    /// `unused-component-prop` detector to flag a prop referenced nowhere in its
    /// own SFC. Each entry carries `used_in_script` / `used_in_template`.
    pub component_props: Vec<ComponentProp>,
    /// `true` when the template spreads the whole props/attrs object
    /// (`v-bind="$attrs"` / `v-bind="$props"` / `v-bind="props"`) or the
    /// `defineProps` return is destructured with a rest element. Either form can
    /// consume a prop indirectly, so the detector abstains on the whole file.
    pub has_props_attrs_fallthrough: bool,
    /// `true` when the SFC calls `defineExpose(...)`. A prop may be re-exposed,
    /// so the detector conservatively abstains on the whole file.
    pub has_define_expose: bool,
    /// `true` when the SFC calls `defineModel(...)`. Two-way model props are out
    /// of scope for v1, so the detector abstains on the whole file.
    pub has_define_model: bool,
    /// `true` when `defineProps` was called with an unharvestable argument (a
    /// type-reference type argument such as `defineProps<Props>()` whose names
    /// require cross-file type resolution). The detector abstains on the whole
    /// file so a prop is never falsely flagged.
    pub has_unharvestable_props: bool,
    /// Vue `<script setup>` `defineEmits` declared events. Consumed by the
    /// `unused-component-emit` detector to flag an event emitted nowhere in its
    /// own SFC. Each entry carries `used`.
    pub component_emits: Vec<ComponentEmit>,
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
    /// `unused-load-data-key` detector's project-wide abstain. Derived in
    /// `release_resolution_payload` from `whole_object_uses` BEFORE that vector is
    /// released (mirroring `referenced_import_bindings`), so it survives the
    /// release the detector runs after; it is never cached (recomputed each run
    /// from the cached `whole_object_uses`). Reassignment forms
    /// (`const all = $page.data`) are not whole-object-tracked and stay out of
    /// scope, matching the syntactic analyzer's conservative posture.
    pub has_page_data_store_whole_use: bool,
}

impl ModuleInfo {
    /// Release extraction payload that resolution has already copied into the graph.
    ///
    /// This keeps fields needed by analysis, health, security, LSP, coverage,
    /// and hash drift checks, while dropping vectors that otherwise duplicate
    /// data owned by `ResolvedModule` or already credited into the module graph.
    pub fn release_resolution_payload(&mut self) {
        // Derive the referenced-binding set BEFORE releasing `unused_import_bindings`:
        // the analyze-layer `unrendered-component` detector needs "which imports are
        // actually used" but runs after this release, so capture the compact
        // complement here. Skip empty local names (side-effect imports).
        self.referenced_import_bindings = self
            .imports
            .iter()
            .map(|import| import.local_name.clone())
            .filter(|name| !name.is_empty() && !self.unused_import_bindings.contains(name))
            .collect();
        self.referenced_import_bindings.sort_unstable();
        self.referenced_import_bindings.dedup();

        // Derive the project-wide page-data-store whole-use signal BEFORE
        // releasing `whole_object_uses`: the `unused-load-data-key` detector runs
        // after this release and needs to know whether ANY module reflectively
        // consumes the whole `page.data` / `$page.data` store.
        self.has_page_data_store_whole_use = self
            .whole_object_uses
            .iter()
            .any(|name| name == "page.data" || name == "$page.data");

        Self::release_vec(&mut self.dynamic_imports);
        Self::release_vec(&mut self.require_calls);
        Self::release_vec(&mut self.package_path_references);
        Self::release_vec(&mut self.whole_object_uses);
        Self::release_vec(&mut self.unused_import_bindings);
        Self::release_vec(&mut self.type_referenced_import_bindings);
        Self::release_vec(&mut self.value_referenced_import_bindings);
        Self::release_vec(&mut self.namespace_object_aliases);
        Self::release_vec(&mut self.auto_import_candidates);
    }

    fn release_vec<T>(values: &mut Vec<T>) {
        *values = Vec::new();
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
/// `crates/core/data/security_matchers.toml`.
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
                "source file exceeds u32::MAX bytes — line offsets would overflow"
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
    /// Distinct color values in the stylesheet, in their authored form, sorted.
    /// Distinct notations of the same color (`red` vs `#f00`) count separately,
    /// since inconsistent notation is itself a design-token-sprawl signal.
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
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, bitcode::Encode, bitcode::Decode)]
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

/// A Vue `<script setup>` `defineProps` declared prop, harvested from the
/// runtime object form (`defineProps({ foo: {...} })`) or the inline TS literal
/// form (`defineProps<{ foo: T }>()`). `used_in_script` / `used_in_template`
/// are set during extraction; the `unused-component-prop` detector flags a prop
/// where neither is true. See `harvest_define_props` in `sfc.rs`.
#[derive(Debug, Clone, bitcode::Encode, bitcode::Decode)]
pub struct ComponentProp {
    /// The declared prop name.
    pub name: String,
    /// The template/script-visible local binding name: the destructure alias for
    /// `const { name: alias } = defineProps()`, otherwise the prop name itself.
    /// A renamed prop is read through this local, so usage must be checked against
    /// it, not the declared name.
    pub local: String,
    /// Start byte offset of the prop declaration (anchors the finding).
    pub span_start: u32,
    /// Whether this prop is referenced in the component's `<script>` (a
    /// destructured local binding with a resolved reference, or a `props.<name>`
    /// member access).
    pub used_in_script: bool,
    /// Whether this prop name is referenced in the component's `<template>`.
    /// Set by `apply_template_usage` when the template scanner credits the name.
    pub used_in_template: bool,
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
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize)]
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
#[derive(Debug, Clone, PartialEq, Eq)]
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
const _: () = assert!(std::mem::size_of::<ExportInfo>() == 112);
#[cfg(target_pointer_width = "64")]
const _: () = assert!(std::mem::size_of::<ImportInfo>() == 96);
#[cfg(target_pointer_width = "64")]
const _: () = assert!(std::mem::size_of::<ExportName>() == 24);
#[cfg(target_pointer_width = "64")]
const _: () = assert!(std::mem::size_of::<ImportedName>() == 24);
#[cfg(target_pointer_width = "64")]
const _: () = assert!(std::mem::size_of::<MemberAccess>() == 48);
#[cfg(target_pointer_width = "64")]
const _: () = assert!(std::mem::size_of::<SinkSite>() == 216);
#[cfg(target_pointer_width = "64")]
const _: () = assert!(std::mem::size_of::<ModuleInfo>() == 968);

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
            assert_eq!($values.capacity(), 0);
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
            package_path_references: vec!["react".to_string()],
            member_accesses: vec![MemberAccess {
                object: "Status".to_string(),
                member: "Active".to_string(),
            }],
            whole_object_uses: vec!["Status".to_string()],
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
            di_key_sites: Vec::new(),
            has_dynamic_provide: false,
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
            load_return_keys: Vec::new(),
            has_unharvestable_load: false,
            has_load_data_whole_use: false,
            has_page_data_store_whole_use: false,
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
