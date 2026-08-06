//! Concrete output-contract aliases shared by schema and adapter crates.

/// Concrete `fallow audit` envelope with this crate's verdict, summary, and
/// attribution payloads filled into the generic output shell.
pub type AuditOutput = fallow_output::AuditOutput<
    crate::AuditVerdict,
    crate::AuditSummary,
    crate::AuditAttribution,
    fallow_output::CheckOutput,
    crate::DupesReportPayload,
    fallow_output::HealthReport,
>;

/// Concrete combined check + dupes + health envelope for full analyses.
pub type CombinedOutput = fallow_output::CombinedOutput<
    fallow_output::CheckOutput,
    crate::DupesReportPayload,
    fallow_output::HealthReport,
>;

/// Concrete `fallow list boundaries` envelope with config-crate group and
/// rule payloads.
pub type ListBoundariesOutput = fallow_output::ListBoundariesOutput<
    fallow_config::LogicalGroupStatus,
    fallow_config::AuthoredRule,
>;

/// Concrete `fallow list workspaces` envelope with config-crate diagnostics.
pub type WorkspacesOutput = fallow_output::WorkspacesOutput<fallow_config::WorkspaceDiagnostic>;

/// Concrete boundaries listing body used inside list envelopes.
pub type BoundariesListing = fallow_output::BoundariesListing<
    fallow_config::LogicalGroupStatus,
    fallow_config::AuthoredRule,
>;

/// Re-export of the boundaries zone row so adapters need only this crate.
pub type BoundariesListZone = fallow_output::BoundariesListZone;

/// Re-export of the boundaries rule row so adapters need only this crate.
pub type BoundariesListRule = fallow_output::BoundariesListRule;

/// Concrete logical-group row with config-crate status and rule payloads.
pub type BoundariesListLogicalGroup = fallow_output::BoundariesListLogicalGroup<
    fallow_config::LogicalGroupStatus,
    fallow_config::AuthoredRule,
>;

/// Concrete `fallow list` body combining plugins, files, entry points,
/// boundaries, and workspaces sections.
pub type ListOutput =
    fallow_output::ListOutput<BoundariesListing, fallow_config::WorkspaceDiagnostic>;

/// Re-export of the entry-point row so adapters need only this crate.
pub type ListEntryPointOutput = fallow_output::ListEntryPointOutput;

/// Re-export of the plugin row so adapters need only this crate.
pub type ListPluginOutput = fallow_output::ListPluginOutput;

/// Concrete security gate with this crate's gate-mode payload.
pub type SecurityGate = fallow_output::SecurityGate<crate::SecurityGateMode>;

/// Concrete security output configuration with config-crate severities.
pub type SecurityOutputConfig = fallow_output::SecurityOutputConfig<fallow_config::Severity>;

/// Concrete `fallow security` summary envelope.
pub type SecuritySummaryOutput =
    fallow_output::SecuritySummaryOutput<SecurityOutputConfig, SecurityGate>;

/// Concrete full `fallow security` envelope.
pub type SecurityOutput = fallow_output::SecurityOutput<SecurityOutputConfig, SecurityGate>;

/// Union of every `fallow trace` payload shape.
///
/// Serialized untagged: the JSON body is exactly one of the trace shapes with
/// no discriminant field, so consumers dispatch on structure.
#[derive(Debug, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(untagged)]
pub enum TraceOutput {
    /// Trace of a single export's usage.
    Export(Box<fallow_types::trace::ExportTrace>),
    /// Trace of a single class member's usage.
    ClassMember(Box<fallow_types::trace::ClassMemberTrace>),
    /// Trace of a file's reachability.
    File(Box<fallow_types::trace::FileTrace>),
    /// Trace of a dependency's usage.
    Dependency(Box<fallow_types::trace::DependencyTrace>),
    /// Trace of a clone group's instances.
    Clone(Box<fallow_types::trace::CloneTrace>),
    /// Transitive impact closure for a changed file set.
    ImpactClosure(Box<fallow_types::trace::ImpactClosureTrace>),
    /// Symbol-level call chain trace.
    SymbolChain(Box<fallow_types::trace_chain::SymbolChainTrace>),
    /// Type-aware semantic symbol trace.
    SemanticSymbol(Box<fallow_types::semantic::SemanticSymbolTrace>),
}

/// Union of `fallow impact` payload shapes, serialized untagged like
/// [`TraceOutput`].
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(untagged)]
pub enum ImpactOutput {
    /// Project-level impact report for a changed file set.
    Project(Box<fallow_output::ImpactReport>),
    /// Type-aware impact for a single semantic symbol.
    SemanticSymbol(Box<fallow_types::semantic::SemanticSymbolImpact>),
}

/// Concrete review-brief wire envelope with every typed section filled in.
#[allow(
    clippy::type_complexity,
    reason = "the concrete review brief contract names every typed wire section"
)]
pub type ReviewBriefWireOutput = fallow_output::ReviewBriefWireOutput<
    fallow_output::FocusMap,
    fallow_output::WeakeningSignal,
    fallow_output::RoutingFacts,
    fallow_output::DecisionSurface,
    crate::AuditVerdict,
    crate::AuditSummary,
    crate::AuditAttribution,
    fallow_output::CheckOutput,
    crate::DupesReportPayload,
    fallow_output::HealthReport,
>;

/// Concrete root output union covering every fallow command payload; this is
/// the top-level shape the published JSON schema is generated from.
#[allow(
    clippy::type_complexity,
    reason = "concrete root union intentionally fills every output payload slot"
)]
pub type FallowOutput = fallow_output::FallowOutput<
    AuditOutput,
    fallow_output::ExplainOutput,
    fallow_output::InspectOutput,
    TraceOutput,
    fallow_output::ReviewEnvelopeOutput,
    fallow_output::ReviewReconcileOutput,
    fallow_output::CoverageSetupOutput,
    fallow_output::CoverageAnalyzeOutput,
    ListBoundariesOutput,
    WorkspacesOutput,
    fallow_output::HealthOutput<fallow_output::HealthReport, fallow_output::HealthGroup>,
    fallow_output::DupesOutput<crate::DupesReportPayload, crate::DuplicationGroup>,
    fallow_output::CheckGroupedOutput,
    ImpactOutput,
    fallow_output::CrossRepoImpactReport,
    SecuritySummaryOutput,
    SecurityOutput,
    fallow_output::SecuritySurvivorsOutput,
    fallow_output::SecurityBlindSpotsOutput,
    fallow_output::CheckOutput,
    CombinedOutput,
    fallow_output::FeatureFlagsOutput,
    ReviewBriefWireOutput,
    fallow_output::DecisionSurfaceOutput,
    fallow_output::StandardWalkthroughGuide,
    fallow_output::WalkthroughValidation,
    fallow_output::SuppressionInventoryOutput,
    fallow_output::TypeAwareStatusOutput,
>;
