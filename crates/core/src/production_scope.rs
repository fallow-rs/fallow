//! Production mode leaves out the files that only a user `support` entry rule
//! seeds (issue #4068).
//!
//! A user `framework[]` entry (or plugin file) with the `support` role
//! declares dev tooling. Entry discovery keeps the files that only such a rule
//! matches out of the entry points and records them in
//! [`CategorizedEntryPoints::production_excluded`]. This module makes them take
//! no part in the analysis, like a test file in production mode:
//!
//! - their resolved modules become inert, so their imports credit no export,
//!   file or dependency;
//! - the findings that they own are hidden.
//!
//! A candidate that another module imports stays a normal module, because a
//! file that the analyzed code loads is not dev-only.
//!
//! Resolution itself does not change. The graph-cache resolver payload is
//! written before [`neutralize`] runs, and the persisted graph is keyed on the
//! entry-point set, which holds these files in a normal run only.

use std::path::Path;
use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};

use crate::discover::{CategorizedEntryPoints, DiscoveredFile, FileId};
use crate::resolve::ResolvedModule;
use crate::results::AnalysisResults;

/// The files of this run that take no part in the analysis.
///
/// Empty outside production mode. The result is the same before and after
/// [`neutralize`], because only modules outside the set seed the import walk.
pub fn excluded_file_ids(
    entry_points: &CategorizedEntryPoints,
    files: &[DiscoveredFile],
    modules: &[ResolvedModule],
) -> FxHashSet<FileId> {
    let candidates = entry_points.production_excluded();
    if candidates.is_empty() {
        return FxHashSet::default();
    }
    let candidate_paths: FxHashSet<&Path> = candidates.iter().map(AsRef::as_ref).collect();
    let mut excluded: FxHashSet<FileId> = files
        .iter()
        .filter(|file| candidate_paths.contains(file.path.as_path()))
        .map(|file| file.id)
        .collect();
    if excluded.is_empty() {
        return excluded;
    }

    let candidate_modules: FxHashMap<FileId, &ResolvedModule> = modules
        .iter()
        .filter(|module| excluded.contains(&module.file_id))
        .map(|module| (module.file_id, module))
        .collect();
    let mut imported: Vec<FileId> = modules
        .iter()
        .filter(|module| !excluded.contains(&module.file_id))
        .flat_map(internal_targets)
        .filter(|target| excluded.contains(target))
        .collect();
    while let Some(file_id) = imported.pop() {
        if !excluded.remove(&file_id) {
            continue;
        }
        if let Some(module) = candidate_modules.get(&file_id) {
            imported.extend(internal_targets(module).filter(|target| excluded.contains(target)));
        }
    }
    excluded
}

fn internal_targets(module: &ResolvedModule) -> impl Iterator<Item = FileId> + '_ {
    module
        .all_resolved_source_edges()
        .filter_map(|edge| edge.target().internal_file_id())
        .chain(
            module
                .resolved_dynamic_patterns
                .iter()
                .flat_map(|(_, targets)| targets.iter().copied()),
        )
}

/// Replace each excluded module with an inert module: same file, no exports,
/// no imports and no usage facts.
pub fn neutralize(modules: &mut [ResolvedModule], excluded: &FxHashSet<FileId>) {
    if excluded.is_empty() {
        return;
    }
    for module in modules
        .iter_mut()
        .filter(|module| excluded.contains(&module.file_id))
    {
        let path = std::mem::take(&mut module.path);
        *module = inert_module(module.file_id, path);
    }
}

fn inert_module(file_id: FileId, path: std::path::PathBuf) -> ResolvedModule {
    ResolvedModule {
        file_id,
        path,
        exports: Arc::from(Vec::new()),
        re_exports: Vec::new(),
        resolved_imports: Vec::new(),
        resolved_dynamic_imports: Vec::new(),
        resolved_dynamic_patterns: Vec::new(),
        member_accesses: Arc::from(Vec::new()),
        semantic_facts: Arc::from(Vec::new()),
        whole_object_uses: Arc::from(Vec::new()),
        has_cjs_exports: false,
        has_angular_component_template_url: false,
        unused_import_bindings: FxHashSet::default(),
        type_referenced_import_bindings: Vec::new(),
        value_referenced_import_bindings: Vec::new(),
        namespace_object_aliases: Vec::new(),
        exported_factory_returns: Arc::from(Vec::new()),
        exported_factory_return_object_shapes: Arc::from(Vec::new()),
        type_member_types: Arc::from(Vec::new()),
        missing_export_targets: Vec::new(),
    }
}

/// Hide the source-owned findings of the excluded files, and the stale
/// suppressions in them: an inert module consumes no suppression.
pub fn hide_findings(
    results: &mut AnalysisResults,
    files: &[DiscoveredFile],
    excluded: &FxHashSet<FileId>,
) {
    if excluded.is_empty() {
        return;
    }
    let paths: FxHashSet<&Path> = files
        .iter()
        .filter(|file| excluded.contains(&file.id))
        .map(|file| file.path.as_path())
        .collect();
    results.remove_ignored_dead_code_findings(|path| paths.contains(path));
    results
        .stale_suppressions
        .retain(|suppression| !paths.contains(suppression.path.as_path()));
}
