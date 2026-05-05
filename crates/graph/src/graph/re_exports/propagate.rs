//! Propagation functions for re-export chain resolution.
//!
//! Handles both star (`export * from`) and named (`export { foo } from`) re-exports,
//! including entry-point special cases where exports are consumed externally.

use rustc_hash::{FxHashMap, FxHashSet};

use fallow_types::discover::FileId;
use fallow_types::extract::{ExportName, VisibilityTag};

use crate::graph::types::{ExportSymbol, ModuleNode, ReferenceKind, SymbolReference};
use crate::graph::{Edge, ImportedName};

/// Handle `export * from './source'` — propagate named imports through to the source module.
///
/// Star re-exports don't create named `ExportSymbol` entries on the barrel. Instead we look
/// at which named imports other modules make from the barrel and propagate each to the
/// matching export in the source module.
///
/// Returns `true` if any new references were added.
pub(in crate::graph) fn propagate_star_re_export(
    modules: &mut [ModuleNode],
    edges: &[Edge],
    edges_by_target: &rustc_hash::FxHashMap<FileId, Vec<usize>>,
    barrel_id: FileId,
    barrel_idx: usize,
    source_idx: usize,
    entry_star_targets: &FxHashSet<FileId>,
) -> bool {
    // Entry point barrels with star re-exports: all source exports are
    // transitively exposed to external consumers — mark them as used.
    // Also applies to barrels that are themselves star-re-exported from an
    // entry point (e.g., `types/index.ts` does `export * from 'Component.vue'`
    // and the package entry does `export * from './types'`). Without this,
    // types accessible only via multi-level star re-export chains get zero
    // references and are falsely reported as unused.
    if modules[barrel_idx].is_entry_point()
        || entry_star_targets.contains(&modules[barrel_idx].file_id)
    {
        return propagate_entry_point_star(modules, barrel_id, source_idx);
    }

    // Collect named imports that target the barrel using the pre-built reverse index.
    // Previously this scanned ALL edges O(E) per call — now O(incoming edges to barrel).
    let barrel_file_id = modules[barrel_idx].file_id;
    let named_refs: Vec<(String, SymbolReference)> = edges_by_target
        .get(&barrel_file_id)
        .map(|indices| {
            indices
                .iter()
                .flat_map(|&idx| {
                    let edge = &edges[idx];
                    edge.symbols.iter().filter_map(move |sym| {
                        if let ImportedName::Named(name) = &sym.imported_name {
                            Some((
                                name.clone(),
                                SymbolReference {
                                    from_file: edge.source,
                                    kind: ReferenceKind::NamedImport,
                                    import_span: sym.import_span,
                                },
                            ))
                        } else {
                            None
                        }
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    // Collect barrel exports with references: one entry per export (not per reference).
    // Previously this was O(exports × refs) String allocations; now O(exports_with_refs).
    let barrel_refs: Vec<(String, Vec<SymbolReference>)> = modules[barrel_idx]
        .exports
        .iter()
        .filter(|e| !e.references.is_empty())
        .map(|e| (e.name.to_string(), e.references.clone()))
        .collect();

    // Check if the source module itself has star re-exports (for multi-level chains).
    // If so, we may need to create synthetic ExportSymbol entries on it so
    // that the next iteration can propagate names further down the chain.
    let source_has_star_re_exports = modules[source_idx]
        .re_exports
        .iter()
        .any(|re| re.exported_name == "*");

    // Group all references by name: combine named edge imports with barrel export refs.
    // This looks up each export in the source at most once instead of per-reference.
    let mut refs_by_name: FxHashMap<String, Vec<SymbolReference>> = FxHashMap::default();
    for (name, ref_item) in named_refs {
        refs_by_name.entry(name).or_default().push(ref_item);
    }
    for (name, refs) in barrel_refs {
        refs_by_name.entry(name).or_default().extend(refs);
    }

    let mut changed = false;
    let mut existing_files: FxHashSet<FileId> = FxHashSet::default();
    let source = &mut modules[source_idx];
    for (name, refs) in &refs_by_name {
        let export_name = if name == "default" {
            ExportName::Default
        } else {
            ExportName::Named(name.clone())
        };
        if let Some(export) = source.exports.iter_mut().find(|e| e.name == export_name) {
            // Use a HashSet for O(1) duplicate detection instead of O(n) linear scan.
            // Reference lists grow across iterations, making the linear check quadratic.
            existing_files.clear();
            existing_files.extend(export.references.iter().map(|r| r.from_file));
            for ref_item in refs {
                if existing_files.insert(ref_item.from_file) {
                    export.references.push(*ref_item);
                    changed = true;
                }
            }
        } else if source_has_star_re_exports {
            source.exports.push(ExportSymbol {
                name: export_name,
                is_type_only: false,
                is_side_effect_used: false,
                visibility: VisibilityTag::None,
                span: oxc_span::Span::new(0, 0),
                references: refs.clone(),
                members: Vec::new(),
            });
            changed = true;
        }
    }
    changed
}

/// Entry point barrel with `export *` — mark all non-default source exports as used.
fn propagate_entry_point_star(
    modules: &mut [ModuleNode],
    barrel_id: FileId,
    source_idx: usize,
) -> bool {
    let mut changed = false;
    let source = &mut modules[source_idx];
    for export in &mut source.exports {
        // `export *` does not re-export the default export per ES spec.
        if matches!(export.name, ExportName::Default) {
            continue;
        }
        if export.references.iter().all(|r| r.from_file != barrel_id) {
            export.references.push(SymbolReference {
                from_file: barrel_id,
                kind: ReferenceKind::ReExport,
                import_span: oxc_span::Span::new(0, 0),
            });
            changed = true;
        }
    }
    changed
}

/// Handle named re-exports (`export { foo } from './source'`) — propagate barrel references
/// to the source module's matching export.
///
/// Returns `true` if any new references were added.
pub(in crate::graph) fn propagate_named_re_export(
    modules: &mut [ModuleNode],
    barrel_id: FileId,
    barrel_idx: usize,
    source_idx: usize,
    imported_name: &str,
    exported_name: &str,
    existing_refs: &mut FxHashSet<FileId>,
) -> bool {
    // Find references to the exported name on the barrel
    let refs_on_barrel: Vec<SymbolReference> = modules[barrel_idx]
        .exports
        .iter()
        .filter(|e| e.name.matches_str(exported_name))
        .flat_map(|e| e.references.iter().copied())
        .collect();

    if refs_on_barrel.is_empty() {
        // Entry point barrels' re-exports are consumed externally (not
        // tracked in the graph). Synthesize a ReExport reference so the
        // source export is correctly marked as used.
        if modules[barrel_idx].is_entry_point() {
            return propagate_entry_point_named(modules, barrel_id, source_idx, imported_name);
        }
        return false;
    }

    // Propagate to source module's export
    let mut changed = false;
    let source = &mut modules[source_idx];
    let target_exports: Vec<usize> = source
        .exports
        .iter()
        .enumerate()
        .filter(|(_, e)| e.name.matches_str(imported_name))
        .map(|(i, _)| i)
        .collect();

    for export_idx in target_exports {
        existing_refs.clear();
        existing_refs.extend(
            source.exports[export_idx]
                .references
                .iter()
                .map(|r| r.from_file),
        );
        for ref_item in &refs_on_barrel {
            if !existing_refs.contains(&ref_item.from_file) {
                source.exports[export_idx].references.push(*ref_item);
                changed = true;
            }
        }
    }
    changed
}

/// Entry point barrel with named re-export and no in-graph consumers — synthesize
/// a `ReExport` reference so the source export is correctly marked as used.
fn propagate_entry_point_named(
    modules: &mut [ModuleNode],
    barrel_id: FileId,
    source_idx: usize,
    imported_name: &str,
) -> bool {
    let synthetic_ref = SymbolReference {
        from_file: barrel_id,
        kind: ReferenceKind::ReExport,
        import_span: oxc_span::Span::new(0, 0),
    };
    let mut changed = false;
    let source = &mut modules[source_idx];
    let target_exports: Vec<usize> = source
        .exports
        .iter()
        .enumerate()
        .filter(|(_, e)| e.name.matches_str(imported_name))
        .map(|(i, _)| i)
        .collect();
    for export_idx in target_exports {
        if source.exports[export_idx]
            .references
            .iter()
            .all(|r| r.from_file != barrel_id)
        {
            source.exports[export_idx].references.push(synthetic_ref);
            changed = true;
        }
    }
    changed
}
