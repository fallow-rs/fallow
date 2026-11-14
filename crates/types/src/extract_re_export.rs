//! Re-export declarations of a module.

use super::VisibilityTag;

/// A re-export declaration.
#[derive(Debug, Clone, Default)]
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
    /// Span of the whole re-export statement. A multi-binding statement
    /// yields one `ReExportInfo` per binding, each with a per-binding
    /// `span`; this field lets consumers reason about the enclosing
    /// statement (e.g. suppression coverage). Empty (`start == end`) for
    /// synthesized re-exports that have no single owning statement.
    pub statement_span: oxc_span::Span,
    /// Span of the source string literal (the specifier in quotes), used to
    /// anchor unresolved-import findings on the specifier. Empty
    /// (`start == end`) when no literal exists in the statement.
    pub source_span: oxc_span::Span,
    /// Visibility tag from the JSDoc block on the re-export statement, such
    /// as `/** @public */ export { a } from './a'`.
    pub visibility: VisibilityTag,
    /// Reason text after `--` in an `@expected-unused` tag on the re-export
    /// statement, such as `@expected-unused -- kept for the next major`.
    pub expected_unused_reason: Option<String>,
}
