//! The direct references of a traced export: the references that credit its
//! declaration, and the importers of the implicit default of a component file.

use std::path::Path;

use fallow_types::extract::ImportedName;
use fallow_types::trace::{ExportReference, NamespacedExportReferences};
use rustc_hash::FxHashSet;

use super::trace_impl::{
    format_reference_kind, reference_to_export_reference, relative_module_path,
};
use crate::graph::{ExportNamespace, ModuleGraph, ReferenceKind};
use crate::search_visibility::{SearchCheck, SearchTarget, SourceCache};

/// Reachable files that import the implicit default export of a component
/// file (`.vue`, `.svelte`, `.astro`).
///
/// The graph keeps no export entry for this default, so the references come
/// from the import edges: a default import (also `{ default as X }`) or a
/// namespace import of the file.
/// A re-export is not a direct reference; the re-export chains report it.
pub(super) fn implicit_default_references(
    graph: &ModuleGraph,
    root: &Path,
    file_id: crate::discover::FileId,
) -> Vec<ExportReference> {
    let mut seen = FxHashSet::default();
    graph
        .importers_of(file_id)
        .iter()
        .filter(|importer| seen.insert(**importer))
        .filter_map(|&importer| {
            let module = graph.modules.get(importer.0 as usize)?;
            if !module.is_reachable() {
                return None;
            }
            let kind = graph
                .outgoing_symbol_edges(importer)
                .filter(|(target, _)| *target == file_id)
                .flat_map(|(_, symbols)| symbols.iter())
                .find_map(|symbol| match &symbol.imported_name {
                    ImportedName::Default => Some(ReferenceKind::DefaultImport),
                    ImportedName::Named(name) if name == "default" => {
                        Some(ReferenceKind::DefaultImport)
                    }
                    ImportedName::Namespace => Some(ReferenceKind::NamespaceImport),
                    ImportedName::Named(_) | ImportedName::SideEffect => None,
                })?;
            Some(ExportReference::new(
                relative_module_path(&module.path, root),
                format_reference_kind(kind),
            ))
        })
        .collect()
}

/// Distinct referencing files of one module export surface in one namespace.
///
/// Each reference carries the literal-search check: the consumer source is
/// read on demand and compared with a whole-word search for the export name.
fn direct_export_references(
    graph: &ModuleGraph,
    root: &Path,
    sources: &mut SourceCache<'_>,
    file_id: crate::discover::FileId,
    export_name: &str,
    namespace: ExportNamespace,
) -> Vec<ExportReference> {
    let mut referenced_files = FxHashSet::default();
    let references: Vec<_> = graph
        .effective_export_surface_references(file_id, export_name, namespace)
        .into_iter()
        .filter(|reference| {
            graph
                .modules
                .get(reference.from_file.0 as usize)
                .is_some_and(crate::graph::ModuleNode::is_reachable)
        })
        .filter(|reference| referenced_files.insert(reference.from_file))
        .collect();
    if references.is_empty() {
        return Vec::new();
    }
    let target = SearchTarget::new(graph, sources, file_id, export_name, namespace);
    references
        .into_iter()
        .map(|reference| {
            let mut export_reference = reference_to_export_reference(graph, root, reference);
            if let Some(check) = target.check(graph, sources, reference) {
                apply_search_check(&mut export_reference, check);
            }
            export_reference
        })
        .collect()
}

fn apply_search_check(reference: &mut ExportReference, check: SearchCheck) {
    reference.line = check.line;
    reference.search_visibility = Some(check.visibility);
    reference.reason = check.reason;
    reference.search_as = check.search_as;
}

/// References that credit the traced declaration, with the namespace that
/// carries them.
///
/// The preferred surface wins whenever its lane carries a reference. When it
/// carries none, the other namespace is consulted only if it resolves to the
/// same effective binding: that is the type lane falling back onto a
/// value-only declaration (`import type { helper }` of `export const helper`),
/// which the unused-export analyzer counts as a use regardless of namespace.
/// A distinct same-name declaration in the other namespace keeps the preferred
/// lane, because its references credit that other declaration and dead-code
/// still reports the traced one. See issue #2371.
pub(super) fn crediting_export_references(
    graph: &ModuleGraph,
    root: &Path,
    file_id: crate::discover::FileId,
    export_name: &str,
    surface: crate::graph::EffectiveExportSurface<'_>,
) -> (
    ExportNamespace,
    Vec<ExportReference>,
    Vec<NamespacedExportReferences>,
) {
    let namespace = surface.namespace();
    let mut sources = SourceCache::new(graph);
    let references =
        direct_export_references(graph, root, &mut sources, file_id, export_name, namespace);
    let other = match namespace {
        ExportNamespace::Type => ExportNamespace::Value,
        ExportNamespace::Value => ExportNamespace::Type,
    };
    let same_binding = graph
        .effective_export_surface(file_id, export_name, other)
        .is_some_and(|candidate| {
            graph.effective_bindings_share_declaration_group(candidate.binding(), surface.binding())
        });
    if !same_binding {
        return (namespace, references, Vec::new());
    }
    let other_references =
        direct_export_references(graph, root, &mut sources, file_id, export_name, other);
    let by_namespace = if references.is_empty() || other_references.is_empty() {
        Vec::new()
    } else {
        vec![
            namespaced_references(namespace, references.clone()),
            namespaced_references(other, other_references.clone()),
        ]
    };
    if references.is_empty() && !other_references.is_empty() {
        (other, other_references, by_namespace)
    } else {
        (namespace, references, by_namespace)
    }
}

fn namespaced_references(
    namespace: ExportNamespace,
    references: Vec<ExportReference>,
) -> NamespacedExportReferences {
    NamespacedExportReferences {
        namespace: match namespace {
            ExportNamespace::Type => fallow_types::semantic::SemanticNamespace::Type,
            ExportNamespace::Value => fallow_types::semantic::SemanticNamespace::Value,
        },
        reference_count: references.len(),
        references,
    }
}
