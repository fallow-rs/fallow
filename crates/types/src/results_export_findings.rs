//! Unused files and exports, deprecated exports in use, private type leaks,
//! and the client and server directive findings.

use super::*;

/// A file that is not reachable from any entry point.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct UnusedFile {
    /// Absolute path to the unused file.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
}

/// An export that is never imported by other modules.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct UnusedExport {
    /// File containing the unused export.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// Name of the unused export.
    pub export_name: String,
    /// Whether this is a type-only export.
    pub is_type_only: bool,
    /// 1-based line number of the export.
    pub line: u32,
    /// 0-based byte column offset.
    pub col: u32,
    /// Byte offset into the source file (used by the fix command).
    pub span_start: u32,
    /// Whether this finding comes from a barrel/index re-export rather than the source definition.
    pub is_re_export: bool,
    /// Whether the export's leading JSDoc carries `@deprecated`. Absent from
    /// the wire when false.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub deprecated: bool,
    /// Plain-text message of the `@deprecated` tag, capped at
    /// [`DEPRECATED_REASON_MAX_CHARS`] characters. Absent when the export is
    /// not deprecated or the tag carries no text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deprecated_reason: Option<String>,
}

/// Maximum number of consumers a [`DeprecatedExportInUse`] finding carries in
/// its `consumers` sample. The exact total is in `consumer_count`; the full
/// list is available through `fallow dead-code --trace <file>:<export>`.
pub const DEPRECATED_CONSUMER_SAMPLE_CAP: usize = 10;

/// Maximum number of characters kept from a `@deprecated` tag message. A
/// longer message is cut at a character boundary and ends with an ellipsis.
pub const DEPRECATED_REASON_MAX_CHARS: usize = 200;

/// How a consumer references a deprecated export.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum DeprecatedConsumerKind {
    /// A named import (`import { foo }`).
    NamedImport,
    /// A default import (`import Foo`).
    DefaultImport,
    /// A namespace import (`import * as ns`): a member access, or a use of
    /// the whole namespace object.
    NamespaceImport,
    /// A re-export (`export { foo } from './bar'`).
    ReExport,
    /// A dynamic import (`import('./foo')`).
    DynamicImport,
    /// A side-effect import (`import './foo'`).
    SideEffectImport,
}

/// One file location that references a deprecated export.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct DeprecatedExportConsumer {
    /// File that references the deprecated export.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// 1-based line number of the import or re-export statement.
    pub line: u32,
    /// 0-based byte column offset of the import or re-export statement.
    pub col: u32,
    /// How the file references the export.
    pub kind: DeprecatedConsumerKind,
}

/// An export whose leading JSDoc carries `@deprecated` and that still has at
/// least one consumer in a reachable file.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct DeprecatedExportInUse {
    /// File that declares the deprecated export.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// Name of the deprecated export.
    pub export_name: String,
    /// Whether this is a type-only export.
    pub is_type_only: bool,
    /// 1-based line number of the export.
    pub line: u32,
    /// 0-based byte column offset of the export.
    pub col: u32,
    /// Byte offset of the export in the source file.
    pub span_start: u32,
    /// Plain-text message of the `@deprecated` tag, capped at
    /// [`DEPRECATED_REASON_MAX_CHARS`] characters. Absent when the tag
    /// carries no text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deprecated_reason: Option<String>,
    /// Exact number of distinct consumers: reference sites in reachable
    /// files, one per path, line, column and kind. `consumers` holds the
    /// first [`DEPRECATED_CONSUMER_SAMPLE_CAP`] of them, so the sample is
    /// complete when this count is at most the cap.
    pub consumer_count: usize,
    /// Consumer sample sorted by path, line, column and kind, capped at
    /// [`DEPRECATED_CONSUMER_SAMPLE_CAP`] entries.
    pub consumers: Vec<DeprecatedExportConsumer>,
    /// True when the export is part of the public API: it lives in an entry
    /// point, or a re-export chain reaches an entry point. External consumers
    /// are not visible, so the finding makes no removal claim.
    pub public_api: bool,
}

impl DeprecatedExportInUse {
    /// One-line plain-text description shared by the SARIF and CodeClimate
    /// formats.
    #[must_use]
    pub fn description(&self) -> String {
        let count = self.consumer_count;
        let noun = if count == 1 { "consumer" } else { "consumers" };
        let reason = self
            .deprecated_reason
            .as_deref()
            .map_or_else(String::new, |reason| format!(": {reason}"));
        format!(
            "Deprecated export '{}' is still used by {count} {noun}{reason}",
            self.export_name
        )
    }
}

/// A public export signature that references a same-file private type.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct PrivateTypeLeak {
    /// File containing the exported symbol.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// Export whose public signature leaks the private type.
    pub export_name: String,
    /// Private type referenced by the public signature.
    pub type_name: String,
    /// 1-based line number of the leaking type reference.
    pub line: u32,
    /// 0-based byte column offset.
    pub col: u32,
    /// Byte offset of the type reference.
    pub span_start: u32,
    /// Exact checker-backed provenance when type-aware analysis confirmed the
    /// package-public leak across files or re-exports.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub semantic: Option<crate::semantic::SemanticPrivateTypeLeak>,
}

/// A `"use client"` file that exports a Next.js server-only / route-segment
/// config name. Next.js rejects this combination at build time; fallow catches
/// it statically before the build runs.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct InvalidClientExport {
    /// File carrying the `"use client"` directive and the illegal export.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// Name of the server-only / route-config export that is illegal in a
    /// client file (e.g. `metadata`, `generateMetadata`, `revalidate`, `GET`).
    pub export_name: String,
    /// The file-level directive that makes the export illegal. Always
    /// `"use client"` today; carried so the message can name it verbatim.
    pub directive: String,
    /// 1-based line number of the export.
    pub line: u32,
    /// 0-based byte column offset of the export.
    pub col: u32,
}

/// A barrel file that re-exports both a `"use client"` origin module and a
/// server-only origin module. Importing one name from such a barrel drags the
/// other's directive context across the React Server Components boundary (the
/// Next.js App Router footgun); fallow catches it statically.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MixedClientServerBarrel {
    /// The barrel file re-exporting both a client and a server-only origin.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// The `"use client"` origin's relative path or specifier as written in the
    /// barrel's offending re-export.
    pub client_origin: String,
    /// The server-only origin's relative path or specifier as written in the
    /// barrel's offending re-export.
    pub server_origin: String,
    /// 1-based line number of the barrel's first offending re-export.
    pub line: u32,
    /// 0-based byte column offset of the barrel's first offending re-export.
    pub col: u32,
}

/// A `"use client"` / `"use server"` directive written as an expression
/// statement after a non-directive statement (an import, a const). The RSC
/// bundler only honors a directive in the leading prologue, so once any
/// statement precedes it the string is parsed as an ordinary expression and
/// silently ignored: the intended client/server boundary never takes effect.
/// The fix is to move the directive to the very top of the file.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MisplacedDirective {
    /// The file carrying the misplaced directive.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// The directive string as written, either `"use client"` or
    /// `"use server"` (without the surrounding quotes).
    pub directive: String,
    /// 1-based line number of the misplaced directive statement.
    pub line: u32,
    /// 0-based byte column offset of the misplaced directive statement.
    pub col: u32,
}
