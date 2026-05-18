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
    FileHealthScore, HealthActionsMeta, HealthFinding, HealthScore, HotspotEntry,
    LargeFunctionEntry, RefactoringTarget, VitalSigns,
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
    /// Group identifier produced by the resolver. For 'package' grouping:
    /// workspace package name (e.g. '@scope/app-a') or '(root)' for files
    /// outside any workspace. For 'owner' grouping: the CODEOWNERS team. For
    /// 'directory' grouping: the top-level directory prefix. For 'section'
    /// grouping: the GitLab CODEOWNERS section name, or '(no section)' /
    /// '(unowned)' for unmatched files.
    pub key: String,
    /// Section default owners (GitLab CODEOWNERS `[Section] @owner1 @owner2`).
    /// Present only when grouped_by is 'section'.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owners: Option<Vec<String>>,
    /// Files participating in this group after workspace and ignore filters.
    pub files_analyzed: usize,
    /// Number of findings in this group, mirroring the project-level
    /// `summary.functions_above_threshold` semantics post-baseline /
    /// post-`--top` truncation. When `--top` was supplied this reflects the
    /// rendered finding count, not the un-truncated total.
    pub functions_above_threshold: usize,
    /// Per-group vital signs recomputed from the files in this group. Absent
    /// when --score-only suppressed top-level vital signs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vital_signs: Option<VitalSigns>,
    /// Per-group health score recomputed from the per-group vital signs. Absent
    /// when --score was not requested.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health_score: Option<HealthScore>,
    /// Findings restricted to files in this group. Each entry is the typed
    /// [`HealthFinding`] wrapper around a
    /// [`ComplexityViolation`](crate::health_types::ComplexityViolation)
    /// payload.
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
    /// Auditable breadcrumb recording why `suppress-line` action hints
    /// were omitted from this group's findings. Mirrors the project-level
    /// `HealthReport.actions_meta`; populated at construction time when the
    /// per-group [`HealthActionContext`](crate::health_types::HealthActionContext)
    /// suppresses inline hints.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actions_meta: Option<HealthActionsMeta>,
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
