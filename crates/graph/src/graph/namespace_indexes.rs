//! Ephemeral indexes shared by namespace propagation passes.

use std::collections::VecDeque;

use rustc_hash::FxHashMap;

use fallow_types::discover::FileId;
use fallow_types::extract::{ImportedName, ModuleLoadMechanism};

use crate::resolve::{ResolvedImport, ResolvedModule};

use super::ModuleGraph;
use super::effective_exports::ExportNamespaces;
use super::types::{
    ReferencePathId, ReferencePathInterner, ReferenceRouteGraphId, ReferenceRouteGraphSpec,
    ReferenceRouteNodeId, ReferenceRouteNodeSpec,
};
use super::{EffectiveExportResolution, ExportNamespace};

#[derive(Default)]
struct ReExportTargets {
    named: FxHashMap<String, Vec<(FileId, String, bool)>>,
    star_barrels: Vec<(FileId, bool)>,
}

/// One resolved consumer import matching a reachable re-export name.
pub(super) struct ConsumerImport<'a> {
    pub(super) consumer: &'a ResolvedModule,
    pub(super) import: &'a ResolvedImport,
    pub(super) namespaces: ExportNamespaces,
}

/// One namespace export state in the compact transition graph.
pub(super) struct ReachableNamespaceExport {
    pub(super) file_id: FileId,
    pub(super) exported_name: String,
    state_index: usize,
    /// Inward transitions from this outward export toward the namespace seed.
    inward: Vec<usize>,
}

/// All reachable `(file, export-name)` states for one namespace export.
///
/// A state is retained once, while named/star diamonds become transition
/// edges. Cycles remain ordinary graph cycles and are evaluated later by the
/// profile-aware monotone worklist; no simple paths are materialized.
pub(super) struct ReachableNamespaceExports {
    exports: Vec<ReachableNamespaceExport>,
}

struct NamespaceTraversal {
    reachable: ReachableNamespaceExports,
    frontier: VecDeque<usize>,
    state_by_export: FxHashMap<(FileId, String), usize>,
}

impl NamespaceTraversal {
    fn new(seed_file: FileId, seed_name: &str) -> Self {
        let seed = ReachableNamespaceExport {
            file_id: seed_file,
            exported_name: seed_name.to_string(),
            state_index: 0,
            inward: Vec::new(),
        };
        Self {
            reachable: ReachableNamespaceExports {
                exports: vec![seed],
            },
            frontier: VecDeque::from([0]),
            state_by_export: FxHashMap::from_iter([((seed_file, seed_name.to_string()), 0)]),
        }
    }

    fn connect(&mut self, source_index: usize, barrel_file: FileId, exported_name: &str) {
        let key = (barrel_file, exported_name.to_string());
        let barrel_index = if let Some(index) = self.state_by_export.get(&key) {
            *index
        } else {
            let index = self.reachable.exports.len();
            self.reachable.exports.push(ReachableNamespaceExport {
                file_id: barrel_file,
                exported_name: exported_name.to_string(),
                state_index: index,
                inward: Vec::new(),
            });
            self.state_by_export.insert(key, index);
            self.frontier.push_back(index);
            index
        };
        if source_index != barrel_index {
            self.reachable.exports[barrel_index]
                .inward
                .push(source_index);
        }
    }

    fn finish(mut self) -> ReachableNamespaceExports {
        for export in &mut self.reachable.exports {
            export.inward.sort_unstable();
            export.inward.dedup();
        }
        self.reachable
    }
}

/// One canonical persisted route graph plus the local node for every export
/// state in `ReachableNamespaceExports`.
pub(super) enum NamespaceReferenceRoutes {
    Untracked,
    Exact {
        graph: ReferenceRouteGraphId,
        route_node_by_state: Vec<ReferenceRouteNodeId>,
        terminal: ReferenceRouteNodeId,
    },
}

