//! Post-processing of `unused-type` findings.

use rustc_hash::{FxHashMap, FxHashSet};

use fallow_config::ResolvedConfig;
use fallow_types::extract::{ExportName, ImportedName, ModuleInfo};

use crate::discover::FileId;
use crate::graph::{ExportNamespace, ModuleGraph};
use crate::results::UnusedExport;

/// Sorted Effect schema export names per file, or `None` when
/// `effect.schemaExportsAsTypes` is off.
///
/// An export also counts as a schema when it forwards a schema of another
/// module (`import { A } from "./a"; export { A }` or `export { A } from
/// "./a"`), or when its chain starts at an imported schema
/// (`Imported.pipe(Schema.brand("B"))`). A forwarded non-schema value stays
/// a value export.
pub(super) fn schema_exports_by_file(
    config: &ResolvedConfig,
    graph: &ModuleGraph,
    modules: &[ModuleInfo],
) -> Option<FxHashMap<FileId, Vec<String>>> {
    if !config.effect.schema_exports_as_types {
        return None;
    }
    let mut schemas: FxHashSet<(FileId, String)> = modules
        .iter()
        .filter_map(|module| Some((module.file_id, module.effect_facts.as_deref()?)))
        .flat_map(|(file_id, facts)| {
            facts
                .schema_exports
                .iter()
                .map(move |name| (file_id, name.clone()))
        })
        .collect();
    let candidates: Vec<ForwardCandidate> = modules
        .iter()
        .flat_map(|module| forward_candidates(graph, module))
        .collect();
    loop {
        let before = schemas.len();
        for candidate in &candidates {
            if schemas.contains(&candidate.origin) {
                schemas.insert((candidate.file_id, candidate.export_name.clone()));
            }
        }
        if schemas.len() == before {
            break;
        }
    }
    let mut by_file: FxHashMap<FileId, Vec<String>> = FxHashMap::default();
    for (file_id, name) in schemas {
        by_file.entry(file_id).or_default().push(name);
    }
    for names in by_file.values_mut() {
        names.sort_unstable();
    }
    Some(by_file)
}

/// The declaration behind export `name` of `file_id`, through star and named
/// re-exports. A name without a unique declaration stays as given.
fn declaring_origin(graph: &ModuleGraph, file_id: FileId, name: String) -> (FileId, String) {
    graph
        .resolve_export_origin(file_id, &name, ExportNamespace::Value)
        .map_or((file_id, name), |origin| {
            (origin.file_id(), origin.export().name.to_string())
        })
}

/// An export that is a schema when the export `origin` is one.
struct ForwardCandidate {
    file_id: FileId,
    export_name: String,
    origin: (FileId, String),
}

/// The value exports of `module` that forward an imported binding, or whose
/// schema chain starts at one.
fn forward_candidates(graph: &ModuleGraph, module: &ModuleInfo) -> Vec<ForwardCandidate> {
    let file_id = module.file_id;
    let mut imports: FxHashMap<&str, (FileId, &str)> = FxHashMap::default();
    for (target, symbols) in graph.outgoing_symbol_edges(file_id) {
        for symbol in symbols
            .iter()
            .filter(|symbol| !symbol.is_type_only && !symbol.local_name.is_empty())
        {
            let imported = match &symbol.imported_name {
                ImportedName::Named(name) => name.as_str(),
                ImportedName::Default => "default",
                ImportedName::Namespace | ImportedName::SideEffect => continue,
            };
            imports.insert(symbol.local_name.as_str(), (target, imported));
        }
    }
    let candidate = |export_name: String, local: &str| {
        imports
            .get(local)
            .map(|&(target, imported)| ForwardCandidate {
                file_id,
                export_name,
                origin: declaring_origin(graph, target, imported.to_string()),
            })
    };
    let mut candidates: Vec<ForwardCandidate> = module
        .exports
        .iter()
        .filter(|export| !export.is_type_only)
        .filter_map(|export| {
            let local = export.local_name.as_deref().or(match &export.name {
                ExportName::Named(name) => Some(name.as_str()),
                ExportName::Default => None,
            })?;
            candidate(export.name.to_string(), local)
        })
        .collect();
    if let Some(facts) = module.effect_facts.as_deref() {
        candidates.extend(
            facts.derived_schema_exports.iter().filter_map(|derived| {
                candidate(derived.export_name.clone(), &derived.import_local)
            }),
        );
    }
    if let Some(node) = graph.modules.get(file_id.0 as usize) {
        candidates.extend(
            node.re_exports
                .iter()
                .filter(|re_export| !re_export.is_type_only && re_export.imported_name != "*")
                .map(|re_export| ForwardCandidate {
                    file_id,
                    export_name: re_export.exported_name.clone(),
                    origin: declaring_origin(
                        graph,
                        re_export.source_file,
                        re_export.imported_name.clone(),
                    ),
                }),
        );
    }
    candidates
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
