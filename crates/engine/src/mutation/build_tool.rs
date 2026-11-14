//! The files that only tool config files reach.

use std::path::Path;

use fallow_types::discover::FileId;
use rustc_hash::FxHashSet;

use super::FrameworkConfigExports;
use crate::graph::ModuleGraph;
use crate::module_graph::relative_key_path;

/// The reach facts of the tool config files.
#[derive(Default)]
pub(super) struct BuildToolReach {
    /// The files that an import path from a tool config file reaches and
    /// that no other entry point reaches. The tool config files themselves
    /// are not in the set.
    pub(super) only: FxHashSet<FileId>,
    /// The tool config files that a test, a setup file or an application
    /// entry reaches. An example is an Angular `src/app/app.config.ts` that
    /// `src/main.ts` imports. That file is application code.
    pub(super) shared_configs: FxHashSet<FileId>,
}

/// Splits the files that tool config files reach from the files that other
/// entry points reach. A file that a test, a setup file or an application
/// entry reaches can run outside the build tool.
pub(super) fn build_tool_reach(
    graph: &ModuleGraph,
    root: &Path,
    exports: &FrameworkConfigExports,
) -> BuildToolReach {
    let config_files: FxHashSet<FileId> = graph
        .modules
        .iter()
        .filter(|node| exports.is_config_file(&relative_key_path(&node.path, root)))
        .map(|node| node.file_id)
        .collect();
    if config_files.is_empty() {
        return BuildToolReach::default();
    }
    let other_roots = graph
        .entry_points
        .iter()
        .chain(&graph.runtime_entry_points)
        .chain(&graph.test_entry_points)
        .copied()
        .filter(|file_id| !config_files.contains(file_id));
    let reached_elsewhere = reach(graph, other_roots);
    let mut only = reach(graph, config_files.iter().copied());
    only.retain(|file_id| !config_files.contains(file_id) && !reached_elsewhere.contains(file_id));
    let shared_configs = config_files
        .into_iter()
        .filter(|file_id| reached_elsewhere.contains(file_id))
        .collect();
    BuildToolReach {
        only,
        shared_configs,
    }
}

/// The files that the import paths from `roots` reach, the roots included.
fn reach(graph: &ModuleGraph, roots: impl Iterator<Item = FileId>) -> FxHashSet<FileId> {
    let mut seen: FxHashSet<FileId> = FxHashSet::default();
    let mut stack: Vec<FileId> = roots.filter(|&file_id| seen.insert(file_id)).collect();
    while let Some(file_id) = stack.pop() {
        for target in graph.edges_for(file_id) {
            if seen.insert(target) {
                stack.push(target);
            }
        }
    }
    seen
}
