//! Serialization types for the incremental parse cache.
//!
//! All types use bitcode `Encode`/`Decode` for fast binary serialization.

use bitcode::{Decode, Encode};

use crate::MemberKind;

/// Cache version. Bump it when the serialized cache shape or the cached
/// extraction semantics change, and give the reason in the commit message and
/// the CHANGELOG. A stale version serves old extraction results from a warm
/// cache. The `assert_cached_type_size!` guards below catch shape changes.
pub(super) const CACHE_VERSION: u32 = 297;

/// Duplication token cache version. Bump it when duplicate tokenization,
/// normalization, or the on-disk token cache schema changes, and give the
/// reason in the commit message and the CHANGELOG.
pub const DUPES_CACHE_VERSION: u32 = 13;

/// Default maximum cache size (256 MB). Overridable per-project via
/// `cache.maxSizeMb` in the config file or `FALLOW_CACHE_MAX_SIZE` env var.
/// Also used as the hard ceiling on load-time deserialization as a defence
/// against pathological on-disk files.
pub const DEFAULT_CACHE_MAX_SIZE: usize = 256 * 1024 * 1024;

/// Trigger LRU eviction when the serialized cache exceeds 80% of the cap.
/// Basis points (1/100 of a percent) for integer arithmetic without floats.
pub(super) const EVICTION_TRIGGER_BPS: usize = 8000;

/// Evict down to 60% of the cap so subsequent saves leave headroom.
pub(super) const EVICTION_TARGET_BPS: usize = 6000;

/// Promote the eviction log from `debug!` to `info!` when at least 25% of
/// entries are removed in a single save. Default-noise concerns mean
/// small-turnover saves should not be visible without `RUST_LOG=debug`.
pub(super) const EVICTION_SIGNIFICANT_BPS: usize = 2500;

/// Import kind discriminant for `CachedImport`:
/// 0 = Named, 1 = Default, 2 = Namespace, 3 = `SideEffect`.
pub(super) const IMPORT_KIND_NAMED: u8 = 0;
pub(super) const IMPORT_KIND_DEFAULT: u8 = 1;
pub(super) const IMPORT_KIND_NAMESPACE: u8 = 2;
pub(super) const IMPORT_KIND_SIDE_EFFECT: u8 = 3;

macro_rules! assert_cached_type_size {
    ($ty:ty, $size:expr) => {
        const _: () = assert!(
            std::mem::size_of::<$ty>() == $size,
            concat!(
                stringify!($ty),
                " size changed; bump CACHE_VERSION if the cached wire shape or extraction semantics changed, then update this assertion"
            )
        );
    };
}

assert_cached_type_size!(CachedModule, 1360);
assert_cached_type_size!(CachedNamespaceObjectAlias, 72);
assert_cached_type_size!(CachedLocalTypeDeclaration, 32);
assert_cached_type_size!(CachedPublicSignatureTypeReference, 56);
assert_cached_type_size!(CachedSuppression, 88);
assert_cached_type_size!(CachedUnknownSuppressionKind, 56);
assert_cached_type_size!(CachedExport, 152);
assert_cached_type_size!(CachedImport, 96);
assert_cached_type_size!(CachedDynamicImport, 88);
assert_cached_type_size!(CachedRequireCall, 96);
assert_cached_type_size!(CachedReExport, 104);
assert_cached_type_size!(CachedMember, 64);
assert_cached_type_size!(CachedDynamicImportPattern, 64);
assert_cached_type_size!(crate::MemberAccess, 48);
assert_cached_type_size!(fallow_types::extract::SemanticFact, 96);
assert_cached_type_size!(fallow_types::extract::CalleeUse, 32);
assert_cached_type_size!(fallow_types::extract::MisplacedDirectiveSite, 8);
assert_cached_type_size!(fallow_types::extract::SinkSite, 216);
assert_cached_type_size!(fallow_types::extract::FunctionComplexity, 96);
assert_cached_type_size!(fallow_types::extract::ComplexityContribution, 16);
assert_cached_type_size!(fallow_types::extract::FlagUse, 80);
assert_cached_type_size!(fallow_types::extract::ClassHeritageInfo, 168);
assert_cached_type_size!(fallow_types::extract::FactoryReturnExport, 48);
assert_cached_type_size!(fallow_types::extract::TypeMemberTypeEntry, 72);
assert_cached_type_size!(fallow_types::extract::LoadReturnKey, 32);

