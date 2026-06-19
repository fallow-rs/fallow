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
pub(in crate::graph) struct StarReExportPropagation<'a> {
    pub(in crate::graph) modules: &'a mut [ModuleNode],
    pub(in crate::graph) edges: &'a [Edge],
    pub(in crate::graph) edges_by_target: &'a FxHashMap<FileId, Vec<usize>>,
    pub(in crate::graph) barrel_id: FileId,
    pub(in crate::graph) barrel_idx: usize,
    pub(in crate::graph) source_id: FileId,
    pub(in crate::graph) source_idx: usize,
    pub(in crate::graph) entry_star_targets: &'a FxHashSet<FileId>,
    pub(in crate::graph) triggering_is_type_only: bool,
    pub(in crate::graph) synthetic_stubs: &'a mut FxHashSet<(FileId, String)>,
}

pub(in crate::graph) fn propagate_star_re_export(input: StarReExportPropagation<'_>) -> bool {
    let StarReExportPropagation {
        modules,
        edges,
        edges_by_target,
        barrel_id,
        barrel_idx,
        source_id,
        source_idx,
        entry_star_targets,
        triggering_is_type_only,
        synthetic_stubs,
    } = input;

    if modules[barrel_idx].is_entry_point()
        || entry_star_targets.contains(&modules[barrel_idx].file_id)
    {
        return propagate_entry_point_star(modules, barrel_id, source_idx);
    }

    let barrel_file_id = modules[barrel_idx].file_id;
    let refs_by_name =
        collect_star_refs_by_name(modules, edges, edges_by_target, barrel_file_id, barrel_idx);

    let source_has_star_re_exports = modules[source_idx]
        .re_exports
        .iter()
        .any(|re| re.exported_name == "*");

    let mut changed = false;
    let mut existing_files: FxHashSet<FileId> = FxHashSet::default();
    let source = &mut modules[source_idx];
    for (name, refs) in &refs_by_name {
        changed |= apply_star_refs_to_source(ApplyStarRefs {
            source: &mut *source,
            name,
            refs,
            source_id,
            triggering_is_type_only,
            source_has_star_re_exports,
            existing_files: &mut existing_files,
            synthetic_stubs: &mut *synthetic_stubs,
        });
    }
    changed
}

/// Collect the per-name references that must propagate through a star
/// re-export: named imports made directly from the barrel plus any references
/// already attached to the barrel's own exports.
fn collect_star_refs_by_name(
    modules: &[ModuleNode],
    edges: &[Edge],
    edges_by_target: &FxHashMap<FileId, Vec<usize>>,
    barrel_file_id: FileId,
    barrel_idx: usize,
) -> FxHashMap<String, Vec<SymbolReference>> {
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

    let barrel_refs: Vec<(String, Vec<SymbolReference>)> = modules[barrel_idx]
        .exports
        .iter()
        .filter(|e| !e.references.is_empty())
        .map(|e| (e.name.to_string(), e.references.clone()))
        .collect();

    let mut refs_by_name: FxHashMap<String, Vec<SymbolReference>> = FxHashMap::default();
    for (name, ref_item) in named_refs {
        refs_by_name.entry(name).or_default().push(ref_item);
    }
    for (name, refs) in barrel_refs {
        refs_by_name.entry(name).or_default().extend(refs);
    }
    refs_by_name
}

struct ApplyStarRefs<'a> {
    source: &'a mut ModuleNode,
    name: &'a str,
    refs: &'a [SymbolReference],
    source_id: FileId,
    triggering_is_type_only: bool,
    source_has_star_re_exports: bool,
    existing_files: &'a mut FxHashSet<FileId>,
    synthetic_stubs: &'a mut FxHashSet<(FileId, String)>,
}

/// Attach the collected references for one re-exported name to the source
/// module, creating a synthetic stub when the source forwards via its own
/// `export *`. Returns `true` if any reference or stub was added.
fn apply_star_refs_to_source(input: ApplyStarRefs<'_>) -> bool {
    let ApplyStarRefs {
        source,
        name,
        refs,
        source_id,
        triggering_is_type_only,
        source_has_star_re_exports,
        existing_files,
        synthetic_stubs,
    } = input;

    let export_name = if name == "default" {
        ExportName::Default
    } else {
        ExportName::Named(name.to_string())
    };

    if let Some(export) = source.exports.iter_mut().find(|e| e.name == export_name) {
        let mut changed = if !triggering_is_type_only
            && export.is_type_only
            && synthetic_stubs.contains(&(source_id, name.to_string()))
        {
            export.is_type_only = false;
            true
        } else {
            false
        };
        existing_files.clear();
        existing_files.extend(export.references.iter().map(|r| r.from_file));
        for ref_item in refs {
            if existing_files.insert(ref_item.from_file) {
                export.references.push(*ref_item);
                changed = true;
            }
        }
        changed
    } else if source_has_star_re_exports {
        source.exports.push(ExportSymbol {
            name: export_name,
            is_type_only: triggering_is_type_only,
            is_side_effect_used: false,
            visibility: VisibilityTag::None,
            expected_unused_reason: None,
            span: oxc_span::Span::new(0, 0),
            references: refs.to_vec(),
            members: Vec::new(),
        });
        synthetic_stubs.insert((source_id, name.to_string()));
        true
    } else {
        false
    }
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
pub(in crate::graph) struct NamedReExportPropagation<'a> {
    pub(in crate::graph) modules: &'a mut [ModuleNode],
    pub(in crate::graph) barrel_id: FileId,
    pub(in crate::graph) barrel_idx: usize,
    pub(in crate::graph) source_idx: usize,
    pub(in crate::graph) imported_name: &'a str,
    pub(in crate::graph) exported_name: &'a str,
    pub(in crate::graph) existing_refs: &'a mut FxHashSet<FileId>,
}

pub(in crate::graph) fn propagate_named_re_export(input: NamedReExportPropagation<'_>) -> bool {
    let NamedReExportPropagation {
        modules,
        barrel_id,
        barrel_idx,
        source_idx,
        imported_name,
        exported_name,
        existing_refs,
    } = input;

    let refs_on_barrel: Vec<SymbolReference> = modules[barrel_idx]
        .exports
        .iter()
        .filter(|e| e.name.matches_str(exported_name))
        .flat_map(|e| e.references.iter().copied())
        .collect();

    if refs_on_barrel.is_empty() {
        if modules[barrel_idx].is_entry_point() {
            return propagate_entry_point_named(modules, barrel_id, source_idx, imported_name);
        }
        return false;
    }

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
