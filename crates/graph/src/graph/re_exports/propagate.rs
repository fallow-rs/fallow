//! Propagation functions for re-export chain resolution.
//!
//! Handles both star (`export * from`) and named (`export { foo } from`) re-exports,
//! including entry-point special cases where exports are consumed externally.

use rustc_hash::{FxHashMap, FxHashSet};

use fallow_types::discover::FileId;
use fallow_types::extract::{ExportName, VisibilityTag};

use crate::graph::types::{ExportSymbol, ModuleNode, ReferenceKind, SymbolReference};
use crate::graph::{Edge, ImportedName};
use crate::resolve::ResolvedModule;

/// Handle `export * from './source'`: propagate named imports through to the source module.
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
    pub(in crate::graph) module_by_id: &'a FxHashMap<FileId, &'a ResolvedModule>,
    pub(in crate::graph) barrel_id: FileId,
    pub(in crate::graph) barrel_idx: usize,
    pub(in crate::graph) source_id: FileId,
    pub(in crate::graph) source_idx: usize,
    pub(in crate::graph) entry_star_targets: &'a FxHashSet<FileId>,
    pub(in crate::graph) triggering_is_type_only: bool,
    pub(in crate::graph) synthetic_stubs: &'a mut FxHashSet<(FileId, String, bool)>,
}

pub(in crate::graph) fn propagate_star_re_export(input: StarReExportPropagation<'_>) -> bool {
    let StarReExportPropagation {
        modules,
        edges,
        edges_by_target,
        module_by_id,
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
            module_by_id,
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
) -> FxHashMap<String, Vec<StarReference>> {
    let named_refs: Vec<(String, StarReference)> = edges_by_target
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
                                StarReference {
                                    reference: SymbolReference {
                                        from_file: edge.source,
                                        kind: ReferenceKind::NamedImport,
                                        import_span: sym.import_span,
                                    },
                                    origin: StarReferenceOrigin::NamedImport {
                                        local_name: sym.local_name.clone(),
                                        is_type_only: sym.is_type_only,
                                    },
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

    let named_import_origin_by_reference = named_import_origin_by_reference(edges);
    let barrel_refs: Vec<(String, Vec<StarReference>)> = modules[barrel_idx]
        .exports
        .iter()
        .filter(|e| !e.references.is_empty())
        .map(|e| {
            let name = e.name.to_string();
            (
                name.clone(),
                e.references
                    .iter()
                    .copied()
                    .map(|reference| StarReference {
                        reference,
                        origin: named_import_origin_by_reference
                            .get(&(
                                reference.from_file,
                                reference.import_span.start,
                                reference.import_span.end,
                                name.clone(),
                            ))
                            .cloned()
                            .unwrap_or(StarReferenceOrigin::BarrelExport {
                                is_type_only: e.is_type_only,
                            }),
                    })
                    .collect(),
            )
        })
        .collect();

    let mut refs_by_name: FxHashMap<String, Vec<StarReference>> = FxHashMap::default();
    for (name, ref_item) in named_refs {
        refs_by_name.entry(name).or_default().push(ref_item);
    }
    for (name, refs) in barrel_refs {
        refs_by_name.entry(name).or_default().extend(refs);
    }
    refs_by_name
}

fn named_import_origin_by_reference(
    edges: &[Edge],
) -> FxHashMap<(FileId, u32, u32, String), StarReferenceOrigin> {
    edges
        .iter()
        .flat_map(|edge| {
            edge.symbols.iter().filter_map(move |sym| {
                let ImportedName::Named(name) = &sym.imported_name else {
                    return None;
                };
                Some((
                    (
                        edge.source,
                        sym.import_span.start,
                        sym.import_span.end,
                        name.clone(),
                    ),
                    StarReferenceOrigin::NamedImport {
                        local_name: sym.local_name.clone(),
                        is_type_only: sym.is_type_only,
                    },
                ))
            })
        })
        .collect()
}

#[derive(Clone)]
struct StarReference {
    reference: SymbolReference,
    origin: StarReferenceOrigin,
}

#[derive(Clone)]
enum StarReferenceOrigin {
    NamedImport {
        local_name: String,
        is_type_only: bool,
    },
    BarrelExport {
        is_type_only: bool,
    },
}

struct ApplyStarRefs<'a> {
    source: &'a mut ModuleNode,
    name: &'a str,
    refs: &'a [StarReference],
    source_id: FileId,
    module_by_id: &'a FxHashMap<FileId, &'a ResolvedModule>,
    triggering_is_type_only: bool,
    source_has_star_re_exports: bool,
    existing_files: &'a mut FxHashSet<FileId>,
    synthetic_stubs: &'a mut FxHashSet<(FileId, String, bool)>,
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
        module_by_id,
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

    let matching_exports: Vec<usize> = source
        .exports
        .iter()
        .enumerate()
        .filter_map(|(idx, export)| (export.name == export_name).then_some(idx))
        .collect();

    if !matching_exports.is_empty() {
        apply_star_refs_to_matching_exports(ApplyMatchingStarRefs {
            source,
            name,
            refs,
            source_id,
            module_by_id,
            triggering_is_type_only,
            source_has_star_re_exports,
            matching_exports: &matching_exports,
            existing_files,
            synthetic_stubs,
        })
    } else if source_has_star_re_exports {
        create_synthetic_exports_for_refs(CreateSyntheticExports {
            source,
            name,
            export_name,
            refs,
            source_id,
            module_by_id,
            triggering_is_type_only,
            synthetic_stubs,
        })
    } else {
        false
    }
}

struct ApplyMatchingStarRefs<'a> {
    source: &'a mut ModuleNode,
    name: &'a str,
    refs: &'a [StarReference],
    source_id: FileId,
    module_by_id: &'a FxHashMap<FileId, &'a ResolvedModule>,
    triggering_is_type_only: bool,
    source_has_star_re_exports: bool,
    matching_exports: &'a [usize],
    existing_files: &'a mut FxHashSet<FileId>,
    synthetic_stubs: &'a mut FxHashSet<(FileId, String, bool)>,
}