impl NamespaceReferenceRoutes {
    /// Intern the complete consumer-to-target route without expanding its
    /// named/star alternatives.
    pub(super) fn consumer_path(
        &self,
        export: &ReachableNamespaceExport,
        consumer: &ConsumerImport<'_>,
        reference_paths: &mut ReferencePathInterner,
    ) -> Option<ReferencePathId> {
        let Self::Exact {
            graph,
            route_node_by_state,
            terminal,
        } = self
        else {
            return None;
        };
        let mechanism = if consumer.import.target.is_commonjs_require() {
            ModuleLoadMechanism::CommonJsRequire
        } else {
            ModuleLoadMechanism::EsModule
        };
        reference_paths.route(
            None,
            *graph,
            route_node_by_state[export.state_index],
            *terminal,
            Some(mechanism),
        )
    }

    /// Intern an external-entry traversal. The entry module is the reference
    /// source, so traversal begins after its export state.
    pub(super) fn entry_path(
        &self,
        export: &ReachableNamespaceExport,
        reference_paths: &mut ReferencePathInterner,
    ) -> Option<ReferencePathId> {
        let Self::Exact {
            graph,
            route_node_by_state,
            terminal,
        } = self
        else {
            return None;
        };
        reference_paths.route(
            None,
            *graph,
            route_node_by_state[export.state_index],
            *terminal,
            None,
        )
    }
}

impl ReachableNamespaceExports {
    pub(super) fn iter(&self) -> impl Iterator<Item = &ReachableNamespaceExport> {
        self.exports.iter()
    }

    /// Intern one canonical transition graph shared by every consumer and
    /// entry-point reference for this namespace seed.
    pub(super) fn intern_routes(
        &self,
        final_target: FileId,
        final_mechanism: ModuleLoadMechanism,
        reference_paths: &mut ReferencePathInterner,
    ) -> NamespaceReferenceRoutes {
        if !reference_paths.tracks_provenance() {
            return NamespaceReferenceRoutes::Untracked;
        }

        let mut canonical_order: Vec<usize> = (0..self.exports.len()).collect();
        canonical_order.sort_unstable_by(|&left, &right| {
            let left_export = &self.exports[left];
            let right_export = &self.exports[right];
            (left_export.file_id.0, left_export.exported_name.as_str())
                .cmp(&(right_export.file_id.0, right_export.exported_name.as_str()))
        });

        let mut route_node_by_state = vec![ReferenceRouteNodeId(0); self.exports.len()];
        for (route_index, state_index) in canonical_order.iter().copied().enumerate() {
            route_node_by_state[state_index] = ReferenceRouteNodeId(route_index as u32);
        }
        let terminal = ReferenceRouteNodeId(self.exports.len() as u32);

        let mut nodes = Vec::with_capacity(self.exports.len() + 1);
        for state_index in canonical_order {
            let export = &self.exports[state_index];
            let mut successors: Vec<_> = export
                .inward
                .iter()
                .map(|&inward| route_node_by_state[inward])
                .collect();
            if state_index == 0 {
                successors.push(terminal);
            }
            nodes.push(ReferenceRouteNodeSpec::new(
                export.file_id,
                ModuleLoadMechanism::EsModule,
                successors,
            ));
        }
        nodes.push(ReferenceRouteNodeSpec::new(
            final_target,
            final_mechanism,
            Vec::new(),
        ));

        let graph = reference_paths.intern_route_graph(ReferenceRouteGraphSpec::new(nodes));
        NamespaceReferenceRoutes::Exact {
            graph,
            route_node_by_state,
            terminal,
        }
    }

    #[cfg(test)]
    pub(super) fn state_count(&self) -> usize {
        self.exports.len()
    }

    #[cfg(test)]
    pub(super) fn transition_count(&self) -> usize {
        self.exports.iter().map(|export| export.inward.len()).sum()
    }
}

/// Build-only indexes used by both namespace propagation passes.
pub(super) struct NamespacePropagationIndexes<'a> {
    re_exports_by_source: FxHashMap<FileId, ReExportTargets>,
    consumers_by_target: FxHashMap<FileId, FxHashMap<String, Vec<ConsumerImport<'a>>>>,
    consumer_namespaces: ExportNamespaces,
}

