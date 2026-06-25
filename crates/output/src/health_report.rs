//! Top-level health report contract.

use crate::{
    CoverageGaps, CoverageIntelligenceReport, CssAnalyticsReport, FileHealthScore,
    FrameworkHealthDiagnostics, HealthActionsMeta, HealthFinding, HealthScore, HealthSummary,
    HealthTrend, HotspotFinding, HotspotSummary, LargeFunctionEntry, RefactoringTargetFinding,
    RuntimeCoverageReport, TargetThresholds, ThresholdOverrideState, VitalSigns,
};
use fallow_types::output_dead_code::PropDrillingChainFinding;

/// Result of complexity analysis for reporting.
#[derive(Debug, Clone, Default, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct HealthReport {
    /// Functions and synthetic template entries exceeding complexity
    /// thresholds, sorted by the --sort criteria.
    pub findings: Vec<HealthFinding>,
    /// Summary statistics.
    pub summary: HealthSummary,
    /// Configured threshold override states.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub threshold_overrides: Vec<ThresholdOverrideState>,
    /// Project-wide vital signs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vital_signs: Option<VitalSigns>,
    /// Project-wide health score.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub health_score: Option<HealthScore>,
    /// Per-file health scores.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub file_scores: Vec<FileHealthScore>,
    /// Static coverage gaps.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coverage_gaps: Option<CoverageGaps>,
    /// Located prop-drilling chains.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub prop_drilling_chains: Vec<PropDrillingChainFinding>,
    /// Hotspot entries combining git churn with complexity.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hotspots: Vec<HotspotFinding>,
    /// Hotspot analysis summary.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hotspot_summary: Option<HotspotSummary>,
    /// Runtime coverage findings from the paid sidecar.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_coverage: Option<RuntimeCoverageReport>,
    /// Combined coverage, runtime, complexity, and change-scope verdicts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coverage_intelligence: Option<CoverageIntelligenceReport>,
    /// Functions exceeding 60 LOC.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub large_functions: Vec<LargeFunctionEntry>,
    /// Ranked refactoring recommendations.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub targets: Vec<RefactoringTargetFinding>,
    /// Adaptive thresholds used for target scoring.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_thresholds: Option<TargetThresholds>,
    /// Health trend comparison against a previous snapshot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub health_trend: Option<HealthTrend>,
    /// Audit breadcrumb explaining systemic action-array adjustments.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actions_meta: Option<HealthActionsMeta>,
    /// Optional framework-specific detector coverage.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub framework_health: Option<FrameworkHealthDiagnostics>,
    /// Structural CSS analytics.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub css_analytics: Option<CssAnalyticsReport>,
    /// Per-file top render fan-in for the descriptive human drill-down only.
    #[serde(skip)]
    pub render_fan_in_top: rustc_hash::FxHashMap<std::path::PathBuf, (String, u32)>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_report_skips_empty_collections() {
        let report = HealthReport::default();
        let json = serde_json::to_string(&report).expect("health report should serialize");
        assert!(!json.contains("file_scores"));
        assert!(!json.contains("hotspots"));
        assert!(!json.contains("hotspot_summary"));
        assert!(!json.contains("runtime_coverage"));
        assert!(!json.contains("coverage_intelligence"));
        assert!(!json.contains("large_functions"));
        assert!(!json.contains("targets"));
        assert!(!json.contains("threshold_overrides"));
        assert!(!json.contains("vital_signs"));
        assert!(!json.contains("health_score"));
        assert!(!json.contains("framework_health"));
    }

    #[test]
    fn health_score_none_skipped_in_report() {
        let report = HealthReport::default();
        let json = serde_json::to_string(&report).expect("health report should serialize");
        assert!(!json.contains("health_score"));
    }
}
