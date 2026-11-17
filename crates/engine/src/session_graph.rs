//! What a dead-code run of an [`AnalysisSession`] keeps of its module graph.

use crate::EngineResult;
use crate::results::{DeadCodeAnalysisArtifacts, SharedDeadCodeAnalysisArtifacts};
use crate::session::AnalysisSession;

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
}
