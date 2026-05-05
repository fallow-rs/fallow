//! Module extraction types: exports, imports, re-exports, members, and parse results.

use oxc_span::Span;

use crate::discover::FileId;
use crate::suppress::Suppression;

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
    /// Dynamic import patterns from template literals, string concat, or `import.meta.glob`.
    pub dynamic_import_patterns: Vec<DynamicImportPattern>,
    /// All `require()` calls.
    pub require_calls: Vec<RequireCallInfo>,
    /// Static member access expressions (e.g., `Status.Active`).
    pub member_accesses: Vec<MemberAccess>,
    /// Identifiers used in "all members consumed" patterns
    /// (Object.values, Object.keys, Object.entries, Object.getOwnPropertyNames, for..in, spread, computed dynamic access).
    pub whole_object_uses: Vec<String>,
    /// Whether this module uses `CommonJS` exports (`module.exports` or `exports.*`).
    pub has_cjs_exports: bool,
    /// xxh3 hash of the file content for incremental caching.
    pub content_hash: u64,
    /// Inline suppression directives parsed from comments.
    pub suppressions: Vec<Suppression>,
    /// Local names of import bindings that are never referenced in this file.
    /// Populated via `oxc_semantic` scope analysis. Used at graph-build time
    /// to skip adding references for imports whose binding is never read,
    /// improving unused-export detection precision.
    pub unused_import_bindings: Vec<String>,
    /// Local import bindings that are referenced from TypeScript type positions.
    /// Used to distinguish value-namespace and type-namespace references when a
    /// module exports both `const X` and `type X`.
    pub type_referenced_import_bindings: Vec<String>,
    /// Local import bindings that are referenced from runtime/value positions.
    /// Used alongside `type_referenced_import_bindings` for TS namespace-split
    /// exports that share the same name.
    pub value_referenced_import_bindings: Vec<String>,
    /// Pre-computed byte offsets where each line starts, for O(log N) byte-to-line/col conversion.
    /// Entry `i` is the byte offset of the start of line `i` (0-indexed).
    /// Example: for "abc\ndef\n", `line_offsets` = \[0, 4\].
    pub line_offsets: Vec<u32>,
    /// Per-function complexity metrics computed during AST traversal.
    /// Used by the `fallow health` subcommand to report high-complexity functions.
    pub complexity: Vec<FunctionComplexity>,
    /// Feature flag use sites detected during AST traversal.
    /// Used by the `fallow flags` subcommand to report feature flag patterns.
    pub flag_uses: Vec<FlagUse>,
    /// Heritage metadata for exported classes that declare `implements`.
    /// Used to scope `usedClassMembers` rules during analysis.
    pub class_heritage: Vec<ClassHeritageInfo>,
    /// Local type-capable declarations in this module.
    /// Used to detect exported signatures that expose a same-file private type.
    pub local_type_declarations: Vec<LocalTypeDeclaration>,
    /// Type references that appear in exported symbols' public signatures.
    /// The analysis layer checks these against `local_type_declarations`.
    pub public_signature_type_references: Vec<PublicSignatureTypeReference>,
}

/// Compute a table of line-start byte offsets from source text.
///
/// The returned vec contains one entry per line: `line_offsets[i]` is the byte
/// offset where line `i` starts (0-indexed). The first entry is always `0`.
///
/// # Examples
///
/// ```
/// use fallow_types::extract::compute_line_offsets;
///
/// let offsets = compute_line_offsets("abc\ndef\nghi");
/// assert_eq!(offsets, vec![0, 4, 8]);
/// ```
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

