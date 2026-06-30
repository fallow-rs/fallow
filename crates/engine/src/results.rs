//! Analysis result types exposed through the engine boundary.

use std::path::PathBuf;
use std::time::Duration;

use fallow_config::ResolvedConfig;
use fallow_output::{HealthGrouping, HealthReport, HealthTimings};
use fallow_types::discover::DiscoveredFile;
use fallow_types::extract::ModuleInfo;
use fallow_types::workspace::WorkspaceDiagnostic;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::{duplicates, module_graph, trace};

pub use fallow_types::output_dead_code::{
    BoundaryCallViolationFinding, BoundaryCoverageViolationFinding, BoundaryViolationFinding,
    CircularDependencyFinding, DuplicateExportFinding, DuplicatePropShapeFinding,
    DynamicSegmentNameConflictFinding, EmptyCatalogGroupFinding, InvalidClientExportFinding,
    MisconfiguredDependencyOverrideFinding, MisplacedDirectiveFinding,
    MixedClientServerBarrelFinding, PolicyViolationFinding, PrivateTypeLeakFinding,
    PropDrillingChainFinding, ReExportCycleFinding, RouteCollisionFinding,
    TestOnlyDependencyFinding, ThinWrapperFinding, TypeOnlyDependencyFinding,
    UnlistedDependencyFinding, UnprovidedInjectFinding, UnrenderedComponentFinding,
    UnresolvedCatalogReferenceFinding, UnresolvedImportFinding, UnusedCatalogEntryFinding,
    UnusedClassMemberFinding, UnusedComponentEmitFinding, UnusedComponentInputFinding,
    UnusedComponentOutputFinding, UnusedComponentPropFinding, UnusedDependencyFinding,
    UnusedDependencyOverrideFinding, UnusedDevDependencyFinding, UnusedEnumMemberFinding,
    UnusedExportFinding, UnusedFileFinding, UnusedLoadDataKeyFinding,
    UnusedOptionalDependencyFinding, UnusedServerActionFinding, UnusedStoreMemberFinding,
    UnusedSvelteEventFinding, UnusedTypeFinding,
};
pub use fallow_types::results::{
    ActiveSuppression, AnalysisResults, BoundaryCallViolation, BoundaryCoverageViolation,
    BoundaryViolation, CircularDependency, CircularDependencyEdge, DependencyLocation,
    DependencyOverrideMisconfigReason, DependencyOverrideSource, DuplicateExport,
    DuplicateLocation, DuplicatePropShape, DuplicatePropShapeMember, DynamicSegmentNameConflict,
    EmptyCatalogGroup, EntryPointSummary, ExportUsage, FeatureFlag, FlagConfidence, FlagKind,
    ImportSite, InvalidClientExport, MisconfiguredDependencyOverride, MisplacedDirective,
    MixedClientServerBarrel, PolicyRuleKind, PolicyViolation, PolicyViolationSeverity,
    PrivateTypeLeak, PropDrillHop, PropDrillingChain, ReExportCycle, ReExportCycleKind,
    ReactComponentIntel, ReactHookSummary, ReactPropDrill, ReactPropIntel, ReferenceLocation,
    RenderFanInComponent, RenderFanInMetric, RouteCollision, SecurityAttackSurfaceEntry,
    SecurityCandidate, SecurityCandidateBoundary, SecurityCandidateSink, SecurityDeadCodeContext,
    SecurityDeadCodeKind, SecurityDefensiveBoundary, SecurityDefensiveControl, SecurityFinding,
    SecurityFindingKind, SecurityNetworkContext, SecurityReachability, SecurityRuntimeContext,
    SecurityRuntimeState, SecuritySeverity, SecurityTaintFlow, SecurityUnresolvedCalleeDiagnostic,
    SecurityZoneCrossing, StaleSuppression, SuppressionOrigin, TaintConfidence, TaintEndpoint,
    TaintPath, TestOnlyDependency, ThinWrapper, TraceHop, TraceHopRole, TypeOnlyDependency,
    UnlistedDependency, UnprovidedInject, UnrenderedComponent, UnresolvedCatalogReference,
    UnresolvedImport, UnusedCatalogEntry, UnusedComponentEmit, UnusedComponentInput,
    UnusedComponentOutput, UnusedComponentProp, UnusedDependency, UnusedDependencyOverride,
    UnusedExport, UnusedFile, UnusedLoadDataKey, UnusedMember, UnusedServerAction,
    UnusedSvelteEvent,
};

/// Typed dead-code analysis result.
#[derive(Debug)]
pub struct DeadCodeAnalysis {
    pub results: AnalysisResults,
}

/// Typed dead-code analysis result with per-file source hashes.
#[derive(Debug)]
pub struct DeadCodeAnalysisWithHashes {
    pub results: AnalysisResults,
    pub file_hashes: FxHashMap<PathBuf, u64>,
}