/// Cached data for a single module.
#[derive(Debug, Clone, Encode, Decode)]
pub struct CachedModule {
    /// xxh3 hash of the file content.
    pub content_hash: u64,
    /// File modification time in nanoseconds for fast cache validation.
    /// When mtime+ctime+size match the on-disk file, we skip reading file
    /// content entirely.
    pub mtime_ns: u64,
    /// File inode change time in nanoseconds, or `0` on platforms that do not
    /// report one. Stored beside `mtime_ns` because mtime is writer-controlled:
    /// a same-length rewrite with a restored mtime is invisible to
    /// `(mtime, size)` alone, and the metadata-only fast path would then hand
    /// back analysis of the previous content.
    pub ctime_ns: u64,
    /// File size in bytes for fast cache validation.
    pub file_size: u64,
    /// Seconds-since-epoch at the time this entry was last WRITTEN
    /// (first parse or content-change refresh). NOT updated on cache-hit
    /// reads: `update_cache` already iterates every in-scope file every run,
    /// so refreshing on read would collapse the LRU to "last run this file
    /// was discovered" for every retained entry. With write-only refresh,
    /// the LRU genuinely targets stale (in-scope-but-unchanged-for-many-runs)
    /// entries. Used by `CacheStore::save` for write-time eviction ordering.
    pub last_access_secs: u64,
    /// Exported symbols.
    pub exports: Vec<CachedExport>,
    /// Import specifiers.
    pub imports: Vec<CachedImport>,
    /// Re-export specifiers.
    pub re_exports: Vec<CachedReExport>,
    /// Dynamic import specifiers.
    pub dynamic_imports: Vec<CachedDynamicImport>,
    /// `require()` specifiers.
    pub require_calls: Vec<CachedRequireCall>,
    /// Package names statically referenced through package path resolution.
    pub package_path_references: Box<[String]>,
    /// Static member accesses (e.g., `Status.Active`).
    pub member_accesses: Vec<crate::MemberAccess>,
    /// Typed semantic facts produced by extraction for cross-layer analysis.
    /// `None` means no facts, which keeps the common warm-cache payload lean.
    pub semantic_facts: Option<Box<[fallow_types::extract::SemanticFact]>>,
    /// Identifiers used as whole objects (Object.values, for..in, spread, etc.).
    pub whole_object_uses: Box<[String]>,
    /// Dynamic import patterns with partial static resolution.
    pub dynamic_import_patterns: Vec<CachedDynamicImportPattern>,
    /// Number of parser diagnostics the parse of this file produced.
    /// Round-trips so a warm load still reports `source-parse-degraded`
    /// instead of silently inheriting a partial parse.
    pub parse_error_count: u32,
    /// Whether the parser abandoned this file instead of recovering.
    pub parse_panicked: bool,
    /// Whether this module uses CJS exports.
    pub has_cjs_exports: bool,
    /// Whether this module declares at least one Angular `@Component({
    /// templateUrl: ... })` decorator. Mirrors `ModuleInfo.has_angular_component_template_url`
    /// so the CRAP-inherit walker's gate survives a warm-cache load.
    pub has_angular_component_template_url: bool,
    /// Local names of import bindings that are never referenced in this file.
    pub unused_import_bindings: Vec<String>,
    /// Local import bindings referenced from type positions.
    pub type_referenced_import_bindings: Vec<String>,
    /// Local import bindings referenced from value positions.
    pub value_referenced_import_bindings: Vec<String>,
    /// Inline suppression directives.
    pub suppressions: Vec<CachedSuppression>,
    /// Suppression tokens that did not parse to any known `IssueKind`. See #449.
    pub unknown_suppression_kinds: Vec<CachedUnknownSuppressionKind>,
    /// Pre-computed line-start byte offsets for O(log N) byte-to-line/col conversion.
    pub line_offsets: Vec<u32>,
    /// Per-function complexity metrics.
    pub complexity: Vec<fallow_types::extract::FunctionComplexity>,
    /// Whether the run that wrote this entry actually extracted complexity.
    ///
    /// An empty `complexity` vector is ambiguous on its own: a `dead-code` run
    /// passes `need_complexity == false` and produces one, and so does a file
    /// with no functions at all. Reading emptiness as "not cached" made every
    /// later `health` run pay a full cold parse for a file that was already
    /// analyzed. This flag is the honest answer, so a complexity consumer can
    /// hit on a rich entry and a complexity-blind run can refuse to downgrade
    /// one.
    pub complexity_extracted: bool,
    /// Feature flag use sites.
    pub flag_uses: Vec<fallow_types::extract::FlagUse>,
    /// Heritage metadata for exported classes.
    pub class_heritage: Vec<fallow_types::extract::ClassHeritageInfo>,
    /// Exported free-function factories that provably return one class instance
    /// (`export function useApi() { return new RESTApi() }`). Compacted to `None`
    /// when empty so the common no-factory module pays no payload. See #1441 Part A.
    pub exported_factory_returns: Option<Box<[fallow_types::extract::FactoryReturnExport]>>,
    /// Object-literal factory-return shapes (`export function createUi() {
    /// return { orders: factory.ordersPage } }`). Compacted to `None` when empty
    /// so the common no-object-factory module pays no payload. See issue #1858.
    pub exported_factory_return_object_shapes:
        Option<Box<[fallow_types::extract::FactoryReturnObjectShapeExport]>>,
    /// Named-type property types declared by top-level interfaces and
    /// type-literal aliases (`interface Opts { c: OptDep }`). Compacted to
    /// `None` when empty so the common no-interface module pays no payload.
    /// See issue #1785.
    pub type_member_types: Option<Box<[fallow_types::extract::TypeMemberTypeEntry]>>,
    /// Angular `InjectionToken<Interface>` `(token, interface)` pairs (#920).
    pub injection_tokens: Vec<(String, String)>,
    /// Local type-capable declarations.
    pub local_type_declarations: Vec<CachedLocalTypeDeclaration>,
    /// Type references from exported public signatures.
    pub public_signature_type_references: Vec<CachedPublicSignatureTypeReference>,
    /// Namespace-import aliases re-exported through an object literal
    /// (`export const API = { foo }` where `foo` is `import * as foo from './bar'`).
    pub namespace_object_aliases: Vec<CachedNamespaceObjectAlias>,
    /// Iconify collection prefixes found in static icon props (issue #608).
    pub iconify_prefixes: Vec<String>,
    /// Nuxt UI icon class suffixes found in static script-side icon properties
    /// (issue #955).
    pub iconify_icon_names: Vec<String>,
    /// Bare identifier names that are candidates for convention auto-import
    /// resolution (issue #704). Content-local, so they round-trip through the
    /// cache; resolution against the plugin table happens at graph-build time.
    pub auto_import_candidates: Vec<String>,
    /// File-level string directives (`"use client"`, `"use server"`). Content-local,
    /// round-trips through the cache so the security `client-server-leak` detector
    /// sees directives on warm-cache loads.
    pub directives: Vec<String>,
    /// Byte-offset starts of `next/dynamic(..., { ssr: false })` dynamic imports.
    /// Content-local, round-trips so the security `client-server-leak` BFS sees
    /// the ssr:false client-only escape hatch on warm-cache loads.
    pub client_only_dynamic_import_spans: Vec<u32>,
    /// Captured security sink sites (category-blind). Round-trips through the
    /// cache so the catalogue-driven `tainted_sink` detector sees sinks on
    /// warm-cache loads.
    pub security_sinks: Vec<fallow_types::extract::SinkSite>,
    /// Count of sink-shaped nodes whose callee could not be flattened to a
    /// static path. Round-trips so the in-band blind-spot count is stable.
    pub security_sinks_skipped: u32,
    /// Span-level diagnostics for skipped security sink callees.
    pub security_unresolved_callee_sites: Vec<fallow_types::extract::SkippedSecurityCalleeSite>,
    /// Local bindings tied to the member-access path they were sourced from.
    /// Round-trips so the security `tainted_sink` source-to-sink association
    /// sees source-tainted bindings on warm-cache loads.
    pub tainted_bindings: Vec<fallow_types::extract::TaintedBinding>,
    /// Direct sink arguments recognized as sanitizer calls.
    pub sanitized_sink_args: Vec<fallow_types::extract::SanitizedSinkArg>,
    /// Defensive control call sites for security surface output.
    pub security_control_sites: Vec<fallow_types::extract::SecurityControlSite>,
    /// Deduped statically flattenable callee paths. Round-trips so the
    /// `boundaries.calls.forbidden` detector sees call sites on warm-cache
    /// loads.
    pub callee_uses: Vec<fallow_types::extract::CalleeUse>,
    /// Misplaced `"use client"` / `"use server"` directive sites.
    /// Round-trips so the `misplaced-directive` detector sees them on
    /// warm-cache loads.
    pub misplaced_directives: Vec<fallow_types::extract::MisplacedDirectiveSite>,
    /// Export local names of inline `"use server"` body Server Actions.
    /// Round-trips so the `unused-server-action` reclassifier sees them on
    /// warm-cache loads.
    pub inline_server_action_exports: Vec<String>,
    /// Vue `provide`/`inject` and Svelte `setContext`/`getContext` key sites.
    /// Round-trips so the `unprovided-inject` detector sees them on warm-cache
    /// loads.
    pub di_key_sites: Vec<fallow_types::extract::DiKeySite>,
    /// Whether the module had an unknowable-key provide. Round-trips so the
    /// `unprovided-inject` project-wide abstain holds on warm-cache loads.
    pub has_dynamic_provide: bool,
    /// Vue `<script setup>` `defineProps` and Svelte 5 `$props()` declared props.
    /// Round-trips so the `unused-component-prop` detector sees them on
    /// warm-cache loads.
    pub component_props: Vec<fallow_types::extract::ComponentProp>,
    /// Whether the template spreads `$attrs`/`$props`/`props` or the
    /// `defineProps` return is rest-destructured. Round-trips for the abstain.
    pub has_props_attrs_fallthrough: bool,
    /// Whether the SFC calls `defineExpose(...)`. Round-trips for the abstain.
    pub has_define_expose: bool,
    /// Whether the SFC calls `defineModel(...)`. Round-trips for the abstain.
    pub has_define_model: bool,
    /// Whether `defineProps` had an unharvestable type-reference argument.
    /// Round-trips for the abstain.
    pub has_unharvestable_props: bool,
    /// Vue `<script setup>` `defineEmits` declared events. Round-trips so the
    /// `unused-component-emit` detector sees them on warm-cache loads.
    pub component_emits: Vec<fallow_types::extract::ComponentEmit>,
    /// Angular component/directive inputs (`@Input()` decorators and signal
    /// `input()` / `model()` initializers). Round-trips so the
    /// `unused-component-input` detector sees them on warm-cache loads.
    pub angular_inputs: Vec<fallow_types::extract::AngularInputMember>,
    /// Angular component/directive outputs (`@Output()` decorators and signal
    /// `output()` / `outputFromObservable()` initializers). Round-trips so the
    /// `unused-component-output` detector sees them on warm-cache loads.
    pub angular_outputs: Vec<fallow_types::extract::AngularOutputMember>,
    /// Angular `@Component` declarations with their `selector` value(s).
    /// Round-trips so the Angular `unrendered-component` arm sees them on
    /// warm-cache loads.
    pub angular_component_selectors: Vec<fallow_types::extract::AngularComponentSelector>,
    /// Lit / web-component custom elements registered in this file. Round-trips so
    /// the Lit `unrendered-component` arm sees them on warm-cache loads.
    pub registered_custom_elements: Vec<fallow_types::extract::RegisteredCustomElement>,
    /// Custom-element tag names used (rendered) in this file's `html` templates.
    /// Round-trips for the Lit `unrendered-component` rendered-tag union.
    pub used_custom_element_tags: Vec<String>,
    /// Custom element selector tags referenced in this file's Angular templates.
    /// Round-trips for the Angular `unrendered-component` used-selector union.
    pub angular_used_selectors: Vec<String>,
    /// Angular route / bootstrap component class references. Round-trips for the
    /// Angular `unrendered-component` entry-point abstain.
    pub angular_entry_component_refs: Vec<String>,
    /// Whether this file dynamically renders a component (project-wide abstain
    /// signal for the Angular `unrendered-component` detector). Round-trips.
    pub has_dynamic_component_render: bool,
    /// Whether `defineEmits` had an unharvestable argument. Round-trips for the
    /// abstain.
    pub has_unharvestable_emits: bool,
    /// Whether an `emit(<nonLiteral>)` call was seen. Round-trips for the abstain.
    pub has_dynamic_emit: bool,
    /// Whether the emit binding was used as a whole value. Round-trips for the
    /// abstain.
    pub has_emit_whole_object_use: bool,
    /// SvelteKit `load()` return-object keys. Round-trips so the
    /// `unused-load-data-key` detector sees them on warm-cache loads.
    pub load_return_keys: Vec<fallow_types::extract::LoadReturnKey>,
    /// Whether this file's `load()` body could not be harvested safely.
    /// Round-trips for the abstain.
    pub has_unharvestable_load: bool,
    /// Whether this file passes the whole `data` object opaquely. Round-trips
    /// for the `unused-load-data-key` abstain.
    pub has_load_data_whole_use: bool,
    /// React/JSX component definitions. Round-trips so the React-health phases
    /// see them on warm-cache loads.
    pub component_functions: Vec<fallow_types::extract::ComponentFunction>,
    /// React component props. Round-trips so the React `unused-component-prop`
    /// arm sees them on warm-cache loads.
    pub react_props: Vec<fallow_types::extract::ComponentProp>,
    /// React hook call sites. Round-trips for the complexity-fold phase.
    pub hook_uses: Vec<fallow_types::extract::HookUse>,
    /// React render edges (child name captured; resolution deferred to graph
    /// build). Round-trips so the render graph survives a warm cache.
    pub render_edges: Vec<fallow_types::extract::RenderEdge>,
    /// Svelte custom events dispatched via `dispatch('<name>')`. Round-trips so
    /// the `unused-svelte-event` detector sees them on warm-cache loads.
    pub svelte_dispatched_events: Vec<fallow_types::extract::DispatchedEvent>,
    /// Svelte template `on:<name>` listener names on component tags. Round-trips
    /// so the project-wide listened set is correct on warm-cache loads.
    pub svelte_listened_events: Vec<String>,
    /// Whether a `dispatch(<nonLiteral>)` call or whole-`dispatch`-value use was
    /// seen. Round-trips for the `unused-svelte-event` abstain.
    pub has_dynamic_dispatch: bool,
}

