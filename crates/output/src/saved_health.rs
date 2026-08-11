use std::path::PathBuf;

use crate::{
    ComplexityViolation, Confidence, CoverageGapSummary, CoverageGaps,
    CoverageIntelligenceConfidence, CoverageIntelligenceEvidence, CoverageIntelligenceFinding,
    CoverageIntelligenceRecommendation, CoverageIntelligenceReport,
    CoverageIntelligenceSchemaVersion, CoverageIntelligenceSignal, CoverageIntelligenceSummary,
    CoverageIntelligenceVerdict, EffortEstimate, ExceededThreshold, FindingSeverity, HealthFinding,
    HealthReport, HealthSummary, RecommendationCategory, RefactoringTarget,
    RefactoringTargetFinding, RuntimeCoverageConfidence, RuntimeCoverageEvidence,
    RuntimeCoverageFinding, RuntimeCoverageReport, RuntimeCoverageVerdict, StylingFinding,
    StylingFindingSeverity, UntestedExport, UntestedExportFinding, UntestedFile,
    UntestedFileFinding,
};
use serde::Deserialize;

#[derive(Deserialize)]
struct SavedHealthReport {
    #[serde(default)]
    findings: Vec<SavedHealthFinding>,
    #[serde(default)]
    summary: SavedHealthSummary,
    #[serde(default)]
    coverage_gaps: Option<SavedCoverageGaps>,
    #[serde(default)]
    runtime_coverage: Option<SavedRuntimeCoverage>,
    #[serde(default)]
    coverage_intelligence: Option<SavedCoverageIntelligence>,
    #[serde(default)]
    targets: Vec<SavedRefactoringTarget>,
    #[serde(default)]
    styling_findings: Vec<SavedStylingFinding>,
}

#[derive(Deserialize)]
struct SavedHealthFinding {
    path: PathBuf,
    name: String,
    line: u32,
    col: u32,
    cyclomatic: u16,
    cognitive: u16,
    line_count: u32,
    param_count: u8,
    exceeded: ExceededThreshold,
    severity: FindingSeverity,
    #[serde(default)]
    crap: Option<f64>,
    #[serde(default)]
    coverage_pct: Option<f64>,
    #[serde(default)]
    introduced: Option<bool>,
    /// Carried through so a `report --from` re-render describes the finding
    /// against the ceiling it was measured with. Absent in envelopes written by
    /// an older fallow, which then fall back to the summary (issue #2163).
    #[serde(default)]
    effective_thresholds: Option<crate::HealthEffectiveThresholds>,
    #[serde(default)]
    threshold_source: Option<crate::ThresholdSource>,
}

#[derive(Default, Deserialize)]
#[expect(
    clippy::struct_field_names,
    reason = "saved wire fields intentionally mirror the public health summary contract"
)]
struct SavedHealthSummary {
    #[serde(default = "default_max_cyclomatic")]
    max_cyclomatic_threshold: u16,
    #[serde(default = "default_max_cognitive")]
    max_cognitive_threshold: u16,
    #[serde(default = "default_max_crap")]
    max_crap_threshold: f64,
    #[serde(default = "default_max_unit_size")]
    max_unit_size_threshold: u32,
}

const fn default_max_cyclomatic() -> u16 {
    20
}

const fn default_max_cognitive() -> u16 {
    15
}

fn default_max_crap() -> f64 {
    30.0
}

const fn default_max_unit_size() -> u32 {
    crate::DEFAULT_MAX_UNIT_SIZE
}

#[derive(Deserialize)]
struct SavedRuntimeCoverage {
    #[serde(default)]
    findings: Vec<SavedRuntimeCoverageFinding>,
}

#[derive(Deserialize)]
struct SavedRuntimeCoverageFinding {
    #[serde(default)]
    id: String,
    #[serde(default)]
    stable_id: Option<String>,
    #[serde(default)]
    source_hash: Option<String>,
    path: PathBuf,
    function: String,
    line: u32,
    verdict: RuntimeCoverageVerdict,
    #[serde(default)]
    invocations: Option<u64>,
}

#[derive(Deserialize)]
struct SavedCoverageIntelligence {
    #[serde(default)]
    findings: Vec<SavedCoverageIntelligenceFinding>,
}

#[derive(Deserialize)]
struct SavedCoverageIntelligenceFinding {
    id: String,
    path: PathBuf,
    #[serde(default)]
    identity: Option<String>,
    line: u32,
    verdict: CoverageIntelligenceVerdict,
    #[serde(default)]
    signals: Vec<CoverageIntelligenceSignal>,
    recommendation: CoverageIntelligenceRecommendation,
    #[serde(default = "default_coverage_intelligence_confidence")]
    confidence: CoverageIntelligenceConfidence,
    #[serde(default)]
    related_ids: Vec<String>,
}

const fn default_coverage_intelligence_confidence() -> CoverageIntelligenceConfidence {
    CoverageIntelligenceConfidence::Low
}

