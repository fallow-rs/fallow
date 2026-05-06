//! Serialization types for the incremental parse cache.
//!
//! All types use bitcode `Encode`/`Decode` for fast binary serialization.

use bitcode::{Decode, Encode};

use crate::MemberKind;

/// Cache version — bump when the cache format or cached extraction semantics change.
pub(super) const CACHE_VERSION: u32 = 69;

/// Duplication token cache version — bump when duplicate tokenization,
/// normalization, or the on-disk token cache schema changes.
pub const DUPES_CACHE_VERSION: u32 = 2;

/// Maximum cache file size to deserialize (256 MB).
pub(super) const MAX_CACHE_SIZE: usize = 256 * 1024 * 1024;

/// Import kind discriminant for `CachedImport`:
/// 0 = Named, 1 = Default, 2 = Namespace, 3 = `SideEffect`.
pub(super) const IMPORT_KIND_NAMED: u8 = 0;
pub(super) const IMPORT_KIND_DEFAULT: u8 = 1;
pub(super) const IMPORT_KIND_NAMESPACE: u8 = 2;
pub(super) const IMPORT_KIND_SIDE_EFFECT: u8 = 3;

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
    /// Local names of import bindings that are never referenced in this file.
    pub unused_import_bindings: Vec<String>,
    /// Local import bindings referenced from type positions.
    pub type_referenced_import_bindings: Vec<String>,
    /// Local import bindings referenced from value positions.
    pub value_referenced_import_bindings: Vec<String>,
    /// Inline suppression directives.
    pub suppressions: Vec<CachedSuppression>,
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