impl CachedModule {
    /// Source metadata fingerprint stored with this cache entry.
    ///
    #[must_use]
    pub fn source_fingerprint(&self) -> fallow_types::source_fingerprint::SourceFingerprint {
        fallow_types::source_fingerprint::SourceFingerprint::with_ctime(
            self.mtime_ns,
            self.ctime_ns,
            self.file_size,
        )
    }
}

/// Cached namespace-object alias.
#[derive(Debug, Clone, Encode, Decode)]
pub struct CachedNamespaceObjectAlias {
    /// Canonical export name on this module.
    pub via_export_name: String,
    /// Dotted suffix of the property path relative to the export.
    pub suffix: String,
    /// Local name of the namespace import on this module.
    pub namespace_local: String,
}

/// Cached local type declaration.
#[derive(Debug, Clone, Encode, Decode)]
pub struct CachedLocalTypeDeclaration {
    /// Local declaration name.
    pub name: String,
    /// Byte offset of the declaration span start.
    pub span_start: u32,
    /// Byte offset of the declaration span end.
    pub span_end: u32,
}

/// Cached public signature type reference.
#[derive(Debug, Clone, Encode, Decode)]
pub struct CachedPublicSignatureTypeReference {
    /// Exported symbol whose signature contains the reference.
    pub export_name: String,
    /// Referenced type name.
    pub type_name: String,
    /// Byte offset of the reference span start.
    pub span_start: u32,
    /// Byte offset of the reference span end.
    pub span_end: u32,
}

