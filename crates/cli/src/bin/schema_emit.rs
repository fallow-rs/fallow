#![expect(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "schema-emit binary prints the regenerated schema to stdout and errors to stderr"
)]
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        reason = "schema drift tests use unwrap and expect to keep invariant checks concise"
    )
)]

//! Regenerate `docs/output-schema.json` from the Rust source of truth.
//!
//! When the `schema-emit` feature is enabled, this binary derives the Rust-owned
//! definitions, merges them with the committed schema, and prints draft-07 JSON
//! Schema to stdout.

#[cfg(not(test))]
use std::path::PathBuf;
use std::process::ExitCode;

use schemars::generate::SchemaSettings;
use serde_json::{Map, Value};

use fallow_cli::health_types::{
    ComplexityViolation, ContributorEntry, ContributorIdentifierFormat, CoverageGapSummary,
    CoverageGaps, CoverageModel, CoverageTier, ExceededThreshold, FileHealthScore, FindingSeverity,
    HealthActionsMeta, HealthScore, HealthScorePenalties, HealthSummary, HealthTrend, HotspotEntry,
    HotspotFinding, HotspotSummary, LargeFunctionEntry, OwnershipMetrics, RecommendationCategory,
    RefactoringTarget, RefactoringTargetFinding, RiskProfile, RuntimeCoverageReport,
    TargetThresholds, TrendCount, UntestedExport, UntestedExportFinding, UntestedFile,
    UntestedFileFinding, VitalSigns, VitalSignsCounts,
};
use fallow_cli::impact::{
    ContainmentEvent, CrossRepoImpactReport, CrossRepoImpactSchemaVersion, CrossRepoProjectEntry,
    CrossRepoTotals, EnabledSource, ImpactCounts, ImpactReport, ImpactReportSchemaVersion,
    ImpactTrendDirection, ResolutionEvent, TrendSummary,
};
use fallow_cli::output_dupes::{
    AttributedCloneGroupFinding, CloneFamilyAction, CloneFamilyActionType, CloneFamilyFinding,
    CloneGroupAction, CloneGroupActionType, CloneGroupFinding, DupesReportPayload,
};
use fallow_cli::output_envelope::{
    AuditCommand, AuditOutput, BoundariesListLogicalGroup, BoundariesListRule, BoundariesListZone,
    BoundariesListing, CheckGroupedEntry, CheckGroupedOutput, CheckOutput, CodeClimateIssue,
    CodeClimateIssueKind, CodeClimateLines, CodeClimateLocation, CodeClimateOutput,
    CodeClimateSeverity, CombinedOutput, CoverageAnalyzeOutput, CoverageAnalyzeSchemaVersion,
    CoverageSetupFileToEdit, CoverageSetupFramework, CoverageSetupMember, CoverageSetupOutput,
    CoverageSetupPackageManager, CoverageSetupRuntimeTarget, CoverageSetupSchemaVersion,
    CoverageSetupSnippet, DupesOutput, ExplainOutput, FallowOutput, GitHubReviewComment,
    GitHubReviewSide, GitLabReviewComment, GitLabReviewPosition, GitLabReviewPositionType,
    GroupByMode, HealthOutput, InspectEvidence, InspectEvidenceScope, InspectEvidenceSection,
    InspectFileIdentity, InspectIdentity, InspectOutput, InspectSectionStatus,
    InspectSymbolIdentity, InspectTargetDescriptor, ListBoundariesOutput, ReviewCheckConclusion,
    ReviewComment, ReviewEnvelopeEvent, ReviewEnvelopeMeta, ReviewEnvelopeOutput,
    ReviewEnvelopeSchema, ReviewEnvelopeSummary, ReviewProvider, ReviewReconcileOutput,
    ReviewReconcileSchema, WorkspaceInfo, WorkspacesOutput,
};
use fallow_cli::report::dupes_grouping::{
    AttributedCloneGroup, AttributedInstance, DuplicationGroup,
};
use fallow_cli::security::{
    SecurityBlindSpotFile, SecurityBlindSpotGroup, SecurityBlindSpotsOutput,
    SecurityBlindSpotsSchemaVersion, SecurityBlindSpotsSummary, SecurityGate, SecurityGateMode,
    SecurityGateVerdict, SecurityOutput, SecurityReachabilityCounts, SecurityRuntimeStateCounts,
    SecuritySchemaVersion, SecuritySeverityCounts, SecuritySummary, SecuritySummaryOutput,
    SecuritySurvivor, SecuritySurvivorsOutput, SecuritySurvivorsSchemaVersion,
    SecuritySurvivorsSummary, SecurityUnresolvedCalleeDiagnostics,
    SecurityUnresolvedCalleeReasonCount, SecurityUnresolvedCalleeSample,
    SecurityUnresolvedCalleeTopFile, SecurityVerifierVerdict, SecurityVerifierVerdictStatus,
};
use fallow_config::{AuthoredRule, LogicalGroup, LogicalGroupStatus};
use fallow_core::duplicates::{
    CloneFamily, CloneGroup, CloneInstance, DuplicationReport, DuplicationStats, MirroredDirectory,
    RefactoringKind, RefactoringSuggestion,
};
use fallow_types::envelope::{
    AuditIntroduced, BaselineCategoryDelta, BaselineDeltas, BaselineMatch, CheckSummary, ElapsedMs,
    EntryPoints, Meta, MetaMetric, MetaRule, RegressionResult, RegressionStatus,
    RegressionToleranceKind, SchemaVersion, TelemetryMeta, ToolVersion,
};
use fallow_types::extract::{
    MemberKind, SecurityControlKind, SkippedSecurityCalleeExpressionKind,
    SkippedSecurityCalleeReason,
};
use fallow_types::output::{
    AddToConfigAction, AddToConfigKind, AddToConfigValue, FixAction, FixActionType,
    IgnoreExportsRule, IssueAction, NextStep, SuppressFileAction, SuppressFileKind,
    SuppressLineAction, SuppressLineKind, SuppressLineScope,
};
use fallow_types::output_dead_code::{
    BoundaryViolationFinding, CircularDependencyFinding, PrivateTypeLeakFinding,
    TestOnlyDependencyFinding, TypeOnlyDependencyFinding, UnlistedDependencyFinding,
    UnresolvedImportFinding, UnusedClassMemberFinding, UnusedDependencyFinding,
    UnusedDevDependencyFinding, UnusedEnumMemberFinding, UnusedExportFinding, UnusedFileFinding,
    UnusedOptionalDependencyFinding, UnusedTypeFinding,
};
use fallow_types::output_health::{
    HealthFindingAction, HealthFindingActionType, HotspotAction, HotspotActionHeuristic,
    HotspotActionType, RefactoringTargetAction, RefactoringTargetActionType, UntestedExportAction,
    UntestedExportActionType, UntestedFileAction, UntestedFileActionType,
};
use fallow_types::results::{
    AnalysisResults, BoundaryViolation, CircularDependency, DependencyLocation,
    DependencyOverrideMisconfigReason, DependencyOverrideSource, DuplicateExport,
    DuplicateLocation, EmptyCatalogGroup, EntryPointSummary, ExportUsage, FeatureFlag,
    FlagConfidence, FlagKind, ImportSite, MisconfiguredDependencyOverride, PrivateTypeLeak,
    ReferenceLocation, SecurityAttackSurfaceEntry, SecurityCandidate, SecurityCandidateBoundary,
    SecurityCandidateSink, SecurityDeadCodeContext, SecurityDeadCodeKind,
    SecurityDefensiveBoundary, SecurityDefensiveControl, SecurityFinding, SecurityFindingKind,
    SecurityNetworkContext, SecurityReachability, SecurityRuntimeContext, SecurityRuntimeState,
    SecuritySeverity, SecurityTaintFlow, SecurityZoneCrossing, StaleSuppression, SuppressionOrigin,
    TaintEndpoint, TaintPath, TestOnlyDependency, TraceHop, TraceHopRole, TypeOnlyDependency,
    UnlistedDependency, UnresolvedCatalogReference, UnresolvedImport, UnusedCatalogEntry,
    UnusedDependency, UnusedDependencyOverride, UnusedExport, UnusedFile, UnusedMember,
};

/// Workspace-relative path to the committed schema.
#[cfg(not(test))]
const COMMITTED_SCHEMA_REL_PATH: &str = "docs/output-schema.json";

/// Embedded copy used by tests.
#[cfg(test)]
const COMMITTED_SCHEMA: &str = include_str!("../../../../docs/output-schema.json");

/// Locate `docs/output-schema.json` by walking up from `CARGO_MANIFEST_DIR`.
#[cfg(not(test))]
fn read_committed_schema() -> Result<String, String> {
    let start = std::env::var_os("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok())
        .ok_or_else(|| "unable to determine starting directory".to_string())?;
    for dir in start.ancestors() {
        let candidate = dir.join(COMMITTED_SCHEMA_REL_PATH);
        if candidate.is_file() {
            return std::fs::read_to_string(&candidate)
                .map_err(|err| format!("failed to read {}: {err}", candidate.display()));
        }
    }
    Err(format!(
        "could not find {COMMITTED_SCHEMA_REL_PATH} by walking up from {}; run the binary from the workspace root",
        start.display()
    ))
}

/// Test-only helper that uses the embedded schema.
#[cfg(test)]
#[expect(
    clippy::unnecessary_wraps,
    reason = "signature must match the non-test variant's `Result<String, String>` return"
)]
fn committed_schema_source() -> Result<String, String> {
    Ok(COMMITTED_SCHEMA.to_string())
}