/// Convert a byte offset to a 1-based line number and 0-based byte column
/// using a pre-computed line offset table (from [`compute_line_offsets`]).
///
/// Uses binary search for O(log L) lookup where L is the number of lines.
///
/// # Examples
///
/// ```
/// use fallow_types::extract::{compute_line_offsets, byte_offset_to_line_col};
///
/// let offsets = compute_line_offsets("abc\ndef\nghi");
/// assert_eq!(byte_offset_to_line_col(&offsets, 0), (1, 0)); // 'a' on line 1
/// assert_eq!(byte_offset_to_line_col(&offsets, 4), (2, 0)); // 'd' on line 2
/// assert_eq!(byte_offset_to_line_col(&offsets, 9), (3, 1)); // 'h' on line 3
/// ```
#[must_use]
#[expect(
    clippy::cast_possible_truncation,
    reason = "line count is bounded by source size"
)]
pub fn byte_offset_to_line_col(line_offsets: &[u32], byte_offset: u32) -> (u32, u32) {
    // Binary search: find the last line whose start is <= byte_offset
    let line_idx = match line_offsets.binary_search(&byte_offset) {
        Ok(idx) => idx,
        Err(idx) => idx.saturating_sub(1),
    };
    let line = line_idx as u32 + 1; // 1-based
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

/// A feature flag use site detected during AST traversal.
#[derive(Debug, Clone, bitcode::Encode, bitcode::Decode)]
pub struct FlagUse {
    /// Name/identifier of the flag (e.g., `ENABLE_NEW_CHECKOUT`, `new-checkout`).
    pub flag_name: String,
    /// How the flag was detected.
    pub kind: FlagUseKind,
    /// 1-based line number.
    pub line: u32,
    /// 0-based byte column offset.
    pub col: u32,
    /// Start byte offset of the guarded code block (if-branch span), if detected.
    pub guard_span_start: Option<u32>,
    /// End byte offset of the guarded code block (if-branch span), if detected.
    pub guard_span_end: Option<u32>,
    /// SDK/provider name if detected from SDK call pattern (e.g., "LaunchDarkly").
    pub sdk_name: Option<String>,
}

// Size assertion: FlagUse is stored in a Vec per file in the cache.
const _: () = assert!(std::mem::size_of::<FlagUse>() <= 96);

/// A dynamic import with a pattern that can be partially resolved (e.g., template literals).
#[derive(Debug, Clone)]
pub struct DynamicImportPattern {
    /// Static prefix of the import path (e.g., "./locales/"). May contain glob characters.
    pub prefix: String,
    /// Static suffix of the import path (e.g., ".json"), if any.
    pub suffix: Option<String>,
    /// Source span in the original file.
    pub span: Span,
}

/// Visibility tag from JSDoc/TSDoc comments.
///
/// Controls whether an export is reported as unused. All non-`None` variants
/// suppress unused-export detection, but preserve the semantic distinction
/// for API surface reporting and filtering.
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
    /// Whether this export is registered through a runtime side effect at module
    /// load time (e.g. a Lit `@customElement('tag')` class decorator or a
    /// `customElements.define('tag', ClassRef)` call). Such classes are
    /// referenced by their registered tag string, not by their identifier, so
    /// no other file imports them by name. The unused-export detector treats
    /// this flag as an effective reference.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub is_side_effect_used: bool,
    /// Visibility tag from JSDoc/TSDoc comment (`@public`, `@internal`, `@alpha`, `@beta`).
    /// Exports with any visibility tag are never reported as unused.
    #[serde(default, skip_serializing_if = "VisibilityTag::is_none")]
    pub visibility: VisibilityTag,
    /// Source span of the export declaration.
    #[serde(serialize_with = "serialize_span")]
    pub span: Span,
    /// Members of this export (for enums, classes, and namespaces).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub members: Vec<MemberInfo>,
    /// The local name of the parent class from `extends` clause, if any.
    /// Used to build inheritance maps for unused class member detection.
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
    /// Typed instance bindings on this class: pairs of `(local_name, type_name)`
    /// from typed constructor parameters with accessibility modifiers
    /// (`constructor(public svc: Svc)`) and non-private typed property
    /// declarations (`svc: Svc`).
    ///
    /// Used by the analysis layer to resolve Angular template member-access
    /// chains on external templates (`templateUrl`), where the HTML file is
    /// parsed independently and cannot see the component's constructor types.
    /// For `constructor(public dataService: DataService)` in a component that
    /// uses an external template with `{{ dataService.getTotal() }}`, this
    /// field carries `("dataService", "DataService")` so the bridge can credit
    /// `DataService.getTotal` as used.
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
    /// flagged as unused class members.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub has_decorator: bool,
}

/// The kind of member.
///
/// # Examples
///
/// ```
/// use fallow_types::extract::MemberKind;
///
/// let kind = MemberKind::EnumMember;
/// assert_eq!(kind, MemberKind::EnumMember);
/// assert_ne!(kind, MemberKind::ClassMethod);
/// assert_ne!(MemberKind::ClassMethod, MemberKind::ClassProperty);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, bitcode::Encode, bitcode::Decode)]
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
}

