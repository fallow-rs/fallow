//! Ephemeral indexes shared by namespace propagation passes.

use std::collections::VecDeque;

use rustc_hash::{FxHashMap, FxHashSet};

use fallow_types::discover::FileId;
use fallow_types::extract::{ImportedName, ModuleLoadMechanism};

use crate::resolve::{ResolvedImport, ResolvedModule};

use super::ModuleGraph;
use super::types::{ReferencePathId, ReferencePathInterner};

#[derive(Default)]
struct ReExportTargets {
    named: FxHashMap<String, Vec<(FileId, String)>>,
    star_barrels: Vec<FileId>,
}

/// One resolved consumer import matching a reachable re-export name.
pub(super) struct ConsumerImport<'a> {
    pub(super) consumer: &'a ResolvedModule,
    pub(super) import: &'a ResolvedImport,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct NamespaceRouteId(usize);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct NamespaceRouteNode {
    parent: Option<NamespaceRouteId>,
    file_id: FileId,
    exported_name: String,
}

#[derive(Default)]
struct NamespaceRouteInterner {
    nodes: Vec<NamespaceRouteNode>,
    ids: FxHashMap<NamespaceRouteNode, NamespaceRouteId>,
}

impl NamespaceRouteInterner {
    fn extend(
        &mut self,
        parent: Option<NamespaceRouteId>,
        file_id: FileId,
        exported_name: &str,
    ) -> NamespaceRouteId {
        let node = NamespaceRouteNode {
            parent,
            file_id,
            exported_name: exported_name.to_string(),
        };
        if let Some(route) = self.ids.get(&node) {
            return *route;
        }
        let route = NamespaceRouteId(self.nodes.len());
        self.nodes.push(node.clone());
        self.ids.insert(node, route);
        route
    }

    fn contains(
        &self,
        mut route: Option<NamespaceRouteId>,
        file_id: FileId,
        exported_name: &str,
    ) -> bool {
        while let Some(route_id) = route {
            let Some(node) = self.nodes.get(route_id.0) else {
                return false;
            };
            if node.file_id == file_id && node.exported_name == exported_name {
                return true;
            }
            route = node.parent;
        }
        false
    }
}

/// One outward-reachable namespace export plus its exact inward re-export route.
pub(super) struct ReachableNamespaceExport {
    pub(super) file_id: FileId,
    pub(super) exported_name: String,
    route: Option<NamespaceRouteId>,
}

/// All simple named/star re-export paths reachable from one namespace export.
///
/// Distinct routes to the same `(file, name)` remain distinct. Coverage must
/// evaluate the route actually used by each consumer rather than collapsing a
/// diamond into aggregate file reachability.
pub(super) struct ReachableNamespaceExports {
    exports: Vec<ReachableNamespaceExport>,
    routes: NamespaceRouteInterner,
}

struct NamespaceTraversal {
    reachable: ReachableNamespaceExports,
    frontier: VecDeque<usize>,
    seen: FxHashSet<(FileId, String, Option<NamespaceRouteId>)>,
}

impl NamespaceTraversal {
    fn new(seed_file: FileId, seed_name: &str) -> Self {
        let seed = ReachableNamespaceExport {
            file_id: seed_file,
            exported_name: seed_name.to_string(),
            route: None,
        };
        Self {
            reachable: ReachableNamespaceExports {
                exports: vec![seed],
                routes: NamespaceRouteInterner::default(),
            },
            frontier: VecDeque::from([0]),
            seen: FxHashSet::from_iter([(seed_file, seed_name.to_string(), None)]),
        }
    }

    fn push(
        &mut self,
        source_file: FileId,
        source_name: &str,
        source_route: Option<NamespaceRouteId>,
        barrel_file: FileId,
        exported_name: &str,
    ) {
        if (barrel_file == source_file && exported_name == source_name)
            || self
                .reachable
                .routes
                .contains(source_route, barrel_file, exported_name)
        {
            return;
        }
        let route = Some(
            self.reachable
                .routes
                .extend(source_route, source_file, source_name),
        );
        if !self
            .seen
            .insert((barrel_file, exported_name.to_string(), route))
        {
            return;
        }
        self.reachable.exports.push(ReachableNamespaceExport {
            file_id: barrel_file,
            exported_name: exported_name.to_string(),
            route,
        });
        self.frontier.push_back(self.reachable.exports.len() - 1);
    }
}

