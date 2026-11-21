//! The module graph build of an analysis run, with the entry files that
//! `includeEntryExports` covers.

use std::path::Path;

use rustc_hash::{FxHashMap, FxHashSet};

use crate::discover::{CategorizedEntryPoints, DiscoveredFile, FileId};
use crate::graph::{ModuleGraph, ReachabilityRoots};
use crate::resolve::ResolvedProject;
use fallow_config::{ResolvedConfig, WorkspaceInfo};

/// The entry points whose exports need a consumer, because
/// `includeEntryExports` is on for their path.
///
/// The graph seeds every declaration file as an entry point, so a
/// declaration file is a member too when the option covers it.
///
/// The graph build runs before the dead-code pass binds
/// `workspaces.settings`, so this function binds the keys first. The bind is
/// idempotent.
pub fn consumer_checked_entries(
    config: &ResolvedConfig,
    entry_points: &CategorizedEntryPoints,
    files: &[DiscoveredFile],
    workspaces: &[WorkspaceInfo],
) -> FxHashSet<FileId> {
    if !config.include_entry_exports && !config.workspace_settings.any_include_entry_exports() {
        return FxHashSet::default();
    }
    config.workspace_settings.bind(&config.root, workspaces);
    let path_to_id: FxHashMap<&Path, FileId> = files
        .iter()
        .map(|file| (file.path.as_path(), file.id))
        .collect();
    let entry_ids = entry_points.all.iter().filter_map(|entry| {
        let id = path_to_id.get(entry.path.as_path()).copied().or_else(|| {
            dunce::canonicalize(&entry.path)
                .ok()
                .and_then(|path| path_to_id.get(path.as_path()).copied())
        })?;
        config.include_entry_exports_for(&entry.path).then_some(id)
    });
    let declaration_ids = files
        .iter()
        .filter(|file| {
            crate::graph::is_declaration_file_path(&file.path)
                && config.include_entry_exports_for(&file.path)
        })
        .map(|file| file.id);
    entry_ids.chain(declaration_ids).collect()
}

/// Build the module graph from the resolved modules of `project`.
pub fn build_module_graph(
    config: &ResolvedConfig,
    project: &ResolvedProject,
    entry_points: &CategorizedEntryPoints,
    files: &[DiscoveredFile],
    workspaces: &[WorkspaceInfo],
    unseeded_declaration_files: &FxHashSet<FileId>,
) -> ModuleGraph {
    let consumer_checked = consumer_checked_entries(config, entry_points, files, workspaces);
    ModuleGraph::build_with_declaration_seeding(
        &project.modules,
        &project.replaced_module_targets,
        &ReachabilityRoots {
            all: &entry_points.all,
            runtime: &entry_points.runtime,
            test: &entry_points.test,
            consumer_checked: &consumer_checked,
        },
        files,
        unseeded_declaration_files,
    )
}
