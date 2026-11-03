//! Package usage credits that the graph build does not record from import
//! edges: workspace package imports, package path references, and
//! `node_modules/.bin` paths in source.

use rustc_hash::FxHashSet;

use crate::discover::FileId;
use crate::{extract, graph, resolve, scripts};

fn record_graph_package_usage(
    graph: &mut graph::ModuleGraph,
    package_name: &str,
    file_id: FileId,
    is_type_only: bool,
) {
    graph
        .package_usage
        .entry(package_name.to_owned())
        .or_default()
        .push(file_id);
    if is_type_only {
        graph
            .type_only_package_usage
            .entry(package_name.to_owned())
            .or_default()
            .push(file_id);
    }
}

fn workspace_package_name<'a>(
    source: &str,
    workspace_names: &FxHashSet<&'a str>,
) -> Option<&'a str> {
    if !resolve::is_bare_specifier(source) {
        return None;
    }
    let package_name = resolve::extract_package_name(source);
    workspace_names.get(package_name.as_str()).copied()
}

pub fn credit_workspace_package_usage(
    graph: &mut graph::ModuleGraph,
    resolved: &[resolve::ResolvedModule],
    workspaces: &[fallow_config::WorkspaceInfo],
) {
    if workspaces.is_empty() {
        return;
    }

    let workspace_names: FxHashSet<&str> = workspaces.iter().map(|ws| ws.name.as_str()).collect();
    for module in resolved {
        for import in module.all_resolved_imports() {
            if matches!(
                import.target,
                resolve::ResolveResult::InternalModule(_)
                    | resolve::ResolveResult::CommonJsInternalModule(_)
            ) && let Some(package_name) =
                workspace_package_name(&import.info.source, &workspace_names)
            {
                record_graph_package_usage(
                    graph,
                    package_name,
                    module.file_id,
                    import.info.is_type_only,
                );
            }
        }

        for re_export in &module.re_exports {
            if matches!(re_export.target, resolve::ResolveResult::InternalModule(_))
                && let Some(package_name) =
                    workspace_package_name(&re_export.info.source, &workspace_names)
            {
                record_graph_package_usage(
                    graph,
                    package_name,
                    module.file_id,
                    re_export.info.is_type_only,
                );
            }
        }
    }
}

/// Credit the package behind each package path and module augmentation in
/// source. A module in `excluded` gives no credit. See [`crate::production_scope`].
pub fn credit_package_path_references(
    graph: &mut graph::ModuleGraph,
    modules: &[extract::ModuleInfo],
    excluded: &FxHashSet<FileId>,
) {
    for module in modules.iter().filter(|m| !excluded.contains(&m.file_id)) {
        for package_name in &module.package_path_references {
            record_graph_package_usage(graph, package_name, module.file_id, false);
        }
        // A module augmentation credits the package as a type-only use. The
        // unlisted-dependency check reports a package only at a resolved
        // import site, so an undeclared augmented package stays unreported.
        for package_name in &module.type_package_references {
            record_graph_package_usage(graph, package_name, module.file_id, true);
        }
    }
}

/// Credit the package behind each `node_modules/.bin/<name>` path in source.
///
/// The persisted graph cache does not hold these credits: the bin map comes
/// from `node_modules`, which the cache manifest does not track. Each run
/// applies them to a built or a loaded graph.
pub fn credit_bin_path_references(
    graph: &mut graph::ModuleGraph,
    modules: &[extract::ModuleInfo],
    binaries: &scripts::DependencyBinaries,
    excluded: &FxHashSet<FileId>,
) {
    for module in modules.iter().filter(|m| !excluded.contains(&m.file_id)) {
        for binary in &module.bin_path_references {
            if let Some(package_name) = binaries.package_for(binary) {
                record_graph_package_usage(graph, &package_name, module.file_id, false);
            }
        }
    }
}