/// Cached suppression directive.
#[derive(Debug, Clone, Encode, Decode)]
pub struct CachedSuppression {
    /// 1-based line this suppression applies to. 0 = file-wide.
    pub(crate) line: u32,
    /// 1-based line where the comment itself appears.
    pub(crate) comment_line: u32,
    /// 0 = suppress all, otherwise `IssueKind` discriminant.
    pub(crate) kind: u8,
    /// Rule-pack name for scoped policy suppressions. Empty for all other
    /// suppression targets.
    pub(crate) policy_pack: String,
    /// Rule id for scoped policy suppressions. Empty for all other suppression
    /// targets.
    pub(crate) policy_rule_id: String,
    /// Human-authored reason after `--`, when present.
    pub(crate) reason: Option<String>,
}

/// Cached unknown suppression kind token (see #449).
#[derive(Debug, Clone, Encode, Decode)]
pub struct CachedUnknownSuppressionKind {
    /// 1-based line where the comment itself appears.
    pub comment_line: u32,
    /// True when the marker was `fallow-ignore-file`.
    pub is_file_level: bool,
    /// The verbatim token that did not parse.
    pub token: String,
    /// Human-authored reason after `--`, when present.
    pub reason: Option<String>,
}

/// Cached export data for a single export declaration.
#[derive(Debug, Clone, Encode, Decode)]
pub struct CachedExport {
    /// Export name (or "default" for default exports).
    pub(crate) name: String,
    /// Whether this is a default export.
    pub(crate) is_default: bool,
    /// Whether this is a type-only export.
    pub(crate) is_type_only: bool,
    /// Whether this export is registered through a runtime side effect at
    /// module load time (Lit `@customElement` decorator or
    /// `customElements.define` call). Persisted so warm-cache runs continue
    /// to skip unused-export reporting for these classes.
    pub(crate) is_side_effect_used: bool,
    /// Visibility tag discriminant (0=None, 1=Public, 2=Internal, 3=Beta, 4=Alpha).
    pub(crate) visibility: u8,
    /// Human-authored reason on `@expected-unused -- <reason>`, when present.
    pub(crate) expected_unused_reason: Option<String>,
    /// Whether the leading JSDoc carries a `@deprecated` tag.
    pub(crate) deprecated: bool,
    /// Plain-text `@deprecated` message, `None` for a bare tag.
    pub(crate) deprecated_reason: Option<Box<str>>,
    /// The local binding name, if different.
    pub(crate) local_name: Option<String>,
    /// Byte offset of the export span start.
    pub(crate) span_start: u32,
    /// Byte offset of the export span end.
    pub(crate) span_end: u32,
    /// Members of this export (for enums and classes).
    pub(crate) members: Vec<CachedMember>,
    /// The local name of the parent class from `extends` clause, if any.
    pub(crate) super_class: Option<String>,
}

