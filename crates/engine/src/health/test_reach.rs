//! Test reachability for production-mode health runs.
//!
//! Production mode keeps test, story and dev files out of discovery, so the
//! analysis graph has no test entry points. The static coverage estimate
//! behind CRAP and coverage gaps then marks every function as untested. This
//! module builds a graph of the full project, test files included, and maps
//! the analysis file ids onto it. Findings still come from the production
//! file set only.
//!
//! A health run builds this graph at most one time. File scoring and the
//! runtime coverage signal index use the same graph.

use std::path::Path;

use fallow_config::ResolvedConfig;
use rustc_hash::FxHashMap;

use crate::discover::FileId;
use crate::module_graph::{RetainedModuleGraph, StaticTestCoverage};
use crate::session::AnalysisSession;

/// Graph of the full project plus a map from analysis file ids to its ids.
///
/// The type is opaque. Downstream crates get it through the health seams and
/// give it back to engine helpers such as
/// [`crate::module_graph::module_value_exports_with_test_reach`].
#[derive(Debug)]
pub struct TestReachGraph {
    graph: RetainedModuleGraph,
    file_map: FxHashMap<FileId, FileId>,
}

impl TestReachGraph {
    /// Build the graph when `config` is a production run. Returns `None`
    /// outside production mode, and also when the full-project analysis
    /// fails. In that case the caller keeps the analysis graph.
    fn for_production(
        config: &ResolvedConfig,
        file_paths: &FxHashMap<FileId, &std::path::PathBuf>,
    ) -> Option<Self> {
        if !config.production {
            return None;
        }
        let mut full_config = config.clone();
        full_config.production = false;
        let artifacts = AnalysisSession::from_resolved_config(full_config)
            .and_then(|session| session.analyze_dead_code_retaining_files(false, true))
            .inspect_err(|error| {
                tracing::warn!(
                    "cannot build the full-project graph for production test reachability: {error}"
                );
            })
            .ok()?;
        let graph = artifacts.graph?;
        let files = artifacts.files?;
        let ids_by_path: FxHashMap<&Path, FileId> = files
            .iter()
            .map(|file| (file.path.as_path(), file.id))
            .collect();
        let file_map = file_paths
            .iter()
            .filter_map(|(id, path)| {
                ids_by_path
                    .get(path.as_path())
                    .map(|full_id| (*id, *full_id))
            })
            .collect();
        Some(Self { graph, file_map })
    }

    pub(crate) fn static_test_coverage(&self) -> StaticTestCoverage<'_> {
        StaticTestCoverage::mapped(self.graph.as_graph(), &self.file_map)
    }
}

/// The test reachability graph of one health run, built on first use.
///
/// File scoring and the runtime coverage seam share one instance. The first
/// call to [`SharedTestReach::get`] builds the graph, and later calls reuse
/// it. A run that does not call `get` builds nothing. Both callers run while
/// git churn loads on a different thread, so the build overlaps that load.
pub struct SharedTestReach<'a> {
    config: &'a ResolvedConfig,
    file_paths: &'a FxHashMap<FileId, &'a std::path::PathBuf>,
    graph: std::cell::OnceCell<Option<TestReachGraph>>,
}

impl<'a> SharedTestReach<'a> {
    pub const fn new(
        config: &'a ResolvedConfig,
        file_paths: &'a FxHashMap<FileId, &'a std::path::PathBuf>,
    ) -> Self {
        Self {
            config,
            file_paths,
            graph: std::cell::OnceCell::new(),
        }
    }

    /// The graph, built on the first call. `None` outside production mode.
    pub fn get(&self) -> Option<&TestReachGraph> {
        self.graph
            .get_or_init(|| TestReachGraph::for_production(self.config, self.file_paths))
            .as_ref()
    }
}

/// Build the set of export names that have at least one test-reachable reference.
///
/// This is the per-function signal: if an export named "foo" has a reference from
/// a test-reachable module, the function "foo" is considered directly tested.
pub(super) fn build_test_referenced_exports(
    file_id: FileId,
    exports: &[fallow_graph::graph::ExportSymbol],
    test_coverage: StaticTestCoverage<'_>,
) -> rustc_hash::FxHashSet<String> {
    let mut set = rustc_hash::FxHashSet::default();
    for export in exports {
        if export.is_type_only {
            continue;
        }
        let has_test_ref = test_coverage.covers_any_reference(file_id, export);
        if has_test_ref {
            set.insert(export.name.to_string());
        }
    }
    set
}
