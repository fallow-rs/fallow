//! The `publicPackages` filter: drop the dead-code findings on the public
//! surface of the selected workspace packages.

use rustc_hash::FxHashSet;

use fallow_config::ResolvedConfig;

use super::{
    add_exportless_package_source_indexes, add_package_public_api_entry_points,
    graph_file_ids_by_path,
};
use crate::discover::FileId;
use crate::graph::{ExportNamespace, ModuleGraph};
use crate::results::AnalysisResults;

/// Drop the findings on the public surface of the `publicPackages` workspaces.
///
/// The surface is each entry point of a public package plus every declaration
/// that an entry point exports or re-exports, also for a `private: true`
/// manifest. An export on the surface is
/// used, and a class or enum on the surface keeps all of its members. Code in
/// a public package that the surface cannot reach stays reported.
pub(super) fn filter_public_workspace_results(
    graph: &ModuleGraph,
    config: &ResolvedConfig,
    workspaces: &[fallow_config::WorkspaceInfo],
    results: &mut AnalysisResults,
) {
    if config.public_packages.is_empty() || workspaces.is_empty() {
        return;
    }
    let path_to_file_id = graph_file_ids_by_path(graph);
    let canonical_project_root =
        dunce::canonicalize(&config.root).unwrap_or_else(|_| config.root.clone());
    let mut entry_points = FxHashSet::default();
    for workspace in workspaces.iter().filter(|workspace| {
        fallow_config::workspace_is_public(&workspace.name, &config.public_packages)
    }) {
        let Some(mut pkg) = fallow_config::load_dir_package_json(&workspace.root) else {
            continue;
        };
        // `publicPackages` names the package explicitly, so a `private: true`
        // manifest does not remove its surface.
        pkg.private = None;
        add_package_public_api_entry_points(
            &mut entry_points,
            graph,
            &path_to_file_id,
            &workspace.root,
            &pkg,
            &canonical_project_root,
        );
        add_exportless_package_source_indexes(&mut entry_points, graph, &workspace.root, &pkg);
    }
    if entry_points.is_empty() {
        return;
    }
    let surface: FxHashSet<(FileId, String)> = [ExportNamespace::Value, ExportNamespace::Type]
        .into_iter()
        .flat_map(|namespace| graph.public_export_origins_in_namespace(&entry_points, namespace))
        .map(|origin| (origin.file_id(), origin.export_name().to_owned()))
        .collect();
    let on_surface = |path: &std::path::Path, name: &str| {
        path_to_file_id.get(path).is_some_and(|&file_id| {
            entry_points.contains(&file_id) || surface.contains(&(file_id, name.to_owned()))
        })
    };

    results
        .unused_exports
        .retain(|e| !on_surface(&e.export.path, &e.export.export_name));
    results
        .unused_types
        .retain(|e| !on_surface(&e.export.path, &e.export.export_name));
    results
        .unused_enum_members
        .retain(|e| !on_surface(&e.member.path, &e.member.parent_name));
    results
        .unused_class_members
        .retain(|e| !on_surface(&e.member.path, &e.member.parent_name));
    results
        .unused_protected_class_members
        .retain(|e| !on_surface(&e.member.path, &e.member.parent_name));
    results
        .unused_namespace_members
        .retain(|e| !on_surface(&e.member.path, &e.member.parent_name));
}
