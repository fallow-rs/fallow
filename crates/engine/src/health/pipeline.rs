//! Health pipeline carrier types shared by the engine health executor.

use fallow_config::{ResolvedConfig, WorkspaceInfo};
use fallow_output::DiffIndex;
use fallow_types::workspace::WorkspaceDiagnostic;
use rustc_hash::{FxHashMap, FxHashSet};
use std::{path::PathBuf, sync::Arc};

use crate::{
    duplicates::DuplicationReport,
    results::{AnalysisResults, DeadCodeAnalysisArtifacts},
};

use super::StylingAnalysisArtifacts;

/// Discovery / parse inputs the CLI resolves before calling the engine.
pub struct HealthPipelineInputs {
    /// Resolved project config the run executes under.
    pub config: ResolvedConfig,
    /// Discovered files for the analysis scope.
    pub files: Vec<fallow_types::discover::DiscoveredFile>,
    /// Parsed modules for the discovered files.
    pub modules: Vec<fallow_types::extract::ModuleInfo>,
    /// Config resolution wall time in milliseconds.
    pub config_ms: f64,
    /// File discovery wall time in milliseconds.
    pub discover_ms: f64,
    /// Parse wall time in milliseconds.
    pub parse_ms: f64,
    /// Summed parse CPU time across rayon workers in milliseconds; `0.0` when
    /// parse output was reused.
    pub parse_cpu_ms: f64,
    /// True when discover + parse were reused from the upstream check pass.
    pub shared_parse: bool,
    /// Dead-code analysis artifacts precomputed by the caller, so the health
    /// pipeline skips its own graph build and dead-code pass.
    pub pre_computed_analysis: Option<DeadCodeAnalysisArtifacts>,
    /// Dead-code results reused by advisory sections that do not need the graph.
    pub dead_code_results: Option<AnalysisResults>,
    /// Styling analysis artifacts shared from an upstream pass.
    pub styling_artifacts: Option<Arc<StylingAnalysisArtifacts>>,
    /// Duplication report precomputed by the caller, so the health pipeline
    /// skips its own duplication detection.
    pub pre_computed_duplication: Option<DuplicationReport>,
    /// Workspace metadata discovered during config resolution.
    pub workspaces: Vec<WorkspaceInfo>,
    /// Diagnostics from workspace discovery (undeclared or invalid members).
    pub workspace_diagnostics: Vec<WorkspaceDiagnostic>,
}

pub(super) struct HealthPipelineRunInputs<M> {
    pub(super) config: ResolvedConfig,
    pub(super) files: Vec<fallow_types::discover::DiscoveredFile>,
    pub(super) modules: M,
    pub(super) config_ms: f64,
    pub(super) discover_ms: f64,
    pub(super) parse_ms: f64,
    pub(super) parse_cpu_ms: f64,
    pub(super) shared_parse: bool,
    pub(super) pre_computed_analysis: Option<DeadCodeAnalysisArtifacts>,
    pub(super) dead_code_results: Option<AnalysisResults>,
    pub(super) styling_artifacts: Option<Arc<StylingAnalysisArtifacts>>,
    pub(super) pre_computed_duplication: Option<DuplicationReport>,
    pub(super) workspaces: Vec<WorkspaceInfo>,
    pub(super) workspace_diagnostics: Vec<WorkspaceDiagnostic>,
}

impl From<HealthPipelineInputs>
    for HealthPipelineRunInputs<Vec<fallow_types::extract::ModuleInfo>>
{
    fn from(input: HealthPipelineInputs) -> Self {
        Self {
            config: input.config,
            files: input.files,
            modules: input.modules,
            config_ms: input.config_ms,
            discover_ms: input.discover_ms,
            parse_ms: input.parse_ms,
            parse_cpu_ms: input.parse_cpu_ms,
            shared_parse: input.shared_parse,
            pre_computed_analysis: input.pre_computed_analysis,
            dead_code_results: input.dead_code_results,
            styling_artifacts: input.styling_artifacts,
            pre_computed_duplication: input.pre_computed_duplication,
            workspaces: input.workspaces,
            workspace_diagnostics: input.workspace_diagnostics,
        }
    }
}

/// Scope inputs the CLI resolves before calling the engine.
///
/// The engine no longer fetches changed files, workspace roots, the shared diff
/// index, or the CODEOWNERS-backed grouping resolver itself: those touch CLI
/// state (the shared-diff `OnceLock`, CODEOWNERS parsing, workspace discovery
/// error rendering), so the CLI resolves them and threads them in here.
pub struct HealthScopeInputs<'a, R> {
    /// Files changed since the requested git ref, when the run is diff-scoped.
    pub changed_files: Option<FxHashSet<PathBuf>>,
    /// Diff index scoping findings to changed lines.
    pub diff_index: Option<&'a DiffIndex>,
    /// Workspace roots limiting the analysis scope.
    pub ws_roots: Option<Vec<PathBuf>>,
    /// Grouping resolver for `--group-by` output; `None` disables grouping.
    pub group_resolver: Option<R>,
}

pub(super) struct HealthPipelineTimings {
    pub(super) config: f64,
    pub(super) discover: f64,
    pub(super) parse: f64,
    /// Summed parse CPU time across rayon workers; `0.0` when parse was reused.
    pub(super) parse_cpu: f64,
    /// True when discover + parse were reused from the upstream check pass.
    pub(super) shared_parse: bool,
}

impl HealthPipelineTimings {
    pub(super) fn into_base_input(self, complexity_ms: f64) -> HealthTimingBaseInput {
        HealthTimingBaseInput {
            config_ms: self.config,
            discover_ms: self.discover,
            parse_ms: self.parse,
            parse_cpu_ms: self.parse_cpu,
            complexity_ms,
            shared_parse: self.shared_parse,
        }
    }
}

pub(super) struct HealthScope<'a, R> {
    pub(super) max_cyclomatic: u16,
    pub(super) max_cognitive: u16,
    pub(super) max_crap: f64,
    pub(super) enforce_crap: bool,
    pub(super) ignore_set: globset::GlobSet,
    pub(super) changed_files: Option<FxHashSet<PathBuf>>,
    pub(super) diff_index: Option<&'a DiffIndex>,
    pub(super) ws_roots: Option<Vec<PathBuf>>,
    pub(super) group_resolver: Option<R>,
    pub(super) file_paths: FxHashMap<crate::discover::FileId, &'a PathBuf>,
}

pub(super) struct HealthTimingBaseInput {
    pub(super) config_ms: f64,
    pub(super) discover_ms: f64,
    pub(super) parse_ms: f64,
    pub(super) parse_cpu_ms: f64,
    pub(super) complexity_ms: f64,
    pub(super) shared_parse: bool,
}
