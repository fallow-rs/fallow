//! Per-group health output for `--group-by`.
//!
//! When health is invoked with `--group-by package` (or any other grouping
//! mode), the orchestrator partitions the project's files by the resolver and
//! emits one [`HealthGroup`] per bucket. Each group carries its own
//! [`VitalSigns`] and [`HealthScore`] computed from the files in that group
//! alone, plus the per-file output (findings, file scores, hotspots, large
//! functions, refactoring targets) restricted to the same subset. A group
//! carries a per-file list only when the project report shows that list.

use serde::Serialize;

use crate::{
    CoverageSourceConsistency, FileHealthScore, HealthActionsMeta, HealthFinding, HealthScore,
    HealthTrend, HotspotFinding, LargeFunctionEntry, RefactoringTargetFinding, VitalSigns,
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
/// project-level health summary.
///
/// A group carries a per-file list (`findings`, `file_scores`, `hotspots`,
/// `large_functions`, `targets`) only when the project report shows the same
/// list. A `--score` run keeps the score and the counts of each group and
/// omits the lists.
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owners: Option<Vec<String>>,
    /// Files participating in this group after workspace and ignore filters.
    pub files_analyzed: usize,
    /// Number of findings in this group, mirroring the project-level
    /// `summary.functions_above_threshold` semantics post-baseline /
    /// post-`--top` truncation. When `--top` was supplied this reflects the
    /// rendered finding count of the group, not the un-truncated total.
    pub functions_above_threshold: usize,
    /// Number of critical-severity findings in this group, after the baseline
    /// filter and before `--top`. The project `summary.severity_critical_count`
    /// counts before the baseline filter, so with `--baseline` the group
    /// counts can add up to less.
    pub severity_critical_count: usize,
    /// Number of high-severity findings in this group, after the baseline
    /// filter and before `--top`. The project `summary.severity_high_count`
    /// counts before the baseline filter, so with `--baseline` the group
    /// counts can add up to less.
    pub severity_high_count: usize,
    /// Number of moderate-severity findings in this group, after the baseline
    /// filter and before `--top`. The project `summary.severity_moderate_count`
    /// counts before the baseline filter, so with `--baseline` the group
    /// counts can add up to less.
    pub severity_moderate_count: usize,
    /// Number of ranked hotspot entries in this group, before `--top`. This
    /// is the length of the group's ranked hotspot list. It is not
    /// `vital_signs.hotspot_count`, which counts only the files with a
    /// hotspot score of 50 or more and feeds the health score.
    pub hotspot_count: usize,
    /// Whether CRAP findings in this group share a single coverage-source kind
    /// (`uniform`) or combine Istanbul / estimated / inherited sources
    /// (`mixed`). Absent when no grouped finding carries CRAP source data.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coverage_source_consistency: Option<CoverageSourceConsistency>,
    /// Per-group vital signs recomputed from the files in this group. Absent
    /// when --score-only suppressed top-level vital signs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vital_signs: Option<VitalSigns>,
    /// Per-group health score recomputed from the per-group vital signs. Absent
    /// when --score was not requested. The duplication penalty counts only
    /// the clone groups with two or more instances in this group, so a clone
    /// that spans two groups lowers the project score but no group score.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub health_score: Option<HealthScore>,
    /// Trend of this group against the same group in the baseline snapshot.
    /// Present only when `--trend` or `--trend-from` was requested and the
    /// baseline holds this group with the same `grouped_by` mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trend: Option<HealthTrend>,
    /// Why `trend` is present or absent. Present only when a trend was
    /// requested and a baseline snapshot was loaded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trend_status: Option<GroupTrendStatus>,
    /// Findings restricted to files in this group. Each entry is the typed
    /// [`HealthFinding`] wrapper around a
    /// `ComplexityViolation`
    /// payload.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub findings: Vec<HealthFinding>,
    /// File scores restricted to files in this group.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub file_scores: Vec<FileHealthScore>,
    /// Hotspots restricted to files in this group. Each entry is the typed
    /// [`HotspotFinding`] wrapper around a
    /// `HotspotEntry` payload.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hotspots: Vec<HotspotFinding>,
    /// Large functions in files belonging to this group.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub large_functions: Vec<LargeFunctionEntry>,
    /// Refactoring targets in files belonging to this group. Each entry is
    /// the typed [`RefactoringTargetFinding`] wrapper around a
    /// `RefactoringTarget`
    /// payload.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub targets: Vec<RefactoringTargetFinding>,
    /// Auditable breadcrumb recording why `suppress-line` action hints
    /// were omitted from this group's findings. Mirrors the project-level
    /// `HealthReport.actions_meta`; populated at construction time when the
    /// per-group `HealthActionContext`
    /// suppresses inline hints.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actions_meta: Option<HealthActionsMeta>,
}

/// Group label of the files that no CODEOWNERS rule matches.
const UNOWNED_GROUP_KEY: &str = "(unowned)";

/// The groups in display order for the per-group summary tables.
///
/// With scores, the order is score ascending (worst first), with the
/// unowned group last. Without scores, the resolver order is kept (file count
/// descending, unowned last). The human block, the Markdown table and the
/// GitHub job summary use this order.
#[must_use]
pub fn health_groups_in_display_order(groups: &[HealthGroup]) -> Vec<&HealthGroup> {
    let mut ordered: Vec<&HealthGroup> = groups.iter().collect();
    if groups.iter().any(|group| group.health_score.is_some()) {
        ordered.sort_by(|a, b| {
            let unowned = (a.key == UNOWNED_GROUP_KEY).cmp(&(b.key == UNOWNED_GROUP_KEY));
            let a_score = a.health_score.as_ref().map_or(f64::INFINITY, |hs| hs.score);
            let b_score = b.health_score.as_ref().map_or(f64::INFINITY, |hs| hs.score);
            unowned.then(
                a_score
                    .partial_cmp(&b_score)
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
        });
    }
    ordered
}

/// Short label for the score change of a group against the trend baseline.
///
/// `+2.3 \u{2191}` for a compared group with a score metric, `new` for a group
/// that the baseline does not hold, and `-` otherwise.
#[must_use]
pub fn group_score_delta_label(group: &HealthGroup) -> String {
    match group.trend_status {
        Some(GroupTrendStatus::NewGroup) => "new".to_owned(),
        Some(GroupTrendStatus::Compared) => group
            .trend
            .as_ref()
            .and_then(|trend| trend.metrics.iter().find(|metric| metric.name == "score"))
            .map_or_else(
                || "-".to_owned(),
                |metric| format!("{:+.1} {}", metric.delta, metric.direction.arrow()),
            ),
        _ => "-".to_owned(),
    }
}

/// What the group trend compared, for one group.
///
/// The value set is open: read an unknown value as "no trend for this group".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum GroupTrendStatus {
    /// The baseline holds this group, and `trend` compares the two.
    Compared,
    /// The baseline holds group data, but not for this group key.
    NewGroup,
    /// The baseline did not measure this group. Either the baseline holds no
    /// group data for this `grouped_by` mode, and a
    /// `trend-group-baseline-unavailable` diagnostic says why, or the
    /// `--group` selection of the baseline run left this group key out.
    NoGroupBaseline,
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
    /// The `--group` selector patterns, as the user gave them. `None` when no
    /// selector was given.
    pub filter: Option<Vec<String>>,
    /// The positive `--group` patterns that matched no group key.
    pub unmatched_filters: Vec<String>,
}