/// A static member access expression (e.g., `Status.Active`, `MyClass.create()`).
///
/// # Examples
///
/// ```
/// use fallow_types::extract::MemberAccess;
///
/// let access = MemberAccess {
///     object: "Status".to_string(),
///     member: "Active".to_string(),
/// };
/// assert_eq!(access.object, "Status");
/// assert_eq!(access.member, "Active");
/// ```
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, bitcode::Encode, bitcode::Decode)]
pub struct MemberAccess {
    /// The identifier being accessed (the import name).
    pub object: String,
    /// The member being accessed.
    pub member: String,
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
///
/// # Examples
///
/// ```
/// use fallow_types::extract::ExportName;
///
/// let named = ExportName::Named("foo".to_string());
/// assert_eq!(named.to_string(), "foo");
/// assert!(named.matches_str("foo"));
///
/// let default = ExportName::Default;
/// assert_eq!(default.to_string(), "default");
/// assert!(default.matches_str("default"));
/// ```
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
    /// Whether this import originated from a CSS-context (an SFC `<style lang="scss">` block,
    /// `<style src="...">` reference, or other style-section parser). The resolver uses this
    /// to enable SCSS partial / include-path / node_modules fallbacks for SFC importers
    /// without applying them to JS-context imports from the same file.
    pub from_style: bool,
    /// Source span of the import declaration.
    pub span: Span,
    /// Span of the source string literal (e.g., the `'./utils'` in `import { foo } from './utils'`).
    /// Used by the LSP to highlight just the specifier in diagnostics.
    pub source_span: Span,
}

/// How a symbol is imported.
///
/// # Examples
///
/// ```
/// use fallow_types::extract::ImportedName;
///
/// let named = ImportedName::Named("useState".to_string());
/// assert_eq!(named, ImportedName::Named("useState".to_string()));
/// assert_ne!(named, ImportedName::Default);
///
/// // Side-effect imports have no binding
/// assert_eq!(ImportedName::SideEffect, ImportedName::SideEffect);
/// ```
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

// Size assertions to prevent memory regressions in hot-path types.
// These types are stored in Vecs inside `ModuleInfo` (one per file) and are
// iterated during graph construction and analysis. Keeping them compact
// improves cache locality on large projects with thousands of files.
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
// `ModuleInfo` is the per-file extraction result — stored in a Vec during parallel parsing.
#[cfg(target_pointer_width = "64")]
const _: () = assert!(std::mem::size_of::<ModuleInfo>() == 448);

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
    /// Used for line-number reporting when an unused re-export is detected.
    /// Defaults to `Span::default()` (0, 0) for re-exports without a meaningful
    /// source location (e.g., synthesized in the graph layer).
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
}

/// A `require()` call.
#[derive(Debug, Clone)]
pub struct RequireCallInfo {
    /// The require specifier.
    pub source: String,
    /// Source span of the `require()` call.
    pub span: Span,
    /// Names destructured from the `require()` result.
    /// Non-empty means `const { a, b } = require(...)` -> Named imports.
    /// Empty means simple `require(...)` or `const x = require(...)` -> Namespace.
    pub destructured_names: Vec<String>,
    /// The local variable name for `const x = require(...)`.
    /// Used for namespace import narrowing via member access tracking.
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
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── compute_line_offsets ──────────────────────────────────────────

    #[test]
    fn line_offsets_empty_string() {
        assert_eq!(compute_line_offsets(""), vec![0]);
    }

    #[test]
    fn line_offsets_single_line_no_newline() {
        assert_eq!(compute_line_offsets("hello"), vec![0]);
    }

    #[test]
    fn line_offsets_single_line_with_newline() {
        // "hello\n" => line 0 starts at 0, line 1 starts at 6
        assert_eq!(compute_line_offsets("hello\n"), vec![0, 6]);
    }

    #[test]
    fn line_offsets_multiple_lines() {
        // "abc\ndef\nghi"
        // line 0: offset 0 ("abc")
        // line 1: offset 4 ("def")
        // line 2: offset 8 ("ghi")
        assert_eq!(compute_line_offsets("abc\ndef\nghi"), vec![0, 4, 8]);
    }

