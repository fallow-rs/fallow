//! Aliased-import adapters in duplicate-export groups.

use fallow_types::extract::ImportedName;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::discover::FileId;
use crate::graph::{EffectiveExportResolution, ModuleGraph};

use super::ExportEntry;

/// The group files that are aliased-import adapters of another group member.
///
/// An adapter imports `export_name` under another local name from a module
/// whose export resolves to another group member, and declares its own
/// `export_name`. An example is
/// `import { configureAuth as core } from "../ci/auth"` followed by
/// `export function configureAuth(env) { return core(env.url); }`. The
/// adapter keeps the name on purpose and derives its export from the origin,
/// as a re-export chain member does. Thus it is not an independent origin.
/// Only a static named import counts. A re-export or a dynamic import pattern
/// has an empty import span. When a third module exports the same name
/// independently, the finding stays between the origin and that module.
///
/// A file is an adapter only when its adapter links reach a group member that
/// is not an adapter itself. Two modules that import the name from each other
/// under aliases form a cycle without an origin. Both stay in the group, so
/// the finding is not lost.
///
/// The check is syntactic. It does not read the body of the local export. A
/// module that aliases the import only to avoid a name clash, and holds an
/// unrelated implementation, is also treated as an adapter.
pub(super) fn aliased_import_adapters(
    entries: &[ExportEntry],
    export_name: &str,
    graph: &ModuleGraph,
) -> FxHashSet<FileId> {
    let group_files: FxHashSet<FileId> = entries.iter().map(|e| e.file_id).collect();
    let mut wrapped: FxHashMap<FileId, FxHashSet<FileId>> = FxHashMap::default();
    for entry in entries {
        let targets = wrapped.entry(entry.file_id).or_default();
        targets.extend(wrapped_group_origins(
            entry,
            export_name,
            &group_files,
            graph,
        ));
    }

    let mut grounded: FxHashSet<FileId> = wrapped
        .iter()
        .filter(|(_, targets)| targets.is_empty())
        .map(|(&file, _)| file)
        .collect();
    let origins = grounded.clone();
    loop {
        let reached: Vec<FileId> = wrapped
            .iter()
            .filter(|(file, targets)| {
                !grounded.contains(*file) && targets.iter().any(|t| grounded.contains(t))
            })
            .map(|(&file, _)| file)
            .collect();
        if reached.is_empty() {
            break;
        }
        grounded.extend(reached);
    }
    grounded.retain(|file| !origins.contains(file));
    grounded
}

/// The other group members whose `export_name` the module of `entry` imports
/// under another local name.
fn wrapped_group_origins<'a>(
    entry: &'a ExportEntry,
    export_name: &'a str,
    group_files: &'a FxHashSet<FileId>,
    graph: &'a ModuleGraph,
) -> impl Iterator<Item = FileId> + 'a {
    graph
        .outgoing_edge_symbols(entry.file_id)
        .filter(move |(_, symbols)| {
            symbols.iter().any(|symbol| {
                matches!(&symbol.imported_name, ImportedName::Named(imported) if imported == export_name)
                    && symbol.local_name != export_name
                    && !symbol.import_span.is_empty()
            })
        })
        .flat_map(move |(target, _)| {
            entry.namespaces().iter().filter_map(move |&namespace| {
                match graph.resolve_export(target, export_name, namespace) {
                    EffectiveExportResolution::Unique(binding)
                        if binding.origin_file() != entry.file_id
                            && group_files.contains(&binding.origin_file()) =>
                    {
                        Some(binding.origin_file())
                    }
                    _ => None,
                }
            })
        })
}
