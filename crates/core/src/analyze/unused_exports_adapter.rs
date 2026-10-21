//! Aliased-import adapters in duplicate-export groups.

use fallow_types::extract::ImportedName;
use rustc_hash::FxHashSet;

use crate::discover::FileId;
use crate::graph::{EffectiveExportResolution, ModuleGraph};

use super::ExportEntry;

/// Whether the module of `entry` imports `export_name` under another local
/// name from a module whose export resolves to another group member, and
/// declares its own `export_name`.
///
/// Such a module is an adapter, for example
/// `import { configureAuth as core } from "../ci/auth"` followed by
/// `export function configureAuth(env) { return core(env.url); }`. The
/// adapter keeps the name on purpose and derives its export from the origin,
/// as a re-export chain member does. Thus it is not an independent origin.
/// Only a static named import counts. A re-export or a dynamic import pattern
/// has an empty import span. When a third module exports the same name
/// independently, the finding stays between the origin and that module.
pub(super) fn is_aliased_import_wrapper(
    entry: &ExportEntry,
    export_name: &str,
    group_files: &FxHashSet<FileId>,
    graph: &ModuleGraph,
) -> bool {
    graph
        .outgoing_edge_symbols(entry.file_id)
        .any(|(target, symbols)| {
            symbols.iter().any(|symbol| {
                matches!(&symbol.imported_name, ImportedName::Named(imported) if imported == export_name)
                    && symbol.local_name != export_name
                    && !symbol.import_span.is_empty()
            }) && entry.namespaces().iter().any(|&namespace| {
                matches!(
                    graph.resolve_export(target, export_name, namespace),
                    EffectiveExportResolution::Unique(binding)
                        if binding.origin_file() != entry.file_id
                            && group_files.contains(&binding.origin_file())
                )
            })
        })
}