    #[test]
    fn line_offsets_trailing_newline() {
        // "abc\ndef\n"
        // line 0: offset 0, line 1: offset 4, line 2: offset 8 (empty line after trailing \n)
        assert_eq!(compute_line_offsets("abc\ndef\n"), vec![0, 4, 8]);
    }

    #[test]
    fn line_offsets_consecutive_newlines() {
        // "\n\n\n" = 3 newlines => 4 lines
        assert_eq!(compute_line_offsets("\n\n\n"), vec![0, 1, 2, 3]);
    }

    #[test]
    fn line_offsets_multibyte_utf8() {
        // "á\n" => 'á' is 2 bytes (0xC3 0xA1), '\n' at byte 2 => next line at byte 3
        assert_eq!(compute_line_offsets("á\n"), vec![0, 3]);
    }

    // ── byte_offset_to_line_col ──────────────────────────────────────

    #[test]
    fn line_col_offset_zero() {
        let offsets = compute_line_offsets("abc\ndef\nghi");
        let (line, col) = byte_offset_to_line_col(&offsets, 0);
        assert_eq!((line, col), (1, 0)); // line 1, col 0
    }

    #[test]
    fn line_col_middle_of_first_line() {
        let offsets = compute_line_offsets("abc\ndef\nghi");
        let (line, col) = byte_offset_to_line_col(&offsets, 2);
        assert_eq!((line, col), (1, 2)); // 'c' in "abc"
    }

    #[test]
    fn line_col_start_of_second_line() {
        let offsets = compute_line_offsets("abc\ndef\nghi");
        // byte 4 = start of "def"
        let (line, col) = byte_offset_to_line_col(&offsets, 4);
        assert_eq!((line, col), (2, 0));
    }

    #[test]
    fn line_col_middle_of_second_line() {
        let offsets = compute_line_offsets("abc\ndef\nghi");
        // byte 5 = 'e' in "def"
        let (line, col) = byte_offset_to_line_col(&offsets, 5);
        assert_eq!((line, col), (2, 1));
    }

    #[test]
    fn line_col_start_of_third_line() {
        let offsets = compute_line_offsets("abc\ndef\nghi");
        // byte 8 = start of "ghi"
        let (line, col) = byte_offset_to_line_col(&offsets, 8);
        assert_eq!((line, col), (3, 0));
    }

    #[test]
    fn line_col_end_of_file() {
        let offsets = compute_line_offsets("abc\ndef\nghi");
        // byte 10 = 'i' (last char)
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
        // byte 3 = the '\n' character itself, still part of line 1
        let (line, col) = byte_offset_to_line_col(&offsets, 3);
        assert_eq!((line, col), (1, 3));
    }

    // ── ExportName ───────────────────────────────────────────────────

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

    // ── ExportName equality & hashing ────────────────────────────

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

    // ── ExportName::matches_str edge cases ───────────────────────

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

    // ── ImportedName equality ────────────────────────────────────

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

    // ── MemberKind equality ────────────────────────────────────

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

    // ── MemberKind bitcode roundtrip ─────────────────────────────

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

    // ── MemberAccess bitcode roundtrip ─────────────────────────

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

    // ── compute_line_offsets with Windows line endings ───────────

    #[test]
    fn line_offsets_crlf_only_counts_lf() {
        // \r\n should produce offsets at the \n boundary
        // "ab\r\ncd" => bytes: a(0) b(1) \r(2) \n(3) c(4) d(5)
        // Line 0: offset 0, line 1: offset 4
        let offsets = compute_line_offsets("ab\r\ncd");
        assert_eq!(offsets, vec![0, 4]);
    }

    // ── byte_offset_to_line_col edge cases ──────────────────────

    #[test]
    fn line_col_empty_file_offset_zero() {
        let offsets = compute_line_offsets("");
        let (line, col) = byte_offset_to_line_col(&offsets, 0);
        assert_eq!((line, col), (1, 0));
    }

    // ── FunctionComplexity bitcode roundtrip ──────────────────────

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
        };
        let bytes = bitcode::encode(&fc);
        let decoded: FunctionComplexity = bitcode::decode(&bytes).unwrap();
        assert_eq!(decoded.name, "processData");
        assert_eq!(decoded.line, 42);
        assert_eq!(decoded.col, 4);
        assert_eq!(decoded.cyclomatic, 15);
        assert_eq!(decoded.cognitive, 25);
        assert_eq!(decoded.line_count, 80);
    }
}