#[cfg(not(test))]
fn committed_schema_source() -> Result<String, String> {
    read_committed_schema()
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("fallow-schema-emit: {err}");
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<(), String> {
    let derived = derived_definitions();
    let mut merged = merge_with_committed(&derived)?;
    normalize_output_punctuation(&mut merged);
    let pretty = serde_json::to_string_pretty(&merged)
        .map_err(|err| format!("failed to serialize merged schema: {err}"))?;
    println!("{pretty}");
    Ok(())
}

fn normalize_output_punctuation(value: &mut Value) {
    match value {
        Value::String(text) => {
            *text = text.replace('\u{2014}', ",");
        }
        Value::Array(items) => {
            for item in items {
                normalize_output_punctuation(item);
            }
        }
        Value::Object(map) => {
            for child in map.values_mut() {
                normalize_output_punctuation(child);
            }
        }
        _ => {}
    }
}

/// Definitions owned by this binary; everything else is copied from the committed schema.
const DERIVED_DEFINITION_NAMES: &[&str] = &[
    "BoundaryViolation",
    "CircularDependency",
    "DuplicateExport",
    "DuplicateLocation",
    "EmptyCatalogGroup",
    "ImportSite",
    "MisconfiguredDependencyOverride",
    "PrivateTypeLeak",
    "StaleSuppression",
    "TestOnlyDependency",
    "TypeOnlyDependency",
    "UnlistedDependency",
    "UnresolvedCatalogReference",
    "UnresolvedImport",
    "UnusedCatalogEntry",
    "UnusedDependency",
    "UnusedDependencyOverride",
    "UnusedExport",
    "UnusedFile",
    "UnusedMember",
    "CloneFamily",
    "CloneGroup",
    "CloneInstance",
    "MirroredDirectory",
    "AddToConfigAction",
    "FixAction",
    "IssueAction",
    "NextStep",
    "SuppressFileAction",
    "SuppressLineAction",
    "ComplexityViolation",
    "ContributorEntry",
    "CoverageGapSummary",
    "CoverageGaps",
    "FileHealthScore",
    "HealthActionsMeta",
    "HealthFinding",
    "HealthScore",
    "HealthScorePenalties",
    "HealthSummary",
    "HealthTrend",
    "HotspotEntry",
    "HotspotFinding",
    "HotspotSummary",
    "LargeFunctionEntry",
    "OwnershipMetrics",
    "RefactoringTarget",
    "RefactoringTargetFinding",
    "RiskProfile",
    "RuntimeCoverageReport",
    "TargetThresholds",
    "TrendCount",
    "UntestedExport",
    "UntestedExportFinding",
    "UntestedFile",
    "UntestedFileFinding",
    "VitalSigns",
    "VitalSignsCounts",
    "HealthFindingAction",
    "HotspotAction",
    "RefactoringTargetAction",
    "UntestedExportAction",
    "UntestedFileAction",
    "AuditIntroduced",
    "BaselineDeltas",
    "BaselineMatch",
    "CheckSummary",
    "ElapsedMs",
    "EntryPoints",
    "Meta",
    "RegressionResult",
    "SchemaVersion",
    "ToolVersion",
    "RuntimeCoverageAction",
    "RuntimeCoverageBlastRadiusEntry",
    "RuntimeCoverageCaptureQuality",
    "RuntimeCoverageConfidence",
    "RuntimeCoverageEvidence",
    "RuntimeCoverageFinding",
    "RuntimeCoverageHotPath",
    "RuntimeCoverageImportanceEntry",
    "RuntimeCoverageMessage",
    "RuntimeCoverageReportVerdict",
    "RuntimeCoverageRiskBand",
    "RuntimeCoverageSignal",
    "RuntimeCoverageSummary",
    "RuntimeCoverageVerdict",
    "RuntimeCoverageWatermark",
    "DuplicationReport",
    "HealthReport",
    "AuditOutput",
    "CheckGroupedEntry",
    "CheckGroupedOutput",
    "CheckOutput",
    "CodeClimateIssue",
    "CodeClimateOutput",
    "CombinedOutput",
    "CoverageSetupFileToEdit",
    "CoverageSetupMember",
    "CoverageSetupOutput",
    "CoverageSetupSnippet",
    "DupesOutput",
    "ExplainOutput",
    "InspectEvidence",
    "InspectEvidenceSection",
    "InspectFileIdentity",
    "InspectIdentity",
    "InspectOutput",
    "InspectSymbolIdentity",
    "InspectTargetDescriptor",
    "GitHubReviewComment",
    "GitLabReviewComment",
    "GitLabReviewPosition",
    "HealthGroup",
    "HealthOutput",
    "ReviewEnvelopeOutput",
    "ReviewEnvelopeSummary",
    "ReviewReconcileOutput",
    "InspectEvidenceScope",
    "InspectSectionStatus",
    "FallowOutput",
    "BoundariesListLogicalGroup",
    "BoundariesListRule",
    "BoundariesListZone",
    "BoundariesListing",
    "ListBoundariesOutput",
    "WorkspaceInfo",
    "WorkspacesOutput",
    "AuthoredRule",
    "LogicalGroup",
    "LogicalGroupStatus",
    "AttributedCloneGroup",
    "AttributedInstance",
    "DuplicationGroup",
    "AttributedCloneGroupFinding",
    "CloneFamilyAction",
    "CloneFamilyActionType",
    "CloneFamilyFinding",
    "CloneGroupAction",
    "CloneGroupActionType",
    "CloneGroupFinding",
    "DupesReportPayload",
    "CoverageAnalyzeOutput",
    "CoverageAnalyzeSchemaVersion",
    "ContainmentEvent",
    "CrossRepoImpactReport",
    "CrossRepoImpactSchemaVersion",
    "CrossRepoProjectEntry",
    "CrossRepoTotals",
    "EnabledSource",
    "ImpactCounts",
    "ImpactReport",
    "ImpactReportSchemaVersion",
    "ImpactTrendDirection",
    "ResolutionEvent",
    "TrendSummary",
    "SecurityOutput",
    "SecurityUnresolvedCalleeDiagnostics",
    "SecurityUnresolvedCalleeReasonCount",
    "SecurityUnresolvedCalleeSample",
    "SecurityUnresolvedCalleeTopFile",
    "SkippedSecurityCalleeExpressionKind",
    "SkippedSecurityCalleeReason",
    "SecuritySummaryOutput",
    "SecuritySummary",
    "SecuritySeverityCounts",
    "SecurityReachabilityCounts",
    "SecurityRuntimeStateCounts",
    "SecuritySurvivorsOutput",
    "SecuritySurvivorsSchemaVersion",
    "SecuritySurvivorsSummary",
    "SecuritySurvivor",
    "SecurityVerifierVerdict",
    "SecurityVerifierVerdictStatus",
    "SecurityBlindSpotsOutput",
    "SecurityBlindSpotsSchemaVersion",
    "SecurityBlindSpotsSummary",
    "SecurityBlindSpotGroup",
    "SecurityBlindSpotFile",
    "SecuritySchemaVersion",
    "SecurityGate",
    "SecurityGateMode",
    "SecurityGateVerdict",
    "SecurityFinding",
    "SecurityFindingKind",
    "SecuritySeverity",
    "SecurityDeadCodeContext",
    "SecurityDeadCodeKind",
    "SecurityReachability",
    "SecurityCandidate",
    "SecurityCandidateSink",
    "SecurityCandidateBoundary",
    "SecurityZoneCrossing",
    "SecurityNetworkContext",
    "SecurityTaintFlow",
    "SecurityControlKind",
    "SecurityDefensiveControl",
    "SecurityDefensiveBoundary",
    "SecurityAttackSurfaceEntry",
    "TaintEndpoint",
    "TaintPath",
    "TraceHop",
    "TraceHopRole",
];

pub(crate) fn derived_definition_names() -> &'static [&'static str] {
    DERIVED_DEFINITION_NAMES
}

/// Finding definitions that get `actions` and optional `introduced` grafts.
fn finding_definition_names() -> &'static [&'static str] {
    &[]
}

/// Per-finding override for `augment_finding_definition`.
///
/// Default augmentation for dead-code findings.
#[derive(Debug, Clone, Copy)]
struct FindingAugmentation {
    /// Schema `$ref` for the items in the `actions` array.
    actions_item_ref: &'static str,
    /// Whether to attach the optional `introduced` audit breadcrumb.
    include_introduced: bool,
}

/// Augmentation applied to dead-code findings: actions ref `IssueAction`,
/// `introduced` flag attached.
const DEFAULT_FINDING_AUGMENTATION: FindingAugmentation = FindingAugmentation {
    actions_item_ref: "#/definitions/IssueAction",
    include_introduced: true,
};

/// Pick the augmentation for a specific finding.
fn finding_augmentation(_name: &str) -> FindingAugmentation {
    DEFAULT_FINDING_AUGMENTATION
}

/// Build derived schemas for every in-scope type using one shared generator.
#[allow(
    clippy::too_many_lines,
    reason = "this function is fundamentally a registration list: one `subschema_for::<T>()` call per type in the public output contract. Splitting by module obscures the registration set; the linear list is the cleanest representation."
)]
fn derived_definitions() -> Map<String, Value> {
    let mut generator = SchemaSettings::draft07().into_generator();

    register_analysis_definitions(&mut generator);
    register_duplication_definitions(&mut generator);
    register_dead_code_action_definitions(&mut generator);
    register_dead_code_finding_definitions(&mut generator);
    register_health_definitions(&mut generator);
    register_meta_definitions(&mut generator);
    register_per_command_envelope_definitions(&mut generator);
    register_impact_definitions(&mut generator);
    register_security_definitions(&mut generator);
    let _ = generator.subschema_for::<FallowOutput>();
    register_list_boundaries_definitions(&mut generator);
    register_health_action_definitions(&mut generator);

    generator.take_definitions(true)
}