/// Cached import data for a single import declaration.
#[derive(Debug, Clone, Encode, Decode)]
pub struct CachedImport {
    /// The import specifier.
    pub(crate) source: String,
    /// For Named imports, the imported symbol name. Empty for other kinds.
    pub(crate) imported_name: String,
    /// The local binding name.
    pub(crate) local_name: String,
    /// Whether this is a type-only import.
    pub(crate) is_type_only: bool,
    /// Whether this whole-module import forwards type meanings only
    /// (`export type *` inside a `declare module '...'` body).
    pub(crate) is_type_only_star: bool,
    /// Whether this import originated from an SFC `<style>` block / `<style src>` (CSS context).
    pub(crate) from_style: bool,
    /// Import kind: 0=Named, 1=Default, 2=Namespace, 3=SideEffect.
    pub(crate) kind: u8,
    /// Byte offset of the import span start.
    pub(crate) span_start: u32,
    /// Byte offset of the import span end.
    pub(crate) span_end: u32,
    /// Byte offset of the source string literal span start.
    pub(crate) source_span_start: u32,
    /// Byte offset of the source string literal span end.
    pub(crate) source_span_end: u32,
}

/// Cached dynamic import data.
#[derive(Debug, Clone, Encode, Decode)]
pub struct CachedDynamicImport {
    /// The import specifier.
    pub(crate) source: String,
    /// Byte offset of the span start.
    pub(crate) span_start: u32,
    /// Byte offset of the span end.
    pub(crate) span_end: u32,
    /// Names destructured from the import result.
    pub(crate) destructured_names: Vec<String>,
    /// Local variable name for namespace imports.
    pub(crate) local_name: Option<String>,
    /// True when this dynamic import was synthesised by fallow (see
    /// `DynamicImportInfo::is_speculative`).
    pub(crate) is_speculative: bool,
}

