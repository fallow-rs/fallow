//! Propagation functions for re-export chain resolution.
//!
//! Handles both star (`export * from`) and named (`export { foo } from`) re-exports,
//! including entry-point special cases where exports are consumed externally.

use rustc_hash::{FxHashMap, FxHashSet};

#[cfg(test)]
use std::cell::Cell;

use fallow_types::discover::FileId;
use fallow_types::extract::{ExportName, VisibilityTag};

use crate::graph::types::{ExportSymbol, ModuleNode, ReferenceKind, SymbolReference};
use crate::graph::{Edge, ImportedName};
use crate::resolve::ResolvedModule;

#[cfg(test)]
thread_local! {
    static NAMED_IMPORT_ORIGIN_INDEX_BUILDS: Cell<usize> = const { Cell::new(0) };
    static STAR_REFERENCE_SET_REBUILDS: Cell<usize> = const { Cell::new(0) };
}

#[cfg(test)]
pub(super) fn count_named_import_origin_index_builds<T>(run: impl FnOnce() -> T) -> (T, usize) {
    NAMED_IMPORT_ORIGIN_INDEX_BUILDS.set(0);
    let result = run();
    let builds = NAMED_IMPORT_ORIGIN_INDEX_BUILDS.get();
    (result, builds)
}

#[cfg(test)]
pub(super) fn count_star_reference_set_rebuilds<T>(run: impl FnOnce() -> T) -> (T, usize) {
    STAR_REFERENCE_SET_REBUILDS.set(0);
    let result = run();
    let rebuilds = STAR_REFERENCE_SET_REBUILDS.get();
    (result, rebuilds)
}

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
    pub(in crate::graph) named_import_origin_index: &'a NamedImportOriginIndex,
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
        named_import_origin_index,
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
    let refs_by_name = collect_star_refs_by_name(
        modules,
        edges,
        edges_by_target,
        named_import_origin_index,
        barrel_file_id,
        barrel_idx,
    );

    let source_has_star_re_exports = modules[source_idx]
        .re_exports
        .iter()
        .any(|re| re.exported_name == "*");

    let matching_exports_by_name = build_named_export_index(&modules[source_idx]);

    let mut changed = false;
    let mut existing_files: FxHashSet<FileId> = FxHashSet::default();
    let source = &mut modules[source_idx];
    for (name, refs) in &refs_by_name {
        let matching_exports: &[usize] = matching_exports_by_name
            .get(name.as_str())
            .map_or(&[], Vec::as_slice);
        changed |= apply_star_refs_to_source(ApplyStarRefs {
            source: &mut *source,
            name,
            refs,
            matching_exports,
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

/// Index the source module's named exports by name so the per-re-exported-name
/// star propagation can look up matching exports in O(1) instead of rescanning
/// `source.exports` once per name.
///
/// Issue #1843 follow-up: the removed per-name scan was O(names x source_exports)
/// and re-ran on every fixpoint visit, dominating wide-barrel re-export
/// resolution. The map is built once over the module's pre-existing exports;
/// synthetic exports appended during propagation are always named after the name
/// currently being processed (each name is visited exactly once), so they never
/// need to appear in a later name's lookup. The result is therefore byte-identical
/// to the exact-`ExportName::Named`-match, ascending-index scan it replaces.
fn build_named_export_index(source: &ModuleNode) -> FxHashMap<String, Vec<usize>> {
    let mut index: FxHashMap<String, Vec<usize>> = FxHashMap::default();
    for (idx, export) in source.exports.iter().enumerate() {
        if let ExportName::Named(name) = &export.name {
            index.entry(name.clone()).or_default().push(idx);
        }
    }
    index
}

/// Collect the per-name references that must propagate through a star
/// re-export: named imports made directly from the barrel plus any references
/// already attached to the barrel's own exports.
fn collect_star_refs_by_name(
    modules: &[ModuleNode],
    edges: &[Edge],
    edges_by_target: &FxHashMap<FileId, Vec<usize>>,
    named_import_origin_index: &NamedImportOriginIndex,
    barrel_file_id: FileId,
    barrel_idx: usize,
) -> FxHashMap<String, Vec<StarReference>> {
    let named_refs = named_star_refs(edges, edges_by_target, barrel_file_id);
    let barrel_refs = barrel_star_refs(&modules[barrel_idx], named_import_origin_index);

    let mut refs_by_name: FxHashMap<String, Vec<StarReference>> = FxHashMap::default();
    for (name, ref_item) in named_refs {
        refs_by_name.entry(name).or_default().push(ref_item);
    }
    for (name, refs) in barrel_refs {
        refs_by_name.entry(name).or_default().extend(refs);
    }
    refs_by_name
}

fn named_star_refs(
    edges: &[Edge],
    edges_by_target: &FxHashMap<FileId, Vec<usize>>,
    barrel_file_id: FileId,
) -> Vec<(String, StarReference)> {
    edges_by_target
        .get(&barrel_file_id)
        .map(|indices| {
            indices
                .iter()
                .flat_map(|&idx| named_refs_for_edge(&edges[idx]))
                .collect()
        })
        .unwrap_or_default()
}

fn named_refs_for_edge(edge: &Edge) -> Vec<(String, StarReference)> {
    edge.symbols
        .iter()
        .filter_map(|sym| {
            let ImportedName::Named(name) = &sym.imported_name else {
                return None;
            };
            if name == "default" {
                return None;
            }
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
        })
        .collect()
}

fn barrel_star_refs(
    module: &ModuleNode,
    named_import_origin_index: &NamedImportOriginIndex,
) -> Vec<(String, Vec<StarReference>)> {
    module
        .exports
        .iter()
        .filter(|export| !export.references.is_empty())
        .map(|export| {
            let name = export.name.to_string();
            let refs = export
                .references
                .iter()
                .copied()
                .map(|reference| {
                    barrel_star_ref(
                        reference,
                        &name,
                        export.is_type_only,
                        named_import_origin_index,
                    )
                })
                .collect();
            (name, refs)
        })
        .collect()
}

fn barrel_star_ref(
    reference: SymbolReference,
    name: &str,
    is_type_only: bool,
    named_import_origin_index: &NamedImportOriginIndex,
) -> StarReference {
    StarReference {
        reference,
        origin: named_import_origin_index
            .get(reference, name)
            .cloned()
            .unwrap_or(StarReferenceOrigin::BarrelExport { is_type_only }),
    }
}

type ReferenceSite = (FileId, u32, u32);

#[derive(Default)]
pub(in crate::graph) struct NamedImportOriginIndex(
    FxHashMap<ReferenceSite, FxHashMap<String, StarReferenceOrigin>>,
);

impl NamedImportOriginIndex {
    pub(in crate::graph) fn from_edges(edges: &[Edge]) -> Self {
        #[cfg(test)]
        NAMED_IMPORT_ORIGIN_INDEX_BUILDS.set(NAMED_IMPORT_ORIGIN_INDEX_BUILDS.get() + 1);

        let mut index: FxHashMap<ReferenceSite, FxHashMap<String, StarReferenceOrigin>> =
            FxHashMap::default();
        for edge in edges {
            for sym in &edge.symbols {
                let ImportedName::Named(name) = &sym.imported_name else {
                    continue;
                };
                index
                    .entry((edge.source, sym.import_span.start, sym.import_span.end))
                    .or_default()
                    .insert(
                        name.clone(),
                        StarReferenceOrigin::NamedImport {
                            local_name: sym.local_name.clone(),
                            is_type_only: sym.is_type_only,
                        },
                    );
            }
        }
        Self(index)
    }

    fn get(&self, reference: SymbolReference, name: &str) -> Option<&StarReferenceOrigin> {
        self.0
            .get(&(
                reference.from_file,
                reference.import_span.start,
                reference.import_span.end,
            ))
            .and_then(|origins| origins.get(name))
    }
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
    /// Indices into `source.exports` whose name exactly matches `name`, prebuilt
    /// once per source module by `build_named_export_index` (issue #1843 follow-up).
    matching_exports: &'a [usize],
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
        matching_exports,
        source_id,
        module_by_id,
        triggering_is_type_only,
        source_has_star_re_exports,
        existing_files,
        synthetic_stubs,
    } = input;

    if name == "default" {
        return false;
    }

    if !matching_exports.is_empty() {
        apply_star_refs_to_matching_exports(ApplyMatchingStarRefs {
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
        })
    } else if source_has_star_re_exports {
        create_synthetic_exports_for_refs(CreateSyntheticExports {
            source,
            name,
            export_name: ExportName::Named(name.to_string()),
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

struct MatchingStarExports {
    type_indices: Vec<usize>,
    value_indices: Vec<usize>,
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

    let can_synthesize = source_has_star_re_exports;
    let mut exports = matching_star_exports(source, matching_exports);
    let (needs_type_export, needs_value_export) = required_matching_star_exports(
        refs,
        module_by_id,
        !exports.type_indices.is_empty() || can_synthesize,
        !exports.value_indices.is_empty() || (can_synthesize && !triggering_is_type_only),
        triggering_is_type_only,
    );

    let mut changed = ensure_matching_star_exports(EnsureMatchingStarExports {
        source,
        name,
        source_id,
        needs_type_export,
        needs_value_export,
        can_synthesize,
        exports: &mut exports,
        synthetic_stubs,
    });
    changed |= attach_matching_star_refs(AttachMatchingStarRefs {
        source,
        refs,
        module_by_id,
        triggering_is_type_only,
        exports: &exports,
        existing_files,
    });
    changed
}

fn matching_star_exports(source: &ModuleNode, matching_exports: &[usize]) -> MatchingStarExports {
    MatchingStarExports {
        type_indices: matching_exports
            .iter()
            .copied()
            .filter(|idx| source.exports[*idx].is_type_only)
            .collect(),
        value_indices: matching_exports
            .iter()
            .copied()
            .filter(|idx| !source.exports[*idx].is_type_only)
            .collect(),
    }
}

fn required_matching_star_exports(
    refs: &[StarReference],
    module_by_id: &FxHashMap<FileId, &ResolvedModule>,
    effective_has_type_exports: bool,
    effective_has_value_exports: bool,
    triggering_is_type_only: bool,
) -> (bool, bool) {
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
    (needs_type_export, needs_value_export)
}

struct EnsureMatchingStarExports<'a> {
    source: &'a mut ModuleNode,
    name: &'a str,
    source_id: FileId,
    needs_type_export: bool,
    needs_value_export: bool,
    can_synthesize: bool,
    exports: &'a mut MatchingStarExports,
    synthetic_stubs: &'a mut FxHashSet<(FileId, String, bool)>,
}

fn ensure_matching_star_exports(input: EnsureMatchingStarExports<'_>) -> bool {
    let EnsureMatchingStarExports {
        source,
        name,
        source_id,
        needs_type_export,
        needs_value_export,
        can_synthesize,
        exports,
        synthetic_stubs,
    } = input;

    let mut changed = false;
    if needs_type_export && exports.type_indices.is_empty() && can_synthesize {
        changed |= create_empty_synthetic_export(source, source_id, name, true, synthetic_stubs);
        if let Some(idx) = matching_synthetic_export_index(source, name, true) {
            exports.type_indices.push(idx);
        }
    }
    if needs_value_export && exports.value_indices.is_empty() && can_synthesize {
        changed |= create_empty_synthetic_export(source, source_id, name, false, synthetic_stubs);
        if let Some(idx) = matching_synthetic_export_index(source, name, false) {
            exports.value_indices.push(idx);
        }
    }
    changed
}

fn matching_synthetic_export_index(
    source: &ModuleNode,
    name: &str,
    is_type_only: bool,
) -> Option<usize> {
    source
        .exports
        .iter()
        .position(|export| export.name.matches_str(name) && export.is_type_only == is_type_only)
}

struct AttachMatchingStarRefs<'a> {
    source: &'a mut ModuleNode,
    refs: &'a [StarReference],
    module_by_id: &'a FxHashMap<FileId, &'a ResolvedModule>,
    triggering_is_type_only: bool,
    exports: &'a MatchingStarExports,
    existing_files: &'a mut FxHashSet<FileId>,
}

fn attach_matching_star_refs(input: AttachMatchingStarRefs<'_>) -> bool {
    let AttachMatchingStarRefs {
        source,
        refs,
        module_by_id,
        triggering_is_type_only,
        exports,
        existing_files,
    } = input;

    let mut type_refs = Vec::new();
    let mut value_refs = Vec::new();
    for star_ref in refs {
        let (attach_type_exports, attach_value_exports) = star_ref.attach_targets(
            module_by_id,
            !exports.type_indices.is_empty(),
            !exports.value_indices.is_empty(),
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
        changed |=
            attach_star_refs_to_exports(source, &exports.type_indices, &type_refs, existing_files);
    }
    if !value_refs.is_empty() {
        changed |= attach_star_refs_to_exports(
            source,
            &exports.value_indices,
            &value_refs,
            existing_files,
        );
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

fn attach_star_refs_to_exports(
    source: &mut ModuleNode,
    export_indices: &[usize],
    references: &[SymbolReference],
    existing_files: &mut FxHashSet<FileId>,
) -> bool {
    let mut changed = false;
    for export_idx in export_indices {
        #[cfg(test)]
        STAR_REFERENCE_SET_REBUILDS.set(STAR_REFERENCE_SET_REBUILDS.get() + 1);

        existing_files.clear();
        existing_files.extend(
            source.exports[*export_idx]
                .references
                .iter()
                .map(|r| r.from_file),
        );
        for reference in references {
            if existing_files.insert(reference.from_file) {
                source.exports[*export_idx].references.push(*reference);
                changed = true;
            }
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

#[cfg(test)]
mod star_index_tests {
    use super::*;
    use std::path::PathBuf;

    fn named(name: &str) -> ExportSymbol {
        export_symbol(ExportName::Named(name.to_string()))
    }

    fn export_symbol(name: ExportName) -> ExportSymbol {
        ExportSymbol {
            name,
            is_type_only: false,
            is_side_effect_used: false,
            visibility: VisibilityTag::None,
            expected_unused_reason: None,
            span: oxc_span::Span::new(0, 0),
            references: Vec::new(),
            members: Vec::new(),
        }
    }

    fn module_with(exports: Vec<ExportSymbol>) -> ModuleNode {
        ModuleNode {
            file_id: FileId(0),
            path: PathBuf::from("/project/source.ts"),
            edge_range: 0..0,
            exports,
            re_exports: Vec::new(),
            flags: 0,
        }
    }

    /// The prebuilt name -> indices map must reproduce, byte-for-byte, the indices
    /// the removed per-name `source.exports` scan produced: exact
    /// `ExportName::Named` matching, ascending index order, `Default` excluded.
    /// This pins the behavior-preserving contract for a wide-barrel source module.
    #[test]
    fn named_export_index_matches_reference_scan() {
        // A wide barrel source: many named exports, a duplicate name, and a
        // Default export interleaved to prove index alignment survives it.
        let module = module_with(vec![
            named("a"),
            named("b"),
            export_symbol(ExportName::Default),
            named("c"),
            named("b"), // duplicate name at a later index
            named("d"),
        ]);

        let index = build_named_export_index(&module);

        // Reference computation mirrors the old inline scan for each candidate name.
        let reference = |name: &str| -> Vec<usize> {
            let export_name = ExportName::Named(name.to_string());
            module
                .exports
                .iter()
                .enumerate()
                .filter_map(|(idx, export)| (export.name == export_name).then_some(idx))
                .collect()
        };

        for name in ["a", "b", "c", "d"] {
            let looked_up = index.get(name).cloned().unwrap_or_default();
            assert_eq!(looked_up, reference(name), "index mismatch for `{name}`");
        }

        // A repeated name keeps both indices in ascending order (the `Default`
        // export at index 2 is skipped, so the second `b` lands at index 4).
        assert_eq!(index.get("b").map(Vec::as_slice), Some(&[1usize, 4][..]));

        // Default is never keyed as a named export, and an absent name is empty.
        assert!(!index.contains_key("default"));
        assert!(!index.contains_key("missing"));
    }
}
