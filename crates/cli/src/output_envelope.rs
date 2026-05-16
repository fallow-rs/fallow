//! Typed envelope structs for the JSON output contract.
//!
//! Each top-level fallow command (`check`, `dupes`, `health`, `audit`,
//! `explain`, `coverage setup`, plus the bare combined invocation and the
//! CodeClimate / review-envelope side outputs) emits a distinct envelope
//! shape. This module is the schema-side source of truth for those shapes:
//! every type carries `Serialize` plus a cfg-gated `JsonSchema` derive so the
//! committed `docs/output-schema.json` can be regenerated from Rust.
//!
//! Living in `fallow-cli` rather than `fallow-types` because the body fields
//! pull in `DuplicationReport` (from `fallow-core`) and `HealthReport` (from
//! this crate), neither of which is reachable from the lower-level types
//! crate. The shared utility shapes (`SchemaVersion`, `Meta`,
//! `BaselineDeltas`, ...) still live in `fallow_types::envelope` because they
//! depend only on serde primitives.
//!
//! Runtime construction of these envelopes happens in
//! `crates/cli/src/report/json.rs`; the JSON layer builds an envelope struct
//! and converts it to a `serde_json::Value` via `serde_json::to_value`. Path
//! relativisation and the per-finding `actions` injection still run as
//! post-passes on the `Value` tree because they cross result-type boundaries
//! that typed wrappers do not.

use fallow_core::duplicates::DuplicationReport;
use fallow_core::results::AnalysisResults;
use fallow_types::envelope::{
    BaselineDeltas, BaselineMatch, CheckSummary, ElapsedMs, EntryPoints, Meta, RegressionResult,
    SchemaVersion, ToolVersion,
};
use serde::Serialize;

/// Envelope emitted by `fallow dupes --format json` (plus the `dupes` block
/// inside the combined and audit envelopes).
///
/// The body is the full `DuplicationReport` flattened into the envelope so
/// the wire shape stays `{ schema_version, version, elapsed_ms, clone_groups,
/// clone_families, stats, ... }` exactly as the existing JSON layer emits.
/// `grouped_by` / `groups` / `total_issues` are populated by the grouped
/// builder; on the ungrouped path they stay `None` and `skip_serializing_if`
/// drops them.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct DupesOutput {
    /// Schema version for this output format.
    pub schema_version: SchemaVersion,
    /// Fallow tool version that produced this output.
    pub version: ToolVersion,
    /// Analysis duration in milliseconds.
    pub elapsed_ms: ElapsedMs,
    /// Project-level duplication payload (`clone_groups`, `clone_families`,
    /// `stats`, optional `mirrored_directories`). Flattened so the wire shape
    /// stays a single object.
    #[serde(flatten)]
    pub report: DuplicationReport,
    /// Resolver mode used for partitioning. Present only when `--group-by` is
    /// active.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grouped_by: Option<GroupByMode>,
    /// Total clone groups across all buckets when `--group-by` is active.
    /// Mirrors the grouped check / health envelopes which expose
    /// `total_issues` so MCP and CI consumers can read the same key across
    /// commands.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_issues: Option<usize>,
    /// Per-group buckets when `--group-by` is active. Each clone group is
    /// attributed to its largest-owner key (most instances; alphabetical
    /// tiebreak).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub groups: Option<serde_json::Value>,
    /// `_meta` block with metric / rule definitions, emitted when `--explain`
    /// is passed (always present in MCP responses).
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Meta>,
}

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
pub struct CheckOutput {
    /// Schema version for this output format.
    pub schema_version: SchemaVersion,
    /// Fallow tool version that produced this output.
    pub version: ToolVersion,
    /// Analysis duration in milliseconds.
    pub elapsed_ms: ElapsedMs,
    /// Total number of issues found across all categories.
    pub total_issues: usize,
    /// Entry-point detection summary. Present when the analysis populated
    /// the metadata block; absent in synthesised fixtures.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry_points: Option<EntryPoints>,
    /// Per-category issue counts. Always present in real runs.
    pub summary: CheckSummary,
    /// All issue arrays flattened in from `AnalysisResults`.
    #[serde(flatten)]
    pub results: AnalysisResults,
    /// Per-category delta comparison against a saved baseline. Only present
    /// when `--baseline` is used (today only via the combined invocation).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_deltas: Option<BaselineDeltas>,
    /// Baseline match statistics. Only present when `--baseline` is used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline: Option<BaselineMatch>,
    /// Regression check result. Only present when `--fail-on-regression` is
    /// used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub regression: Option<RegressionResult>,
    /// `_meta` block with metric / rule definitions, emitted when `--explain`
    /// is passed (always present in MCP responses).
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Meta>,
}

/// Envelope emitted by `fallow dead-code --group-by ... --format json`.
///
/// Issues are partitioned into resolver buckets (CODEOWNERS team, directory
/// prefix, workspace package, or GitLab CODEOWNERS section) instead of flat
/// arrays. Each bucket carries the same issue-array shape as the ungrouped
/// `CheckOutput` body, plus per-group `key` / `owners` / `total_issues`.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CheckGroupedOutput {
    /// Schema version for this output format.
    pub schema_version: SchemaVersion,
    /// Fallow tool version that produced this output.
    pub version: ToolVersion,
    /// Analysis duration in milliseconds.
    pub elapsed_ms: ElapsedMs,
    /// The grouping strategy used.
    pub grouped_by: GroupByMode,
    /// Total number of issues across all groups.
    pub total_issues: usize,
    /// One entry per group; each contains the same issue arrays as
    /// `CheckOutput` plus the group key and per-group total.
    pub groups: Vec<CheckGroupedEntry>,
    /// `_meta` block with metric / rule definitions, emitted when `--explain`
    /// is passed.
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Meta>,
}

/// Single resolver bucket inside `CheckGroupedOutput`. Carries the group's
/// identifier, optional section owners, and a per-group flattened
/// `AnalysisResults`.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CheckGroupedEntry {
    /// Group identifier produced by the resolver. For `package` grouping:
    /// workspace package name. For `owner` grouping: the CODEOWNERS team.
    /// For `directory` grouping: the top-level directory prefix. For
    /// `section` grouping: the GitLab CODEOWNERS section name (or
    /// `(no section)` / `(unowned)` for unmatched files).
    pub key: String,
    /// Section default owners (GitLab CODEOWNERS `[Section] @owner1
    /// @owner2`). Emitted only when `grouped_by` is `section`. Empty for
    /// the `(no section)` and `(unowned)` buckets.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owners: Option<Vec<String>>,
    /// Total number of issues in this group.
    pub total_issues: usize,
    /// Per-group issue arrays restricted to files in this group.
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
    /// Group by CODEOWNERS team.
    Owner,
    /// Group by top-level directory prefix.
    Directory,
    /// Group by workspace package name.
    Package,
    /// Group by GitLab CODEOWNERS `[Section]` header name.
    Section,
}