/// Cached `require()` call data.
#[derive(Debug, Clone, Encode, Decode)]
pub struct CachedRequireCall {
    /// The require specifier.
    pub(crate) source: String,
    /// Byte offset of the span start.
    pub(crate) span_start: u32,
    /// Byte offset of the span end.
    pub(crate) span_end: u32,
    /// Byte offset of the specifier string-literal span start.
    pub(crate) source_span_start: u32,
    /// Byte offset of the specifier string-literal span end.
    pub(crate) source_span_end: u32,
    /// Names destructured from the require result.
    pub(crate) destructured_names: Vec<String>,
    /// Local variable name for namespace requires.
    pub(crate) local_name: Option<String>,
    /// `true` for the type-erased `import type X = require('...')` spelling.
    pub(crate) is_type_only: bool,
}

/// Cached re-export data.
#[derive(Debug, Clone, Encode, Decode)]
pub struct CachedReExport {
    /// The module being re-exported from.
    pub(crate) source: String,
    /// Name imported from the source.
    pub(crate) imported_name: String,
    /// Name exported from this module.
    pub(crate) exported_name: String,
    /// Whether this is a type-only re-export.
    pub(crate) is_type_only: bool,
    /// Byte offset of the re-export span start (for line-number reporting).
    pub(crate) span_start: u32,
    /// Byte offset of the re-export span end.
    pub(crate) span_end: u32,
    /// Byte offset of the enclosing statement span start.
    pub(crate) statement_span_start: u32,
    /// Byte offset of the enclosing statement span end.
    pub(crate) statement_span_end: u32,
    /// Byte offset of the source string-literal span start.
    pub(crate) source_span_start: u32,
    /// Byte offset of the source string-literal span end.
    pub(crate) source_span_end: u32,
}

