//! Post-processing of `unused-type` findings.

use rustc_hash::{FxHashMap, FxHashSet};

use fallow_config::ResolvedConfig;
use fallow_types::extract::ModuleInfo;

use crate::discover::FileId;
use crate::graph::ModuleGraph;
use crate::results::UnusedExport;

/// Sorted Effect schema export names per file, or `None` when
/// `effect.schemaExportsAsTypes` is off.
pub(super) fn schema_exports_by_file<'a>(
    config: &ResolvedConfig,
    modules: &'a [ModuleInfo],
) -> Option<FxHashMap<FileId, &'a [String]>> {
    config.effect.schema_exports_as_types.then(|| {
        modules
            .iter()
            .filter_map(|module| {
                let facts = module.effect_facts.as_deref()?;
                (!facts.schema_exports.is_empty())
                    .then_some((module.file_id, facts.schema_exports.as_slice()))
            })
            .collect()
    })
}

/// Keep one finding when a schema and its same-name type alias
/// (`export type User = typeof User.Type`) are both unused. The two share a
/// finding identity, the path and the export name. The schema finding stays,
/// because the schema declares the type.
pub(super) fn drop_types_paired_with_schemas(unused_types: &mut Vec<UnusedExport>) {
    let schema_names: FxHashSet<String> = unused_types
        .iter()
        .filter(|unused| !unused.is_type_only)
        .map(|unused| unused.export_name.clone())
        .collect();
    if schema_names.is_empty() {
        return;
    }
    unused_types
        .retain(|unused| !unused.is_type_only || !schema_names.contains(&unused.export_name));
}

/// Remove exported type findings when the type is only exported to support
/// another public signature in the same module.
///
/// A backing export counts as live when it is not in `unused_values` or
/// `unused_types`. A type stays hidden only while at least one of its backing
/// exports is live. A hidden type counts as live for the types that it backs,
/// so the check repeats until no more types become hidden.
pub fn suppress_signature_backing_types(
    unused_types: &mut Vec<UnusedExport>,
    unused_values: &[UnusedExport],
    graph: &ModuleGraph,
    modules: &[ModuleInfo],
) {
    if unused_types.is_empty() {
        return;
    }

    let path_by_id: FxHashMap<FileId, &std::path::Path> = graph
        .modules
        .iter()
        .map(|module| (module.file_id, module.path.as_path()))
        .collect();
    let mut backers_by_type: FxHashMap<(&std::path::Path, &str), Vec<&str>> = FxHashMap::default();
    for (module, path) in modules
        .iter()
        .filter_map(|module| path_by_id.get(&module.file_id).map(|path| (module, *path)))
    {
        for reference in &module.public_signature_type_references {
            backers_by_type
                .entry((path, reference.type_name.as_str()))
                .or_default()
                .push(reference.export_name.as_str());
        }
    }

    let mut dead: FxHashSet<(&std::path::Path, &str)> = unused_values
        .iter()
        .chain(unused_types.iter())
        .map(|unused| (unused.path.as_path(), unused.export_name.as_str()))
        .collect();
    let keys: Vec<(&std::path::Path, &str)> = unused_types
        .iter()
        .map(|unused| (unused.path.as_path(), unused.export_name.as_str()))
        .collect();
    let mut hidden = vec![false; keys.len()];
    loop {
        let newly_hidden: Vec<usize> = keys
            .iter()
            .enumerate()
            .filter(|(index, _)| !hidden[*index])
            .filter(|(_, key)| {
                backers_by_type.get(*key).is_some_and(|backers| {
                    backers
                        .iter()
                        .any(|backer| !dead.contains(&(key.0, *backer)))
                })
            })
            .map(|(index, _)| index)
            .collect();
        if newly_hidden.is_empty() {
            break;
        }
        for index in newly_hidden {
            dead.remove(&keys[index]);
            hidden[index] = true;
        }
    }

    let mut hidden = hidden.into_iter();
    unused_types.retain(|_| !hidden.next().unwrap_or(false));
}
