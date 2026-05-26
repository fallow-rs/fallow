//! Health / complexity analysis report types.
//!
//! Separated from the `health` command module so that report formatters
//! (which are compiled as part of both the lib and bin targets) can
//! reference these types without pulling in binary-only dependencies.

mod coverage;
mod finding;
mod grouped;
mod runtime_coverage;
mod scores;
mod targets;
mod trends;
mod vital_signs;

pub use coverage::*;
pub use finding::*;
pub use grouped::*;
pub use runtime_coverage::*;
pub use scores::*;
pub use targets::*;
pub use trends::*;
pub use vital_signs::*;

/// Detailed timing breakdown for the health pipeline.
///
/// Only populated when `--performance` is passed.
#[derive(Debug, Clone, serde::Serialize)]
pub struct HealthTimings {
    pub config_ms: f64,
    pub discover_ms: f64,
    pub parse_ms: f64,
    /// Summed wall-clock time of the actual AST parses across all rayon
    /// workers (the parse stage's CPU cost). `parse_ms` is the stage's
    /// wall-clock time. Observational and non-deterministic; do not assert
    /// against it. `0.0` when `shared_parse` is true (parse was reused).
    pub parse_cpu_ms: f64,
    pub complexity_ms: f64,
    pub file_scores_ms: f64,
    pub git_churn_ms: f64,
    pub git_churn_cache_hit: bool,
    pub hotspots_ms: f64,
    pub duplication_ms: f64,
    pub targets_ms: f64,
    pub total_ms: f64,
    /// True when discover + parse were reused from the upstream dead-code
    /// (check) pass in combined mode, so their timings are `0.0` here and
    /// the cost is attributed to the `Pipeline Performance` table instead.
    /// The renderer shows those two stages as `(measured above)`.
    pub shared_parse: bool,
}

/// Auditable breadcrumb recording when health-finding `suppress-line`
/// action hints were omitted from the report.
///
/// Set at construction time on [`HealthReport::actions_meta`] (and on
/// each [`HealthGroup::actions_meta`](crate::health_types::HealthGroup)
/// when grouped) by the report builder, derived from the active
/// [`HealthActionContext`]. Lets consumers see "where did the
/// suppress-line hints go?" without having to grep the config or CLI
/// history.
///
/// Stable `reason` codes:
/// - `baseline-active`: a baseline is active and inline ignores would
///   become dead annotations once the baseline regenerates.
/// - `config-disabled`: `health.suggestInlineSuppression` is `false`.
/// - `unspecified`: the caller did not record a reason.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct HealthActionsMeta {
    /// Always `true` when the breadcrumb is emitted. Absent from the wire
    /// when no suppression occurred.
    pub suppression_hints_omitted: bool,
    /// Stable code describing why the suppression occurred.
    pub reason: String,
    /// Scope of the omission. Always `"health-findings"` today.
    pub scope: String,
}

/// Result of complexity analysis for reporting.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct HealthReport {
    /// Functions and synthetic template entries exceeding complexity
    /// thresholds, sorted by the --sort criteria. Each entry wraps its
    /// inner [`ComplexityViolation`] payload (flattened on the wire) with
    /// the typed `actions` list and an optional audit-mode `introduced`
    /// flag.
    pub findings: Vec<HealthFinding>,
    /// Summary statistics.
    pub summary: HealthSummary,
    /// Project-wide vital signs (always computed from available data).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vital_signs: Option<VitalSigns>,
    /// Project-wide health score (only populated with `--score`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub health_score: Option<HealthScore>,
    /// Per-file health scores. Only present when --file-scores is used. Sorted
    /// by maintainability_index ascending (worst first). Zero-function files
    /// (barrels) are excluded by default.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub file_scores: Vec<FileHealthScore>,
    /// Static coverage gaps.
    ///
    /// Populated when coverage gaps are explicitly requested, or when the
    /// top-level `health` command allows config severity to surface them in the
    /// default report.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coverage_gaps: Option<CoverageGaps>,
    /// Hotspot entries combining git churn with complexity. Only present when
    /// --hotspots is used. Sorted by score descending (highest risk first).
    /// Each entry wraps its inner [`HotspotEntry`] payload (flattened on the
    /// wire) with a typed `actions` list.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hotspots: Vec<HotspotFinding>,
    /// Hotspot analysis summary (only set with `--hotspots`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hotspot_summary: Option<HotspotSummary>,
    /// Runtime coverage findings from the paid sidecar (only populated with
    /// `--runtime-coverage`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_coverage: Option<RuntimeCoverageReport>,
    /// Functions exceeding 60 LOC (very high risk). Only present when unit size
    /// very-high-risk bin >= 3%. Sorted by line count descending.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub large_functions: Vec<LargeFunctionEntry>,
    /// Ranked refactoring recommendations. Only present when --targets is used.
    /// Sorted by efficiency (priority/effort) descending. Each entry wraps
    /// its inner [`RefactoringTarget`] payload (flattened on the wire) with
    /// a typed `actions` list.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub targets: Vec<RefactoringTargetFinding>,
    /// Adaptive thresholds used for target scoring (only set with `--targets`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_thresholds: Option<TargetThresholds>,
    /// Health trend comparison against a previous snapshot (only set with `--trend`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub health_trend: Option<HealthTrend>,
    /// Audit breadcrumb explaining systemic action-array adjustments. Present
    /// only when at least one adjustment was made (e.g., health finding
    /// suppression hints omitted because a baseline is active). When --group-by
    /// is active, each entry of `groups` may carry its own `actions_meta`
    /// describing the same omission so per-group consumers do not need to walk
    /// back to the report root.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actions_meta: Option<HealthActionsMeta>,
}

#[cfg(test)]
#[expect(
    clippy::derivable_impls,
    reason = "test-only Default with custom HealthSummary thresholds (20/15)"
)]
impl Default for HealthReport {
    fn default() -> Self {
        Self {
            findings: vec![],
            summary: HealthSummary::default(),
            vital_signs: None,
            health_score: None,
            file_scores: vec![],
            coverage_gaps: None,
            hotspots: vec![],
            hotspot_summary: None,
            runtime_coverage: None,
            large_functions: vec![],
            targets: vec![],
            target_thresholds: None,
            health_trend: None,
            actions_meta: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_report_skips_empty_collections() {
        let report = HealthReport::default();
        let json = serde_json::to_string(&report).unwrap();
        // Empty vecs should be omitted due to skip_serializing_if
        assert!(!json.contains("file_scores"));
        assert!(!json.contains("hotspots"));
        assert!(!json.contains("hotspot_summary"));
        assert!(!json.contains("runtime_coverage"));
        assert!(!json.contains("large_functions"));
        assert!(!json.contains("targets"));
        assert!(!json.contains("vital_signs"));
        assert!(!json.contains("health_score"));
    }

    #[test]
    fn health_score_none_skipped_in_report() {
        let report = HealthReport::default();
        let json = serde_json::to_string(&report).unwrap();
        assert!(!json.contains("health_score"));
    }
}
