//! Serialization types for the incremental parse cache.
//!
//! All types use bitcode `Encode`/`Decode` for fast binary serialization.

use bitcode::{Decode, Encode};

use crate::MemberKind;

/// Cache version, bump when the cache format or cached extraction semantics change.
///
/// Bumped to 89 for issue #475: extraction now strips a leading UTF-8 BOM
/// before hashing and computing line offsets, so pre-fix entries whose source
/// included a BOM carry hashes over the wrong byte sequence and would
/// fast-path into stale `member_accesses` / `exports` for any BOM-bearing
/// file. The bump invalidates user caches once on upgrade; subsequent runs
/// are warm.
///
/// Bumped to 90 for issue #540: CSS Modules class extraction now strips
/// `@layer` and `@import` at-rule preludes before scanning class names, so
/// pre-fix entries for `.module.css` files using nested cascade-layer syntax
/// (`@layer foo.bar { ... }`) carry phantom `bar` / `baz` exports that the
/// new scanner no longer produces.
///
/// Bumped to 91 for issue #549: CSS Modules class extraction now records a
/// real `Span` pointing at each class's declaration position in the source.
/// Pre-fix cache entries for `.module.css` / `.module.scss` files carry
/// `Span::default()` (start=0, end=0) on every export, which renders every
/// finding at line:1 col:0; the new scanner produces real offsets.
///
/// Bumped to 92 for issue #563: feature flag extraction recognizes additional
/// built-in SDK providers (PostHog, Vercel Flags, Optimizely, Eppo, plus more
/// ConfigCat surfaces) and Vercel `flag({ key: "..." })` object arguments, so
/// pre-fix entries can carry stale `flag_uses`.
///
/// Bumped to 93 for issue #589: Node `module.register()` loader calls now
/// emit `DynamicImportInfo.destructured_names` populated with the loader-hook
/// allowlist (current `initialize` / `resolve` / `load` / `globalPreload`
/// plus legacy `getFormat` / `getSource` / `transformSource`) for every
/// relative or `file:` specifier, including specifiers bound via
/// `new URL(..., import.meta.url)`. Pre-fix entries carry empty
/// `destructured_names` for the same source, so they would silently miss
/// the named-export credit until the file is touched.
///
/// Bumped to 94 for issue #586: Playwright helper fixture extraction recognizes
/// helpers with local setup before the final `return base.extend<T>(...)`, so
/// pre-fix entries can miss fixture definition sentinels.
///
/// Bumped to 95 for the Glimmer `<template>` scanner: imported-binding usage
/// and `MemberAccess { object: "this", member }` records for `{{this.foo}}`
/// template references are now folded into the extractor before
/// `into_module_info`. Pre-fix entries for `.gts` / `.gjs` files omit both,
/// so template-only imports surface as `unused-import` and template-only
/// class members as `unused-class-member` until the cache is re-extracted.
///
/// Bumped to 96 for issue #640: generic JSX `<script src>` and
/// `<link rel="stylesheet|modulepreload" href>` attributes no longer emit
/// synthetic `SideEffect` imports, so pre-fix entries can carry stale JSX
/// resource edges that surface as false `unresolved-imports`.
///
/// Bumped to 97 for issue #639: MDX import/export extraction now skips
/// fenced Markdown code blocks, so pre-fix entries can carry stale example
/// imports that surface as false `unresolved-imports`.
///
/// Bumped to 98 for issue #638: statically resolvable `child_process.fork()`
/// targets now emit `DynamicImportInfo` entries for local runner files.
/// Pre-fix entries omit those dynamic imports, so forked script files can be
/// reported as unused until the file is re-extracted.
///
/// Bumped to 99 for issue #605: methods reached via `new Class(...).method()`
/// receivers (direct and fluent-chain) now emit member accesses crediting the
/// constructed class. Pre-fix entries lack those accesses, so such methods can
/// be reported as unused class members until the file is re-extracted.
pub(super) const CACHE_VERSION: u32 = 99;