fn apply_star_refs_to_matching_exports(input: ApplyMatchingStarRefs<'_>) -> bool {
    let ApplyMatchingStarRefs {
        source,
        name,
        refs,
        source_id,
        module_by_id,
        triggering_is_type_only,
        source_has_star_re_exports,
        matching_exports,
        existing_files,
        synthetic_stubs,
    } = input;

    let mut type_exports: Vec<usize> = matching_exports
        .iter()
        .copied()
        .filter(|idx| source.exports[*idx].is_type_only)
        .collect();
    let mut value_exports: Vec<usize> = matching_exports
        .iter()
        .copied()
        .filter(|idx| !source.exports[*idx].is_type_only)
        .collect();

    let mut changed = false;
    let can_synthesize = source_has_star_re_exports;
    let effective_has_type_exports = !type_exports.is_empty() || can_synthesize;
    let effective_has_value_exports =
        !value_exports.is_empty() || (can_synthesize && !triggering_is_type_only);
    let mut needs_type_export = false;
    let mut needs_value_export = false;

    for star_ref in refs {
        let (attach_type_exports, attach_value_exports) = star_ref.attach_targets(
            module_by_id,
            effective_has_type_exports,
            effective_has_value_exports,
            triggering_is_type_only,
        );
        needs_type_export |= attach_type_exports;
        needs_value_export |= attach_value_exports;
    }

    if needs_type_export && type_exports.is_empty() && can_synthesize {
        changed |= create_empty_synthetic_export(source, source_id, name, true, synthetic_stubs);
        if let Some(idx) = source
            .exports
            .iter()
            .position(|export| export.name.matches_str(name) && export.is_type_only)
        {
            type_exports.push(idx);
        }
    }
    if needs_value_export && value_exports.is_empty() && can_synthesize {
        changed |= create_empty_synthetic_export(source, source_id, name, false, synthetic_stubs);
        if let Some(idx) = source
            .exports
            .iter()
            .position(|export| export.name.matches_str(name) && !export.is_type_only)
        {
            value_exports.push(idx);
        }
    }

    for star_ref in refs {
        let (attach_type_exports, attach_value_exports) = star_ref.attach_targets(
            module_by_id,
            !type_exports.is_empty(),
            !value_exports.is_empty(),
            triggering_is_type_only,
        );
        if attach_type_exports {
            changed |= attach_star_ref_to_exports(
                source,
                &type_exports,
                star_ref.reference,
                existing_files,
            );
        }
        if attach_value_exports {
            changed |= attach_star_ref_to_exports(
                source,
                &value_exports,
                star_ref.reference,
                existing_files,
            );
        }
    }

    changed
}

struct CreateSyntheticExports<'a> {
    source: &'a mut ModuleNode,
    name: &'a str,
    export_name: ExportName,
    refs: &'a [StarReference],
    source_id: FileId,
    module_by_id: &'a FxHashMap<FileId, &'a ResolvedModule>,
    triggering_is_type_only: bool,
    synthetic_stubs: &'a mut FxHashSet<(FileId, String, bool)>,
}

fn create_synthetic_exports_for_refs(input: CreateSyntheticExports<'_>) -> bool {
    let CreateSyntheticExports {
        source,
        name,
        export_name,
        refs,
        source_id,
        module_by_id,
        triggering_is_type_only,
        synthetic_stubs,
    } = input;

    let mut type_refs = Vec::new();
    let mut value_refs = Vec::new();
    for star_ref in refs {
        let (attach_type_exports, attach_value_exports) = star_ref.attach_targets(
            module_by_id,
            true,
            !triggering_is_type_only,
            triggering_is_type_only,
        );
        if attach_type_exports {
            type_refs.push(star_ref.reference);
        }
        if attach_value_exports {
            value_refs.push(star_ref.reference);
        }
    }

    let mut changed = false;
    if !type_refs.is_empty() {
        changed |= create_synthetic_export(CreateSyntheticExport {
            source,
            source_id,
            name,
            export_name: export_name.clone(),
            is_type_only: true,
            references: type_refs,
            synthetic_stubs,
        });
    }
    if !value_refs.is_empty() {
        changed |= create_synthetic_export(CreateSyntheticExport {
            source,
            source_id,
            name,
            export_name,
            is_type_only: false,
            references: value_refs,
            synthetic_stubs,
        });
    }
    changed
}

