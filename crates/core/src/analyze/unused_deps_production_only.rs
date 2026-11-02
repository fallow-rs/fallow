//! The type-only dependency rule, which only a `--production` run reports,
//! and the credit that a normal run gives to the `ignoreDependencies` entries
//! that a `--production` run could need.

use rustc_hash::FxHashSet;

use fallow_config::{PackageJson, ResolvedConfig};

use super::{
    RootImporters, TypeOnlyDependency, find_dep_line_in_json, production_exclude_globset,
    read_pkg_json_content,
};
use crate::discover::FileId;
use crate::graph::ModuleGraph;

/// Find production dependencies that are only imported via type-only imports.
///
/// In production mode, `import type { Foo } from 'pkg'` is erased at compile time,
/// meaning the dependency is not needed at runtime. Such dependencies should be
/// moved to devDependencies.
///
/// Only importers that reach the package through the root declaration count,
/// see [`RootImporters`]. A root entry without such an importer is an unused
/// dependency, not a type-only one.
pub fn find_type_only_dependencies(
    graph: &ModuleGraph,
    pkg: &PackageJson,
    config: &ResolvedConfig,
    workspaces: &[fallow_config::WorkspaceInfo],
) -> Vec<TypeOnlyDependency> {
    let root_pkg_path = config.root.join("package.json");
    let root_pkg_content = read_pkg_json_content(&root_pkg_path);
    let workspace_names: FxHashSet<&str> = workspaces.iter().map(|ws| ws.name.as_str()).collect();
    let root_importers = RootImporters::new(graph, config, workspaces);
    let production_peers = root_importers.production_peers(pkg, config);

    let mut type_only_deps = Vec::new();

    for dep in pkg.production_dependency_names() {
        if workspace_names.contains(dep.as_str()) {
            continue;
        }
        if production_peers.contains(&dep) {
            continue;
        }

        let total_count = root_importers.of(&dep).len();
        let type_only_count = root_importers.type_only_of(&dep).len();

        let reported = total_count > 0 && type_only_count == total_count;
        if reported && !config.ignore_dependencies.suppresses(&dep) {
            let line = root_pkg_content
                .as_deref()
                .map_or(1, |c| find_dep_line_in_json(c, &dep));
            type_only_deps.push(TypeOnlyDependency {
                package_name: dep,
                path: root_pkg_path.clone(),
                line,
            });
        }
    }

    type_only_deps
}

/// Credit the `ignoreDependencies` entries that a `--production` run could need.
///
/// A normal run does not run the type-only rule, and tests and non-production
/// scripts make a package used. An entry for a `dependencies` package can then
/// hide no finding here and still hide one in a production run, so the entry is
/// not redundant.
///
/// This reads the normal graph without the files that production discovery
/// excludes. It does not repeat the exemptions of the production rules, such as
/// a peer of a production package: the worst case is one redundant note less,
/// never a wrong note.
pub fn credit_production_ignores(
    graph: &ModuleGraph,
    pkg: &PackageJson,
    config: &ResolvedConfig,
    workspaces: &[fallow_config::WorkspaceInfo],
) {
    let ignore_deps = &config.ignore_dependencies;
    let unused_on = config.rules.unused_dependencies != fallow_config::Severity::Off;
    let type_only_on = config.rules.type_only_dependencies != fallow_config::Severity::Off;
    if config.production || ignore_deps.is_empty() || !(unused_on || type_only_on) {
        return;
    }
    let Some(excluded_globs) = production_exclude_globset() else {
        return;
    };

    let in_production_run = |id: &FileId| {
        graph.modules.get(id.0 as usize).is_some_and(|module| {
            let relative = module
                .path
                .strip_prefix(&config.root)
                .unwrap_or(&module.path);
            !excluded_globs.is_match(relative)
        })
    };
    let root_importers = RootImporters::new(graph, config, workspaces);

    for dep in pkg.production_dependency_names() {
        if !ignore_deps.is_ignored(&dep) {
            continue;
        }
        let imports = root_importers.of(&dep);
        let imports = imports.iter().filter(|id| in_production_run(id)).count();
        let type_only = root_importers.type_only_of(&dep);
        let type_only = type_only.iter().filter(|id| in_production_run(id)).count();
        let may_be_unused = unused_on && imports == 0;
        let may_be_type_only = type_only_on && imports > 0 && imports == type_only;
        if may_be_unused || may_be_type_only {
            let _ = ignore_deps.suppresses(&dep);
        }
    }

    if !unused_on {
        return;
    }
    // The type-only rule reads only the root manifest, and the unused rule reads
    // only the workspaces that the source walk reaches.
    let walked = fallow_config::source_walked_workspaces(&config.root, workspaces);
    for (index, manifest) in root_importers.manifests.iter().enumerate() {
        if !walked
            .iter()
            .any(|workspace| workspace.root == manifest.root)
        {
            continue;
        }
        for dep in &manifest.production {
            if !ignore_deps.is_ignored(dep) {
                continue;
            }
            let imported_here = graph.package_usage.get(dep).is_some_and(|ids| {
                ids.iter().any(|id| {
                    root_importers.ownership.workspace_index_for_file(*id) == Some(index)
                        && in_production_run(id)
                })
            });
            if !imported_here {
                let _ = ignore_deps.suppresses(dep);
            }
        }
    }
}
