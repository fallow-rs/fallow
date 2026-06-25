//! Reusable output contract types for fallow.
//!
//! This crate owns stable report DTOs and output-format metadata that are not
//! tied to CLI rendering. Human, SARIF, markdown, CodeClimate, and JSON
//! builders still live in `fallow-cli`; this crate is the typed boundary those
//! builders and non-CLI consumers can share.
#![cfg_attr(
    test,
    allow(
        clippy::expect_used,
        reason = "tests use expect to keep serialization assertions concise"
    )
)]

mod check;
mod codeclimate;
mod coverage_envelopes;
mod dupes;
mod format;
mod health;
mod health_actions;
mod health_coverage;
mod health_coverage_gaps;
mod health_coverage_intelligence;
mod health_css;
mod health_diagnostics;
mod health_findings;
mod health_grouped;
mod health_report;
mod health_runtime_coverage;
mod health_scores;
mod health_targets;
mod health_trends;
mod health_vital_signs;
mod inspect_envelopes;
mod issue_contract;
mod report_contract;
mod review_envelopes;

pub use check::{
    CHECK_SCHEMA_VERSION, CheckGroupedEntry, CheckGroupedOutput, CheckOutput, CheckOutputInput,
    GroupByMode, WorkspaceDiagnosticOutput, apply_config_fixable_to_duplicate_exports,
    build_check_output, build_check_summary,
};
pub use codeclimate::{
    CodeClimateIssue, CodeClimateIssueKind, CodeClimateLines, CodeClimateLocation,
    CodeClimateOutput, CodeClimateSeverity,
};
pub use coverage_envelopes::{
    CoverageAnalyzeOutput, CoverageAnalyzeSchemaVersion, CoverageSetupFileToEdit,
    CoverageSetupFramework, CoverageSetupMember, CoverageSetupOutput, CoverageSetupPackageManager,
    CoverageSetupRuntimeTarget, CoverageSetupSchemaVersion, CoverageSetupSnippet,
};
pub use dupes::{
    CloneFamilyAction, CloneFamilyActionType, CloneGroupAction, CloneGroupActionType,
    DUPES_SUPPRESS_COMMENT, DUPES_SUPPRESS_DESCRIPTION, DupesOutput, DupesOutputInput,
    build_dupes_output, clone_family_actions, clone_group_actions,
};
pub use fallow_types::envelope;
pub use fallow_types::output;
pub use fallow_types::output_dead_code;
pub use fallow_types::output_health;
pub use format::OutputFormat;
pub use health::{HealthOutput, HealthOutputInput, build_health_output};
pub use health_actions::HealthActionsMeta;
pub use health_coverage::CoverageModel;
pub use health_coverage_gaps::{
    CoverageGapSummary, CoverageGaps, UntestedExport, UntestedExportFinding, UntestedFile,
    UntestedFileFinding,
};
pub use health_coverage_intelligence::{
    CoverageIntelligenceAction, CoverageIntelligenceConfidence, CoverageIntelligenceEvidence,
    CoverageIntelligenceFinding, CoverageIntelligenceMatchConfidence,
    CoverageIntelligenceRecommendation, CoverageIntelligenceReport,
    CoverageIntelligenceSchemaVersion, CoverageIntelligenceSignal, CoverageIntelligenceSummary,
    CoverageIntelligenceVerdict,
};
pub use health_css::{
    CssAnalyticsReport, CssAnalyticsSummary, CssBlockOccurrence, CssCandidateAction,
    CssCandidateActionType, CssDuplicateBlock, CssFileAnalytics, CssNotationConsistency,
    CssNotationCount, ScopedUnusedClasses, TailwindArbitraryValue, UndefinedKeyframes,
    UnreferencedCssClass, UnreferencedKeyframes, UnresolvedClassReference, UnusedAtRule,
    UnusedAtRuleKind, UnusedFontFace, UnusedThemeToken,
};
pub use health_diagnostics::{
    FrameworkHealthDetector, FrameworkHealthDetectorStatus, FrameworkHealthDiagnostics,
    HealthTimings,
};
pub use health_findings::{
    HealthActionContext, HealthActionOptions, HealthFinding, HotspotFinding,
    RefactoringTargetFinding, build_health_finding_actions,
};
pub use health_grouped::{HealthGroup, HealthGrouping};
pub use health_report::HealthReport;
pub use health_runtime_coverage::{
    RuntimeCoverageAction, RuntimeCoverageBlastRadiusEntry, RuntimeCoverageCaptureQuality,
    RuntimeCoverageConfidence, RuntimeCoverageDataSource, RuntimeCoverageEvidence,
    RuntimeCoverageFinding, RuntimeCoverageHotPath, RuntimeCoverageImportanceEntry,
    RuntimeCoverageMessage, RuntimeCoverageReport, RuntimeCoverageReportVerdict,
    RuntimeCoverageRiskBand, RuntimeCoverageSchemaVersion, RuntimeCoverageSignal,
    RuntimeCoverageSummary, RuntimeCoverageVerdict, RuntimeCoverageWatermark,
};
pub use health_scores::{
    COGNITIVE_EXTRACTION_THRESHOLD, ComplexityViolation, ComponentRollup, ContributorEntry,
    ContributorIdentifierFormat, CoverageSource, CoverageSourceConsistency, CoverageTier,
    DEFAULT_COGNITIVE_CRITICAL, DEFAULT_COGNITIVE_HIGH, DEFAULT_CRAP_CRITICAL, DEFAULT_CRAP_HIGH,
    DEFAULT_CYCLOMATIC_CRITICAL, DEFAULT_CYCLOMATIC_HIGH, ExceededThreshold, FileHealthScore,
    FindingSeverity, HEALTH_SCORE_FORMULA_VERSION, HOTSPOT_SCORE_THRESHOLD,
    HealthConfiguredThresholds, HealthEffectiveThresholds, HealthScore, HealthScorePenalties,
    HealthSummary, HotspotEntry, HotspotSummary, LargeFunctionEntry, MI_DENSITY_MIN_LINES,
    OwnershipMetrics, OwnershipState, ReactHookProfile, ThresholdOverrideMetrics,
    ThresholdOverrideState, ThresholdOverrideStatus, ThresholdSource, compute_finding_severity,
    letter_grade, summarize_coverage_source_consistency,
};
pub use health_targets::{
    CloneSiblingEvidence, Confidence, ContributingFactor, DirectCallerEvidence,
    DirectCallerSymbolEvidence, EffortEstimate, EvidenceFunction, RecommendationCategory,
    RefactoringTarget, TargetEvidence, TargetThresholds,
};
pub use health_trends::{HealthTrend, TrendCount, TrendDirection, TrendMetric, TrendPoint};
pub use health_vital_signs::{
    RenderFanInTopComponent, RiskProfile, SNAPSHOT_SCHEMA_VERSION, VitalSigns, VitalSignsCounts,
    VitalSignsSnapshot,
};
pub use inspect_envelopes::{
    ExplainOutput, InspectEvidence, InspectEvidenceScope, InspectEvidenceSection,
    InspectFileIdentity, InspectIdentity, InspectOutput, InspectSectionStatus,
    InspectSymbolIdentity, InspectTargetDescriptor,
};
pub use issue_contract::{
    ACTIONS_AUTO_FIXABLE_FIELD_DEFINITION, ACTIONS_FIELD_DEFINITION, CHECK_DOCS,
    CODECLIMATE_RESULT_CODES, IssueOutputContract, TsAliasMeta, check_meta, dead_code_docs_url,
    issue_output_contract_by_code, issue_output_contracts, rule_docs_url,
};
pub use report_contract::{
    COVERAGE_ANALYZE_DOCS, COVERAGE_SETUP_DOCS, DUPES_DOCS, HEALTH_DOCS, SECURITY_DOCS,
    SecurityRuleMeta, coverage_analyze_meta, coverage_setup_meta, dupes_meta, health_meta,
    security_meta,
};
pub use review_envelopes::{
    GitHubReviewComment, GitHubReviewSide, GitLabReviewComment, GitLabReviewPosition,
    GitLabReviewPositionType, MARKER_REGEX_FLAGS_V2, MARKER_REGEX_V2, ReviewCheckConclusion,
    ReviewComment, ReviewEnvelopeEvent, ReviewEnvelopeMeta, ReviewEnvelopeOutput,
    ReviewEnvelopeSchema, ReviewEnvelopeSummary, ReviewProvider, ReviewReconcileOutput,
    ReviewReconcileSchema, default_marker_regex, default_marker_regex_flags, is_false,
};
