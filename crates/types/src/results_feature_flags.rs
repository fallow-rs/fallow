//! Feature flags, export usages, and reference locations.

use super::*;

/// The detection method used to identify a feature flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum FlagKind {
    /// Environment variable check (e.g., `process.env.FEATURE_X`).
    EnvironmentVariable,
    /// Feature flag SDK call (e.g., `useFlag('name')`, `variation('name', false)`).
    SdkCall,
    /// Config object property access (e.g., `config.features.newCheckout`).
    ConfigObject,
}

/// Detection confidence for a feature flag finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum FlagConfidence {
    /// Low confidence: heuristic match (config object patterns).
    Low,
    /// Medium confidence: a generic SDK name, such as `isEnabled`, in a file
    /// that imports no flag SDK or flag module.
    Medium,
    /// High confidence: unambiguous pattern (env vars, specific SDK calls,
    /// generic SDK calls in a file that imports a flag SDK or flag module).
    High,
}

/// A detected feature flag use site.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct FeatureFlag {
    /// File containing the feature flag usage.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// Name or identifier of the flag (e.g., `ENABLE_NEW_CHECKOUT`, `new-checkout`).
    pub flag_name: String,
    /// How the flag was detected.
    pub kind: FlagKind,
    /// Detection confidence level.
    pub confidence: FlagConfidence,
    /// 1-based line number.
    pub line: u32,
    /// 0-based byte column offset.
    pub col: u32,
    /// Start byte offset of the guarded code block (if-branch span), if detected.
    #[serde(skip)]
    pub guard_span_start: Option<u32>,
    /// End byte offset of the guarded code block (if-branch span), if detected.
    #[serde(skip)]
    pub guard_span_end: Option<u32>,
    /// SDK or provider name (e.g., "LaunchDarkly", "Statsig"), if detected from SDK call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sdk_name: Option<String>,
    /// Line range of the guarded code block (derived from guard_span + line_offsets).
    /// Used for cross-reference with dead code findings.
    #[serde(skip)]
    pub guard_line_start: Option<u32>,
    /// End line of the guarded code block.
    #[serde(skip)]
    pub guard_line_end: Option<u32>,
    /// Unused exports found within the guarded code block.
    /// Populated by cross-reference with dead code analysis.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub guarded_dead_exports: Vec<String>,
}

// Size assertion: FeatureFlag is stored in a Vec per analysis run.

/// Usage count for an export symbol. Used by the LSP Code Lens to show
/// the number of importing files above each export declaration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ExportUsage {
    /// File containing the export.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// Name of the exported symbol.
    pub export_name: String,
    /// 1-based line number.
    pub line: u32,
    /// 0-based byte column offset.
    pub col: u32,
    /// Number of distinct files that import this export. Two imports in one
    /// file count as one file.
    pub reference_count: usize,
    /// Import sites that reference this export, one entry per physical import.
    /// A file with two imports gives two entries, so this list can be longer
    /// than `reference_count`. An import without a source span has no entry.
    /// Used by the LSP Code Lens to enable click-to-navigate via
    /// `editor.action.showReferences`.
    pub reference_locations: Vec<ReferenceLocation>,
}

/// A location where an export is referenced (import site in another file).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ReferenceLocation {
    /// File containing the import that references the export.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// 1-based line number.
    pub line: u32,
    /// 0-based byte column offset.
    pub col: u32,
}