fn register_analysis_definitions(generator: &mut schemars::SchemaGenerator) {
    let _ = generator.subschema_for::<AnalysisResults>();
    let _ = generator.subschema_for::<DuplicationReport>();

    let _ = generator.subschema_for::<UnusedFile>();
    let _ = generator.subschema_for::<UnusedExport>();
    let _ = generator.subschema_for::<PrivateTypeLeak>();
    let _ = generator.subschema_for::<UnusedDependency>();
    let _ = generator.subschema_for::<DependencyLocation>();
    let _ = generator.subschema_for::<UnusedMember>();
    let _ = generator.subschema_for::<UnresolvedImport>();
    let _ = generator.subschema_for::<UnlistedDependency>();
    let _ = generator.subschema_for::<ImportSite>();
    let _ = generator.subschema_for::<DuplicateExport>();
    let _ = generator.subschema_for::<DuplicateLocation>();
    let _ = generator.subschema_for::<TypeOnlyDependency>();
    let _ = generator.subschema_for::<UnusedCatalogEntry>();
    let _ = generator.subschema_for::<EmptyCatalogGroup>();
    let _ = generator.subschema_for::<UnresolvedCatalogReference>();
    let _ = generator.subschema_for::<DependencyOverrideSource>();
    let _ = generator.subschema_for::<UnusedDependencyOverride>();
    let _ = generator.subschema_for::<DependencyOverrideMisconfigReason>();
    let _ = generator.subschema_for::<MisconfiguredDependencyOverride>();
    let _ = generator.subschema_for::<TestOnlyDependency>();
    let _ = generator.subschema_for::<CircularDependency>();
    let _ = generator.subschema_for::<BoundaryViolation>();
    let _ = generator.subschema_for::<SuppressionOrigin>();
    let _ = generator.subschema_for::<StaleSuppression>();
    let _ = generator.subschema_for::<FlagKind>();
    let _ = generator.subschema_for::<FlagConfidence>();
    let _ = generator.subschema_for::<FeatureFlag>();
    let _ = generator.subschema_for::<ExportUsage>();
    let _ = generator.subschema_for::<ReferenceLocation>();
    let _ = generator.subschema_for::<EntryPointSummary>();
    let _ = generator.subschema_for::<MemberKind>();
    let _ = generator.subschema_for::<CloneInstance>();
    let _ = generator.subschema_for::<CloneGroup>();
    let _ = generator.subschema_for::<RefactoringKind>();
    let _ = generator.subschema_for::<RefactoringSuggestion>();
    let _ = generator.subschema_for::<CloneFamily>();
    let _ = generator.subschema_for::<MirroredDirectory>();
}

fn register_duplication_definitions(generator: &mut schemars::SchemaGenerator) {
    let _ = generator.subschema_for::<AttributedInstance>();
    let _ = generator.subschema_for::<AttributedCloneGroup>();
    let _ = generator.subschema_for::<DuplicationGroup>();
    let _ = generator.subschema_for::<DuplicationStats>();

    let _ = generator.subschema_for::<CloneGroupFinding>();
    let _ = generator.subschema_for::<CloneFamilyFinding>();
    let _ = generator.subschema_for::<AttributedCloneGroupFinding>();
    let _ = generator.subschema_for::<CloneGroupAction>();
    let _ = generator.subschema_for::<CloneGroupActionType>();
    let _ = generator.subschema_for::<CloneFamilyAction>();
    let _ = generator.subschema_for::<CloneFamilyActionType>();
    let _ = generator.subschema_for::<DupesReportPayload>();
}

fn register_dead_code_action_definitions(generator: &mut schemars::SchemaGenerator) {
    let _ = generator.subschema_for::<IssueAction>();
    let _ = generator.subschema_for::<FixAction>();
    let _ = generator.subschema_for::<FixActionType>();
    let _ = generator.subschema_for::<SuppressLineAction>();
    let _ = generator.subschema_for::<SuppressLineKind>();
    let _ = generator.subschema_for::<SuppressLineScope>();
    let _ = generator.subschema_for::<SuppressFileAction>();
    let _ = generator.subschema_for::<SuppressFileKind>();
    let _ = generator.subschema_for::<AddToConfigAction>();
    let _ = generator.subschema_for::<AddToConfigKind>();
    let _ = generator.subschema_for::<AddToConfigValue>();
    let _ = generator.subschema_for::<IgnoreExportsRule>();
    let _ = generator.subschema_for::<NextStep>();
}

fn register_dead_code_finding_definitions(generator: &mut schemars::SchemaGenerator) {
    let _ = generator.subschema_for::<UnusedFileFinding>();
    let _ = generator.subschema_for::<PrivateTypeLeakFinding>();
    let _ = generator.subschema_for::<UnresolvedImportFinding>();
    let _ = generator.subschema_for::<CircularDependencyFinding>();
    let _ = generator.subschema_for::<BoundaryViolationFinding>();
    let _ = generator.subschema_for::<UnusedExportFinding>();
    let _ = generator.subschema_for::<UnusedTypeFinding>();
    let _ = generator.subschema_for::<UnusedEnumMemberFinding>();
    let _ = generator.subschema_for::<UnusedClassMemberFinding>();
    let _ = generator.subschema_for::<UnusedDependencyFinding>();
    let _ = generator.subschema_for::<UnusedDevDependencyFinding>();
    let _ = generator.subschema_for::<UnusedOptionalDependencyFinding>();
    let _ = generator.subschema_for::<UnlistedDependencyFinding>();
    let _ = generator.subschema_for::<TypeOnlyDependencyFinding>();
    let _ = generator.subschema_for::<TestOnlyDependencyFinding>();
}

fn register_health_definitions(generator: &mut schemars::SchemaGenerator) {
    let _ = generator.subschema_for::<HealthSummary>();
    let _ = generator.subschema_for::<ComplexityViolation>();
    let _ = generator.subschema_for::<ExceededThreshold>();
    let _ = generator.subschema_for::<FindingSeverity>();
    let _ = generator.subschema_for::<CoverageTier>();
    let _ = generator.subschema_for::<CoverageModel>();
    let _ = generator.subschema_for::<LargeFunctionEntry>();
    let _ = generator.subschema_for::<FileHealthScore>();
    let _ = generator.subschema_for::<HotspotEntry>();
    let _ = generator.subschema_for::<HotspotFinding>();
    let _ = generator.subschema_for::<HotspotSummary>();
    let _ = generator.subschema_for::<OwnershipMetrics>();
    let _ = generator.subschema_for::<ContributorEntry>();
    let _ = generator.subschema_for::<ContributorIdentifierFormat>();
    let _ = generator.subschema_for::<RefactoringTarget>();
    let _ = generator.subschema_for::<RefactoringTargetFinding>();
    let _ = generator.subschema_for::<RecommendationCategory>();
    let _ = generator.subschema_for::<TargetThresholds>();
    let _ = generator.subschema_for::<HealthTrend>();
    let _ = generator.subschema_for::<TrendCount>();
    let _ = generator.subschema_for::<CoverageGaps>();
    let _ = generator.subschema_for::<CoverageGapSummary>();
    let _ = generator.subschema_for::<UntestedFile>();
    let _ = generator.subschema_for::<UntestedFileFinding>();
    let _ = generator.subschema_for::<UntestedExport>();
    let _ = generator.subschema_for::<UntestedExportFinding>();
    let _ = generator.subschema_for::<HealthScore>();
    let _ = generator.subschema_for::<HealthScorePenalties>();
    let _ = generator.subschema_for::<VitalSigns>();
    let _ = generator.subschema_for::<VitalSignsCounts>();
    let _ = generator.subschema_for::<RiskProfile>();
    let _ = generator.subschema_for::<RuntimeCoverageReport>();
    let _ = generator.subschema_for::<HealthActionsMeta>();
}

fn register_meta_definitions(generator: &mut schemars::SchemaGenerator) {
    let _ = generator.subschema_for::<SchemaVersion>();
    let _ = generator.subschema_for::<ToolVersion>();
    let _ = generator.subschema_for::<ElapsedMs>();
    let _ = generator.subschema_for::<AuditIntroduced>();
    let _ = generator.subschema_for::<EntryPoints>();
    let _ = generator.subschema_for::<CheckSummary>();
    let _ = generator.subschema_for::<BaselineDeltas>();
    let _ = generator.subschema_for::<BaselineCategoryDelta>();
    let _ = generator.subschema_for::<BaselineMatch>();
    let _ = generator.subschema_for::<RegressionResult>();
    let _ = generator.subschema_for::<RegressionStatus>();
    let _ = generator.subschema_for::<RegressionToleranceKind>();
    let _ = generator.subschema_for::<Meta>();
    let _ = generator.subschema_for::<MetaMetric>();
    let _ = generator.subschema_for::<MetaRule>();
    let _ = generator.subschema_for::<TelemetryMeta>();
}