/// Typed dead-code analysis result with retained parser artifacts.
#[derive(Debug)]
pub struct DeadCodeAnalysisOutput {
    pub results: AnalysisResults,
    pub modules: Option<Vec<ModuleInfo>>,
    pub files: Option<Vec<DiscoveredFile>>,
}

/// Typed dead-code analysis result with all reusable pipeline artifacts.
#[derive(Debug)]
pub struct DeadCodeAnalysisArtifacts {
    pub results: AnalysisResults,
    pub timings: Option<trace::PipelineTimings>,
    pub graph: Option<module_graph::RetainedModuleGraph>,
    pub modules: Option<Vec<ModuleInfo>>,
    pub files: Option<Vec<DiscoveredFile>>,
    pub script_used_packages: FxHashSet<String>,
    pub file_hashes: FxHashMap<PathBuf, u64>,
}

/// Typed project analysis result combining dead-code and duplication outputs.
#[derive(Debug)]
pub struct ProjectAnalysisOutput {
    pub dead_code: DeadCodeAnalysisOutput,
    pub duplication: duplicates::DuplicationReport,
}

/// Typed duplication analysis result.
#[derive(Debug)]
pub struct DuplicationAnalysis {
    pub report: duplicates::DuplicationReport,
    pub default_ignore_skips: duplicates::DefaultIgnoreSkips,
}

/// Typed health analysis result shared by CLI, API, NAPI, and future embedders.
///
/// The result contract belongs at the engine boundary so downstream callers can
/// depend on a command-neutral shape.
#[derive(Debug)]
pub struct HealthAnalysisResult<GroupResolver = ()> {
    pub report: HealthReport,
    /// Per-group health output when grouping is active.
    ///
    /// `None` for the default run; `Some` for any grouped invocation. The
    /// top-level report reflects the active run scope; consumers that want
    /// per-group metrics read from `grouping.groups`.
    pub grouping: Option<HealthGrouping>,
    /// Optional grouping resolver retained by callers that need to tag findings
    /// after analysis without rediscovering ownership or package metadata.
    pub group_resolver: Option<GroupResolver>,
    pub config: ResolvedConfig,
    pub workspace_diagnostics: Vec<WorkspaceDiagnostic>,
    pub elapsed: Duration,
    pub timings: Option<HealthTimings>,
    pub coverage_gaps_has_findings: bool,
    pub should_fail_on_coverage_gaps: bool,
}

impl<GroupResolver> HealthAnalysisResult<GroupResolver> {
    /// Drop presentation-only grouping resolver state while preserving the
    /// command-neutral health analysis payload.
    #[must_use]
    pub fn without_group_resolver(self) -> HealthAnalysisResult<()> {
        HealthAnalysisResult {
            report: self.report,
            grouping: self.grouping,
            group_resolver: None,
            config: self.config,
            workspace_diagnostics: self.workspace_diagnostics,
            elapsed: self.elapsed,
            timings: self.timings,
            coverage_gaps_has_findings: self.coverage_gaps_has_findings,
            should_fail_on_coverage_gaps: self.should_fail_on_coverage_gaps,
        }
    }
}

#[cfg(test)]
mod tests {
    use fallow_config::ProductionAnalysis;
    use fallow_types::output_format::OutputFormat;

    use super::*;

    #[test]
    fn health_analysis_result_drops_presentation_resolver() {
        let project = tempfile::tempdir().expect("temp dir");
        let project_config = crate::config_for_project_analysis(
            project.path(),
            None,
            crate::ProjectConfigOptions {
                output: OutputFormat::Json,
                no_cache: true,
                threads: 1,
                production_override: None,
                quiet: true,
                analysis: ProductionAnalysis::Health,
            },
        )
        .expect("project config loads");
        let result = HealthAnalysisResult {
            report: HealthReport::default(),
            grouping: None,
            group_resolver: Some("resolver"),
            config: project_config.config,
            workspace_diagnostics: Vec::new(),
            elapsed: Duration::from_millis(7),
            timings: None,
            coverage_gaps_has_findings: true,
            should_fail_on_coverage_gaps: true,
        };

        let neutral = result.without_group_resolver();

        assert!(neutral.group_resolver.is_none());
        assert_eq!(neutral.elapsed, Duration::from_millis(7));
        assert!(neutral.coverage_gaps_has_findings);
        assert!(neutral.should_fail_on_coverage_gaps);
    }

    #[test]
    fn engine_result_surface_uses_explicit_reexports() {
        let source = include_str!("results.rs");
        let output_dead_code_wildcard = concat!("pub use fallow_types::output_dead_code::", "*");
        let results_wildcard = concat!("pub use fallow_types::results::", "*");

        assert!(!source.contains(output_dead_code_wildcard));
        assert!(!source.contains(results_wildcard));
    }
}
