//! Propagation functions for re-export chain resolution.
//!
//! Handles both star (`export * from`) and named (`export { foo } from`) re-exports,
//! including entry-point special cases where exports are consumed externally.

use rustc_hash::{FxHashMap, FxHashSet};

#[cfg(test)]
use std::cell::Cell;

use fallow_types::discover::FileId;
use fallow_types::extract::{ExportName, ModuleLoadMechanism, VisibilityTag};

use crate::graph::effective_exports::{
    EffectiveExportIndex, EffectiveExportResolution, ExportNamespace,
};
use crate::graph::types::{
    ExportSymbol, ModuleNode, ReferenceKind, ReferencePathInterner, RoutedReference,
    RoutedReferenceKey, SymbolReference,
};
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
    pub(in crate::graph) effective_exports: &'a EffectiveExportIndex,
    pub(in crate::graph) barrel_id: FileId,
    pub(in crate::graph) barrel_idx: usize,
    pub(in crate::graph) source_id: FileId,
    pub(in crate::graph) source_idx: usize,
    pub(in crate::graph) entry_star_targets: &'a FxHashSet<FileId>,
    pub(in crate::graph) triggering_is_type_only: bool,
    pub(in crate::graph) synthetic_stubs: &'a mut FxHashSet<(FileId, String, bool)>,
    pub(in crate::graph) reference_paths: &'a mut ReferencePathInterner,
}