#[derive(Deserialize)]
struct SavedCoverageGaps {
    #[serde(default)]
    summary: SavedCoverageGapSummary,
    #[serde(default)]
    files: Vec<SavedUntestedFile>,
    #[serde(default)]
    exports: Vec<SavedUntestedExport>,
}

#[derive(Default, Deserialize)]
struct SavedCoverageGapSummary {
    #[serde(default)]
    runtime_files: usize,
    #[serde(default)]
    covered_files: usize,
    #[serde(default)]
    file_coverage_pct: f64,
    #[serde(default)]
    untested_files: usize,
    #[serde(default)]
    untested_exports: usize,
}

#[derive(Deserialize)]
struct SavedUntestedFile {
    path: PathBuf,
    value_export_count: usize,
}

#[derive(Deserialize)]
struct SavedUntestedExport {
    path: PathBuf,
    export_name: String,
    line: u32,
    col: u32,
}

#[derive(Deserialize)]
struct SavedRefactoringTarget {
    path: PathBuf,
    priority: f64,
    efficiency: f64,
    recommendation: String,
    category: RecommendationCategory,
    effort: EffortEstimate,
    confidence: Confidence,
}

#[derive(Deserialize)]
struct SavedStylingFinding {
    code: String,
    sub_kind: String,
    path: String,
    line: u32,
    value: String,
    effective_severity: StylingFindingSeverity,
}

/// Rehydrate the finding-bearing portion of a saved health JSON section.
///
/// Presentation-only fields are intentionally omitted because CI renderers
/// consume findings, thresholds, coverage gaps, refactoring targets, and
/// styling findings. Returning `None` keeps forward-incompatible envelopes on
/// the generic compatibility renderer instead of emitting partial native data.
#[must_use]
pub fn health_report_from_saved_value(envelope: &serde_json::Value) -> Option<HealthReport> {
    let saved = serde_json::from_value::<SavedHealthReport>(envelope.clone()).ok()?;
    Some(saved.into())
}

impl From<SavedHealthReport> for HealthReport {
    fn from(saved: SavedHealthReport) -> Self {
        Self {
            findings: saved.findings.into_iter().map(Into::into).collect(),
            summary: saved.summary.into(),
            coverage_gaps: saved.coverage_gaps.map(Into::into),
            runtime_coverage: saved.runtime_coverage.map(Into::into),
            coverage_intelligence: saved.coverage_intelligence.map(Into::into),
            targets: saved.targets.into_iter().map(Into::into).collect(),
            styling_findings: saved.styling_findings.into_iter().map(Into::into).collect(),
            ..Self::default()
        }
    }
}

impl From<SavedHealthFinding> for HealthFinding {
    fn from(saved: SavedHealthFinding) -> Self {
        Self::new(
            ComplexityViolation {
                path: saved.path,
                name: saved.name,
                line: saved.line,
                col: saved.col,
                cyclomatic: saved.cyclomatic,
                cognitive: saved.cognitive,
                line_count: saved.line_count,
                param_count: saved.param_count,
                react_hook_count: 0,
                react_jsx_max_depth: 0,
                react_prop_count: 0,
                react_hook_profile: None,
                exceeded: saved.exceeded,
                severity: saved.severity,
                crap: saved.crap,
                coverage_pct: saved.coverage_pct,
                coverage_tier: None,
                coverage_source: None,
                inherited_from: None,
                component_rollup: None,
                contributions: Vec::new(),
                effective_thresholds: saved.effective_thresholds,
                threshold_source: saved.threshold_source,
            },
            Vec::new(),
            saved.introduced,
        )
    }
}

impl From<SavedHealthSummary> for HealthSummary {
    fn from(saved: SavedHealthSummary) -> Self {
        Self {
            max_cyclomatic_threshold: saved.max_cyclomatic_threshold,
            max_cognitive_threshold: saved.max_cognitive_threshold,
            max_crap_threshold: saved.max_crap_threshold,
            max_unit_size_threshold: saved.max_unit_size_threshold,
            ..Self::default()
        }
    }
}

impl From<SavedRuntimeCoverage> for RuntimeCoverageReport {
    fn from(saved: SavedRuntimeCoverage) -> Self {
        Self {
            findings: saved.findings.into_iter().map(Into::into).collect(),
            ..Self::default()
        }
    }
}

impl From<SavedRuntimeCoverageFinding> for RuntimeCoverageFinding {
    fn from(saved: SavedRuntimeCoverageFinding) -> Self {
        Self {
            id: saved.id,
            stable_id: saved.stable_id,
            source_hash: saved.source_hash,
            path: saved.path,
            function: saved.function,
            line: saved.line,
            verdict: saved.verdict,
            invocations: saved.invocations,
            confidence: RuntimeCoverageConfidence::Unknown,
            evidence: RuntimeCoverageEvidence {
                static_status: String::new(),
                test_coverage: String::new(),
                v8_tracking: String::new(),
                untracked_reason: None,
                observation_days: 0,
                deployments_observed: 0,
            },
            actions: Vec::new(),
            discriminators: None,
        }
    }
}