impl<'a> NamespacePropagationIndexes<'a> {
    pub(super) fn new(
        graph: &ModuleGraph,
        module_by_id: &FxHashMap<FileId, &'a ResolvedModule>,
    ) -> Self {
        let mut re_exports_by_source: FxHashMap<FileId, ReExportTargets> = FxHashMap::default();
        for module in &graph.modules {
            for edge in &module.re_exports {
                let targets = re_exports_by_source.entry(edge.source_file).or_default();
                if edge.imported_name == "*" && edge.exported_name == "*" {
                    targets
                        .star_barrels
                        .push((module.file_id, edge.is_type_only));
                } else if edge.imported_name != "*" {
                    targets
                        .named
                        .entry(edge.imported_name.clone())
                        .or_default()
                        .push((
                            module.file_id,
                            edge.exported_name.clone(),
                            edge.is_type_only,
                        ));
                }
            }
        }
        for targets in re_exports_by_source.values_mut() {
            targets
                .star_barrels
                .sort_unstable_by_key(|(file_id, type_only)| (file_id.0, *type_only));
            targets.star_barrels.dedup();
            for named in targets.named.values_mut() {
                named.sort_unstable_by(|left, right| {
                    (left.0.0, left.1.as_str(), left.2).cmp(&(right.0.0, right.1.as_str(), right.2))
                });
                named.dedup();
            }
        }

        let mut consumers_by_target: FxHashMap<FileId, FxHashMap<String, Vec<ConsumerImport<'a>>>> =
            FxHashMap::default();
        let mut consumer_namespaces = ExportNamespaces::default();
        for consumer in module_by_id.values() {
            for import in &consumer.resolved_imports {
                let Some(target) = import.target.internal_file_id() else {
                    continue;
                };
                let imported_name = match &import.info.imported_name {
                    ImportedName::Named(name) => name.as_str(),
                    ImportedName::Default => "default",
                    _ => continue,
                };
                let namespaces = consumer_import_namespaces(consumer, import);
                if namespaces.is_empty() {
                    continue;
                }
                consumer_namespaces.extend(namespaces);
                consumers_by_target
                    .entry(target)
                    .or_default()
                    .entry(imported_name.to_string())
                    .or_default()
                    .push(ConsumerImport {
                        consumer,
                        import,
                        namespaces,
                    });
            }
        }
        for by_name in consumers_by_target.values_mut() {
            for consumers in by_name.values_mut() {
                consumers.sort_unstable_by_key(|consumer| {
                    (
                        consumer.consumer.file_id.0,
                        consumer.import.info.span.start,
                        consumer.import.info.span.end,
                    )
                });
            }
        }

        Self {
            re_exports_by_source,
            consumers_by_target,
            consumer_namespaces,
        }
    }

    pub(super) const fn has_consumers_in(&self, namespace: ExportNamespace) -> bool {
        self.consumer_namespaces.contains(namespace)
    }

    pub(super) fn enumerate_reachable_barrels(
        &self,
        graph: &ModuleGraph,
        seed_file: FileId,
        seed_name: &str,
        namespace: ExportNamespace,
    ) -> ReachableNamespaceExports {
        self.enumerate_reachable_barrels_with(
            seed_file,
            seed_name,
            namespace,
            |source_file, source_name, barrel_file, barrel_name| {
                uniquely_forwards_binding(
                    graph,
                    source_file,
                    source_name,
                    barrel_file,
                    barrel_name,
                    namespace,
                )
            },
        )
    }

    fn enumerate_reachable_barrels_with(
        &self,
        seed_file: FileId,
        seed_name: &str,
        namespace: ExportNamespace,
        mut forwards: impl FnMut(FileId, &str, FileId, &str) -> bool,
    ) -> ReachableNamespaceExports {
        let mut traversal = NamespaceTraversal::new(seed_file, seed_name);

        while let Some(source_index) = traversal.frontier.pop_front() {
            let source_file = traversal.reachable.exports[source_index].file_id;
            let source_name = traversal.reachable.exports[source_index]
                .exported_name
                .clone();
            let Some(targets) = self.re_exports_by_source.get(&source_file) else {
                continue;
            };
            if let Some(named) = targets.named.get(source_name.as_str()) {
                for (barrel_file, exported_name, is_type_only) in named {
                    if (namespace == ExportNamespace::Type || !is_type_only)
                        && forwards(source_file, &source_name, *barrel_file, exported_name)
                    {
                        traversal.connect(source_index, *barrel_file, exported_name);
                    }
                }
            }
            for &(barrel_file, is_type_only) in &targets.star_barrels {
                if (namespace == ExportNamespace::Type || !is_type_only)
                    && forwards(source_file, &source_name, barrel_file, &source_name)
                {
                    traversal.connect(source_index, barrel_file, &source_name);
                }
            }
        }

        traversal.finish()
    }

