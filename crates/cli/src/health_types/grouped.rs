//! Per-group health output for `--group-by`.
//!
//! When health is invoked with `--group-by package` (or any other grouping
//! mode), the orchestrator partitions the project's files by the resolver and
//! emits one [`HealthGroup`] per bucket. Each group carries its own
//! [`VitalSigns`] and [`HealthScore`] computed from the files in that group
//! alone, plus the per-file output (findings, file scores, hotspots, large
//! functions, refactoring targets) restricted to the same subset.

use serde::Serialize;

use crate::health_types::{
    FileHealthScore, HealthFinding, HealthScore, HotspotEntry, LargeFunctionEntry,
    RefactoringTarget, VitalSigns,
};

/// A health report scoped to a single group.
///
/// `key` is the group label produced by the resolver (workspace package name,
/// CODEOWNERS owner, directory, or section). `owners` is populated only for
/// `--group-by section` (mirrors dead-code grouped output).
///
/// Per-group `vital_signs` and `health_score` are recomputed from the
/// files in the group, so they answer "what is the health of workspace X" in
/// a single invocation. `files_analyzed` and `functions_above_threshold`
/// summarise the subset for parity with the project-level
/// [`crate::health_types::HealthSummary`].
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct HealthGroup {
    /// Group label.
    pub key: String,
    /// Section default owners (`--group-by section` only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owners: Option<Vec<String>>,
    /// Files participating in this group after workspace and ignore filters.
    pub files_analyzed: usize,
    /// Number of findings in this group, mirroring the project-level
    /// `summary.functions_above_threshold` semantics post-baseline /
    /// post-`--top` truncation. When `--top` was supplied this reflects the
    /// rendered finding count, not the un-truncated total.
    pub functions_above_threshold: usize,
    /// Per-group vital signs (None when `--score-only` suppressed them).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vital_signs: Option<VitalSigns>,
    /// Per-group health score (None when `--score` was not requested).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health_score: Option<HealthScore>,
    /// Findings restricted to files in this group.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub findings: Vec<HealthFinding>,
    /// File scores restricted to files in this group.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub file_scores: Vec<FileHealthScore>,
    /// Hotspots restricted to files in this group.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hotspots: Vec<HotspotEntry>,
    /// Large functions in files belonging to this group.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub large_functions: Vec<LargeFunctionEntry>,
    /// Refactoring targets in files belonging to this group.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub targets: Vec<RefactoringTarget>,
}

/// Wrapper carrying the resolver mode label alongside the partitioned groups.
///
/// Stored on `crate::health::HealthResult` when `--group-by` is active and
/// consumed by formatters that either render grouped data directly or annotate
/// per-finding machine output with the group key.
#[derive(Debug, Clone)]
pub struct HealthGrouping {
    /// Resolver mode label (`"package"`, `"owner"`, `"directory"`, `"section"`).
    pub mode: &'static str,
    /// Groups in the same order the resolver produced them.
    pub groups: Vec<HealthGroup>,
}