fn register_impact_definitions(generator: &mut schemars::SchemaGenerator) {
    let _ = generator.subschema_for::<ImpactCounts>();
    let _ = generator.subschema_for::<EnabledSource>();
    let _ = generator.subschema_for::<ImpactReportSchemaVersion>();
    let _ = generator.subschema_for::<ImpactTrendDirection>();
    let _ = generator.subschema_for::<ResolutionEvent>();
    let _ = generator.subschema_for::<TrendSummary>();
    let _ = generator.subschema_for::<ContainmentEvent>();
    let _ = generator.subschema_for::<ImpactReport>();
    let _ = generator.subschema_for::<CrossRepoImpactSchemaVersion>();
    let _ = generator.subschema_for::<CrossRepoTotals>();
    let _ = generator.subschema_for::<CrossRepoProjectEntry>();
    let _ = generator.subschema_for::<CrossRepoImpactReport>();
}

fn register_security_definitions(generator: &mut schemars::SchemaGenerator) {
    let _ = generator.subschema_for::<SecurityFindingKind>();
    let _ = generator.subschema_for::<SecuritySeverity>();
    let _ = generator.subschema_for::<SecurityDeadCodeKind>();
    let _ = generator.subschema_for::<SecurityDeadCodeContext>();
    let _ = generator.subschema_for::<TraceHopRole>();
    let _ = generator.subschema_for::<TraceHop>();
    let _ = generator.subschema_for::<SecurityReachability>();
    let _ = generator.subschema_for::<SecurityCandidateSink>();
    let _ = generator.subschema_for::<SecurityZoneCrossing>();
    let _ = generator.subschema_for::<SecurityCandidateBoundary>();
    let _ = generator.subschema_for::<SecurityNetworkContext>();
    let _ = generator.subschema_for::<SecurityCandidate>();
    let _ = generator.subschema_for::<TaintEndpoint>();
    let _ = generator.subschema_for::<TaintPath>();
    let _ = generator.subschema_for::<SecurityTaintFlow>();
    let _ = generator.subschema_for::<SecurityRuntimeState>();
    let _ = generator.subschema_for::<SecurityRuntimeContext>();
    let _ = generator.subschema_for::<SecurityControlKind>();
    let _ = generator.subschema_for::<SecurityDefensiveControl>();
    let _ = generator.subschema_for::<SecurityDefensiveBoundary>();
    let _ = generator.subschema_for::<SecurityAttackSurfaceEntry>();
    let _ = generator.subschema_for::<SecurityFinding>();
    let _ = generator.subschema_for::<SecurityGateMode>();
    let _ = generator.subschema_for::<SecurityGateVerdict>();
    let _ = generator.subschema_for::<SecurityGate>();
    let _ = generator.subschema_for::<SecuritySchemaVersion>();
    let _ = generator.subschema_for::<SecurityOutput>();
    let _ = generator.subschema_for::<SecurityUnresolvedCalleeDiagnostics>();
    let _ = generator.subschema_for::<SecurityUnresolvedCalleeReasonCount>();
    let _ = generator.subschema_for::<SecurityUnresolvedCalleeSample>();
    let _ = generator.subschema_for::<SecurityUnresolvedCalleeTopFile>();
    let _ = generator.subschema_for::<SkippedSecurityCalleeExpressionKind>();
    let _ = generator.subschema_for::<SkippedSecurityCalleeReason>();
    let _ = generator.subschema_for::<SecuritySummaryOutput>();
    let _ = generator.subschema_for::<SecuritySummary>();
    let _ = generator.subschema_for::<SecuritySeverityCounts>();
    let _ = generator.subschema_for::<SecurityReachabilityCounts>();
    let _ = generator.subschema_for::<SecurityRuntimeStateCounts>();
    let _ = generator.subschema_for::<SecuritySurvivorsOutput>();
    let _ = generator.subschema_for::<SecuritySurvivorsSchemaVersion>();
    let _ = generator.subschema_for::<SecuritySurvivorsSummary>();
    let _ = generator.subschema_for::<SecuritySurvivor>();
    let _ = generator.subschema_for::<SecurityVerifierVerdict>();
    let _ = generator.subschema_for::<SecurityVerifierVerdictStatus>();
    let _ = generator.subschema_for::<SecurityBlindSpotsOutput>();
    let _ = generator.subschema_for::<SecurityBlindSpotsSchemaVersion>();
    let _ = generator.subschema_for::<SecurityBlindSpotsSummary>();
    let _ = generator.subschema_for::<SecurityBlindSpotGroup>();
    let _ = generator.subschema_for::<SecurityBlindSpotFile>();
}

fn register_health_action_definitions(generator: &mut schemars::SchemaGenerator) {
    let _ = generator.subschema_for::<HealthFindingAction>();
    let _ = generator.subschema_for::<HealthFindingActionType>();
    let _ = generator.subschema_for::<HotspotAction>();
    let _ = generator.subschema_for::<HotspotActionType>();
    let _ = generator.subschema_for::<HotspotActionHeuristic>();
    let _ = generator.subschema_for::<RefactoringTargetAction>();
    let _ = generator.subschema_for::<RefactoringTargetActionType>();
    let _ = generator.subschema_for::<UntestedFileAction>();
    let _ = generator.subschema_for::<UntestedFileActionType>();
    let _ = generator.subschema_for::<UntestedExportAction>();
    let _ = generator.subschema_for::<UntestedExportActionType>();
}

/// Register per-command envelope structs.
fn register_per_command_envelope_definitions(generator: &mut schemars::SchemaGenerator) {
    let _ = generator.subschema_for::<AuditOutput>();
    let _ = generator.subschema_for::<AuditCommand>();
    let _ = generator.subschema_for::<CoverageSetupOutput>();
    let _ = generator.subschema_for::<CoverageSetupMember>();
    let _ = generator.subschema_for::<CoverageSetupFileToEdit>();
    let _ = generator.subschema_for::<CoverageSetupSnippet>();
    let _ = generator.subschema_for::<CoverageSetupSchemaVersion>();
    let _ = generator.subschema_for::<CoverageSetupFramework>();
    let _ = generator.subschema_for::<CoverageSetupPackageManager>();
    let _ = generator.subschema_for::<CoverageSetupRuntimeTarget>();
    let _ = generator.subschema_for::<CoverageAnalyzeOutput>();
    let _ = generator.subschema_for::<CoverageAnalyzeSchemaVersion>();
    let _ = generator.subschema_for::<CombinedOutput>();
    let _ = generator.subschema_for::<CheckOutput>();
    let _ = generator.subschema_for::<CheckGroupedOutput>();
    let _ = generator.subschema_for::<CheckGroupedEntry>();
    let _ = generator.subschema_for::<DupesOutput>();
    let _ = generator.subschema_for::<HealthOutput>();
    let _ = generator.subschema_for::<fallow_cli::health_types::HealthGroup>();
    let _ = generator.subschema_for::<fallow_cli::health_types::HealthReport>();
    let _ = generator.subschema_for::<GroupByMode>();
    let _ = generator.subschema_for::<ExplainOutput>();
    let _ = generator.subschema_for::<InspectOutput>();
    let _ = generator.subschema_for::<InspectTargetDescriptor>();
    let _ = generator.subschema_for::<InspectIdentity>();
    let _ = generator.subschema_for::<InspectFileIdentity>();
    let _ = generator.subschema_for::<InspectSymbolIdentity>();
    let _ = generator.subschema_for::<InspectEvidence>();
    let _ = generator.subschema_for::<InspectEvidenceSection>();
    let _ = generator.subschema_for::<InspectSectionStatus>();
    let _ = generator.subschema_for::<InspectEvidenceScope>();
    let _ = generator.subschema_for::<CodeClimateOutput>();
    let _ = generator.subschema_for::<CodeClimateIssue>();
    let _ = generator.subschema_for::<CodeClimateIssueKind>();
    let _ = generator.subschema_for::<CodeClimateSeverity>();
    let _ = generator.subschema_for::<CodeClimateLocation>();
    let _ = generator.subschema_for::<CodeClimateLines>();
    let _ = generator.subschema_for::<ReviewEnvelopeOutput>();
    let _ = generator.subschema_for::<ReviewEnvelopeSummary>();
    let _ = generator.subschema_for::<ReviewEnvelopeEvent>();
    let _ = generator.subschema_for::<ReviewComment>();
    let _ = generator.subschema_for::<GitHubReviewComment>();
    let _ = generator.subschema_for::<GitHubReviewSide>();
    let _ = generator.subschema_for::<GitLabReviewComment>();
    let _ = generator.subschema_for::<GitLabReviewPosition>();
    let _ = generator.subschema_for::<GitLabReviewPositionType>();
    let _ = generator.subschema_for::<ReviewEnvelopeMeta>();
    let _ = generator.subschema_for::<ReviewEnvelopeSchema>();
    let _ = generator.subschema_for::<ReviewProvider>();
    let _ = generator.subschema_for::<ReviewCheckConclusion>();
    let _ = generator.subschema_for::<ReviewReconcileOutput>();
    let _ = generator.subschema_for::<ReviewReconcileSchema>();
}

/// Register the `fallow list --boundaries --format json` envelope.
fn register_list_boundaries_definitions(generator: &mut schemars::SchemaGenerator) {
    let _ = generator.subschema_for::<ListBoundariesOutput>();
    let _ = generator.subschema_for::<WorkspacesOutput>();
    let _ = generator.subschema_for::<WorkspaceInfo>();
    let _ = generator.subschema_for::<BoundariesListing>();
    let _ = generator.subschema_for::<BoundariesListZone>();
    let _ = generator.subschema_for::<BoundariesListRule>();
    let _ = generator.subschema_for::<BoundariesListLogicalGroup>();
    let _ = generator.subschema_for::<LogicalGroup>();
    let _ = generator.subschema_for::<LogicalGroupStatus>();
    let _ = generator.subschema_for::<AuthoredRule>();
}