/// Duplication token cache version. Bump when duplicate tokenization,
/// normalization, or the on-disk token cache schema changes.
pub const DUPES_CACHE_VERSION: u32 = 4;

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

assert_cached_type_size!(CachedModule, 520);
assert_cached_type_size!(CachedNamespaceObjectAlias, 72);
assert_cached_type_size!(CachedLocalTypeDeclaration, 32);
assert_cached_type_size!(CachedPublicSignatureTypeReference, 56);
assert_cached_type_size!(CachedSuppression, 12);
assert_cached_type_size!(CachedUnknownSuppressionKind, 32);
assert_cached_type_size!(CachedExport, 112);
assert_cached_type_size!(CachedImport, 96);
assert_cached_type_size!(CachedDynamicImport, 88);
assert_cached_type_size!(CachedRequireCall, 80);
assert_cached_type_size!(CachedReExport, 88);
assert_cached_type_size!(CachedMember, 64);
assert_cached_type_size!(CachedDynamicImportPattern, 56);
assert_cached_type_size!(crate::MemberAccess, 48);
assert_cached_type_size!(fallow_types::extract::FunctionComplexity, 48);
assert_cached_type_size!(fallow_types::extract::FlagUse, 80);
assert_cached_type_size!(fallow_types::extract::ClassHeritageInfo, 96);

/// Cached data for a single module.
#[derive(Debug, Clone, Encode, Decode)]
pub struct CachedModule {
    /// xxh3 hash of the file content.
    pub content_hash: u64,
    /// File modification time (seconds since epoch) for fast cache validation.
    /// When mtime+size match the on-disk file, we skip reading file content entirely.
    pub mtime_secs: u64,
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
    /// Static member accesses (e.g., `Status.Active`).
    pub member_accesses: Vec<crate::MemberAccess>,
    /// Identifiers used as whole objects (Object.values, for..in, spread, etc.).
    pub whole_object_uses: Vec<String>,
    /// Dynamic import patterns with partial static resolution.
    pub dynamic_import_patterns: Vec<CachedDynamicImportPattern>,
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
    /// Feature flag use sites.
    pub flag_uses: Vec<fallow_types::extract::FlagUse>,
    /// Heritage metadata for exported classes.
    pub class_heritage: Vec<fallow_types::extract::ClassHeritageInfo>,
    /// Local type-capable declarations.
    pub local_type_declarations: Vec<CachedLocalTypeDeclaration>,
    /// Type references from exported public signatures.
    pub public_signature_type_references: Vec<CachedPublicSignatureTypeReference>,
    /// Namespace-import aliases re-exported through an object literal
    /// (`export const API = { foo }` where `foo` is `import * as foo from './bar'`).
    pub namespace_object_aliases: Vec<CachedNamespaceObjectAlias>,
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
    pub line: u32,
    /// 1-based line where the comment itself appears.
    pub comment_line: u32,
    /// 0 = suppress all, 1-20 = `IssueKind` discriminant.
    pub kind: u8,
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
}

/// Cached export data for a single export declaration.
#[derive(Debug, Clone, Encode, Decode)]
pub struct CachedExport {
    /// Export name (or "default" for default exports).
    pub name: String,
    /// Whether this is a default export.
    pub is_default: bool,
    /// Whether this is a type-only export.
    pub is_type_only: bool,
    /// Whether this export is registered through a runtime side effect at
    /// module load time (Lit `@customElement` decorator or
    /// `customElements.define` call). Persisted so warm-cache runs continue
    /// to skip unused-export reporting for these classes.
    pub is_side_effect_used: bool,
    /// Visibility tag discriminant (0=None, 1=Public, 2=Internal, 3=Beta, 4=Alpha).
    pub visibility: u8,
    /// The local binding name, if different.
    pub local_name: Option<String>,
    /// Byte offset of the export span start.
    pub span_start: u32,
    /// Byte offset of the export span end.
    pub span_end: u32,
    /// Members of this export (for enums and classes).
    pub members: Vec<CachedMember>,
    /// The local name of the parent class from `extends` clause, if any.
    pub super_class: Option<String>,
}

