//! The module graph build of an analysis run, with the entry files that
//! `includeEntryExports` covers.

pub mod import_elision_configs;
mod visibility_scope;

use std::path::Path;

use rustc_hash::{FxHashMap, FxHashSet};

use crate::discover::{CategorizedEntryPoints, DiscoveredFile, FileId};
use crate::graph::{ModuleGraph, ReachabilityRoots};
use crate::resolve::{ResolvedModule, ResolvedProject};
use fallow_config::{ResolvedConfig, WorkspaceInfo};
use fallow_types::extract::ImportedName;

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

/// The fixture-only entry files whose exports need a consumer: a module
/// outside the fixture directories imports a name from the file, or a fixture
/// file that is already in the set does.
///
/// A plugin fixture pattern makes each file below `fixtures/` an entry point,
/// so a test can load it by path. A file that no module imports is such input
/// data, and each export of it stays used. A file that a test imports by name
/// is a module of the project, so an export of it with no importer is unused.
/// Only a static ES import that names a binding and a re-export count. A
/// side-effect import names no export. A file that a dynamic import, an
/// import pattern or a `require()` loads is left out, together with the
/// files that only it imports, because the graph does not follow the member
/// reads of such a module object.
pub fn imported_fixture_entries(
    config: &ResolvedConfig,
    project: &ResolvedProject,
    entry_points: &CategorizedEntryPoints,
    files: &[DiscoveredFile],
) -> FxHashSet<FileId> {
    let fixture_paths = entry_points.fixture_only();
    if fixture_paths.is_empty() {
        return FxHashSet::default();
    }
    let path_to_id: FxHashMap<&Path, FileId> = files
        .iter()
        .map(|file| (file.path.as_path(), file.id))
        .collect();
    let untracked = crate::graph::untracked_load_targets(&project.modules);
    let fixtures: FxHashSet<FileId> = fixture_paths
        .iter()
        .filter_map(|path| path_to_id.get(path.as_path()).copied())
        .filter(|id| !untracked.contains(id))
        .collect();
    if fixtures.is_empty() {
        return FxHashSet::default();
    }
    let modules: FxHashMap<FileId, &ResolvedModule> = project
        .modules
        .iter()
        .map(|module| (module.file_id, module))
        .collect();
    let mut checked = FxHashSet::default();
    let mut queue = Vec::new();
    let mut visit = |module: &ResolvedModule, queue: &mut Vec<FileId>| {
        for target in named_import_targets(module) {
            if fixtures.contains(&target) && checked.insert(target) {
                queue.push(target);
            }
        }
    };
    for module in &project.modules {
        if !fixtures.contains(&module.file_id)
            && !crate::discover::is_below_fixture_dir(&module.path, &config.root)
        {
            visit(module, &mut queue);
        }
    }
    while let Some(id) = queue.pop() {
        if let Some(module) = modules.get(&id) {
            visit(module, &mut queue);
        }
    }
    checked
}

/// The project files that `module` imports a binding from with a static ES
/// import, or re-exports names from.
fn named_import_targets(module: &ResolvedModule) -> impl Iterator<Item = FileId> + '_ {
    let imports = module
        .resolved_imports
        .iter()
        .filter(|import| {
            !import.target.is_commonjs_require()
                && !matches!(import.info.imported_name, ImportedName::SideEffect)
        })
        .filter_map(|import| import.target.internal_file_id());
    let re_exports = module
        .re_exports
        .iter()
        .filter_map(|re_export| re_export.target.internal_file_id());
    imports.chain(re_exports)
}

/// Build the module graph from the resolved modules of `project`.
///
/// The build first clears each JSDoc visibility tag that `keepExportTags`
/// does not keep (see [`visibility_scope`]).
pub fn build_module_graph(
    config: &ResolvedConfig,
    project: &mut ResolvedProject,
    entry_points: &CategorizedEntryPoints,
    files: &[DiscoveredFile],
    workspaces: &[WorkspaceInfo],
    unseeded_declaration_files: &FxHashSet<FileId>,
) -> ModuleGraph {
    visibility_scope::apply(&mut project.modules, &config.keep_export_tags);
    let mut consumer_checked = consumer_checked_entries(config, entry_points, files, workspaces);
    let fixtures = imported_fixture_entries(config, project, entry_points, files);
    consumer_checked.extend(fixtures.iter().copied());
    let mut graph = ModuleGraph::build_with_declaration_seeding(
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
    );
    graph.set_fixture_entries(&fixtures);
    import_elision_configs::read_decorator_metadata_options(&mut graph, &config.root);
    graph
}
