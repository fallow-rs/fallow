use std::fmt;
use std::path::PathBuf;

use fallow_types::serde_path;

/// Coverage-intelligence JSON contract version. Scoped to the
/// `coverage_intelligence` block and independent of the top-level fallow
/// JSON `schema_version`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum CoverageIntelligenceSchemaVersion {
    /// First release of the coverage-intelligence block contract.
    #[default]
    #[serde(rename = "1")]
    V1,
}

/// Headline verdict for the combined coverage-intelligence report.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum CoverageIntelligenceVerdict {
    /// A changed hot path lacks test coverage.
    RiskyChangeDetected,
    /// Statically unused and runtime-cold; safe delete candidate.
    HighConfidenceDelete,
    /// Evidence conflicts; a human should look before acting.
    ReviewRequired,
    /// Runtime-reachable but poorly tested; behavior must be preserved.
    RefactorCarefully,
    /// No combined-signal findings.
    Clean,
    /// Evidence was insufficient to reach a verdict.
    #[default]
    Unknown,
}

impl CoverageIntelligenceVerdict {
    /// Kebab-case wire value of the verdict.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RiskyChangeDetected => "risky-change-detected",
            Self::HighConfidenceDelete => "high-confidence-delete",
            Self::ReviewRequired => "review-required",
            Self::RefactorCarefully => "refactor-carefully",
            Self::Clean => "clean",
            Self::Unknown => "unknown",
        }
    }
}

impl fmt::Display for CoverageIntelligenceVerdict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Ordered evidence signals behind a coverage-intelligence finding.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum CoverageIntelligenceSignal {
    /// The unit was changed in the current scope.
    Changed,
    /// Runtime coverage shows frequent execution.
    HotPath,
    /// Test coverage is below the low threshold.
    LowTestCoverage,
    /// CRAP score is above the risk threshold.
    HighCrap,
    /// Static analysis found no consumer.
    StaticUnused,
    /// Runtime coverage never or rarely saw it execute.
    RuntimeCold,
    /// No test dependency path reaches the unit.
    NoTestPath,
    /// Runtime coverage saw it execute.
    RuntimeReachable,
    /// File ownership drifted from its historic maintainers.
    OwnershipDrift,
    /// A test dependency path reaches the unit.
    TestCovered,
}

impl CoverageIntelligenceSignal {
    /// Snake-case wire value of the signal.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Changed => "changed",
            Self::HotPath => "hot_path",
            Self::LowTestCoverage => "low_test_coverage",
            Self::HighCrap => "high_crap",
            Self::StaticUnused => "static_unused",
            Self::RuntimeCold => "runtime_cold",
            Self::NoTestPath => "no_test_path",
            Self::RuntimeReachable => "runtime_reachable",
            Self::OwnershipDrift => "ownership_drift",
            Self::TestCovered => "test_covered",
        }
    }
}

impl fmt::Display for CoverageIntelligenceSignal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Recommended action family for a combined finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum CoverageIntelligenceRecommendation {
    /// Cover or split the risky change before merging.
    AddTestOrSplitBeforeMerge,
    /// Delete the unit once an owner confirms it is dead.
    DeleteAfterConfirmingOwner,
    /// Have a human review before changing the unit.
    ReviewBeforeChanging,
    /// Refactor with behavior-preserving steps only.
    RefactorCarefullyKeepBehavior,
}

impl CoverageIntelligenceRecommendation {
    /// Kebab-case wire value of the recommendation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AddTestOrSplitBeforeMerge => "add-test-or-split-before-merge",
            Self::DeleteAfterConfirmingOwner => "delete-after-confirming-owner",
            Self::ReviewBeforeChanging => "review-before-changing",
            Self::RefactorCarefullyKeepBehavior => "refactor-carefully-keep-behavior",
        }
    }
}

impl fmt::Display for CoverageIntelligenceRecommendation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Confidence in the joined evidence and resulting recommendation.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum CoverageIntelligenceConfidence {
    /// All contributing surfaces agree.
    High,
    /// Evidence is consistent but incomplete.
    Medium,
    /// Evidence is sparse or partially conflicting.
    Low,
}

impl CoverageIntelligenceConfidence {
    /// Snake-case wire value of the confidence level.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::High => "high",
            Self::Medium => "medium",
            Self::Low => "low",
        }
    }
}