/// Merge derived definitions back into the hand-written schema document.
fn merge_with_committed(derived: &Map<String, Value>) -> Result<Value, String> {
    let source = committed_schema_source()?;
    let mut document: Value = serde_json::from_str(&source)
        .map_err(|err| format!("failed to parse committed docs/output-schema.json: {err}"))?;

    let definitions = document
        .get_mut("definitions")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| {
            "committed docs/output-schema.json has no top-level `definitions` object".to_string()
        })?;

    let finding_names: rustc_hash::FxHashSet<&'static str> =
        finding_definition_names().iter().copied().collect();

    for name in derived_definition_names() {
        let derived_schema = derived.get(*name).ok_or_else(|| {
            format!(
                "derived schema missing for '{name}'; check that the type carries `#[cfg_attr(feature = \"schema\", derive(schemars::JsonSchema))]` and is registered in derived_definitions"
            )
        })?;
        let mut value = derived_schema.clone();
        normalize_schema(&mut value);
        if finding_names.contains(name) {
            augment_finding_definition(&mut value, finding_augmentation(name))?;
        }
        definitions.insert((*name).to_string(), value);
    }

    let in_scope: rustc_hash::FxHashSet<&'static str> =
        derived_definition_names().iter().copied().collect();
    for (name, value) in derived {
        if in_scope.contains(name.as_str()) {
            continue;
        }
        let mut value = value.clone();
        normalize_schema(&mut value);
        definitions.insert(name.clone(), value);
    }

    rewrite_fallow_output_definition(definitions)?;
    rewrite_document_root_one_of(&mut document)?;

    Ok(document)
}

/// Hand-maintained root envelopes that still need top-level `oneOf` entries.
const HAND_MAINTAINED_ROOT_ENVELOPES: &[&str] = &[];
const FALLOW_OUTPUT_VARIANTS: &[(&str, &[&str], &str)] = &[
    (
        "audit",
        &["AuditOutput"],
        "`fallow audit --format json`. Required `command: \"audit\"` singleton\nplus `verdict` and `summary`.",
    ),
    (
        "explain",
        &["ExplainOutput"],
        "`fallow explain <issue-type> --format json`. Required `id`, `name`,\n`rationale`, `example`, `how_to_fix`, `docs`; no `schema_version`.",
    ),
    (
        "inspect_target",
        &["InspectOutput"],
        "`fallow inspect --format json`. Required `target`, `identity`,\n`evidence`, and `warnings`; no `schema_version`.",
    ),
    (
        "review-envelope",
        &["ReviewEnvelopeOutput"],
        "`fallow --format review-github` / `--format review-gitlab`. Required\n`body`, `comments`, `meta`; no `schema_version`.",
    ),
    (
        "review-reconcile",
        &["ReviewReconcileOutput"],
        "`fallow ci reconcile-review --format json`. Required `schema`\nsingleton plus `provider`, `comments`, and the various\n`*_fingerprints` arrays.",
    ),
    (
        "coverage-setup",
        &["CoverageSetupOutput"],
        "`fallow coverage setup --json`. Required `schema_version` singleton\nplus `framework_detected`, `members`, `commands`, `snippets`.",
    ),
    (
        "coverage-analyze",
        &["CoverageAnalyzeOutput"],
        "`fallow coverage analyze --format json`. Required\n`schema_version: \"1\"` singleton plus `version`, `elapsed_ms`,\n`runtime_coverage`.",
    ),
    (
        "list-boundaries",
        &["ListBoundariesOutput"],
        "`fallow list --boundaries --format json`. Required `boundaries`\nsub-object; no `schema_version`.",
    ),
    (
        "list-workspaces",
        &["WorkspacesOutput"],
        "`fallow workspaces --format json`. Required `workspace_count`,\n`workspaces`, and `workspace_diagnostics`; no `schema_version`.",
    ),
    (
        "health",
        &["HealthOutput"],
        "`fallow health --format json`.",
    ),
    ("dupes", &["DupesOutput"], "`fallow dupes --format json`."),
    (
        "dead-code-grouped",
        &["CheckGroupedOutput"],
        "`fallow dead-code --format json --group-by <mode>`. Required `grouped_by`\nplus a `groups` array.",
    ),
    (
        "impact",
        &["ImpactReport"],
        "`fallow impact --format json`. Required `enabled`, `record_count`,\n`containment_count`, `recent_containment`; no global `schema_version`,\n`command`, `total_issues`, or `report`.",
    ),
    (
        "impact-cross-repo",
        &["CrossRepoImpactReport"],
        "`fallow impact --all --format json`. Required `project_count`,\n`tracked_count`, `unreadable_count`, `totals`, `projects`; each `projects[]`\nentry embeds a per-project `report`. Independently versioned via\n`CrossRepoImpactSchemaVersion`.",
    ),
    (
        "security",
        &["SecurityOutput", "SecuritySummaryOutput"],
        "`fallow security --format json`. Full mode requires `security_findings`,\n`unresolved_edge_files`, and `unresolved_callee_sites`; summary mode requires\n`summary` and omits per-finding arrays.",
    ),
    (
        "security-survivors",
        &["SecuritySurvivorsOutput"],
        "`fallow security survivors --format json`. Required `survivors` and\n`needs_human_review`, keyed by `finding_id`.",
    ),
    (
        "security-blind-spots",
        &["SecurityBlindSpotsOutput"],
        "`fallow security blind-spots --format json`. Required `summary` and\ngrouped unresolved-callee diagnostics.",
    ),
    (
        "dead-code",
        &["CheckOutput"],
        "`fallow dead-code --format json`.\nRequired `total_issues` plus `summary: CheckSummary`.",
    ),
    (
        "combined",
        &["CombinedOutput"],
        "Bare `fallow --format json` (combined dead-code + dupes + health).\nRequired `schema_version`, `version`, and `elapsed_ms`, with optional\n`check`, `dupes`, and `health` subreports.",
    ),
];

/// Schemars emits internally tagged newtype variants as `$ref` plus sibling
/// constraints. The schema declares draft-07, where `$ref` siblings are ignored
/// by many validators and code generators. Rewrite the root union into explicit
/// intersections so the `kind` discriminator is part of the public contract.
fn rewrite_fallow_output_definition(definitions: &mut Map<String, Value>) -> Result<(), String> {
    let output = definitions
        .get_mut("FallowOutput")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| "derived schema has no object definition for `FallowOutput`".to_string())?;

    let one_of = FALLOW_OUTPUT_VARIANTS
        .iter()
        .map(|(kind, definitions, description)| {
            fallow_output_variant_schema(kind, definitions, description)
        })
        .collect();

    output.remove("anyOf");
    output.insert("oneOf".to_string(), Value::Array(one_of));
    Ok(())
}

fn fallow_output_variant_schema(kind: &str, definitions: &[&str], description: &str) -> Value {
    serde_json::json!({
        "description": description,
        "allOf": [
            fallow_output_payload_schema(definitions),
            {
                "type": "object",
                "properties": {
                    "kind": {
                        "type": "string",
                        "const": kind
                    }
                },
                "required": ["kind"]
            }
        ]
    })
}

fn fallow_output_payload_schema(definitions: &[&str]) -> Value {
    if definitions.len() == 1 {
        serde_json::json!({ "$ref": format!("#/definitions/{}", definitions[0]) })
    } else {
        serde_json::json!({
            "oneOf": definitions
                .iter()
                .map(|definition| serde_json::json!({ "$ref": format!("#/definitions/{definition}") }))
                .collect::<Vec<_>>()
        })
    }
}

/// Drive the document-root `oneOf` from the typed `FallowOutput` enum.
fn rewrite_document_root_one_of(document: &mut Value) -> Result<(), String> {
    let root = document
        .as_object_mut()
        .ok_or_else(|| "schema document root is not a JSON object".to_string())?;

    let mut one_of: Vec<Value> = Vec::with_capacity(2 + HAND_MAINTAINED_ROOT_ENVELOPES.len());
    one_of.push(serde_json::json!({ "$ref": "#/definitions/FallowOutput" }));
    one_of.push(serde_json::json!({ "$ref": "#/definitions/CodeClimateOutput" }));
    for name in HAND_MAINTAINED_ROOT_ENVELOPES {
        one_of.push(serde_json::json!({ "$ref": format!("#/definitions/{name}") }));
    }
    root.insert("oneOf".to_string(), Value::Array(one_of));

    root.insert(
        "description".to_string(),
        Value::String(
            "Schemas for the JSON output of fallow commands. Object-shaped \
             envelopes covered by the `FallowOutput` contract carry a top-level \
             `kind` discriminator (for example `dead-code`, `dead-code-grouped`, \
             `health`, `dupes`, `combined`, `audit`, `explain`, `inspect_target`, \
             `impact`, `security`, `coverage-setup`, `coverage-analyze`, `list-boundaries`, \
             `review-envelope`, and `review-reconcile`). Consumers should branch on `kind` instead of \
             probing for unique field presence. `--legacy-envelope` removes \
             only the document-root `kind` for one compatibility cycle. \
             `CodeClimateOutput` is a bare JSON array (per the Code Climate / \
             GitLab Code Quality spec) and stays a sibling root branch \
             discriminated by checking whether the document root is an array."
                .to_string(),
        ),
    );

    Ok(())
}

