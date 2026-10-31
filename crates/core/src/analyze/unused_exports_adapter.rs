//! Star re-export overlays in duplicate-export groups.

use fallow_types::extract::ImportedName;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::discover::FileId;
use crate::graph::{EffectiveExportResolution, ModuleGraph};

use super::ExportEntry;

/// The group files that are overlays of another group member.
///
/// An overlay imports `export_name` under another local name from a module
/// whose export resolves to another group member, declares its own
/// `export_name`, and also declares `export * from` that same module. An
/// example is `import { Package as NativePackage } from "../native.js"`,
/// `export * from "../native.js"` and
/// `export class Package extends NativePackage {}`. The overlay re-exports the
/// full surface of the origin and replaces one binding, as a re-export chain
/// member does. Thus it is not an independent origin. Only a static named
/// import counts. A re-export or a dynamic import pattern has an empty import
/// span. When a third module exports the same name independently, the finding
/// stays between the origin and that module.
///
/// A module that imports the name under an alias and declares its own export,
/// but does not star re-export the origin module, is a wrapper. Importers see
/// two exports with different behavior, so a wrapper stays in the group
/// (issue #4024).
///
/// A file is an overlay only when its overlay links reach a group member that
/// is not an overlay itself. Two modules that import the name from each other
/// form a cycle without an origin. Both stay in the group, so the finding is
/// not lost.
pub(super) fn star_re_export_overlays(
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
/// under another local name from a module that it also star re-exports.
fn wrapped_group_origins<'a>(
    entry: &'a ExportEntry,
    export_name: &'a str,
    group_files: &'a FxHashSet<FileId>,
    graph: &'a ModuleGraph,
) -> impl Iterator<Item = FileId> + 'a {
    graph
        .outgoing_edge_symbols(entry.file_id)
        .filter(move |(target, symbols)| {
            star_re_exports(graph, entry.module_idx, *target)
                && symbols.iter().any(|symbol| {
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

/// Whether the module at `module_idx` declares `export * from` the module
/// `target`. A namespace re-export (`export * as ns from`) does not re-export
/// the full surface and does not count. A star edge that the graph layer
/// synthesizes has an empty span and does not count.
fn star_re_exports(graph: &ModuleGraph, module_idx: usize, target: FileId) -> bool {
    graph.modules.get(module_idx).is_some_and(|module| {
        module.re_exports.iter().any(|edge| {
            edge.source_file == target
                && edge.imported_name == "*"
                && edge.exported_name == "*"
                && !edge.span.is_empty()
        })
    })
}