impl fmt::Display for CoverageIntelligenceConfidence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Confidence tier for the cross-surface evidence match.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum CoverageIntelligenceMatchConfidence {
    /// Surfaces matched on path, function identity, and line.
    PathFunctionLine,
    /// Surfaces matched on path and line only.
    PathLine,
    /// Single-surface evidence; no cross-surface join was needed.
    #[default]
    Direct,
}

impl CoverageIntelligenceMatchConfidence {
    /// Kebab-case wire value of the match confidence.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PathFunctionLine => "path-function-line",
            Self::PathLine => "path-line",
            Self::Direct => "direct",
        }
    }
}

impl fmt::Display for CoverageIntelligenceMatchConfidence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Machine-actionable next step for a coverage-intelligence finding.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CoverageIntelligenceAction {
    /// Action identifier, normalized to `type` in JSON output.
    #[serde(rename = "type")]
    pub kind: String,
    /// Human-readable action description.
    pub description: String,
    /// Whether fallow can apply this action automatically.
    pub auto_fixable: bool,
}

/// Compact evidence values that led to a recommendation.
#[derive(Debug, Clone, Default, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CoverageIntelligenceEvidence {
    /// Test coverage percentage (0-100), when coverage data exists.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coverage_pct: Option<f64>,
    /// CRAP score, when complexity and coverage both exist.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crap: Option<f64>,
    /// Runtime-coverage verdict label, e.g. `hot` or `cold`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_verdict: Option<String>,
    /// Observed runtime invocation count.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invocations: Option<u64>,
    /// Static usage status label, e.g. `unused`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub static_status: Option<String>,
    /// Static test-coverage status label, e.g. `no-test-path`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub test_coverage: Option<String>,
    /// True when the unit is inside the current change scope; omitted when
    /// false.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    #[cfg_attr(feature = "schema", schemars(default))]
    pub changed: bool,
    /// Ownership-drift state label, when ownership analysis ran.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ownership_state: Option<String>,
    /// Confidence tier of the cross-surface evidence join.
    pub match_confidence: CoverageIntelligenceMatchConfidence,
}

/// One combined coverage-intelligence finding.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CoverageIntelligenceFinding {
    /// Stable finding ID of the form `fallow:coverage-intel:<hash>`.
    pub id: String,
    /// File path relative to the project root.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// Function or export identity when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity: Option<String>,
    /// 1-indexed source line.
    pub line: u32,
    /// Verdict for this specific unit.
    pub verdict: CoverageIntelligenceVerdict,
    /// Ordered evidence signals behind the verdict.
    pub signals: Vec<CoverageIntelligenceSignal>,
    /// Recommended action family.
    pub recommendation: CoverageIntelligenceRecommendation,
    /// Confidence in the joined evidence.
    pub confidence: CoverageIntelligenceConfidence,
    /// IDs of related findings from other fallow surfaces.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[cfg_attr(feature = "schema", schemars(default))]
    pub related_ids: Vec<String>,
    /// Compact evidence values behind the recommendation.
    pub evidence: CoverageIntelligenceEvidence,
    /// Machine-actionable follow-up actions.
    pub actions: Vec<CoverageIntelligenceAction>,
}

/// Aggregate metadata for coverage-intelligence output.
#[derive(Debug, Clone, Default, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CoverageIntelligenceSummary {
    /// Total combined findings.
    pub findings: usize,
    /// Findings with the risky-change verdict.
    pub risky_changes: usize,
    /// Findings with the high-confidence-delete verdict.
    pub high_confidence_deletes: usize,
    /// Findings with the review-required verdict.
    pub review_required: usize,
    /// Findings with the refactor-carefully verdict.
    pub refactor_carefully: usize,
    /// Candidate joins dropped because the cross-surface match was ambiguous.
    pub skipped_ambiguous_matches: usize,
}

/// Combined coverage, runtime, complexity, and change-scope verdicts.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CoverageIntelligenceReport {
    /// Coverage-intelligence block contract version.
    pub schema_version: CoverageIntelligenceSchemaVersion,
    /// Headline verdict, taken from the highest-ranked finding; `clean` when
    /// there are none.
    pub verdict: CoverageIntelligenceVerdict,
    /// Aggregate finding counts.
    pub summary: CoverageIntelligenceSummary,
    /// Combined findings, one per matched unit.
    pub findings: Vec<CoverageIntelligenceFinding>,
}
