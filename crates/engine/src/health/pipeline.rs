//! Health pipeline carrier types shared by the engine health executor.

use std::path::PathBuf;

use fallow_config::{ResolvedConfig, WorkspaceInfo};
use fallow_output::DiffIndex;
use fallow_types::workspace::WorkspaceDiagnostic;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::{
    duplicates::DuplicationReport,
    results::{AnalysisResults, DeadCodeAnalysisArtifacts},
};

use super::StylingAnalysisArtifacts;

/// Discovery / parse inputs the CLI resolves before calling the engine.
pub struct HealthPipelineInputs {
    pub config: ResolvedConfig,
    pub files: Vec<fallow_types::discover::DiscoveredFile>,
    pub modules: Vec<fallow_types::extract::ModuleInfo>,
    /// Pre-parse pipeline timings (config / discover / parse milliseconds).
    pub config_ms: f64,
    pub discover_ms: f64,
    pub parse_ms: f64,
    pub parse_cpu_ms: f64,
    /// True when discover + parse were reused from the upstream check pass.
    pub shared_parse: bool,
    pub pre_computed_analysis: Option<DeadCodeAnalysisArtifacts>,
    pub dead_code_results: Option<AnalysisResults>,
    pub styling_artifacts: Option<StylingAnalysisArtifacts>,
    pub pre_computed_duplication: Option<DuplicationReport>,
    pub workspaces: Vec<WorkspaceInfo>,
    pub workspace_diagnostics: Vec<WorkspaceDiagnostic>,
}

/// Scope inputs the CLI resolves before calling the engine.
///
/// The engine no longer fetches changed files, workspace roots, the shared diff
/// index, or the CODEOWNERS-backed grouping resolver itself: those touch CLI
/// state (the shared-diff `OnceLock`, CODEOWNERS parsing, workspace discovery
/// error rendering), so the CLI resolves them and threads them in here.
pub struct HealthScopeInputs<'a, R> {
    pub changed_files: Option<FxHashSet<PathBuf>>,
    pub diff_index: Option<&'a DiffIndex>,
    pub ws_roots: Option<Vec<PathBuf>>,
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