impl From<SavedCoverageIntelligence> for CoverageIntelligenceReport {
    fn from(saved: SavedCoverageIntelligence) -> Self {
        Self {
            schema_version: CoverageIntelligenceSchemaVersion::default(),
            verdict: CoverageIntelligenceVerdict::Unknown,
            summary: CoverageIntelligenceSummary::default(),
            findings: saved.findings.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<SavedCoverageIntelligenceFinding> for CoverageIntelligenceFinding {
    fn from(saved: SavedCoverageIntelligenceFinding) -> Self {
        Self {
            id: saved.id,
            path: saved.path,
            identity: saved.identity,
            line: saved.line,
            verdict: saved.verdict,
            signals: saved.signals,
            recommendation: saved.recommendation,
            confidence: saved.confidence,
            related_ids: saved.related_ids,
            evidence: CoverageIntelligenceEvidence::default(),
            actions: Vec::new(),
        }
    }
}

impl From<SavedCoverageGaps> for CoverageGaps {
    fn from(saved: SavedCoverageGaps) -> Self {
        Self {
            summary: saved.summary.into(),
            files: saved.files.into_iter().map(Into::into).collect(),
            exports: saved.exports.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<SavedCoverageGapSummary> for CoverageGapSummary {
    fn from(saved: SavedCoverageGapSummary) -> Self {
        Self {
            runtime_files: saved.runtime_files,
            covered_files: saved.covered_files,
            file_coverage_pct: saved.file_coverage_pct,
            untested_files: saved.untested_files,
            untested_exports: saved.untested_exports,
        }
    }
}

impl From<SavedUntestedFile> for UntestedFileFinding {
    fn from(saved: SavedUntestedFile) -> Self {
        Self {
            file: UntestedFile {
                path: saved.path,
                value_export_count: saved.value_export_count,
            },
            actions: Vec::new(),
        }
    }
}

impl From<SavedUntestedExport> for UntestedExportFinding {
    fn from(saved: SavedUntestedExport) -> Self {
        Self {
            export: UntestedExport {
                path: saved.path,
                export_name: saved.export_name,
                line: saved.line,
                col: saved.col,
            },
            actions: Vec::new(),
        }
    }
}

impl From<SavedRefactoringTarget> for RefactoringTargetFinding {
    fn from(saved: SavedRefactoringTarget) -> Self {
        Self::from(RefactoringTarget {
            path: saved.path,
            priority: saved.priority,
            efficiency: saved.efficiency,
            recommendation: saved.recommendation,
            category: saved.category,
            effort: saved.effort,
            confidence: saved.confidence,
            factors: Vec::new(),
            evidence: None,
        })
    }
}

impl From<SavedStylingFinding> for StylingFinding {
    fn from(saved: SavedStylingFinding) -> Self {
        Self {
            code: saved.code,
            sub_kind: saved.sub_kind,
            path: saved.path,
            line: saved.line,
            value: saved.value,
            effective_severity: saved.effective_severity,
            blast_radius: None,
            confidence: None,
            agent_disposition: None,
            nearest_token: None,
            fix_hint: None,
            actions: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::health_report_from_saved_value;

    fn envelope(extra: &str) -> serde_json::Value {
        serde_json::from_str(&format!(
            r#"{{
              "summary": {{ "max_crap_threshold": 30.0 }},
              "findings": [{{
                "path": "src/Board.astro",
                "name": "<template>",
                "line": 6,
                "col": 3,
                "cyclomatic": 11,
                "cognitive": 4,
                "line_count": 20,
                "param_count": 0,
                "exceeded": "crap",
                "severity": "critical",
                "crap": 132.0{extra}
              }}]
            }}"#
        ))
        .expect("envelope fixture parses")
    }

    /// A `report --from` re-render must describe the finding against the
    /// ceiling it was measured with, matching a direct render (issue #2163).
    #[test]
    fn saved_findings_keep_their_override_thresholds() {
        let report = health_report_from_saved_value(&envelope(
            r#", "threshold_source": "override",
                "effective_thresholds": {
                  "max_cyclomatic": 20,
                  "max_cognitive": 15,
                  "max_crap": 100.0,
                  "max_unit_size": 60
                }"#,
        ))
        .expect("saved envelope rehydrates");

        let thresholds = report.findings[0].resolved_thresholds(&report.summary);
        assert!((thresholds.max_crap - 100.0).abs() < f64::EPSILON);
    }

    /// Envelopes written by an older fallow carry no `effective_thresholds`.
    #[test]
    fn saved_findings_without_thresholds_fall_back_to_the_summary() {
        let report =
            health_report_from_saved_value(&envelope("")).expect("saved envelope rehydrates");

        let thresholds = report.findings[0].resolved_thresholds(&report.summary);
        assert!((thresholds.max_crap - 30.0).abs() < f64::EPSILON);
        assert_eq!(thresholds.max_unit_size, crate::DEFAULT_MAX_UNIT_SIZE);
    }
}