/// Add the `actions` array and optional `introduced` flag to a finding schema.
fn augment_finding_definition(
    value: &mut Value,
    augmentation: FindingAugmentation,
) -> Result<(), String> {
    let object = value
        .as_object_mut()
        .ok_or_else(|| "finding definition is not a JSON object".to_string())?;

    let properties = object
        .entry("properties")
        .or_insert_with(|| Value::Object(Map::new()));
    let properties = properties
        .as_object_mut()
        .ok_or_else(|| "finding definition `properties` is not a JSON object".to_string())?;

    if !properties.contains_key("actions") {
        properties.insert(
            "actions".to_string(),
            serde_json::json!({
                "type": "array",
                "items": { "$ref": augmentation.actions_item_ref },
                "description": "Suggested actions to resolve this issue."
            }),
        );
    }
    if augmentation.include_introduced && !properties.contains_key("introduced") {
        properties.insert(
            "introduced".to_string(),
            serde_json::json!({ "$ref": "#/definitions/AuditIntroduced" }),
        );
    }

    let required = object
        .entry("required")
        .or_insert_with(|| Value::Array(Vec::new()));
    if let Value::Array(arr) = required
        && !arr.iter().any(|v| v.as_str() == Some("actions"))
    {
        arr.push(Value::String("actions".to_string()));
    }

    Ok(())
}

