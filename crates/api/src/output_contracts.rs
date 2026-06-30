//! Concrete output-contract aliases shared by schema and adapter crates.

pub type AuditOutput = fallow_output::AuditOutput<
    crate::AuditVerdict,
    crate::AuditSummary,
    crate::AuditAttribution,
    fallow_output::CheckOutput,
    crate::DupesReportPayload,
    fallow_output::HealthReport,
>;

pub type CombinedOutput = fallow_output::CombinedOutput<
    fallow_output::CheckOutput,
    crate::DupesReportPayload,
    fallow_output::HealthReport,
>;

pub type ListBoundariesOutput = fallow_output::ListBoundariesOutput<
    fallow_config::LogicalGroupStatus,
    fallow_config::AuthoredRule,
>;

pub type WorkspacesOutput = fallow_output::WorkspacesOutput<fallow_config::WorkspaceDiagnostic>;

pub type BoundariesListing = fallow_output::BoundariesListing<
    fallow_config::LogicalGroupStatus,
    fallow_config::AuthoredRule,
>;

pub type BoundariesListZone = fallow_output::BoundariesListZone;

pub type BoundariesListRule = fallow_output::BoundariesListRule;

pub type BoundariesListLogicalGroup = fallow_output::BoundariesListLogicalGroup<
    fallow_config::LogicalGroupStatus,
    fallow_config::AuthoredRule,
>;

pub type ListOutput =
    fallow_output::ListOutput<BoundariesListing, fallow_config::WorkspaceDiagnostic>;

pub type ListEntryPointOutput = fallow_output::ListEntryPointOutput;

pub type ListPluginOutput = fallow_output::ListPluginOutput;

pub type SecurityGate = fallow_output::SecurityGate<crate::SecurityGateMode>;

pub type SecurityOutputConfig = fallow_output::SecurityOutputConfig<fallow_config::Severity>;

pub type SecuritySummaryOutput =
    fallow_output::SecuritySummaryOutput<SecurityOutputConfig, SecurityGate>;

pub type SecurityOutput = fallow_output::SecurityOutput<SecurityOutputConfig, SecurityGate>;

#[allow(
    clippy::type_complexity,
    reason = "concrete root union intentionally fills every output payload slot"
)]
pub type FallowOutput = fallow_output::FallowOutput<
    AuditOutput,
    fallow_output::ExplainOutput,
    fallow_output::InspectOutput,
    fallow_types::trace_chain::SymbolChainTrace,
    fallow_output::ReviewEnvelopeOutput,
    fallow_output::ReviewReconcileOutput,
    fallow_output::CoverageSetupOutput,
    fallow_output::CoverageAnalyzeOutput,
    ListBoundariesOutput,
    WorkspacesOutput,
    fallow_output::HealthOutput<fallow_output::HealthReport, fallow_output::HealthGroup>,
    fallow_output::DupesOutput<crate::DupesReportPayload, crate::DuplicationGroup>,
    fallow_output::CheckGroupedOutput,
    fallow_output::ImpactReport,
    fallow_output::CrossRepoImpactReport,
    SecuritySummaryOutput,
    SecurityOutput,
    fallow_output::SecuritySurvivorsOutput,
    fallow_output::SecurityBlindSpotsOutput,
    fallow_output::CheckOutput,
    CombinedOutput,
    fallow_output::FeatureFlagsOutput,
    fallow_output::StandardReviewBriefOutput,
    fallow_output::DecisionSurfaceOutput,
    fallow_output::StandardWalkthroughGuide,
    fallow_output::WalkthroughValidation,
>;