fn create_empty_synthetic_export(
    source: &mut ModuleNode,
    source_id: FileId,
    name: &str,
    is_type_only: bool,
    synthetic_stubs: &mut FxHashSet<(FileId, String, bool)>,
) -> bool {
    let export_name = if name == "default" {
        ExportName::Default
    } else {
        ExportName::Named(name.to_string())
    };
    create_synthetic_export(CreateSyntheticExport {
        source,
        source_id,
        name,
        export_name,
        is_type_only,
        references: Vec::new(),
        synthetic_stubs,
    })
}

struct CreateSyntheticExport<'a> {
    source: &'a mut ModuleNode,
    source_id: FileId,
    name: &'a str,
    export_name: ExportName,
    is_type_only: bool,
    references: Vec<SymbolReference>,
    synthetic_stubs: &'a mut FxHashSet<(FileId, String, bool)>,
}

fn create_synthetic_export(input: CreateSyntheticExport<'_>) -> bool {
    let CreateSyntheticExport {
        source,
        source_id,
        name,
        export_name,
        is_type_only,
        references,
        synthetic_stubs,
    } = input;

    if !synthetic_stubs.insert((source_id, name.to_string(), is_type_only)) {
        return false;
    }

    source.exports.push(ExportSymbol {
        name: export_name,
        is_type_only,
        is_side_effect_used: false,
        visibility: VisibilityTag::None,
        expected_unused_reason: None,
        span: oxc_span::Span::new(0, 0),
        references,
        members: Vec::new(),
    });
    true
}

fn attach_star_ref_to_exports(
    source: &mut ModuleNode,
    export_indices: &[usize],
    reference: SymbolReference,
    existing_files: &mut FxHashSet<FileId>,
) -> bool {
    let mut changed = false;
    for export_idx in export_indices {
        existing_files.clear();
        existing_files.extend(
            source.exports[*export_idx]
                .references
                .iter()
                .map(|r| r.from_file),
        );
        if existing_files.insert(reference.from_file) {
            source.exports[*export_idx].references.push(reference);
            changed = true;
        }
    }
    changed
}

impl StarReference {
    fn attach_targets(
        &self,
        module_by_id: &FxHashMap<FileId, &ResolvedModule>,
        has_type_exports: bool,
        has_value_exports: bool,
        triggering_is_type_only: bool,
    ) -> (bool, bool) {
        if triggering_is_type_only {
            return (has_type_exports, false);
        }

        match &self.origin {
            StarReferenceOrigin::NamedImport {
                local_name,
                is_type_only,
            } => decide_named_import_attach_targets(
                module_by_id.get(&self.reference.from_file),
                local_name,
                *is_type_only,
                has_type_exports,
                has_value_exports,
            ),
            StarReferenceOrigin::BarrelExport { is_type_only } => {
                decide_barrel_export_attach_targets(
                    *is_type_only,
                    has_type_exports,
                    has_value_exports,
                )
            }
        }
    }
}

fn decide_named_import_attach_targets(
    source_mod: Option<&&ResolvedModule>,
    local_name: &str,
    is_type_only: bool,
    has_type_exports: bool,
    has_value_exports: bool,
) -> (bool, bool) {
    let attach_type_exports = if !has_type_exports {
        false
    } else if !has_value_exports || is_type_only {
        true
    } else {
        import_binding_has_type_usage(source_mod, local_name)
    };

    let attach_value_exports = if !has_value_exports {
        false
    } else if !has_type_exports {
        true
    } else {
        import_binding_has_value_usage(source_mod, local_name)
    };

    if attach_type_exports || attach_value_exports {
        (attach_type_exports, attach_value_exports)
    } else if is_type_only {
        (has_type_exports, !has_type_exports && has_value_exports)
    } else {
        (!has_value_exports && has_type_exports, has_value_exports)
    }
}

const fn decide_barrel_export_attach_targets(
    is_type_only: bool,
    has_type_exports: bool,
    has_value_exports: bool,
) -> (bool, bool) {
    if is_type_only {
        (has_type_exports, !has_type_exports && has_value_exports)
    } else {
        (!has_value_exports && has_type_exports, has_value_exports)
    }
}

fn import_binding_has_type_usage(source_mod: Option<&&ResolvedModule>, local_name: &str) -> bool {
    !local_name.is_empty()
        && source_mod.is_some_and(|m| {
            m.type_referenced_import_bindings
                .iter()
                .any(|binding| binding == local_name)
        })
}

fn import_binding_has_value_usage(source_mod: Option<&&ResolvedModule>, local_name: &str) -> bool {
    !local_name.is_empty()
        && source_mod.is_some_and(|m| {
            m.value_referenced_import_bindings
                .iter()
                .any(|binding| binding == local_name)
        })
}

/// Entry point barrel with `export *`: mark all non-default source exports as used.
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
