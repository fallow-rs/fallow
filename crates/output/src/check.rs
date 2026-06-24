use fallow_types::envelope::{
    BaselineDeltas, BaselineMatch, CheckSummary, ElapsedMs, EntryPoints, Meta, RegressionResult,
    SchemaVersion, ToolVersion,
};
use fallow_types::output::NextStep;
use fallow_types::results::AnalysisResults;
use serde::Serialize;

/// Serialized workspace-discovery diagnostic surfaced on check output.
///
/// The runtime diagnostic type currently lives in `fallow-config`, which is
/// upstream of `fallow-output`. This transparent DTO keeps the wire contract in
/// the output layer without introducing a crate cycle.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(transparent)]
pub struct WorkspaceDiagnosticOutput(pub serde_json::Value);

/// Envelope emitted by `fallow dead-code --format json` (plus the `check`
/// block inside the combined and audit envelopes).
///
/// The body is the full `AnalysisResults` flattened into the envelope so
/// every issue array (`unused_files`, `unused_exports`, ...) lives at the
/// top level, matching the existing wire shape. `entry_points` lifts the
/// otherwise `#[serde(skip)]`'d `AnalysisResults::entry_point_summary` back
/// into the JSON output. `summary` carries the per-category counts the
/// JSON layer always emits.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "schema", schemars(title = "fallow dead-code --format json"))]
pub struct CheckOutput {
    pub schema_version: SchemaVersion,
    pub version: ToolVersion,
    pub elapsed_ms: ElapsedMs,
    pub total_issues: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry_points: Option<EntryPoints>,
    pub summary: CheckSummary,
    #[serde(flatten)]
    pub results: AnalysisResults,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_deltas: Option<BaselineDeltas>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline: Option<BaselineMatch>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub regression: Option<RegressionResult>,
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Meta>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub workspace_diagnostics: Vec<WorkspaceDiagnosticOutput>,
    /// Read-only follow-up commands computed from this run's findings, emitted
    /// at the JSON root so an agent acting on the output is pointed at fallow's
    /// adjacent verification capabilities (trace, complexity breakdown, audit,
    /// workspace scoping). Each command is runnable as-is and never mutating;
    /// see [`NextStep`] for both contracts. Omitted when empty or when
    /// `FALLOW_SUGGESTIONS=off`; does NOT contribute to `total_issues`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub next_steps: Vec<NextStep>,
}

/// Envelope emitted by `fallow dead-code --group-by ... --format json`.
///
/// Issues are partitioned into resolver buckets (CODEOWNERS team, directory
/// prefix, workspace package, or GitLab CODEOWNERS section) instead of flat
/// arrays. Each bucket carries the same issue-array shape as the ungrouped
/// `CheckOutput` body, plus per-group `key` / `owners` / `total_issues`.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(
    feature = "schema",
    schemars(
        title = "fallow dead-code --group-by <owner|directory|package|section> --format json"
    )
)]
pub struct CheckGroupedOutput {
    pub schema_version: SchemaVersion,
    pub version: ToolVersion,
    pub elapsed_ms: ElapsedMs,
    pub grouped_by: GroupByMode,
    pub total_issues: usize,
    pub groups: Vec<CheckGroupedEntry>,
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Meta>,
    /// Read-only follow-up commands computed from the full (ungrouped) findings.
    /// See [`CheckOutput::next_steps`] for the contract.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub next_steps: Vec<NextStep>,
}

/// Single resolver bucket inside `CheckGroupedOutput`. Carries the group's
/// identifier, optional section owners, and a per-group flattened
/// `AnalysisResults`.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CheckGroupedEntry {
    pub key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owners: Option<Vec<String>>,
    pub total_issues: usize,
    #[serde(flatten)]
    pub results: AnalysisResults,
}

/// Resolver mode label for grouped envelopes (dead-code, dupes, health).
///
/// `owner` groups by CODEOWNERS team, `directory` groups by top-level
/// directory prefix, `package` groups by workspace package name, `section`
/// groups by GitLab CODEOWNERS `[Section]` header name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum GroupByMode {
    Owner,
    Directory,
    Package,
    Section,
}
