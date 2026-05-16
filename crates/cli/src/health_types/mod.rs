//! Health / complexity analysis report types.
//!
//! Separated from the `health` command module so that report formatters
//! (which are compiled as part of both the lib and bin targets) can
//! reference these types without pulling in binary-only dependencies.

mod coverage;
mod grouped;
mod runtime_coverage;
mod scores;
mod targets;
mod trends;
mod vital_signs;

pub use coverage::*;
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
    pub complexity_ms: f64,
    pub file_scores_ms: f64,
    pub git_churn_ms: f64,
    pub git_churn_cache_hit: bool,
    pub hotspots_ms: f64,
    pub duplication_ms: f64,
    pub targets_ms: f64,
    pub total_ms: f64,
}

/// Result of complexity analysis for reporting.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct HealthReport {
    /// Functions exceeding thresholds.
    pub findings: Vec<HealthFinding>,
    /// Summary statistics.
    pub summary: HealthSummary,
    /// Project-wide vital signs (always computed from available data).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vital_signs: Option<VitalSigns>,
    /// Project-wide health score (only populated with `--score`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health_score: Option<HealthScore>,
    /// Per-file health scores (only populated with `--file-scores` or `--hotspots`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub file_scores: Vec<FileHealthScore>,
    /// Static coverage gaps.
    ///
    /// Populated when coverage gaps are explicitly requested, or when the
    /// top-level `health` command allows config severity to surface them in the
    /// default report.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coverage_gaps: Option<CoverageGaps>,
    /// Hotspot entries (only populated with `--hotspots`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hotspots: Vec<HotspotEntry>,
    /// Hotspot analysis summary (only set with `--hotspots`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hotspot_summary: Option<HotspotSummary>,
    /// Runtime coverage findings from the paid sidecar (only populated with
    /// `--runtime-coverage`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime_coverage: Option<RuntimeCoverageReport>,
    /// Functions exceeding 60 LOC (only populated when unit size very-high-risk >= 3%).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub large_functions: Vec<LargeFunctionEntry>,
    /// Ranked refactoring recommendations (only populated with `--targets`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub targets: Vec<RefactoringTarget>,
    /// Adaptive thresholds used for target scoring (only set with `--targets`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_thresholds: Option<TargetThresholds>,
    /// Health trend comparison against a previous snapshot (only set with `--trend`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health_trend: Option<HealthTrend>,
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