    pub(super) fn consumers_for(
        &self,
        target: FileId,
        imported_name: &str,
    ) -> &[ConsumerImport<'a>] {
        self.consumers_by_target
            .get(&target)
            .and_then(|by_name| by_name.get(imported_name))
            .map_or(&[], Vec::as_slice)
    }
}

fn consumer_import_namespaces(
    consumer: &ResolvedModule,
    import: &ResolvedImport,
) -> ExportNamespaces {
    let local_name = import.info.local_name.as_str();
    if local_name.is_empty() || consumer.unused_import_bindings.contains(local_name) {
        return ExportNamespaces::default();
    }
    let mut namespaces = ExportNamespaces::default();
    if import.info.is_type_only {
        namespaces.insert(ExportNamespace::Type);
        return namespaces;
    }
    let uses_type = consumer
        .type_referenced_import_bindings
        .iter()
        .any(|binding| binding == local_name);
    let uses_value = consumer
        .value_referenced_import_bindings
        .iter()
        .any(|binding| binding == local_name);
    if uses_type {
        namespaces.insert(ExportNamespace::Type);
    }
    if uses_value || !uses_type {
        namespaces.insert(ExportNamespace::Value);
    }
    namespaces
}

fn uniquely_forwards_binding(
    graph: &ModuleGraph,
    source_file: FileId,
    source_name: &str,
    barrel_file: FileId,
    barrel_name: &str,
    namespace: ExportNamespace,
) -> bool {
    matches!(
        (
            graph.resolve_export(source_file, source_name, namespace),
            graph.resolve_export(barrel_file, barrel_name, namespace),
        ),
        (
            EffectiveExportResolution::Unique(source),
            EffectiveExportResolution::Unique(barrel),
        ) if source == barrel
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_indexes() -> NamespacePropagationIndexes<'static> {
        NamespacePropagationIndexes {
            re_exports_by_source: FxHashMap::default(),
            consumers_by_target: FxHashMap::default(),
            consumer_namespaces: ExportNamespaces::default(),
        }
    }

    #[test]
    fn dense_namespace_cycle_retains_one_state_per_export() {
        const BARREL_COUNT: u32 = 14;
        let mut indexes = empty_indexes();
        for source in 0..BARREL_COUNT {
            let targets = indexes
                .re_exports_by_source
                .entry(FileId(source))
                .or_default();
            targets.star_barrels.extend(
                (0..BARREL_COUNT)
                    .filter(|&barrel| barrel != source)
                    .map(|barrel| (FileId(barrel), false)),
            );
        }

        let reachable = indexes.enumerate_reachable_barrels_with(
            FileId(0),
            "Ns",
            ExportNamespace::Value,
            |_, _, _, _| true,
        );

        assert_eq!(reachable.state_count(), BARREL_COUNT as usize);
        assert_eq!(
            reachable.transition_count(),
            (BARREL_COUNT * (BARREL_COUNT - 1)) as usize
        );
    }

    #[test]
    fn repeated_namespace_diamonds_grow_by_states_and_edges_not_paths() {
        const LAYERS: u32 = 24;
        let mut indexes = empty_indexes();
        let mut previous = vec![FileId(0)];
        let mut next_file = 1_u32;
        for _ in 0..LAYERS {
            let next = vec![FileId(next_file), FileId(next_file + 1)];
            next_file += 2;
            for source in &previous {
                indexes
                    .re_exports_by_source
                    .entry(*source)
                    .or_default()
                    .star_barrels
                    .extend(next.iter().copied().map(|file| (file, false)));
            }
            previous = next;
        }

        let reachable = indexes.enumerate_reachable_barrels_with(
            FileId(0),
            "Ns",
            ExportNamespace::Value,
            |_, _, _, _| true,
        );

        assert_eq!(reachable.state_count(), 1 + (LAYERS as usize * 2));
        assert_eq!(
            reachable.transition_count(),
            2 + ((LAYERS as usize - 1) * 4)
        );
    }
}
