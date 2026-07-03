//! Reusable output contract types for fallow.
//!
//! This crate owns stable report DTOs that are not tied to CLI rendering.
//! Higher-level output assemblers live above this crate in `fallow-api` or the
//! CLI, while this crate remains the shared typed boundary those builders and
//! non-CLI consumers can use. Shared selectors such as `OutputFormat` stay in
//! `fallow-types`; this crate re-exports only the selectors needed to keep
//! output builders on one contract surface.
#![cfg_attr(
    test,
    allow(
        clippy::expect_used,
        reason = "tests use expect to keep serialization assertions concise"
    )
)]

mod audit_brief;
mod audit_decision_surface;
mod audit_focus;
mod audit_routing;
mod audit_walkthrough;
mod audit_weakening;
mod check;
mod ci_output;
mod codeclimate;
mod coverage_envelopes;
mod diff;
mod dupes;
mod feature_flags;
mod fix;
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
mod impact;
mod inspect_envelopes;
mod issue_contract;
mod json_paths;
mod list_envelopes;
mod next_steps;
mod pr_comment_envelope;
mod pr_comment_post_plan;
mod pr_decision;
mod pr_details;
mod pr_status;
mod pr_summary;
mod report_contract;
mod review_envelopes;
mod root_envelopes;
mod sarif;
mod security;
mod trace_envelopes;
mod walkthrough_render;

