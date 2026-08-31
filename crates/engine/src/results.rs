//! Internal analysis result contracts re-exported through typed engine modules.

#![allow(
    unused_imports,
    reason = "private result contract aggregation re-exports types consumed through typed engine modules"
)]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use fallow_config::ResolvedConfig;
use fallow_output::{HealthGrouping, HealthReport, HealthTimings};
use fallow_types::discover::DiscoveredFile;
use fallow_types::extract::ModuleInfo;
use fallow_types::source_fingerprint::SourceFingerprint;
use fallow_types::workspace::WorkspaceDiagnostic;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::{duplicates, module_graph, trace};

pub use crate::security::{
    derive_security_severity, enable_security_rules, security_catalogue_title, security_finding_id,
    security_rule_id,
};
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
    /// Findings across all dead-code categories.
    pub results: AnalysisResults,
}

/// Typed dead-code analysis result with per-file source hashes.
#[derive(Debug)]
pub struct DeadCodeAnalysisWithHashes {
    /// Findings across all dead-code categories.
    pub results: AnalysisResults,
    /// Per-file source content hashes for cache invalidation.
    pub file_hashes: FxHashMap<PathBuf, u64>,
}

/// Typed dead-code analysis result with retained parser artifacts.
#[derive(Debug)]
pub struct DeadCodeAnalysisOutput {
    /// Findings across all dead-code categories.
    pub results: AnalysisResults,
    /// Parsed modules retained for reuse, when the caller asked for them.
    pub modules: Option<Vec<ModuleInfo>>,
    /// Discovered files retained for reuse, when the caller asked for them.
    pub files: Option<Vec<DiscoveredFile>>,
}

/// Typed dead-code analysis result with all reusable pipeline artifacts.
#[derive(Debug)]
pub struct DeadCodeAnalysisArtifacts {
    /// Findings across all dead-code categories.
    pub results: AnalysisResults,
    /// Per-phase pipeline timings, when the run measured them.
    pub timings: Option<trace::PipelineTimings>,
    /// Retained module graph for downstream passes (health, trace, impact).
    pub graph: Option<module_graph::RetainedModuleGraph>,
    /// Parsed modules retained for reuse, when the caller asked for them.
    pub modules: Option<Vec<ModuleInfo>>,
    /// Discovered files retained for reuse, when the caller asked for them.
    pub files: Option<Vec<DiscoveredFile>>,
    /// Package names referenced from package.json scripts, which keeps those
    /// dependencies from being reported unused.
    pub script_used_packages: FxHashSet<String>,
    /// Per-file source content hashes for cache invalidation.
    pub file_hashes: FxHashMap<PathBuf, u64>,
}

/// Shared parser artifacts for workspace-internal session consumers.
///
/// This additive contract lets internal callers retain immutable parsed
/// modules without deep-cloning the session cache. Stable owned APIs continue
/// to return [`DeadCodeAnalysisArtifacts`].
#[doc(hidden)]
#[derive(Debug)]
pub struct SharedDeadCodeAnalysisArtifacts {
    pub results: AnalysisResults,
    pub timings: Option<trace::PipelineTimings>,
    pub graph: Option<module_graph::RetainedModuleGraph>,
    pub modules: Option<Arc<[ModuleInfo]>>,
    pub files: Option<Vec<DiscoveredFile>>,
    pub script_used_packages: FxHashSet<String>,
    pub file_hashes: FxHashMap<PathBuf, u64>,
}

impl SharedDeadCodeAnalysisArtifacts {
    /// Convert shared parser artifacts to the stable owned result contract.
    #[must_use]
    pub fn into_owned(self) -> DeadCodeAnalysisArtifacts {
        let modules = self.modules.map(|modules| {
            let mut owned = modules.to_vec();
            for module in &mut owned {
                module.release_resolution_payload();
            }
            owned
        });
        DeadCodeAnalysisArtifacts {
            results: self.results,
            timings: self.timings,
            graph: self.graph,
            modules,
            files: self.files,
            script_used_packages: self.script_used_packages,
            file_hashes: self.file_hashes,
        }
    }
}