/// Cached enum or class member data.
#[derive(Debug, Clone, Encode, Decode)]
pub struct CachedMember {
    /// Member name.
    pub(crate) name: String,
    /// Member kind (enum, method, or property).
    pub(crate) kind: MemberKind,
    /// Byte offset of the span start.
    pub(crate) span_start: u32,
    /// Byte offset of the span end.
    pub(crate) span_end: u32,
    /// Whether this member has decorators.
    pub(crate) has_decorator: bool,
    /// Full dotted path of each decorator (e.g. `step`, `ns.foo`).
    /// Empty for undecorated members and decorators with non-identifier
    /// expressions.
    pub(crate) decorator_names: Vec<String>,
    /// True when this is a static method that returns a fresh instance of
    /// the class: body returns `new this()` / `new <SameClassName>()`, or the
    /// declared return type matches the class name. Treated as a factory.
    /// See issues #346, #387.
    pub(crate) is_instance_returning_static: bool,
    /// True when this instance method's call result is an instance of the
    /// same class (declared return type matches the class name, or body's
    /// last statement is `return this`). Drives fluent-chain credit. See
    /// issue #387.
    pub(crate) is_self_returning: bool,
}

/// Cached dynamic import pattern data (template literals, `import.meta.glob`).
#[derive(Debug, Clone, Encode, Decode)]
pub struct CachedDynamicImportPattern {
    /// Static prefix of the import path.
    pub(crate) prefix: String,
    /// Static suffix, if any.
    pub(crate) suffix: Option<String>,
    /// Byte offset of the span start.
    pub(crate) span_start: u32,
    /// Byte offset of the span end.
    pub(crate) span_end: u32,
    /// Runtime mechanism used to load modules matching this pattern.
    pub(crate) mechanism: crate::ModuleLoadMechanism,
}
