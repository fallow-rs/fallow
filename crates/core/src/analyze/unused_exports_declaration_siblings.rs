//! Declaration files that type a JS module next to them.
//!
//! TypeScript types an import of `./x.mjs` with the sibling `x.d.mts`. The
//! same pairing applies to `x.js` with `x.d.ts`, and to `x.cjs` with
//! `x.d.cts`. The graph attaches the import edge only to the JS module, so
//! the exports of the declaration file get no references. Under
//! `includeEntryExports`, which checks the exports of the declaration file
//! (an entry point), each of these exports would report as unused.
//!
//! [`declaration_sibling_credit`] collects, for each such declaration file,
//! the export names that the importers of its JS sibling use.

use std::path::{Path, PathBuf};

use rustc_hash::{FxHashMap, FxHashSet};

use fallow_types::extract::ImportedName;

use crate::discover::FileId;
use crate::graph::{ModuleGraph, ModuleNode};

use super::{export_has_reachable_reference, export_name_str};

/// Each declaration suffix with the extension of the JS module it types.
const DECLARATION_RUNTIME_PAIRS: &[(&str, &str)] =
    &[(".d.ts", "js"), (".d.mts", "mjs"), (".d.cts", "cjs")];

/// The used export names of each declaration file, keyed by its `FileId`.
pub(super) type DeclarationSiblingCredit = FxHashMap<FileId, FxHashSet<String>>;

/// Map each declaration file whose JS sibling is a graph module to the names
/// that the reachable importers of that sibling use.
///
/// A name counts when a reachable importer imports it from the sibling by
/// name or as the default, or when the export of the sibling with that name
/// has a reachable reference.
pub(super) fn declaration_sibling_credit(graph: &ModuleGraph) -> DeclarationSiblingCredit {
    let pairs: Vec<(FileId, PathBuf)> = graph
        .modules
        .iter()
        .filter_map(|module| runtime_sibling_path(&module.path).map(|path| (module.file_id, path)))
        .collect();
    if pairs.is_empty() {
        return DeclarationSiblingCredit::default();
    }
    let by_path: FxHashMap<&Path, FileId> = graph
        .modules
        .iter()
        .map(|module| (module.path.as_path(), module.file_id))
        .collect();
    pairs
        .into_iter()
        .filter_map(|(declaration, sibling)| {
            let runtime = *by_path.get(sibling.as_path())?;
            let names = used_names_of(graph, runtime);
            (!names.is_empty()).then_some((declaration, names))
        })
        .collect()
}

/// The path of the JS module that the declaration file at `path` types, or
/// `None` when `path` is no declaration file.
fn runtime_sibling_path(path: &Path) -> Option<PathBuf> {
    let name = path.file_name()?.to_str()?;
    DECLARATION_RUNTIME_PAIRS.iter().find_map(|(suffix, ext)| {
        let stem = name.strip_suffix(suffix)?;
        (!stem.is_empty()).then(|| path.with_file_name(format!("{stem}.{ext}")))
    })
}

fn used_names_of(graph: &ModuleGraph, runtime: FileId) -> FxHashSet<String> {
    let mut names = FxHashSet::default();
    let mut seen_importers = FxHashSet::default();
    for &importer in graph.importers_of(runtime) {
        if !seen_importers.insert(importer) || !is_reachable_module(graph, importer) {
            continue;
        }
        for (target, symbols) in graph.outgoing_symbol_edges(importer) {
            if target != runtime {
                continue;
            }
            for symbol in symbols {
                match &symbol.imported_name {
                    ImportedName::Named(name) => {
                        names.insert(name.clone());
                    }
                    ImportedName::Default => {
                        names.insert("default".to_string());
                    }
                    ImportedName::Namespace | ImportedName::SideEffect => {}
                }
            }
        }
    }
    if let Some(module) = graph.modules.get(runtime.0 as usize) {
        names.extend(
            module
                .exports
                .iter()
                .filter(|export| export_has_reachable_reference(graph, module, export))
                .map(|export| export_name_str(&export.name).to_string()),
        );
    }
    names
}

fn is_reachable_module(graph: &ModuleGraph, file_id: FileId) -> bool {
    graph
        .modules
        .get(file_id.0 as usize)
        .is_some_and(ModuleNode::is_reachable)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_sibling_path_pairs_each_declaration_suffix() {
        for (declaration, runtime) in [
            ("src/lib.d.ts", "src/lib.js"),
            ("src/lib.d.mts", "src/lib.mjs"),
            ("src/lib.d.cts", "src/lib.cjs"),
        ] {
            assert_eq!(
                runtime_sibling_path(Path::new(declaration)),
                Some(PathBuf::from(runtime)),
                "{declaration}"
            );
        }
    }

    #[test]
    fn runtime_sibling_path_ignores_other_files() {
        for path in ["src/lib.ts", "src/lib.mjs", "src/.d.ts"] {
            assert_eq!(runtime_sibling_path(Path::new(path)), None, "{path}");
        }
    }
}
