//! The files that a test file, a runner setup file or a runner config module
//! reaches through static imports. The result cache of a mutation run keys
//! on their content.
//!
//! The traversal follows each graph edge and ignores `vi.mock`. The result is
//! a superset of the files that a test loads, so a change to a replaced
//! module also changes the key. Dynamic imports that the graph does not
//! resolve, and files that a test reads, are not in the result.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use fallow_types::discover::FileId;
use rustc_hash::FxHashMap;

use super::RUNNER_CONFIG_MODULES;
use crate::graph::ModuleGraph;
use crate::module_graph::relative_key_path;

/// The reach of the test files of a plan. Each test file maps to indices
/// into one shared, sorted file list, so a large project does not repeat
/// each path for each test file.
#[derive(Debug, Default)]
pub(super) struct TestReach {
    pub(super) files: Vec<String>,
    pub(super) reach: BTreeMap<String, Vec<u32>>,
}

/// The graph indices that `starts` reach, including `starts`. `seen` must
/// have one `false` slot for each module, and is `false` again on return.
fn reached(graph: &ModuleGraph, starts: &[FileId], seen: &mut [bool]) -> Vec<usize> {
    let mut visited: Vec<usize> = Vec::new();
    for &start in starts {
        let index = start.0 as usize;
        if index < seen.len() && !seen[index] {
            seen[index] = true;
            visited.push(index);
        }
    }
    let mut next = 0;
    while next < visited.len() {
        let file = FileId(u32::try_from(visited[next]).unwrap_or(u32::MAX));
        next += 1;
        for target in graph.edges_for(file) {
            let index = target.0 as usize;
            if index < seen.len() && !seen[index] {
                seen[index] = true;
                visited.push(index);
            }
        }
    }
    for &index in &visited {
        seen[index] = false;
    }
    visited
}

/// The root-relative path of each graph module, by graph index.
fn relative_paths(graph: &ModuleGraph, root: &Path) -> Vec<String> {
    graph
        .modules
        .iter()
        .map(|node| relative_key_path(&node.path, root))
        .collect()
}

/// The reach of each test file of `test_files`, without the test file.
pub(super) fn test_reach(graph: &ModuleGraph, root: &Path, test_files: &[String]) -> TestReach {
    if test_files.is_empty() {
        return TestReach::default();
    }
    let paths = relative_paths(graph, root);
    let wanted: BTreeSet<&str> = test_files.iter().map(String::as_str).collect();
    let ids: FxHashMap<&str, usize> = paths
        .iter()
        .enumerate()
        .filter(|(_, path)| wanted.contains(path.as_str()))
        .map(|(index, path)| (path.as_str(), index))
        .collect();
    let mut seen = vec![false; graph.modules.len()];
    let per_test: Vec<(&String, Vec<usize>)> = test_files
        .iter()
        .map(|test| {
            let reach = ids.get(test.as_str()).map_or_else(Vec::new, |&index| {
                let start = FileId(u32::try_from(index).unwrap_or(u32::MAX));
                let mut reach = reached(graph, &[start], &mut seen);
                reach.retain(|&other| other != index);
                reach
            });
            (test, reach)
        })
        .collect();
    let union: BTreeSet<&str> = per_test
        .iter()
        .flat_map(|(_, reach)| reach.iter().map(|&index| paths[index].as_str()))
        .collect();
    let files: Vec<String> = union.iter().map(|&path| path.to_owned()).collect();
    let position: FxHashMap<&str, u32> = union
        .iter()
        .enumerate()
        .map(|(position, &path)| (path, u32::try_from(position).unwrap_or(u32::MAX)))
        .collect();
    let reach = per_test
        .into_iter()
        .map(|(test, indices)| {
            let mut positions: Vec<u32> = indices
                .iter()
                .filter_map(|&index| position.get(paths[index].as_str()).copied())
                .collect();
            positions.sort_unstable();
            (test.clone(), positions)
        })
        .collect();
    TestReach { files, reach }
}

/// The graph file of a setup path: the same path, or the same path with a
/// source extension.
fn setup_module(graph: &ModuleGraph, resolved: &Path) -> Option<FileId> {
    graph
        .modules
        .iter()
        .find(|node| node.path == resolved || node.path.with_extension("") == resolved)
        .map(|node| node.file_id)
}

/// The graph files of the runner config modules at `root`.
fn config_modules(graph: &ModuleGraph, root: &Path) -> Vec<FileId> {
    let configs: Vec<PathBuf> = RUNNER_CONFIG_MODULES
        .iter()
        .map(|name| root.join(name))
        .collect();
    graph
        .modules
        .iter()
        .filter(|node| configs.contains(&node.path))
        .map(|node| node.file_id)
        .collect()
}

/// The setup files, the runner config modules at the root, and their
/// reach, sorted. A setup path that matches no graph file adds nothing.
pub(super) fn support_files(
    graph: &ModuleGraph,
    root: &Path,
    setup_files: &[PathBuf],
) -> Vec<String> {
    let mut starts: Vec<FileId> = setup_files
        .iter()
        .map(|path| {
            if path.is_absolute() {
                path.clone()
            } else {
                root.join(path)
            }
        })
        .filter_map(|resolved| setup_module(graph, &resolved))
        .collect();
    starts.extend(config_modules(graph, root));
    if starts.is_empty() {
        return Vec::new();
    }
    let mut seen = vec![false; graph.modules.len()];
    let files: BTreeSet<String> = reached(graph, &starts, &mut seen)
        .into_iter()
        .filter_map(|index| graph.modules.get(index))
        .map(|node| relative_key_path(&node.path, root))
        .collect();
    files.into_iter().collect()
}