pub use audit_brief::{
    CoordinationGapFact, DiffTriage, GraphFacts, ImpactClosureFacts, PartitionFacts,
    REVIEW_BRIEF_SCHEMA_VERSION, ReviewBriefOutput, ReviewBriefSchemaVersion,
    ReviewBriefSubtractSections, ReviewDeltas, ReviewEffort, ReviewUnitFact, RiskClass,
    StandardReviewBriefOutput, build_review_brief_json_output,
    serialize_decision_surface_json_output, serialize_review_brief_json_output,
    serialize_walkthrough_guide_json_output, serialize_walkthrough_validation_json_output,
};
pub use audit_decision_surface::{
    ALL_CATEGORIES, DECISION_SURFACE_SCHEMA_VERSION, Decision, DecisionAction, DecisionActionType,
    DecisionCategory, DecisionSurface, DecisionSurfaceOutput, DecisionSurfaceSchemaVersion,
    DecisionWithActions, TruncationNote, build_decision_surface_output, decision_actions,
    suppress_comment,
};
pub use audit_focus::{ConfidenceFlag, FocusLabel, FocusMap, FocusScore, FocusUnit};
pub use audit_routing::{RoutingFacts, RoutingUnit};
pub use audit_walkthrough::{
    AcceptedJudgment, AgentJudgment, AgentSchema, AgentWalkthrough, ChangeAnchor, DirectionUnit,
    INJECTION_NOTE, RejectedJudgment, ReviewDirection, StandardWalkthroughGuide, WalkthroughGuide,
    WalkthroughValidation, agent_schema,
};
pub use audit_weakening::{WeakeningKind, WeakeningSignal};
pub use check::{
    CHECK_SCHEMA_VERSION, CheckGroupedEntry, CheckGroupedOutput, CheckOutput, CheckOutputInput,
    GroupByMode, apply_config_fixable_to_duplicate_exports, build_check_output,
    build_check_summary, harmonize_dead_code_health_suppress_line_actions,
    harmonize_multi_kind_suppress_line_actions, serialize_check_grouped_json_output,
    serialize_check_json_output,
};
pub use ci_output::{
    CiIssue, CiProvider, GroupedReviewIssues, MARKER_PREFIX_V2, MARKER_SUFFIX_V2,
    MAX_COMMENT_BODY_BYTES, PROJECT_LEVEL_RULE_IDS, PrCommentRenderInput, ReviewCommentRenderInput,
    ReviewEnvelopeRenderInput, ReviewEnvelopeRenderResult, ReviewEnvelopeTruncation,
    ReviewGitlabDiffRefs, cap_body_with_marker, command_title, composite_fingerprint, escape_md,
    github_check_conclusion, group_review_issues_by_path_line, is_project_level_rule,
    issues_from_codeclimate, issues_from_codeclimate_issues, render_pr_comment,
    render_review_comment_for_group, render_review_envelope, review_label_from_codeclimate,
    summary_fingerprint, summary_label,
};
pub use codeclimate::{
    CodeClimateAnnotationField, CodeClimateIssue, CodeClimateIssueInput, CodeClimateIssueKind,
    CodeClimateLines, CodeClimateLocation, CodeClimateOutput, CodeClimateSeverity,
    annotate_codeclimate_issues, build_codeclimate_issue, codeclimate_fingerprint_hash,
    codeclimate_issues_to_value,
};
pub use coverage_envelopes::{
    CoverageAnalyzeOutput, CoverageAnalyzeSchemaVersion, CoverageSetupFileToEdit,
    CoverageSetupFramework, CoverageSetupMember, CoverageSetupOutput, CoverageSetupPackageManager,
    CoverageSetupRuntimeTarget, CoverageSetupSchemaVersion, CoverageSetupSnippet,
    build_coverage_analyze_output, serialize_coverage_analyze_json_output,
    serialize_coverage_setup_json_output,
};
pub use diff::{
    DiffIndex, MAX_ADDED_LINES, MAX_DIFF_BYTES, parse_new_hunk_start, relative_to_diff_path,
};
pub use dupes::{
    CloneFamilyAction, CloneFamilyActionType, CloneGroupAction, CloneGroupActionType,
    DUPES_SUPPRESS_COMMENT, DUPES_SUPPRESS_DESCRIPTION, DupesOutput, DupesOutputInput,
    build_dupes_output, clone_family_actions, clone_group_actions, serialize_dupes_json_output,
};
pub use fallow_types::envelope;
pub use fallow_types::output;
pub use fallow_types::output_dead_code;
pub use fallow_types::output_format::OutputFormat;
pub use fallow_types::output_health;
pub use feature_flags::{
    FeatureFlagAction, FeatureFlagActionType, FeatureFlagConfidence, FeatureFlagDeadCodeOverlap,
    FeatureFlagFinding, FeatureFlagKind, FeatureFlagsConfidenceMeta, FeatureFlagsKindMeta,
    FeatureFlagsMeta, FeatureFlagsMetaDetails, FeatureFlagsOutput, FeatureFlagsOutputInput,
    build_feature_flags_output, feature_flags_meta, serialize_feature_flags_json_output,
};
pub use fix::{
    FixJsonOutput, FixJsonOutputInput, build_fix_json_output, count_applied_fixes,
    count_reported_fix_skips, serialize_fix_json_output,
};
pub use health::{
    HealthJsonOutputInput, HealthOutput, HealthOutputInput, build_health_output,
    serialize_health_json_output,
};
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
    ConsumerKind, CssAnalyticsReport, CssAnalyticsSummary, CssBlockOccurrence, CssCandidateAction,
    CssCandidateActionType, CssDuplicateBlock, CssFileAnalytics, CssNotationConsistency,
    CssNotationCount, CvaDuplicateVariantBlock, CvaVariantTokenDrift, NearDuplicateThemeToken,
    NearestStylingToken, RawStyleValue, ScopedUnusedClasses, StylingAgentDisposition,
    StylingFinding, StylingFindingConfidence, StylingFindingSeverity, TOKEN_CONSUMER_SAMPLE_CAP,
    TailwindArbitraryValue, TokenConsumerLocation, TokenConsumers, UndefinedKeyframes,
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
    RUNTIME_STALE_AFTER_DAYS, RuntimeCoverageAction, RuntimeCoverageBlastRadiusEntry,
    RuntimeCoverageCaptureQuality, RuntimeCoverageConfidence, RuntimeCoverageDataSource,
    RuntimeCoverageDiscriminators, RuntimeCoverageEvidence, RuntimeCoverageFinding,
    RuntimeCoverageHotPath, RuntimeCoverageImportanceEntry, RuntimeCoverageMessage,
    RuntimeCoverageProvenance, RuntimeCoverageReport, RuntimeCoverageReportVerdict,
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
    OwnershipMetrics, OwnershipState, ReactHookProfile, STYLING_HEALTH_FORMULA_VERSION,
    StylingHealth, StylingHealthConfidence, StylingHealthPenalties, ThresholdOverrideMetrics,
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
pub use impact::{
    ContainmentEvent, CrossRepoImpactReport, CrossRepoImpactSchemaVersion, CrossRepoProjectEntry,
    CrossRepoTotals, EnabledSource, ImpactCounts, ImpactReport, ImpactReportSchemaVersion,
    ImpactTrendDirection, ResolutionEvent, TrendSummary, serialize_cross_repo_impact_json_output,
    serialize_impact_json_output,
};
pub use inspect_envelopes::{
    ExplainOutput, InspectEvidence, InspectEvidenceScope, InspectEvidenceSection,
    InspectFileIdentity, InspectIdentity, InspectOutput, InspectSectionStatus,
    InspectSymbolIdentity, InspectTargetDescriptor, serialize_explain_json_output,
    serialize_inspect_json_output,
};
pub use issue_contract::{
    ACTIONS_AUTO_FIXABLE_FIELD_DEFINITION, ACTIONS_FIELD_DEFINITION, CHECK_DOCS,
    CODECLIMATE_RESULT_CODES, IssueOutputContract, TsAliasMeta, check_meta, dead_code_docs_url,
    issue_output_contract_by_code, issue_output_contracts, rule_docs_url,
};
pub use json_paths::{normalize_uri, strip_root_prefix};
pub use list_envelopes::{
    BoundariesListLogicalGroup, BoundariesListRule, BoundariesListZone, BoundariesListing,
    ListBoundariesOutput, ListEntryPointOutput, ListOutput, ListPluginOutput, WorkspaceInfo,
    WorkspacesOutput, serialize_list_boundaries_json_output, serialize_list_workspaces_json_output,
};
pub use next_steps::{
    AuditNextStepsInput, CombinedNextStepsInput, DeadCodeNextStepsInput, DupesNextStepsInput,
    HealthNextStepsInput, ImpactDigestCounts, TraceUnusedExportInput, build_audit_next_steps,
    build_audit_next_steps_input, build_combined_next_steps, build_dead_code_next_steps,
    build_dupes_next_steps, build_health_next_steps, build_health_next_steps_input,
    impact_digest_summary, trace_unused_export_input,
};
pub use pr_comment_envelope::{PrCommentEnvelope, PrCommentTruncation};
pub use pr_comment_post_plan::{
    ExistingPrComment, PrCommentPostAction, PrCommentPostPlan, PrCommentPostPlanInput,
    PrCommentPostSkipReason, plan_pr_comment_post,
};
pub use pr_decision::{
    PR_DECISION_SCHEMA, PrDecisionAnnotation, PrDecisionAnnotationLevel, PrDecisionConclusion,
    PrDecisionDetails, PrDecisionGate, PrDecisionSurface,
};
pub use pr_details::{PR_DETAILS_SCHEMA, PrDetailsArtifact, PrDetailsRow, PrDetailsSection};
pub use pr_status::{
    PrStatusContext, PrStatusMode, pr_status_contexts, pr_status_contexts_with_mode,
};
pub use pr_summary::{
    PrCommentLayout, PrSummaryArea, PrSummaryFinding, PrSummaryInput, PrSummaryScope,
    PrSummaryStatus, render_pr_summary,
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
    serialize_review_envelope_json_output, serialize_review_reconcile_json_output,
};
pub use root_envelopes::{
    AuditCommand, AuditOutput, CombinedMeta, CombinedOutput, FallowOutput, RootEnvelopeMode,
    apply_root_kind, attach_telemetry_meta, serialize_audit_json_output,
    serialize_combined_json_output, serialize_json_root_output, serialize_named_json_output,
};
pub use sarif::{
    GHAS_SARIF_FINGERPRINT_KEY, SARIF_FINGERPRINT_KEY, SarifDocumentInput, SarifResultInput,
    SarifRuleInput, build_sarif_document, build_sarif_result, build_sarif_rule,
    normalize_sarif_snippet, sarif_finding_fingerprint,
};
pub use security::{
    SecurityBlindSpotFile, SecurityBlindSpotGroup, SecurityBlindSpotsOutput,
    SecurityBlindSpotsSchemaVersion, SecurityBlindSpotsSummary, SecurityGate, SecurityGateVerdict,
    SecurityOutput, SecurityOutputConfig, SecurityOutputRulesConfig, SecurityReachabilityCounts,
    SecurityRuleSeverityConfig, SecurityRuntimeStateCounts, SecuritySchemaVersion,
    SecuritySeverityCounts, SecuritySummary, SecuritySummaryOutput, SecuritySurvivor,
    SecuritySurvivorsOutput, SecuritySurvivorsSchemaVersion, SecuritySurvivorsSummary,
    SecurityUnresolvedCalleeDiagnostics, SecurityUnresolvedCalleeReasonCount,
    SecurityUnresolvedCalleeSample, SecurityUnresolvedCalleeTopFile, SecurityVerifierVerdict,
    SecurityVerifierVerdictStatus, build_security_summary,
    serialize_security_blind_spots_json_output, serialize_security_json_output,
    serialize_security_summary_json_output, serialize_security_survivors_json_output,
};
pub use trace_envelopes::serialize_trace_json_output;
pub use walkthrough_render::{
    MAX_CONTRACT_MEMBERS, WalkthroughAccounting, cap_names, clean_decision_fact,
    visible_stage_units,
};