/// Typed project analysis result combining dead-code and duplication outputs.
#[derive(Debug)]
pub struct ProjectAnalysisOutput {
    /// Dead-code findings with optionally retained parser artifacts.
    pub dead_code: DeadCodeAnalysisOutput,
    /// Duplication report for the same file set.
    pub duplication: duplicates::DuplicationReport,
}

/// Typed project analysis result with reusable session artifacts.
#[derive(Debug)]
pub struct ProjectAnalysisArtifacts {
    /// Dead-code findings with all reusable pipeline artifacts.
    pub dead_code: DeadCodeAnalysisArtifacts,
    /// Duplication report for the same file set.
    pub duplication: duplicates::DuplicationReport,
    /// Diff scope the run was limited to, when one was resolved.
    pub changed_files: Option<FxHashSet<PathBuf>>,
    /// Per-file source fingerprints for downstream cache invalidation.
    pub source_fingerprints: Option<FxHashMap<PathBuf, SourceFingerprint>>,
}

impl ProjectAnalysisArtifacts {
    /// Drop retained reuse-only artifacts and return the stable project output.
    #[must_use]
    pub fn into_output(self) -> ProjectAnalysisOutput {
        ProjectAnalysisOutput {
            dead_code: DeadCodeAnalysisOutput {
                results: self.dead_code.results,
                modules: self.dead_code.modules,
                files: self.dead_code.files,
            },
            duplication: self.duplication,
        }
    }
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
    /// The assembled health report for the active run scope.
    pub report: HealthReport,
    /// Optional TypeScript semantic metadata for advisory health overlays.
    pub type_aware_meta: Option<fallow_types::envelope::TypeAwareMeta>,
    /// Per-group health output when grouping is active.
    ///
    /// `None` for the default run; `Some` for any grouped invocation. The
    /// top-level report reflects the active run scope; consumers that want
    /// per-group metrics read from `grouping.groups`.
    pub grouping: Option<HealthGrouping>,
    /// Optional grouping resolver retained by callers that need to tag findings
    /// after analysis without rediscovering ownership or package metadata.
    pub group_resolver: Option<GroupResolver>,
    /// Resolved config the run executed under.
    pub config: ResolvedConfig,
    /// Diagnostics from workspace discovery (undeclared or invalid members).
    pub workspace_diagnostics: Vec<WorkspaceDiagnostic>,
    /// Total wall time of the health run.
    pub elapsed: Duration,
    /// Per-phase timings, when the run measured them.
    pub timings: Option<HealthTimings>,
    /// True when the coverage gaps section produced findings.
    pub coverage_gaps_has_findings: bool,
    /// True when coverage gap findings should fail the run (the gate is
    /// enforced rather than advisory).
    pub should_fail_on_coverage_gaps: bool,
}

impl<GroupResolver> HealthAnalysisResult<GroupResolver> {
    /// Drop presentation-only grouping resolver state while preserving the
    /// command-neutral health analysis payload.
    #[must_use]
    pub fn without_group_resolver(self) -> HealthAnalysisResult<()> {
        HealthAnalysisResult {
            report: self.report,
            type_aware_meta: self.type_aware_meta,
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
    use crate::project_config::{ProjectConfigOptions, config_for_project_analysis};
    use fallow_config::ProductionAnalysis;
    use fallow_types::output_format::OutputFormat;

    use super::*;

    #[test]
    fn health_analysis_result_drops_presentation_resolver() {
        let project = tempfile::tempdir().expect("temp dir");
        let project_config = config_for_project_analysis(
            project.path(),
            None,
            ProjectConfigOptions {
                output: OutputFormat::Json,
                no_cache: true,
                threads: 1,
                production_override: None,
                quiet: true,
                analysis: ProductionAnalysis::Health,
                allow_remote_extends: false,
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
            type_aware_meta: None,
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