impl ReachableNamespaceExports {
    pub(super) fn iter(&self) -> impl Iterator<Item = &ReachableNamespaceExport> {
        self.exports.iter()
    }

    fn route_hops(&self, export: &ReachableNamespaceExport) -> impl Iterator<Item = FileId> + '_ {
        let mut next = export.route;
        std::iter::from_fn(move || {
            let route = next?;
            let node = self.routes.nodes.get(route.0)?;
            next = node.parent;
            Some(node.file_id)
        })
    }

    /// Intern the complete consumer-to-target module-load path.
    pub(super) fn consumer_path(
        &self,
        export: &ReachableNamespaceExport,
        consumer: &ConsumerImport<'_>,
        final_target: FileId,
        final_mechanism: ModuleLoadMechanism,
        reference_paths: &mut ReferencePathInterner,
    ) -> ReferencePathId {
        let direct_mechanism = if consumer.import.target.is_commonjs_require() {
            ModuleLoadMechanism::CommonJsRequire
        } else {
            ModuleLoadMechanism::EsModule
        };
        let mut path = reference_paths.direct(export.file_id, direct_mechanism);
        for hop in self.route_hops(export) {
            path = reference_paths.extend(path, hop, ModuleLoadMechanism::EsModule);
        }
        reference_paths.extend(path, final_target, final_mechanism)
    }

    /// Intern the synthetic external-entry-to-target path for an entry barrel.
    pub(super) fn entry_path(
        &self,
        export: &ReachableNamespaceExport,
        final_target: FileId,
        final_mechanism: ModuleLoadMechanism,
        reference_paths: &mut ReferencePathInterner,
    ) -> ReferencePathId {
        let mut hops = self.route_hops(export);
        let Some(first_hop) = hops.next() else {
            return reference_paths.direct(final_target, final_mechanism);
        };
        let mut path = reference_paths.direct(first_hop, ModuleLoadMechanism::EsModule);
        for hop in hops {
            path = reference_paths.extend(path, hop, ModuleLoadMechanism::EsModule);
        }
        reference_paths.extend(path, final_target, final_mechanism)
    }
}

/// Build-only indexes used by both namespace propagation passes.
pub(super) struct NamespacePropagationIndexes<'a> {
    re_exports_by_source: FxHashMap<FileId, ReExportTargets>,
    consumers_by_target: FxHashMap<FileId, FxHashMap<String, Vec<ConsumerImport<'a>>>>,
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
                    targets.star_barrels.push(module.file_id);
                } else if edge.imported_name != "*" {
                    targets
                        .named
                        .entry(edge.imported_name.clone())
                        .or_default()
                        .push((module.file_id, edge.exported_name.clone()));
                }
            }
        }

        let mut consumers_by_target: FxHashMap<FileId, FxHashMap<String, Vec<ConsumerImport<'a>>>> =
            FxHashMap::default();
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
                consumers_by_target
                    .entry(target)
                    .or_default()
                    .entry(imported_name.to_string())
                    .or_default()
                    .push(ConsumerImport { consumer, import });
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
        }
    }

    pub(super) fn enumerate_reachable_barrels(
        &self,
        seed_file: FileId,
        seed_name: &str,
    ) -> ReachableNamespaceExports {
        let mut traversal = NamespaceTraversal::new(seed_file, seed_name);

        while let Some(export_index) = traversal.frontier.pop_front() {
            let source_file = traversal.reachable.exports[export_index].file_id;
            let source_name = traversal.reachable.exports[export_index]
                .exported_name
                .clone();
            let source_route = traversal.reachable.exports[export_index].route;
            let Some(targets) = self.re_exports_by_source.get(&source_file) else {
                continue;
            };
            if let Some(named) = targets.named.get(source_name.as_str()) {
                for (barrel_file, exported_name) in named {
                    traversal.push(
                        source_file,
                        &source_name,
                        source_route,
                        *barrel_file,
                        exported_name,
                    );
                }
            }
            for &barrel_file in &targets.star_barrels {
                traversal.push(
                    source_file,
                    &source_name,
                    source_route,
                    barrel_file,
                    &source_name,
                );
            }
        }

        traversal.reachable
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