pub(in crate::graph) fn propagate_star_re_export(input: StarReExportPropagation<'_>) -> bool {
    let StarReExportPropagation {
        modules,
        edges,
        edges_by_target,
        named_import_origin_index,
        module_by_id,
        effective_exports,
        barrel_id,
        barrel_idx,
        source_id,
        source_idx,
        entry_star_targets,
        triggering_is_type_only,
        synthetic_stubs,
        reference_paths,
    } = input;

    if modules[barrel_idx].is_entry_point()
        || entry_star_targets.contains(&modules[barrel_idx].file_id)
    {
        return propagate_entry_point_star(EntryPointStarPropagation {
            modules,
            barrel_id,
            source_idx,
            source_id,
            effective_exports,
            triggering_is_type_only,
            reference_paths,
        });
    }

    let barrel_file_id = modules[barrel_idx].file_id;
    let refs_by_name = collect_star_refs_by_name(StarReferenceCollection {
        modules,
        edges,
        edges_by_target,
        named_import_origin_index,
        barrel_file_id,
        barrel_idx,
        reference_paths,
    });

    let source_has_star_re_exports = modules[source_idx]
        .re_exports
        .iter()
        .any(|re| re.exported_name == "*");

    let matching_exports_by_name = build_named_export_index(&modules[source_idx]);

    let mut changed = false;
    let mut existing_references: FxHashSet<RoutedReferenceKey> = FxHashSet::default();
    let source = &mut modules[source_idx];
    for (name, refs) in &refs_by_name {
        let type_resolves = effective_exports.resolves_through(
            barrel_id,
            name,
            source_id,
            name,
            ExportNamespace::Type,
        );
        let value_resolves = effective_exports.resolves_through(
            barrel_id,
            name,
            source_id,
            name,
            ExportNamespace::Value,
        );
        if !type_resolves && !value_resolves {
            continue;
        }
        let matching_exports: &[usize] = matching_exports_by_name
            .get(name.as_str())
            .map_or(&[], Vec::as_slice);
        changed |= apply_star_refs_to_source(ApplyStarRefs {
            source: &mut *source,
            name,
            refs,
            type_resolves,
            value_resolves,
            matching_exports,
            source_id,
            module_by_id,
            triggering_is_type_only,
            source_has_star_re_exports,
            existing_references: &mut existing_references,
            synthetic_stubs: &mut *synthetic_stubs,
            reference_paths,
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
struct StarReferenceCollection<'a> {
    modules: &'a [ModuleNode],
    edges: &'a [Edge],
    edges_by_target: &'a FxHashMap<FileId, Vec<usize>>,
    named_import_origin_index: &'a NamedImportOriginIndex,
    barrel_file_id: FileId,
    barrel_idx: usize,
    reference_paths: &'a mut ReferencePathInterner,
}

fn collect_star_refs_by_name(
    input: StarReferenceCollection<'_>,
) -> FxHashMap<String, Vec<StarReference>> {
    let StarReferenceCollection {
        modules,
        edges,
        edges_by_target,
        named_import_origin_index,
        barrel_file_id,
        barrel_idx,
        reference_paths,
    } = input;
    let named_refs = named_star_refs(edges, edges_by_target, barrel_file_id, reference_paths);
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
    reference_paths: &mut ReferencePathInterner,
) -> Vec<(String, StarReference)> {
    edges_by_target
        .get(&barrel_file_id)
        .map(|indices| {
            indices
                .iter()
                .flat_map(|&idx| named_refs_for_edge(&edges[idx], reference_paths))
                .collect()
        })
        .unwrap_or_default()
}

fn named_refs_for_edge(
    edge: &Edge,
    reference_paths: &mut ReferencePathInterner,
) -> Vec<(String, StarReference)> {
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
                    routed: RoutedReference {
                        reference: SymbolReference {
                            from_file: edge.source,
                            kind: ReferenceKind::NamedImport,
                            namespace: if sym.is_type_only {
                                ExportNamespace::Type
                            } else {
                                ExportNamespace::Value
                            },
                            import_span: sym.import_span,
                        },
                        path: reference_paths.direct(edge.target, sym.mechanism),
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
                .routed_references()
                .map(|routed| {
                    barrel_star_ref(
                        routed,
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
    routed: RoutedReference,
    name: &str,
    is_type_only: bool,
    named_import_origin_index: &NamedImportOriginIndex,
) -> StarReference {
    StarReference {
        routed,
        origin: named_import_origin_index
            .get(routed.reference, name)
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
    routed: RoutedReference,
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
    type_resolves: bool,
    value_resolves: bool,
    /// Indices into `source.exports` whose name exactly matches `name`, prebuilt
    /// once per source module by `build_named_export_index` (issue #1843 follow-up).
    matching_exports: &'a [usize],
    source_id: FileId,
    module_by_id: &'a FxHashMap<FileId, &'a ResolvedModule>,
    triggering_is_type_only: bool,
    source_has_star_re_exports: bool,
    existing_references: &'a mut FxHashSet<RoutedReferenceKey>,
    synthetic_stubs: &'a mut FxHashSet<(FileId, String, bool)>,
    reference_paths: &'a mut ReferencePathInterner,
}

/// Attach the collected references for one re-exported name to the source
/// module, creating a synthetic stub when the source forwards via its own
/// `export *`. Returns `true` if any reference or stub was added.
fn apply_star_refs_to_source(input: ApplyStarRefs<'_>) -> bool {
    let ApplyStarRefs {
        source,
        name,
        refs,
        type_resolves,
        value_resolves,
        matching_exports,
        source_id,
        module_by_id,
        triggering_is_type_only,
        source_has_star_re_exports,
        existing_references,
        synthetic_stubs,
        reference_paths,
    } = input;

    if name == "default" {
        return false;
    }

    if !matching_exports.is_empty() {
        apply_star_refs_to_matching_exports(ApplyMatchingStarRefs {
            source,
            name,
            refs,
            type_resolves,
            value_resolves,
            source_id,
            module_by_id,
            triggering_is_type_only,
            source_has_star_re_exports,
            matching_exports,
            existing_references,
            synthetic_stubs,
            reference_paths,
        })
    } else if source_has_star_re_exports {
        create_synthetic_exports_for_refs(CreateSyntheticExports {
            source,
            name,
            export_name: ExportName::Named(name.to_string()),
            refs,
            type_resolves,
            value_resolves,
            source_id,
            module_by_id,
            triggering_is_type_only,
            synthetic_stubs,
            reference_paths,
        })
    } else {
        false
    }
}

struct ApplyMatchingStarRefs<'a> {
    source: &'a mut ModuleNode,
    name: &'a str,
    refs: &'a [StarReference],
    type_resolves: bool,
    value_resolves: bool,
    source_id: FileId,
    module_by_id: &'a FxHashMap<FileId, &'a ResolvedModule>,
    triggering_is_type_only: bool,
    source_has_star_re_exports: bool,
    matching_exports: &'a [usize],
    existing_references: &'a mut FxHashSet<RoutedReferenceKey>,
    synthetic_stubs: &'a mut FxHashSet<(FileId, String, bool)>,
    reference_paths: &'a mut ReferencePathInterner,
}

struct MatchingStarExports {
    type_indices: Vec<usize>,
    value_indices: Vec<usize>,
}

#[derive(Clone, Copy)]
struct StarExportTargets {
    has_type: bool,
    has_value: bool,
    type_resolves: bool,
    value_resolves: bool,
}

fn apply_star_refs_to_matching_exports(input: ApplyMatchingStarRefs<'_>) -> bool {
    let ApplyMatchingStarRefs {
        source,
        name,
        refs,
        type_resolves,
        value_resolves,
        source_id,
        module_by_id,
        triggering_is_type_only,
        source_has_star_re_exports,
        matching_exports,
        existing_references,
        synthetic_stubs,
        reference_paths,
    } = input;

    let can_synthesize = source_has_star_re_exports;
    let mut exports = matching_star_exports(source, matching_exports);
    let (needs_type_export, needs_value_export) = required_matching_star_exports(
        refs,
        module_by_id,
        type_resolves && (!exports.type_indices.is_empty() || can_synthesize),
        value_resolves
            && (!exports.value_indices.is_empty() || (can_synthesize && !triggering_is_type_only)),
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
        type_resolves,
        value_resolves,
        exports: &exports,
        existing_references,
        source_id,
        reference_paths,
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
            StarExportTargets {
                has_type: effective_has_type_exports,
                has_value: effective_has_value_exports,
                type_resolves: effective_has_type_exports,
                value_resolves: effective_has_value_exports,
            },
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
    if needs_type_export
        && exports.type_indices.is_empty()
        && can_synthesize
        && let Some(idx) = synthesize_and_locate_star_export(
            source,
            source_id,
            name,
            true,
            synthetic_stubs,
            &mut changed,
        )
    {
        exports.type_indices.push(idx);
    }
    if needs_value_export
        && exports.value_indices.is_empty()
        && can_synthesize
        && let Some(idx) = synthesize_and_locate_star_export(
            source,
            source_id,
            name,
            false,
            synthetic_stubs,
            &mut changed,
        )
    {
        exports.value_indices.push(idx);
    }
    changed
}

/// Synthesize the star-re-export stub for `name`/`is_type_only` (if not already
/// present) and return the index star references should attach to, preserving the
/// exact first-match semantics of the positional [`matching_synthetic_export_index`]
/// scan it replaces.
///
/// Fast path: a freshly appended stub lands at `exports.len() - 1`, and reaching this
/// branch guarantees that stub is the sole match, so the O(source_exports) `.position()`
/// scan is elided (issue #1916, follow-up to the #1914 main-path index). The branch only
/// runs when `exports.{type,value}_indices` is empty, i.e. the source carries no earlier
/// `Named(name)` export of that type-ness (otherwise `build_named_export_index` would have
/// populated the index and this branch would have been skipped, which also means an
/// already-synthesized stub from a prior fixpoint visit cannot re-enter it, so `create`
/// always appends here). `name` is likewise never `"default"` on this path: the caller
/// `apply_star_refs_to_source` returns early for `"default"` before ever calling into the
/// matching-exports synthesis.
///
/// The positional [`matching_synthetic_export_index`] fallback is retained as a defensive
/// exact-semantics preserver should either upstream invariant ever change (a `"default"`
/// name, whose exports are not keyed into the named index, or a non-appending `create`).
fn synthesize_and_locate_star_export(
    source: &mut ModuleNode,
    source_id: FileId,
    name: &str,
    is_type_only: bool,
    synthetic_stubs: &mut FxHashSet<(FileId, String, bool)>,
    changed: &mut bool,
) -> Option<usize> {
    let appended =
        create_empty_synthetic_export(source, source_id, name, is_type_only, synthetic_stubs);
    *changed |= appended;
    if appended && name != "default" {
        return Some(source.exports.len() - 1);
    }
    matching_synthetic_export_index(source, name, is_type_only)
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
    type_resolves: bool,
    value_resolves: bool,
    exports: &'a MatchingStarExports,
    existing_references: &'a mut FxHashSet<RoutedReferenceKey>,
    source_id: FileId,
    reference_paths: &'a mut ReferencePathInterner,
}

fn attach_matching_star_refs(input: AttachMatchingStarRefs<'_>) -> bool {
    let AttachMatchingStarRefs {
        source,
        refs,
        module_by_id,
        triggering_is_type_only,
        type_resolves,
        value_resolves,
        exports,
        existing_references,
        source_id,
        reference_paths,
    } = input;

    let mut type_refs = Vec::new();
    let mut value_refs = Vec::new();
    for star_ref in refs {
        let (attach_type_exports, attach_value_exports) = star_ref.attach_targets(
            module_by_id,
            StarExportTargets {
                has_type: !exports.type_indices.is_empty(),
                has_value: !exports.value_indices.is_empty(),
                type_resolves,
                value_resolves,
            },
            triggering_is_type_only,
        );
        let routed = through_re_export(star_ref.routed, source_id, reference_paths);
        if attach_type_exports {
            let mut typed = routed;
            typed.reference.namespace = ExportNamespace::Type;
            type_refs.push(typed);
        }
        if attach_value_exports {
            let mut valued = routed;
            valued.reference.namespace = ExportNamespace::Value;
            value_refs.push(valued);
        }
    }

    let mut changed = false;
    if !type_refs.is_empty() {
        changed |= attach_star_refs_to_exports(
            source,
            &exports.type_indices,
            &type_refs,
            existing_references,
        );
    }
    if !value_refs.is_empty() {
        changed |= attach_star_refs_to_exports(
            source,
            &exports.value_indices,
            &value_refs,
            existing_references,
        );
    }
    changed
}

struct CreateSyntheticExports<'a> {
    source: &'a mut ModuleNode,
    name: &'a str,
    export_name: ExportName,
    refs: &'a [StarReference],
    type_resolves: bool,
    value_resolves: bool,
    source_id: FileId,
    module_by_id: &'a FxHashMap<FileId, &'a ResolvedModule>,
    triggering_is_type_only: bool,
    synthetic_stubs: &'a mut FxHashSet<(FileId, String, bool)>,
    reference_paths: &'a mut ReferencePathInterner,
}

fn create_synthetic_exports_for_refs(input: CreateSyntheticExports<'_>) -> bool {
    let CreateSyntheticExports {
        source,
        name,
        export_name,
        refs,
        type_resolves,
        value_resolves,
        source_id,
        module_by_id,
        triggering_is_type_only,
        synthetic_stubs,
        reference_paths,
    } = input;

    let mut type_refs = Vec::new();
    let mut value_refs = Vec::new();
    for star_ref in refs {
        let (attach_type_exports, attach_value_exports) = star_ref.attach_targets(
            module_by_id,
            StarExportTargets {
                has_type: true,
                has_value: !triggering_is_type_only,
                type_resolves,
                value_resolves,
            },
            triggering_is_type_only,
        );
        let routed = through_re_export(star_ref.routed, source_id, reference_paths);
        if attach_type_exports {
            let mut typed = routed;
            typed.reference.namespace = ExportNamespace::Type;
            type_refs.push(typed);
        }
        if attach_value_exports {
            let mut valued = routed;
            valued.reference.namespace = ExportNamespace::Value;
            value_refs.push(valued);
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
    references: Vec<RoutedReference>,
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

    let mut export = ExportSymbol {
        name: export_name,
        is_type_only,
        is_side_effect_used: false,
        visibility: VisibilityTag::None,
        expected_unused_reason: None,
        span: oxc_span::Span::new(0, 0),
        references: Vec::new(),
        reference_paths: Vec::new(),
        members: Vec::new(),
    };
    for routed in references {
        export.push_reference(routed.reference, routed.path);
    }
    source.exports.push(export);
    true
}

fn attach_star_refs_to_exports(
    source: &mut ModuleNode,
    export_indices: &[usize],
    references: &[RoutedReference],
    existing_references: &mut FxHashSet<RoutedReferenceKey>,
) -> bool {
    let mut changed = false;
    for export_idx in export_indices {
        #[cfg(test)]
        STAR_REFERENCE_SET_REBUILDS.set(STAR_REFERENCE_SET_REBUILDS.get() + 1);

        existing_references.clear();
        existing_references.extend(
            source.exports[*export_idx]
                .routed_references()
                .map(RoutedReference::key),
        );
        for routed in references {
            if existing_references.insert(routed.key()) {
                source.exports[*export_idx].push_reference(routed.reference, routed.path);
                changed = true;
            }
        }
    }
    changed
}

fn through_re_export(
    routed: RoutedReference,
    target: FileId,
    reference_paths: &mut ReferencePathInterner,
) -> RoutedReference {
    RoutedReference {
        reference: routed.reference,
        path: reference_paths.extend(routed.path, target, ModuleLoadMechanism::EsModule),
    }
}

impl StarReference {
    fn attach_targets(
        &self,
        module_by_id: &FxHashMap<FileId, &ResolvedModule>,
        targets: StarExportTargets,
        triggering_is_type_only: bool,
    ) -> (bool, bool) {
        if triggering_is_type_only {
            return type_attach_targets(targets);
        }

        match &self.origin {
            StarReferenceOrigin::NamedImport {
                local_name,
                is_type_only,
            } => decide_named_import_attach_targets(
                module_by_id.get(&self.routed.reference.from_file),
                local_name,
                *is_type_only,
                targets,
            ),
            StarReferenceOrigin::BarrelExport { is_type_only } => {
                decide_barrel_export_attach_targets(*is_type_only, targets)
            }
        }
    }
}

fn decide_named_import_attach_targets(
    source_mod: Option<&&ResolvedModule>,
    local_name: &str,
    is_type_only: bool,
    targets: StarExportTargets,
) -> (bool, bool) {
    if is_type_only {
        return type_attach_targets(targets);
    }

    let uses_type = import_binding_has_type_usage(source_mod, local_name);
    let uses_value = import_binding_has_value_usage(source_mod, local_name);
    if !uses_type && !uses_value {
        return (false, targets.value_resolves && targets.has_value);
    }

    let (type_export, type_value_fallback) = type_attach_targets(StarExportTargets {
        type_resolves: uses_type && targets.type_resolves,
        ..targets
    });
    (
        type_export,
        type_value_fallback || (uses_value && targets.value_resolves && targets.has_value),
    )
}

const fn decide_barrel_export_attach_targets(
    is_type_only: bool,
    targets: StarExportTargets,
) -> (bool, bool) {
    if is_type_only {
        type_attach_targets(targets)
    } else {
        (false, targets.value_resolves && targets.has_value)
    }
}

const fn type_attach_targets(targets: StarExportTargets) -> (bool, bool) {
    if !targets.type_resolves {
        return (false, false);
    }
    (targets.has_type, !targets.has_type && targets.has_value)
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
struct EntryPointStarPropagation<'a> {
    modules: &'a mut [ModuleNode],
    barrel_id: FileId,
    source_idx: usize,
    source_id: FileId,
    effective_exports: &'a EffectiveExportIndex,
    triggering_is_type_only: bool,
    reference_paths: &'a mut ReferencePathInterner,
}

fn propagate_entry_point_star(input: EntryPointStarPropagation<'_>) -> bool {
    let EntryPointStarPropagation {
        modules,
        barrel_id,
        source_idx,
        source_id,
        effective_exports,
        triggering_is_type_only,
        reference_paths,
    } = input;
    let path = reference_paths.direct(source_id, ModuleLoadMechanism::EsModule);
    let mut changed = false;
    for export in &mut modules[source_idx].exports {
        if matches!(export.name, ExportName::Default) {
            continue;
        }
        let name = match &export.name {
            ExportName::Named(name) => name.as_str(),
            ExportName::Default => unreachable!(),
        };
        let resolves_type = effective_exports.resolves_through(
            barrel_id,
            name,
            source_id,
            name,
            ExportNamespace::Type,
        );
        let resolves_value = !export.is_type_only
            && !triggering_is_type_only
            && effective_exports.resolves_through(
                barrel_id,
                name,
                source_id,
                name,
                ExportNamespace::Value,
            );
        for (namespace, resolves) in [
            (ExportNamespace::Type, resolves_type),
            (ExportNamespace::Value, resolves_value),
        ] {
            if resolves && !export.has_reference_from(barrel_id, path, namespace) {
                export.push_reference(
                    SymbolReference {
                        from_file: barrel_id,
                        kind: ReferenceKind::ReExport,
                        namespace,
                        import_span: oxc_span::Span::new(0, 0),
                    },
                    path,
                );
                changed = true;
            }
        }
    }
    changed
}

fn namespace_export_indices(
    module: &ModuleNode,
    effective_exports: &EffectiveExportIndex,
    file_id: FileId,
    name: &str,
    namespace: ExportNamespace,
) -> Vec<usize> {
    let EffectiveExportResolution::Unique(binding) =
        effective_exports.resolve(file_id, name, namespace)
    else {
        return Vec::new();
    };
    let matching: Vec<_> = module
        .exports
        .iter()
        .enumerate()
        .filter(|(_, export)| export.name.matches_str(name))
        .map(|(index, _)| index)
        .collect();
    if binding.origin_file() != file_id || binding.origin_slot().is_none() {
        return matching;
    }
    let exact: Vec<_> = matching
        .iter()
        .copied()
        .filter(|&index| module.exports[index].is_type_only == (namespace == ExportNamespace::Type))
        .collect();
    if namespace == ExportNamespace::Type && exact.is_empty() {
        return matching
            .into_iter()
            .filter(|&index| !module.exports[index].is_type_only)
            .collect();
    }
    exact
}

fn namespace_references(
    module: &ModuleNode,
    name: &str,
    namespace: ExportNamespace,
) -> Vec<RoutedReference> {
    module
        .exports
        .iter()
        .filter(|export| export.name.matches_str(name))
        .flat_map(ExportSymbol::routed_references)
        .filter(|routed| routed.reference.namespace == namespace)
        .collect()
}

struct EffectiveOriginPropagation<'a> {
    modules: &'a mut [ModuleNode],
    effective_exports: &'a EffectiveExportIndex,
    source_id: FileId,
    imported_name: &'a str,
    namespace: ExportNamespace,
    references: &'a [RoutedReference],
    existing_refs: &'a mut FxHashSet<RoutedReferenceKey>,
    reference_paths: &'a mut ReferencePathInterner,
}

fn propagate_to_effective_origin(input: EffectiveOriginPropagation<'_>) -> bool {
    let EffectiveOriginPropagation {
        modules,
        effective_exports,
        source_id,
        imported_name,
        namespace,
        references,
        existing_refs,
        reference_paths,
    } = input;
    let Some(route) = super::super::effective_re_exports::effective_declaration_route(
        modules,
        effective_exports,
        source_id,
        imported_name,
        namespace,
    ) else {
        return false;
    };
    let origin_file = route.binding.origin_file();
    let Some(origin_slot) = route.binding.origin_slot() else {
        return false;
    };
    let mut origin_slots: Vec<usize> = if namespace == ExportNamespace::Type {
        effective_exports
            .declaration_group_slots(route.binding)
            .to_vec()
    } else {
        Vec::new()
    };
    if origin_slots.is_empty() {
        origin_slots.push(origin_slot);
    }
    let route = route.intern(reference_paths);
    let Some(origin) = modules.get_mut(origin_file.0 as usize) else {
        return false;
    };
    let mut changed = false;
    for origin_slot in origin_slots {
        existing_refs.clear();
        existing_refs.extend(
            origin.exports[origin_slot]
                .routed_references()
                .map(RoutedReference::key),
        );
        for reference in references {
            let routed = RoutedReference {
                reference: reference.reference,
                path: route.extend_path(reference.path, reference_paths),
            };
            if existing_refs.insert(routed.key()) {
                origin.exports[origin_slot].push_reference(routed.reference, routed.path);
                changed = true;
            }
        }
    }
    changed
}

fn same_effective_binding(
    effective_exports: &EffectiveExportIndex,
    barrel: (FileId, &str),
    source: (FileId, &str),
    namespace: ExportNamespace,
) -> bool {
    matches!(
        (
            effective_exports.resolve(barrel.0, barrel.1, namespace),
            effective_exports.resolve(source.0, source.1, namespace),
        ),
        (
            EffectiveExportResolution::Unique(barrel_binding),
            EffectiveExportResolution::Unique(source_binding),
        ) if barrel_binding == source_binding
    )
}

/// Handle named re-exports (`export { foo } from './source'`) — propagate barrel references
/// to the source module's matching export.
///
/// Returns `true` if any new references were added.
pub(in crate::graph) struct NamedReExportPropagation<'a> {
    pub(in crate::graph) modules: &'a mut [ModuleNode],
    pub(in crate::graph) effective_exports: &'a EffectiveExportIndex,
    pub(in crate::graph) barrel_id: FileId,
    pub(in crate::graph) barrel_idx: usize,
    pub(in crate::graph) source_id: FileId,
    pub(in crate::graph) source_idx: usize,
    pub(in crate::graph) imported_name: &'a str,
    pub(in crate::graph) exported_name: &'a str,
    pub(in crate::graph) is_type_only: bool,
    pub(in crate::graph) existing_refs: &'a mut FxHashSet<RoutedReferenceKey>,
    pub(in crate::graph) reference_paths: &'a mut ReferencePathInterner,
}

pub(in crate::graph) fn propagate_named_re_export(input: NamedReExportPropagation<'_>) -> bool {
    let NamedReExportPropagation {
        modules,
        effective_exports,
        barrel_id,
        barrel_idx,
        source_id,
        source_idx,
        imported_name,
        exported_name,
        is_type_only,
        existing_refs,
        reference_paths,
    } = input;

    let type_resolves = same_effective_binding(
        effective_exports,
        (barrel_id, exported_name),
        (source_id, imported_name),
        ExportNamespace::Type,
    );
    let value_resolves = !is_type_only
        && same_effective_binding(
            effective_exports,
            (barrel_id, exported_name),
            (source_id, imported_name),
            ExportNamespace::Value,
        );
    if !type_resolves && !value_resolves {
        return false;
    }

    let type_refs = if type_resolves {
        namespace_references(&modules[barrel_idx], exported_name, ExportNamespace::Type)
    } else {
        Vec::new()
    };
    let value_refs = if value_resolves {
        namespace_references(&modules[barrel_idx], exported_name, ExportNamespace::Value)
    } else {
        Vec::new()
    };

    if type_refs.is_empty() && value_refs.is_empty() {
        if modules[barrel_idx].is_entry_point() {
            return propagate_entry_point_named(EntryPointNamedPropagation {
                modules,
                effective_exports,
                barrel_id,
                source_id,
                source_idx,
                imported_name,
                type_resolves,
                value_resolves,
                reference_paths,
            });
        }
        return false;
    }

    let mut changed = false;
    for (namespace, refs) in [
        (ExportNamespace::Type, type_refs.as_slice()),
        (ExportNamespace::Value, value_refs.as_slice()),
    ] {
        let export_indices = namespace_export_indices(
            &modules[source_idx],
            effective_exports,
            source_id,
            imported_name,
            namespace,
        );
        if export_indices.is_empty() && !refs.is_empty() {
            changed |= propagate_to_effective_origin(EffectiveOriginPropagation {
                modules,
                effective_exports,
                source_id,
                imported_name,
                namespace,
                references: refs,
                existing_refs,
                reference_paths,
            });
            continue;
        }
        for export_idx in export_indices {
            let source = &mut modules[source_idx];
            existing_refs.clear();
            existing_refs.extend(
                source.exports[export_idx]
                    .routed_references()
                    .map(RoutedReference::key),
            );
            for ref_item in refs {
                let routed = through_re_export(*ref_item, source.file_id, reference_paths);
                if existing_refs.insert(routed.key()) {
                    source.exports[export_idx].push_reference(routed.reference, routed.path);
                    changed = true;
                }
            }
        }
    }
    changed
}

/// Entry point barrel with named re-export and no in-graph consumers — synthesize
/// a `ReExport` reference so the source export is correctly marked as used.
struct EntryPointNamedPropagation<'a> {
    modules: &'a mut [ModuleNode],
    effective_exports: &'a EffectiveExportIndex,
    barrel_id: FileId,
    source_id: FileId,
    source_idx: usize,
    imported_name: &'a str,
    type_resolves: bool,
    value_resolves: bool,
    reference_paths: &'a mut ReferencePathInterner,
}

fn propagate_entry_point_named(input: EntryPointNamedPropagation<'_>) -> bool {
    let EntryPointNamedPropagation {
        modules,
        effective_exports,
        barrel_id,
        source_id,
        source_idx,
        imported_name,
        type_resolves,
        value_resolves,
        reference_paths,
    } = input;
    let synthetic_path = reference_paths.direct(source_id, ModuleLoadMechanism::EsModule);
    let mut changed = false;
    for namespace in [ExportNamespace::Type, ExportNamespace::Value] {
        if (namespace == ExportNamespace::Type && !type_resolves)
            || (namespace == ExportNamespace::Value && !value_resolves)
        {
            continue;
        }
        for export_idx in namespace_export_indices(
            &modules[source_idx],
            effective_exports,
            source_id,
            imported_name,
            namespace,
        ) {
            let source = &mut modules[source_idx];
            if !source.exports[export_idx].has_reference_from(barrel_id, synthetic_path, namespace)
            {
                source.exports[export_idx].push_reference(
                    SymbolReference {
                        from_file: barrel_id,
                        kind: ReferenceKind::ReExport,
                        namespace,
                        import_span: oxc_span::Span::new(0, 0),
                    },
                    synthetic_path,
                );
                changed = true;
            }
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
            reference_paths: Vec::new(),
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