/// Normalize derived schemas to match the committed schema layout.
fn normalize_schema(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.remove("$schema");
            map.remove("default");
            map.remove("examples");
            map.remove("format");
            map.remove("minimum");
            map.remove("maximum");
            map.remove("exclusiveMinimum");
            map.remove("exclusiveMaximum");
            if let Some(Value::String(reference)) = map.get_mut("$ref")
                && let Some(rest) = reference.strip_prefix("#/$defs/")
            {
                *reference = format!("#/definitions/{rest}");
            }
            if let Some(Value::Array(all_of)) = map.get("allOf")
                && all_of.len() == 1
                && let Some(Value::Object(only)) = all_of.first()
                && only.len() == 1
                && only.contains_key("$ref")
            {
                let reference = only.get("$ref").cloned().unwrap_or(Value::Null);
                map.remove("allOf");
                map.insert("$ref".to_string(), reference);
            }
            for (key, child) in map.iter_mut() {
                if matches!(
                    key.as_str(),
                    "properties" | "definitions" | "$defs" | "patternProperties"
                ) && let Value::Object(inner) = child
                {
                    for inner_value in inner.values_mut() {
                        normalize_schema(inner_value);
                    }
                    continue;
                }
                normalize_schema(child);
            }
        }
        Value::Array(items) => {
            for item in items {
                normalize_schema(item);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod drift_tests {
    //! Drift gate for the Rust → `docs/output-schema.json` chain.
    //!
    //! The drift gate compares normalized derived schemas to the committed file.

    use super::*;

    /// Recursively normalize a JSON value for drift comparison.
    fn canonicalize(mut value: Value) -> Value {
        normalize_one(&mut value);
        value
    }

    fn normalize_one(value: &mut Value) {
        match value {
            Value::Object(map) => {
                map.remove("description");
                map.remove("format");
                map.remove("minimum");
                map.remove("maximum");
                map.remove("exclusiveMinimum");
                map.remove("exclusiveMaximum");
                if let Some(Value::Array(arr)) = map.get_mut("type") {
                    arr.retain(|v| v.as_str() != Some("null"));
                    if arr.len() == 1 {
                        let only = arr.remove(0);
                        map.insert("type".to_string(), only);
                    }
                }
                if let Some(Value::Array(all_of)) = map.get("allOf")
                    && all_of.len() == 1
                    && let Some(Value::Object(only)) = all_of.first()
                    && only.len() == 1
                    && only.contains_key("$ref")
                {
                    let reference = only.get("$ref").cloned().unwrap_or(Value::Null);
                    map.remove("allOf");
                    map.insert("$ref".to_string(), reference);
                }
                if let Some(any_of) = map.remove("anyOf") {
                    map.insert("oneOf".to_string(), any_of);
                }
                if let Some(Value::Array(items)) = map.get_mut("required") {
                    items.sort_by(|a, b| {
                        a.as_str()
                            .unwrap_or_default()
                            .cmp(b.as_str().unwrap_or_default())
                    });
                }
                if let Some(Value::Array(items)) = map.get_mut("enum") {
                    items.sort_by(|a, b| {
                        a.as_str()
                            .unwrap_or_default()
                            .cmp(b.as_str().unwrap_or_default())
                    });
                }
                for (key, child) in map.iter_mut() {
                    if matches!(
                        key.as_str(),
                        "properties" | "definitions" | "$defs" | "patternProperties"
                    ) && let Value::Object(inner) = child
                    {
                        for inner_value in inner.values_mut() {
                            normalize_one(inner_value);
                        }
                        continue;
                    }
                    normalize_one(child);
                }
            }
            Value::Array(items) => {
                for item in items {
                    normalize_one(item);
                }
            }
            _ => {}
        }
    }

    fn committed_definitions() -> Map<String, Value> {
        let document: Value = serde_json::from_str(COMMITTED_SCHEMA)
            .expect("committed docs/output-schema.json must parse");
        document
            .get("definitions")
            .and_then(Value::as_object)
            .cloned()
            .expect("committed docs/output-schema.json must carry `definitions`")
    }

    /// Build the normalized derived schema map used by the drift checks.
    fn derived_definitions_for_drift() -> Map<String, Value> {
        let raw = derived_definitions();
        let mut out = Map::new();
        let finding_names: rustc_hash::FxHashSet<&'static str> =
            finding_definition_names().iter().copied().collect();
        let in_scope: rustc_hash::FxHashSet<&'static str> =
            derived_definition_names().iter().copied().collect();
        for (name, raw_value) in &raw {
            let mut value = raw_value.clone();
            normalize_schema(&mut value);
            if in_scope.contains(name.as_str()) && finding_names.contains(name.as_str()) {
                augment_finding_definition(&mut value, finding_augmentation(name))
                    .expect("augment_finding_definition must not fail");
            }
            out.insert(name.clone(), value);
        }
        rewrite_fallow_output_definition(&mut out)
            .expect("FallowOutput postprocess must succeed in drift checks");
        out
    }

    /// Ensure every registered name resolves to a derived schema.
    #[test]
    fn every_registered_name_resolves_to_a_derived_schema() {
        let derived = derived_definitions();
        for name in derived_definition_names() {
            assert!(
                derived.contains_key(*name),
                "no derived schema for `{name}`: either the type lacks `#[cfg_attr(feature = \"schema\", derive(schemars::JsonSchema))]`, or the call to `generator.subschema_for::<{name}>()` is missing in `derived_definitions()`."
            );
        }
    }

    /// Ensure every `FallowOutput` variant stays registered.
    #[test]
    fn every_fallow_output_variant_is_registered_in_derived_definitions() {
        const VARIANTS: &[(&str, &str)] = &[
            ("Audit", "AuditOutput"),
            ("Explain", "ExplainOutput"),
            ("ReviewEnvelope", "ReviewEnvelopeOutput"),
            ("ReviewReconcile", "ReviewReconcileOutput"),
            ("CoverageSetup", "CoverageSetupOutput"),
            ("CoverageAnalyze", "CoverageAnalyzeOutput"),
            ("ListBoundaries", "ListBoundariesOutput"),
            ("Workspaces", "WorkspacesOutput"),
            ("Health", "HealthOutput"),
            ("Dupes", "DupesOutput"),
            ("CheckGrouped", "CheckGroupedOutput"),
            ("Impact", "ImpactReport"),
            ("ImpactCrossRepo", "CrossRepoImpactReport"),
            ("Security", "SecurityOutput"),
            ("SecuritySummary", "SecuritySummaryOutput"),
            ("SecuritySurvivors", "SecuritySurvivorsOutput"),
            ("SecurityBlindSpots", "SecurityBlindSpotsOutput"),
            ("Check", "CheckOutput"),
            ("Combined", "CombinedOutput"),
        ];

        #[expect(
            dead_code,
            reason = "compile-time exhaustiveness guard for the VARIANTS list above; never called at runtime"
        )]
        fn variant_count_is_locked(value: &FallowOutput) -> &'static str {
            match value {
                FallowOutput::Audit(_) => "Audit",
                FallowOutput::Explain(_) => "Explain",
                FallowOutput::Inspect(_) => "Inspect",
                FallowOutput::ReviewEnvelope(_) => "ReviewEnvelope",
                FallowOutput::ReviewReconcile(_) => "ReviewReconcile",
                FallowOutput::CoverageSetup(_) => "CoverageSetup",
                FallowOutput::CoverageAnalyze(_) => "CoverageAnalyze",
                FallowOutput::ListBoundaries(_) => "ListBoundaries",
                FallowOutput::Workspaces(_) => "Workspaces",
                FallowOutput::Health(_) => "Health",
                FallowOutput::Dupes(_) => "Dupes",
                FallowOutput::CheckGrouped(_) => "CheckGrouped",
                FallowOutput::Impact(_) => "Impact",
                FallowOutput::ImpactCrossRepo(_) => "ImpactCrossRepo",
                FallowOutput::SecuritySummary(_) => "SecuritySummary",
                FallowOutput::Security(_) => "Security",
                FallowOutput::SecuritySurvivors(_) => "SecuritySurvivors",
                FallowOutput::SecurityBlindSpots(_) => "SecurityBlindSpots",
                FallowOutput::Check(_) => "Check",
                FallowOutput::Combined(_) => "Combined",
            }
        }

        let derived = derived_definitions();
        let mut missing: Vec<String> = Vec::new();
        for (variant, inner) in VARIANTS {
            if !derived.contains_key(*inner) {
                missing.push(format!(
                    "variant `FallowOutput::{variant}({inner})` produces an inline schema in the root `oneOf` because `{inner}` is not registered in `derived_definitions()`. Add `let _ = generator.subschema_for::<{inner}>();` (or include it via `register_per_command_envelope_definitions` / `register_list_boundaries_definitions`)."
                ));
            }
        }
        assert!(
            missing.is_empty(),
            "{} `FallowOutput` variant(s) missing registration:\n\n{}",
            missing.len(),
            missing.join("\n\n"),
        );
    }

    /// Ensure every finding type is registered before augmentation runs.
    #[test]
    fn finding_names_are_subset_of_registered_names() {
        let registered: rustc_hash::FxHashSet<&'static str> =
            derived_definition_names().iter().copied().collect();
        for name in finding_definition_names() {
            assert!(
                registered.contains(name),
                "finding type `{name}` is augmented with `actions`/`introduced` but never registered as a derived definition. Add it to `derived_definition_names()` (and the corresponding `subschema_for::<{name}>()` call) before listing it as a finding."
            );
        }
    }

    /// Verify augmentation adds the expected `actions` / `introduced` fields.
    #[test]
    fn augmentation_attaches_actions_and_introduced_to_each_finding() {
        let derived = derived_definitions_for_drift();
        for name in finding_definition_names() {
            let entry = derived
                .get(*name)
                .unwrap_or_else(|| panic!("finding `{name}` missing from derived"));
            let properties = entry
                .get("properties")
                .and_then(Value::as_object)
                .unwrap_or_else(|| panic!("finding `{name}` missing properties"));
            assert!(
                properties.contains_key("actions"),
                "finding `{name}` was not augmented with `actions`",
            );
            let aug = finding_augmentation(name);
            if aug.include_introduced {
                assert!(
                    properties.contains_key("introduced"),
                    "finding `{name}` was not augmented with `introduced` (audit-aware finding)",
                );
            } else {
                assert!(
                    !properties.contains_key("introduced"),
                    "finding `{name}` carries `introduced` but `finding_augmentation` opted out",
                );
            }
        }
    }

    /// Verify derived and committed property/required sets stay in sync.
    #[test]
    fn committed_definitions_match_derived_property_keys() {
        let committed = committed_definitions();
        let derived = derived_definitions_for_drift();
        const AUGMENTATION_KEYS: &[&str] = &["actions", "introduced"];

        let mut failures: Vec<String> = Vec::new();
        for name in derived.keys() {
            let Some(committed_entry) = committed.get(name) else {
                failures.push(format!(
                    "definition `{name}` is missing from `docs/output-schema.json`. Add a stub entry to `definitions` (the drift test only compares; it does not insert)."
                ));
                continue;
            };
            let derived_entry = derived
                .get(name)
                .expect("iterating derived's own keys; entry must exist");

            let committed_props = committed_entry.get("properties").and_then(Value::as_object);
            let derived_props = derived_entry.get("properties").and_then(Value::as_object);

            if let (Some(committed_props), Some(derived_props)) = (committed_props, derived_props) {
                for key in derived_props.keys() {
                    if !committed_props.contains_key(key) {
                        failures.push(format!(
                            "drift on `{name}`: property `{key}` is in the Rust struct (derived schema) but missing from `docs/output-schema.json`"
                        ));
                    }
                }
                for key in committed_props.keys() {
                    if !derived_props.contains_key(key)
                        && !AUGMENTATION_KEYS.contains(&key.as_str())
                    {
                        failures.push(format!(
                            "drift on `{name}`: property `{key}` is in `docs/output-schema.json` but missing from the Rust struct (derived schema)"
                        ));
                    }
                }
            }

            let committed_required: rustc_hash::FxHashSet<String> = committed_entry
                .get("required")
                .and_then(Value::as_array)
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            let derived_required: rustc_hash::FxHashSet<String> = derived_entry
                .get("required")
                .and_then(Value::as_array)
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            for key in &derived_required {
                if !committed_required.contains(key) {
                    failures.push(format!(
                        "drift on `{name}`: property `{key}` is required by the Rust struct but optional in `docs/output-schema.json`"
                    ));
                }
            }
            for key in &committed_required {
                if !derived_required.contains(key) && !AUGMENTATION_KEYS.contains(&key.as_str()) {
                    failures.push(format!(
                        "drift on `{name}`: property `{key}` is required by `docs/output-schema.json` but optional in the Rust struct"
                    ));
                }
            }
        }
        assert!(
            failures.is_empty(),
            "schema drift detected ({} issue{}):\n\n  - {}\n\nRegenerate the in-scope `definitions` blocks with:\n    cargo run -p fallow-cli --features schema-emit --bin fallow-schema-emit > /tmp/emitted-schema.json\nthen reconcile the relevant entries in `docs/output-schema.json` against the derived shape, or update the Rust source if the schema change was the intended source of truth.",
            failures.len(),
            if failures.len() == 1 { "" } else { "s" },
            failures.join("\n  - "),
        );
    }

    /// Verify property-level `$ref` targets stay aligned.
    #[test]
    fn committed_property_refs_match_derived_property_refs() {
        let committed = committed_definitions();
        let derived = derived_definitions_for_drift();
        let mut failures: Vec<String> = Vec::new();

        for name in derived.keys() {
            let Some(committed_entry) = committed.get(name) else {
                continue;
            };
            let Some(derived_entry) = derived.get(name) else {
                continue;
            };

            let committed_props = committed_entry.get("properties").and_then(Value::as_object);
            let derived_props = derived_entry.get("properties").and_then(Value::as_object);

            if let (Some(committed_props), Some(derived_props)) = (committed_props, derived_props) {
                for (key, derived_value) in derived_props {
                    let Some(committed_value) = committed_props.get(key) else {
                        continue;
                    };
                    let derived_ref = canonical_ref(derived_value);
                    let committed_ref = canonical_ref(committed_value);
                    if let (Some(dref), Some(cref)) = (&derived_ref, &committed_ref)
                        && dref != cref
                    {
                        failures.push(format!(
                            "drift on `{name}.{key}`: derived schema points at `{dref}` but committed schema points at `{cref}`"
                        ));
                    }
                }
            }
        }

        assert!(
            failures.is_empty(),
            "schema `$ref` drift detected ({} issue{}):\n\n  - {}\n\nThe wire format produced by the Rust source disagrees with the type the committed schema documents. Either update `docs/output-schema.json` to point at the type the wire actually emits, or change the runtime to produce the documented shape.",
            failures.len(),
            if failures.len() == 1 { "" } else { "s" },
            failures.join("\n  - "),
        );
    }

    /// Extract the canonical `$ref` target from a property value.
    fn canonical_ref(value: &Value) -> Option<String> {
        let mut canonical = value.clone();
        normalize_one(&mut canonical);
        if let Some(Value::String(s)) = canonical.get("$ref") {
            return Some(s.clone());
        }
        if let Some(Value::Array(arr)) = canonical.get("oneOf") {
            for variant in arr {
                if let Some(Value::String(s)) = variant.get("$ref") {
                    return Some(s.clone());
                }
            }
        }
        None
    }

    /// Verify every emitted `$ref` resolves in the merged schema.
    #[test]
    fn emitted_schema_has_no_dangling_refs() {
        let derived = derived_definitions();
        let document =
            merge_with_committed(&derived).expect("merge must succeed on committed schema");

        let mut defined: rustc_hash::FxHashSet<String> = rustc_hash::FxHashSet::default();
        if let Some(map) = document.get("definitions").and_then(Value::as_object) {
            for key in map.keys() {
                defined.insert(key.clone());
            }
        }

        let mut refs: rustc_hash::FxHashSet<String> = rustc_hash::FxHashSet::default();
        fn collect_refs(node: &Value, out: &mut rustc_hash::FxHashSet<String>) {
            match node {
                Value::Object(map) => {
                    if let Some(Value::String(reference)) = map.get("$ref")
                        && let Some(name) = reference.strip_prefix("#/definitions/")
                    {
                        out.insert(name.to_string());
                    }
                    for child in map.values() {
                        collect_refs(child, out);
                    }
                }
                Value::Array(items) => {
                    for child in items {
                        collect_refs(child, out);
                    }
                }
                _ => {}
            }
        }
        collect_refs(&document, &mut refs);

        let mut missing: Vec<String> = refs.difference(&defined).cloned().collect();
        missing.sort();
        assert!(
            missing.is_empty(),
            "emitted schema has {} dangling `$ref` target{}: {}\n\n\
             A regenerated `docs/output-schema.json` with dangling refs is invalid; \
             every referenced name must appear under `definitions`. If schemars \
             produced a transitive helper definition, ensure `merge_with_committed` \
             inserts every entry from the derived map (not just names in \
             `derived_definition_names()`).",
            missing.len(),
            if missing.len() == 1 { "" } else { "s" },
            missing.join(", "),
        );
    }

    /// Verify the canonicalized committed schema matches the derived schema.
    #[test]
    fn committed_definitions_match_derived_structurally() {
        let committed = committed_definitions();
        let derived = derived_definitions_for_drift();
        let mut failures: Vec<String> = Vec::new();
        for (name, derived_value) in &derived {
            let Some(committed_value) = committed.get(name) else {
                failures.push(format!(
                    "definition `{name}` is missing from `docs/output-schema.json`."
                ));
                continue;
            };
            let derived_entry = canonicalize(derived_value.clone());
            let committed_entry = canonicalize(committed_value.clone());
            if committed_entry != derived_entry {
                let committed_pretty = serde_json::to_string_pretty(&committed_entry)
                    .unwrap_or_else(|_| "<unprintable>".to_string());
                let derived_pretty = serde_json::to_string_pretty(&derived_entry)
                    .unwrap_or_else(|_| "<unprintable>".to_string());
                failures.push(format!(
                    "drift on `{name}`:\n--- committed (canonicalized) ---\n{committed_pretty}\n--- derived (canonicalized) ---\n{derived_pretty}"
                ));
            }
        }
        const HAND_MAINTAINED_ALLOW_LIST: &[(&str, &str)] = &[];
        let allow_list: rustc_hash::FxHashSet<&'static str> = HAND_MAINTAINED_ALLOW_LIST
            .iter()
            .map(|(name, _)| *name)
            .collect();
        for name in committed.keys() {
            if !derived.contains_key(name) && !allow_list.contains(name.as_str()) {
                failures.push(format!(
                    "orphan in `docs/output-schema.json`: definition `{name}` is not produced by `derived_definitions()`. Either register the type via `subschema_for::<{name}>()` in `derived_definitions`, or delete the stale entry. (If the entry is hand-maintained pending another #384 item, add it to `HAND_MAINTAINED_ALLOW_LIST` with a reason linking the issue.)"
                ));
            }
        }
        assert!(
            failures.is_empty(),
            "{} structural drift issue{}:\n\n{}",
            failures.len(),
            if failures.len() == 1 { "" } else { "s" },
            failures.join("\n\n"),
        );
    }

    /// Regression for issue #394: property names like `format` must survive normalization.
    #[test]
    fn normalize_schema_preserves_property_named_format() {
        let mut value = serde_json::json!({
            "type": "object",
            "properties": {
                "format": { "$ref": "#/definitions/SomeEnum" },
                "minimum": { "type": "integer" },
                "default": { "type": "string" },
                "regular": { "type": "string", "format": "uri" }
            },
            "required": ["format", "minimum", "default", "regular"]
        });
        super::normalize_schema(&mut value);
        let properties = value
            .get("properties")
            .and_then(Value::as_object)
            .expect("properties stays an object");
        assert!(
            properties.contains_key("format"),
            "property `format` must survive normalize_schema (issue #394)"
        );
        assert!(
            properties.contains_key("minimum"),
            "property `minimum` must survive normalize_schema"
        );
        assert!(
            properties.contains_key("default"),
            "property `default` must survive normalize_schema"
        );
        let regular = properties
            .get("regular")
            .and_then(Value::as_object)
            .expect("`regular` stays an object");
        assert!(
            !regular.contains_key("format"),
            "schemars `format` keyword inside a property's schema is still stripped"
        );
    }

    /// Mirror the `format`-property regression on the drift-test side.
    #[test]
    fn normalize_one_preserves_property_named_format() {
        let mut value = serde_json::json!({
            "type": "object",
            "properties": {
                "format": { "$ref": "#/definitions/SomeEnum" },
                "minimum": { "type": "integer" },
                "regular": { "type": "string", "format": "uri" }
            },
            "required": ["format", "minimum", "regular"]
        });
        normalize_one(&mut value);
        let properties = value
            .get("properties")
            .and_then(Value::as_object)
            .expect("properties stays an object");
        assert!(
            properties.contains_key("format"),
            "property `format` must survive normalize_one"
        );
        assert!(
            properties.contains_key("minimum"),
            "property `minimum` must survive normalize_one"
        );
        let regular = properties
            .get("regular")
            .and_then(Value::as_object)
            .expect("`regular` stays an object");
        assert!(
            !regular.contains_key("format"),
            "schemars `format` keyword inside a property's schema is still stripped"
        );
    }

    /// Ensure hand-maintained root envelopes remain referenced from the root union.
    #[test]
    fn hand_maintained_root_envelopes_appear_in_root_one_of() {
        let document: Value = serde_json::from_str(COMMITTED_SCHEMA)
            .expect("committed docs/output-schema.json must parse");
        let one_of = document
            .get("oneOf")
            .and_then(Value::as_array)
            .expect("committed schema must carry a root-level `oneOf`");

        let mut refs: rustc_hash::FxHashSet<String> = rustc_hash::FxHashSet::default();
        for entry in one_of {
            if let Some(reference) = entry.get("$ref").and_then(Value::as_str)
                && let Some(name) = reference.strip_prefix("#/definitions/")
            {
                refs.insert(name.to_string());
            }
        }

        for name in HAND_MAINTAINED_ROOT_ENVELOPES {
            assert!(
                refs.contains(*name),
                "hand-maintained root envelope `{name}` is registered in \
                 `HAND_MAINTAINED_ROOT_ENVELOPES` but is not referenced from \
                 the document-root `oneOf`. Either (a) re-add the entry to \
                 the rewritten `oneOf` in `rewrite_document_root_one_of`, \
                 or (b) remove it from `HAND_MAINTAINED_ROOT_ENVELOPES` \
                 because the migration to a typed `FallowOutput` variant \
                 has landed. Root `oneOf` refs today: {:?}",
                refs.iter().collect::<Vec<_>>(),
            );
        }
    }

    /// Ensure the root union keeps `FallowOutput` and `CodeClimateOutput`.
    #[test]
    fn root_one_of_carries_fallow_output_and_codeclimate() {
        let document: Value = serde_json::from_str(COMMITTED_SCHEMA)
            .expect("committed docs/output-schema.json must parse");
        let one_of = document
            .get("oneOf")
            .and_then(Value::as_array)
            .expect("committed schema must carry a root-level `oneOf`");

        let mut refs: rustc_hash::FxHashSet<String> = rustc_hash::FxHashSet::default();
        for entry in one_of {
            if let Some(reference) = entry.get("$ref").and_then(Value::as_str)
                && let Some(name) = reference.strip_prefix("#/definitions/")
            {
                refs.insert(name.to_string());
            }
        }

        assert!(
            refs.contains("FallowOutput"),
            "document-root `oneOf` must reference `#/definitions/FallowOutput`; \
             found refs: {:?}",
            refs.iter().collect::<Vec<_>>(),
        );
        assert!(
            refs.contains("CodeClimateOutput"),
            "document-root `oneOf` must reference `#/definitions/CodeClimateOutput` \
             as a sibling root branch (the bare-array spec form); found refs: {:?}",
            refs.iter().collect::<Vec<_>>(),
        );
    }

    #[test]
    fn fallow_output_kind_variants_use_draft07_all_of_refs() {
        let document: Value = serde_json::from_str(COMMITTED_SCHEMA)
            .expect("committed docs/output-schema.json must parse");
        let variants = document
            .pointer("/definitions/FallowOutput/oneOf")
            .and_then(Value::as_array)
            .expect("FallowOutput must expose oneOf variants");

        for variant in variants {
            assert!(
                variant.get("$ref").is_none(),
                "FallowOutput variant must not use `$ref` siblings; draft-07 validators ignore sibling constraints: {variant}"
            );

            let all_of = variant
                .get("allOf")
                .and_then(Value::as_array)
                .expect("FallowOutput variant must use allOf to combine the payload ref with the root kind discriminator");
            assert_eq!(
                all_of.len(),
                2,
                "FallowOutput variant allOf should contain exactly payload ref + kind discriminator"
            );
            let payload_branch = &all_of[0];
            let has_payload_ref = payload_branch.get("$ref").is_some()
                || payload_branch
                    .get("oneOf")
                    .and_then(Value::as_array)
                    .is_some_and(|branches| {
                        !branches.is_empty()
                            && branches.iter().all(|branch| branch.get("$ref").is_some())
                    });
            assert!(
                has_payload_ref,
                "first allOf branch should be a payload ref or a oneOf of payload refs: {variant}"
            );
            assert!(
                all_of[1]
                    .pointer("/properties/kind/const")
                    .and_then(Value::as_str)
                    .is_some(),
                "second allOf branch should require a literal kind discriminator: {variant}"
            );
        }
    }

    #[test]
    fn security_kind_accepts_full_and_summary_payloads() {
        let document: Value = serde_json::from_str(COMMITTED_SCHEMA)
            .expect("committed docs/output-schema.json must parse");
        let variants = document
            .pointer("/definitions/FallowOutput/oneOf")
            .and_then(Value::as_array)
            .expect("FallowOutput must expose oneOf variants");
        let security_variant = variants
            .iter()
            .find(|variant| {
                variant
                    .pointer("/allOf/1/properties/kind/const")
                    .and_then(Value::as_str)
                    == Some("security")
            })
            .expect("FallowOutput must include the security kind variant");
        let payload_refs: Vec<&str> = security_variant
            .pointer("/allOf/0/oneOf")
            .and_then(Value::as_array)
            .expect("security kind payload must be a oneOf")
            .iter()
            .map(|branch| {
                branch
                    .get("$ref")
                    .and_then(Value::as_str)
                    .expect("security payload branch must be a ref")
            })
            .collect();

        assert!(
            payload_refs.contains(&"#/definitions/SecurityOutput"),
            "security kind must accept full security output"
        );
        assert!(
            payload_refs.contains(&"#/definitions/SecuritySummaryOutput"),
            "security kind must accept summary security output"
        );
    }
}