/// Cached import data for a single import declaration.
#[derive(Debug, Clone, Encode, Decode)]
pub struct CachedImport {
    /// The import specifier.
    pub source: String,
    /// For Named imports, the imported symbol name. Empty for other kinds.
    pub imported_name: String,
    /// The local binding name.
    pub local_name: String,
    /// Whether this is a type-only import.
    pub is_type_only: bool,
    /// Whether this import originated from an SFC `<style>` block / `<style src>` (CSS context).
    pub from_style: bool,
    /// Import kind: 0=Named, 1=Default, 2=Namespace, 3=SideEffect.
    pub kind: u8,
    /// Byte offset of the import span start.
    pub span_start: u32,
    /// Byte offset of the import span end.
    pub span_end: u32,
    /// Byte offset of the source string literal span start.
    pub source_span_start: u32,
    /// Byte offset of the source string literal span end.
    pub source_span_end: u32,
}

/// Cached dynamic import data.
#[derive(Debug, Clone, Encode, Decode)]
pub struct CachedDynamicImport {
    /// The import specifier.
    pub source: String,
    /// Byte offset of the span start.
    pub span_start: u32,
    /// Byte offset of the span end.
    pub span_end: u32,
    /// Names destructured from the import result.
    pub destructured_names: Vec<String>,
    /// Local variable name for namespace imports.
    pub local_name: Option<String>,
    /// True when this dynamic import was synthesised by fallow (see
    /// `DynamicImportInfo::is_speculative`).
    pub is_speculative: bool,
}

/// Cached `require()` call data.
#[derive(Debug, Clone, Encode, Decode)]
pub struct CachedRequireCall {
    /// The require specifier.
    pub source: String,
    /// Byte offset of the span start.
    pub span_start: u32,
    /// Byte offset of the span end.
    pub span_end: u32,
    /// Names destructured from the require result.
    pub destructured_names: Vec<String>,
    /// Local variable name for namespace requires.
    pub local_name: Option<String>,
}

/// Cached re-export data.
#[derive(Debug, Clone, Encode, Decode)]
pub struct CachedReExport {
    /// The module being re-exported from.
    pub source: String,
    /// Name imported from the source.
    pub imported_name: String,
    /// Name exported from this module.
    pub exported_name: String,
    /// Whether this is a type-only re-export.
    pub is_type_only: bool,
    /// Byte offset of the re-export span start (for line-number reporting).
    pub span_start: u32,
    /// Byte offset of the re-export span end.
    pub span_end: u32,
}

/// Cached enum or class member data.
#[derive(Debug, Clone, Encode, Decode)]
pub struct CachedMember {
    /// Member name.
    pub name: String,
    /// Member kind (enum, method, or property).
    pub kind: MemberKind,
    /// Byte offset of the span start.
    pub span_start: u32,
    /// Byte offset of the span end.
    pub span_end: u32,
    /// Whether this member has decorators.
    pub has_decorator: bool,
    /// Full dotted path of each decorator (e.g. `step`, `ns.foo`).
    /// Empty for undecorated members and decorators with non-identifier
    /// expressions.
    pub decorator_names: Vec<String>,
    /// True when this is a static method that returns a fresh instance of
    /// the class: body returns `new this()` / `new <SameClassName>()`, or the
    /// declared return type matches the class name. Treated as a factory.
    /// See issues #346, #387.
    pub is_instance_returning_static: bool,
    /// True when this instance method's call result is an instance of the
    /// same class (declared return type matches the class name, or body's
    /// last statement is `return this`). Drives fluent-chain credit. See
    /// issue #387.
    pub is_self_returning: bool,
}

/// Cached dynamic import pattern data (template literals, `import.meta.glob`).
#[derive(Debug, Clone, Encode, Decode)]
pub struct CachedDynamicImportPattern {
    /// Static prefix of the import path.
    pub prefix: String,
    /// Static suffix, if any.
    pub suffix: Option<String>,
    /// Byte offset of the span start.
    pub span_start: u32,
    /// Byte offset of the span end.
    pub span_end: u32,
}
