//! Top-level health report contract.

use crate::{
    CoverageGaps, CoverageIntelligenceReport, CssAnalyticsReport, FileHealthScore,
    FrameworkHealthDiagnostics, HealthActionsMeta, HealthFinding, HealthScore, HealthSummary,
    HealthTrend, HotspotFinding, HotspotSummary, LargeFunctionEntry, RefactoringTargetFinding,
    RuntimeCoverageReport, StylingFinding, StylingHealth, TargetThresholds, ThresholdOverrideState,
    VitalSigns,
};
use fallow_types::output_dead_code::PropDrillingChainFinding;

/// Result of complexity analysis for reporting.
#[derive(Debug, Clone, Default, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct HealthReport {
    /// Functions and synthetic template entries exceeding complexity
    /// thresholds, sorted by the --sort criteria. Each entry wraps its
    /// inner `ComplexityViolation` payload (flattened on the wire) with
    /// the typed `actions` list and an optional audit-mode `introduced`
    /// flag.
    pub findings: Vec<HealthFinding>,
    /// Summary statistics.
    pub summary: HealthSummary,
    /// Configured threshold override states. Entries are emitted for active
    /// exceptions, stale exceptions, and full-run no-match cleanup hints.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub threshold_overrides: Vec<ThresholdOverrideState>,
    /// Project-wide vital signs (always computed from available data).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vital_signs: Option<VitalSigns>,
    /// Project-wide health score (only populated with `--score`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub health_score: Option<HealthScore>,
    /// Per-file health scores. Only present when --file-scores is used. Sorted
    /// by risk-aware triage concern, combining low maintainability and high
    /// CRAP risk. Zero-function files (barrels) are excluded by default.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub file_scores: Vec<FileHealthScore>,
    /// Static coverage gaps.
    ///
    /// Populated when coverage gaps are explicitly requested, or when the
    /// top-level `health` command allows config severity to surface them in the
    /// default report.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coverage_gaps: Option<CoverageGaps>,
    /// Located prop-drilling chains (React/Preact props forwarded unchanged
    /// through 3+ pass-through components). Only present when the opt-in
    /// `prop-drilling` rule is enabled (it defaults to off). Each entry carries
    /// the source, every pass-through hop, and the consumer with file + line +
    /// component, so CI / an agent can act. Surfaced alongside hotspots as a
    /// graph-derived health signal.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub prop_drilling_chains: Vec<PropDrillingChainFinding>,
    /// Hotspot entries combining git churn with complexity. Only present when
    /// --hotspots is used. Sorted by score descending (highest risk first).
    /// Each entry wraps its inner `HotspotEntry` payload (flattened on the
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
    /// Combined coverage, runtime, complexity, and change-scope verdicts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coverage_intelligence: Option<CoverageIntelligenceReport>,
    /// Functions exceeding 60 LOC (very high risk). Only present when unit size
    /// very-high-risk bin >= 3%. Sorted by line count descending.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub large_functions: Vec<LargeFunctionEntry>,
    /// Ranked refactoring recommendations. Only present when --targets is used.
    /// Sorted by efficiency (priority/effort) descending. Each entry wraps
    /// its inner `RefactoringTarget` payload (flattened on the wire) with
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
    /// Optional framework-specific detector coverage. Present only when the
    /// health run already needed the dead-code analysis output.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub framework_health: Option<FrameworkHealthDiagnostics>,
    /// Structural CSS analytics (specificity hotspots, `!important` density,
    /// over-complex selectors, deep nesting). Present only with `--css`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub css_analytics: Option<CssAnalyticsReport>,
    /// Styling-health score and letter grade: a SECOND health axis derived from
    /// the CSS analytics (the design-system axis), orthogonal to the JS/TS code
    /// `health_score`. Present only with `--css` (the same condition as
    /// `css_analytics`), so a plain `fallow health` run is byte-unchanged. The
    /// code score is never affected by this field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub styling_health: Option<StylingHealth>,
    /// Advisory STYLING FINDINGS: the graduation of the descriptive css
    /// candidates into first-class, severity-aware, suppressible findings
    /// surfaced in `fallow audit`. Verdict-neutral by default (the rule defaults
    /// to `warn`); the styling domain's OWN findings collection, not the dead-code
    /// `AnalysisResults`. Present only with `--css`; empty is skipped so a plain
    /// run is byte-unchanged.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub styling_findings: Vec<StylingFinding>,
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
        assert!(!json.contains("css_analytics"));
        assert!(!json.contains("styling_health"));
        assert!(!json.contains("styling_findings"));
    }

    #[test]
    fn health_score_none_skipped_in_report() {
        let report = HealthReport::default();
        let json = serde_json::to_string(&report).expect("health report should serialize");
        assert!(!json.contains("health_score"));
    }
}
