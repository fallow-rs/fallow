//! Test reachability for production-mode health runs.
//!
//! Production mode keeps test, story and dev files out of discovery, so the
//! analysis graph has no test entry points. The static coverage estimate
//! behind CRAP and coverage gaps then marks every function as untested. This
//! module builds a graph of the full project, test files included, and maps
//! the analysis file ids onto it. Findings still come from the production
//! file set only.

use std::path::Path;

use fallow_config::ResolvedConfig;
use rustc_hash::FxHashMap;

use crate::discover::FileId;
use crate::module_graph::{RetainedModuleGraph, StaticTestCoverage};
use crate::session::AnalysisSession;

/// Graph of the full project plus a map from analysis file ids to its ids.
pub(super) struct TestReachGraph {
    graph: RetainedModuleGraph,
    file_map: FxHashMap<FileId, FileId>,
}

impl TestReachGraph {
    /// Build the graph when `config` is a production run. Returns `None`
    /// outside production mode, and also when the full-project analysis
    /// fails. In that case the caller keeps the analysis graph.
    pub(super) fn for_production(
        config: &ResolvedConfig,
        file_paths: &FxHashMap<FileId, &std::path::PathBuf>,
    ) -> Option<Self> {
        if !config.production {
            return None;
        }
        let mut full_config = config.clone();
        full_config.production = false;
        let session = AnalysisSession::from_resolved_config(full_config).ok()?;
        let artifacts = session
            .analyze_dead_code_retaining_files(false, true)
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

    pub(super) fn static_test_coverage(&self) -> StaticTestCoverage<'_> {
        StaticTestCoverage::mapped(self.graph.as_graph(), &self.file_map)
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
