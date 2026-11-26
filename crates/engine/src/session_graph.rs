//! What a dead-code run of an [`AnalysisSession`] keeps of its module graph.

use fallow_types::cache_rejection::CacheRejection;
use fallow_types::extract::ModuleInfo;

use crate::EngineResult;
use crate::core_backend;
use crate::module_graph::RetainedModuleGraph;
use crate::results::{DeadCodeAnalysisArtifacts, SharedDeadCodeAnalysisArtifacts};
use crate::session::{AnalysisSession, SharedParsedModules};

/// What a dead-code run keeps of its module graph.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GraphChoice {
    /// Drop the graph after the analysis.
    Drop,
    /// Keep the graph, and collect the trace provenance (with the plugin
    /// export credits) and the pipeline timings.
    WithTrace,
    /// Keep the graph without the trace provenance and the timings.
    GraphOnly,
}

/// The choice of a caller that passes the `retain_graph` flag.
impl From<bool> for GraphChoice {
    fn from(retain_graph: bool) -> Self {
        if retain_graph {
            Self::WithTrace
        } else {
            Self::Drop
        }
    }
}

impl GraphChoice {
    pub const fn keeps_graph(self) -> bool {
        matches!(self, Self::WithTrace | Self::GraphOnly)
    }

    /// True when the run collects the trace provenance and the timings.
    pub const fn traces(self) -> bool {
        matches!(self, Self::WithTrace)
    }
}

impl AnalysisSession {
    /// Run dead-code analysis and keep the module graph, without the trace
    /// provenance and pipeline timings that
    /// [`Self::analyze_dead_code_with_artifacts`] also collects when it keeps
    /// the graph. `fallow fix` reads graph facts such as the named
    /// re-exports and needs no trace data.
    ///
    /// # Errors
    ///
    /// Returns an error if parsing or analysis fails.
    pub fn analyze_dead_code_keeping_graph(&self) -> EngineResult<DeadCodeAnalysisArtifacts> {
        self.analyze_dead_code_with_reuse_artifacts(false, GraphChoice::GraphOnly, false)
            .map(SharedDeadCodeAnalysisArtifacts::into_owned)
    }

    /// Build the module graph of this session without the dead-code
    /// detectors. The graph has the reachability flags and the export
    /// references, and it uses the persisted graph cache of the session.
    ///
    /// # Errors
    ///
    /// Returns an error when the prelude fails or the caller cancels.
    pub(crate) fn build_module_graph(&self) -> EngineResult<RetainedModuleGraph> {
        self.ensure_not_cancelled("parsing")?;
        let SharedParsedModules { modules, .. } =
            self.parse_modules(false, self.cancellation.as_deref());
        self.ensure_not_cancelled("the module graph")?;
        let prelude =
            core_backend::prepare_dead_code_backend_prelude(&self.config, &self.discovery)?;
        let entry_points = core_backend::discover_dead_code_entry_points(&prelude);
        let (_resolved, graph, _rejection) =
            resolve_or_build_dead_code_graph(&prelude, &entry_points, &modules);
        prelude.finish();
        Ok(graph.graph)
    }
}

/// Reuse the persisted module graph, or rebuild it and carry the reason the
/// persisted one was refused.
///
/// The reason is the third element rather than a discarded `Option`: a warm run
/// that paid for a multi-megabyte decode and reused none of it is the case the
/// perf table exists to explain, and every engine-backed command reaches the
/// pipeline through here.
pub fn resolve_or_build_dead_code_graph(
    prelude: &core_backend::DeadCodeBackendPrelude,
    entry_points: &core_backend::DeadCodeEntryPoints,
    modules: &[ModuleInfo],
) -> (
    core_backend::DeadCodeResolvedModules,
    core_backend::DeadCodeGraphRun,
    Option<CacheRejection>,
) {
    let rejection =
        match core_backend::try_load_dead_code_graph_cache(prelude, entry_points, modules) {
            Ok(reused) => return reused,
            Err(rejection) => rejection,
        };

    let mut resolved = core_backend::resolve_dead_code_imports(prelude, modules);
    let graph =
        core_backend::build_dead_code_graph(prelude, &mut resolved.project, entry_points, modules);
    (resolved, graph, rejection)
}
