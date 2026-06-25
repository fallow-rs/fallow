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
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum CoverageIntelligenceVerdict {
    RiskyChangeDetected,
    HighConfidenceDelete,
    ReviewRequired,
    RefactorCarefully,
    Clean,
    #[default]
    Unknown,
}

impl CoverageIntelligenceVerdict {
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum CoverageIntelligenceSignal {
    Changed,
    HotPath,
    LowTestCoverage,
    HighCrap,
    StaticUnused,
    RuntimeCold,
    NoTestPath,
    RuntimeReachable,
    OwnershipDrift,
    TestCovered,
}

impl CoverageIntelligenceSignal {
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum CoverageIntelligenceRecommendation {
    AddTestOrSplitBeforeMerge,
    DeleteAfterConfirmingOwner,
    ReviewBeforeChanging,
    RefactorCarefullyKeepBehavior,
}

impl CoverageIntelligenceRecommendation {
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum CoverageIntelligenceConfidence {
    High,
    Medium,
    Low,
}

impl CoverageIntelligenceConfidence {
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
    PathFunctionLine,
    PathLine,
    #[default]
    Direct,
}

impl CoverageIntelligenceMatchConfidence {
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
    pub description: String,
    /// Whether fallow can apply this action automatically.
    pub auto_fixable: bool,
}

/// Compact evidence values that led to a recommendation.
#[derive(Debug, Clone, Default, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CoverageIntelligenceEvidence {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coverage_pct: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crap: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_verdict: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invocations: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub static_status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub test_coverage: Option<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    #[cfg_attr(feature = "schema", schemars(default))]
    pub changed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ownership_state: Option<String>,
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
    pub verdict: CoverageIntelligenceVerdict,
    pub signals: Vec<CoverageIntelligenceSignal>,
    pub recommendation: CoverageIntelligenceRecommendation,
    pub confidence: CoverageIntelligenceConfidence,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[cfg_attr(feature = "schema", schemars(default))]
    pub related_ids: Vec<String>,
    pub evidence: CoverageIntelligenceEvidence,
    pub actions: Vec<CoverageIntelligenceAction>,
}

/// Aggregate metadata for coverage-intelligence output.
#[derive(Debug, Clone, Default, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CoverageIntelligenceSummary {
    pub findings: usize,
    pub risky_changes: usize,
    pub high_confidence_deletes: usize,
    pub review_required: usize,
    pub refactor_carefully: usize,
    pub skipped_ambiguous_matches: usize,
}

/// Combined coverage, runtime, complexity, and change-scope verdicts.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CoverageIntelligenceReport {
    pub schema_version: CoverageIntelligenceSchemaVersion,
    pub verdict: CoverageIntelligenceVerdict,
    pub summary: CoverageIntelligenceSummary,
    pub findings: Vec<CoverageIntelligenceFinding>,
}
