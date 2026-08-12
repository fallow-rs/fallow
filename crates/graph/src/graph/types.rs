//! Shared graph types: module nodes, re-export edges, export symbols, and references.

use std::cmp::Ordering;
use std::num::NonZeroU32;
use std::ops::Range;
use std::path::PathBuf;

use fallow_types::discover::FileId;
use fallow_types::extract::{ExportName, ModuleLoadMechanism, VisibilityTag};
use rustc_hash::{FxHashMap, FxHashSet};

/// A single module in the graph.
///
/// Boolean flags are packed into a `u8` to keep the struct at 96 bytes
/// (down from 104 with 5 separate `bool` fields), improving cache line
/// utilization in hot graph traversal loops.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct ModuleNode {
    /// Unique identifier for this module.
    pub file_id: FileId,
    /// Absolute path to the module file.
    pub path: PathBuf,
    /// Range into the flat `edges` array.
    pub edge_range: Range<usize>,
    /// Exports declared by this module.
    pub exports: Vec<ExportSymbol>,
    /// Re-exports from this module (export { x } from './y', export * from './z').
    pub re_exports: Vec<ReExportEdge>,
    /// Packed boolean flags (entry point, reachability, CJS).
    pub(crate) flags: u8,
}

const FLAG_ENTRY_POINT: u8 = 1 << 0;
const FLAG_REACHABLE: u8 = 1 << 1;
const FLAG_RUNTIME_REACHABLE: u8 = 1 << 2;
const FLAG_TEST_REACHABLE: u8 = 1 << 3;
const FLAG_CJS_EXPORTS: u8 = 1 << 4;

impl ModuleNode {
    /// Whether this module is an entry point.
    #[inline]
    pub const fn is_entry_point(&self) -> bool {
        self.flags & FLAG_ENTRY_POINT != 0
    }

    /// Whether this module is reachable from any entry point.
    #[inline]
    pub const fn is_reachable(&self) -> bool {
        self.flags & FLAG_REACHABLE != 0
    }

    /// Whether this module is reachable from a runtime/application root.
    #[inline]
    pub const fn is_runtime_reachable(&self) -> bool {
        self.flags & FLAG_RUNTIME_REACHABLE != 0
    }

    /// Whether this module is reachable from a test root.
    #[inline]
    pub const fn is_test_reachable(&self) -> bool {
        self.flags & FLAG_TEST_REACHABLE != 0
    }

    /// Whether this module has CJS exports (module.exports / exports.*).
    #[inline]
    pub const fn has_cjs_exports(&self) -> bool {
        self.flags & FLAG_CJS_EXPORTS != 0
    }

    /// Set whether this module is an entry point.
    #[inline]
    pub fn set_entry_point(&mut self, v: bool) {
        if v {
            self.flags |= FLAG_ENTRY_POINT;
        } else {
            self.flags &= !FLAG_ENTRY_POINT;
        }
    }

    /// Set whether this module is reachable from any entry point.
    #[inline]
    pub fn set_reachable(&mut self, v: bool) {
        if v {
            self.flags |= FLAG_REACHABLE;
        } else {
            self.flags &= !FLAG_REACHABLE;
        }
    }

    /// Set whether this module is reachable from a runtime/application root.
    #[inline]
    pub(crate) fn set_runtime_reachable(&mut self, v: bool) {
        if v {
            self.flags |= FLAG_RUNTIME_REACHABLE;
        } else {
            self.flags &= !FLAG_RUNTIME_REACHABLE;
        }
    }

    /// Set whether this module is reachable from a test root.
    #[inline]
    pub(crate) fn set_test_reachable(&mut self, v: bool) {
        if v {
            self.flags |= FLAG_TEST_REACHABLE;
        } else {
            self.flags &= !FLAG_TEST_REACHABLE;
        }
    }

    /// Set whether this module has CJS exports.
    #[inline]
    pub fn set_cjs_exports(&mut self, v: bool) {
        if v {
            self.flags |= FLAG_CJS_EXPORTS;
        } else {
            self.flags &= !FLAG_CJS_EXPORTS;
        }
    }

    /// Build flags byte from individual booleans (used by graph construction).
    #[inline]
    pub(crate) fn flags_from(
        is_entry_point: bool,
        is_runtime_reachable: bool,
        has_cjs_exports: bool,
    ) -> u8 {
        let mut f = 0u8;
        if is_entry_point {
            f |= FLAG_ENTRY_POINT;
        }
        if is_runtime_reachable {
            f |= FLAG_RUNTIME_REACHABLE;
        }
        if has_cjs_exports {
            f |= FLAG_CJS_EXPORTS;
        }
        f
    }
}

/// A re-export edge, tracking which exports are forwarded from which module.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct ReExportEdge {
    /// The module being re-exported from.
    pub source_file: FileId,
    /// The name imported from the source (or "*" for star re-exports).
    pub imported_name: String,
    /// The name exported from this module.
    pub exported_name: String,
    /// Whether this is a type-only re-export.
    pub is_type_only: bool,
    /// Source span of the re-export declaration on this module, used for
    /// line-number reporting. `(0, 0)` for re-exports synthesized inside the
    /// graph layer (e.g., `export *` chain propagation, namespace narrowing).
    #[serde(with = "crate::cache::span_serde")]
    pub span: oxc_span::Span,
}

/// An export with reference tracking.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct ExportSymbol {
    /// The exported name (named or default).
    pub name: ExportName,
    /// Whether this is a type-only export.
    pub is_type_only: bool,
    /// Whether this export is registered through a runtime side effect at module
    /// load time (e.g. a Lit `@customElement('tag')` decorator or a
    /// `customElements.define('tag', ClassRef)` call). The unused-export
    /// detector treats this as an effective reference.
    pub is_side_effect_used: bool,
    /// Visibility tag from JSDoc/TSDoc comment (`@public`, `@internal`, `@alpha`, `@beta`).
    /// Exports with any visibility tag are never reported as unused.
    pub visibility: VisibilityTag,
    /// Human-authored reason on `@expected-unused -- <reason>`, when present.
    pub expected_unused_reason: Option<String>,
    /// Source span of the export declaration.
    #[serde(with = "crate::cache::span_serde")]
    pub span: oxc_span::Span,
    /// Which files reference this export.
    pub references: Vec<SymbolReference>,
    /// Interned provenance paths parallel to `references`, keyed by reference
    /// index (issue #2083).
    ///
    /// Only populated when the test-reachability plan requires reference
    /// provenance (a replacement mock exists). It stays empty for every other
    /// project so the reference list itself remains 16 bytes per entry and the
    /// side table allocates nothing. Entries can be `None` even when populated:
    /// legacy reachability stores no path, profiled reachability always does.
    #[serde(default)]
    pub reference_paths: Vec<Option<ReferencePathId>>,
    /// Members of this export (enum members, class members).
    ///
    /// `MemberInfo` is a shared `fallow-types` struct whose serde shape is
    /// serialize-only (its `span` uses `serialize_with` with no matching
    /// deserializer), so it cannot round-trip through a plain derive. The cache
    /// routes it through a dedicated lossless mirror in `crate::cache`.
    #[serde(with = "crate::cache::member_serde")]
    pub members: Vec<fallow_types::extract::MemberInfo>,
}

/// A reference to an export from another file.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct SymbolReference {
    /// The file that references this export.
    pub from_file: FileId,
    /// How the export is referenced.
    pub kind: ReferenceKind,
    /// Semantic namespace used by this reference.
    pub namespace: super::ExportNamespace,
    /// Byte span of the import statement in the referencing file.
    /// Used by the LSP to locate references for Code Lens navigation.
    #[serde(with = "crate::cache::span_serde")]
    pub import_span: oxc_span::Span,
}

/// Compact identifier for an interned reference path.
///
/// Opaque outside the graph crate; it only appears in the public API as the
/// element type of [`ExportSymbol::reference_paths`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct ReferencePathId(NonZeroU32);

/// A symbol reference paired with its optional provenance path while build
/// passes route it between exports, before it lands in a reference list plus
/// its provenance side table.
#[derive(Clone, Copy)]
pub(crate) struct RoutedReference {
    pub(crate) reference: SymbolReference,
    pub(crate) path: Option<ReferencePathId>,
}

pub(crate) type RoutedReferenceKey = (FileId, Option<ReferencePathId>, super::ExportNamespace);

impl RoutedReference {
    pub(crate) const fn key(self) -> RoutedReferenceKey {
        (
            self.reference.from_file,
            self.path,
            self.reference.namespace,
        )
    }
}

impl ExportSymbol {
    /// References that use one semantic namespace.
    pub fn references_in(
        &self,
        namespace: super::ExportNamespace,
    ) -> impl Iterator<Item = &SymbolReference> {
        self.references
            .iter()
            .filter(move |reference| reference.namespace == namespace)
    }

    /// Distinct physical reference sites, collapsing Type and Value uses of
    /// the same import while preserving different resolved routes.
    pub fn physical_references(&self) -> impl Iterator<Item = &SymbolReference> {
        let mut seen = (self.references.len() > 1).then(FxHashSet::default);
        self.references
            .iter()
            .enumerate()
            .filter(move |(index, reference)| {
                seen.as_mut().is_none_or(|seen| {
                    seen.insert((
                        reference.from_file,
                        reference.import_span,
                        self.reference_path(*index),
                    ))
                })
            })
            .map(|(_, reference)| reference)
    }

    /// Number of distinct physical reference sites.
    #[must_use]
    pub fn physical_reference_count(&self) -> usize {
        self.physical_references().count()
    }

    /// Provenance path recorded for the reference at `index`, when tracked.
    pub(crate) fn reference_path(&self, index: usize) -> Option<ReferencePathId> {
        self.reference_paths.get(index).copied().flatten()
    }

    /// Whether a reference from `from_file` with this exact provenance path is
    /// already attached.
    pub(crate) fn has_reference_from(
        &self,
        from_file: FileId,
        path: Option<ReferencePathId>,
        namespace: super::ExportNamespace,
    ) -> bool {
        self.references
            .iter()
            .enumerate()
            .any(|(index, reference)| {
                reference.from_file == from_file
                    && self.reference_path(index) == path
                    && reference.namespace == namespace
            })
    }

    /// Attach `reference`, recording `path` in the provenance side table.
    ///
    /// The side table stays untouched until the first tracked path arrives, so
    /// projects without replacement mocks never allocate it.
    pub(crate) fn push_reference(
        &mut self,
        reference: SymbolReference,
        path: Option<ReferencePathId>,
    ) {
        if path.is_some() || !self.reference_paths.is_empty() {
            self.reference_paths.resize(self.references.len(), None);
            self.reference_paths.push(path);
        }
        self.references.push(reference);
    }

    /// Iterate references together with their recorded provenance paths.
    pub(crate) fn routed_references(&self) -> impl Iterator<Item = RoutedReference> + '_ {
        self.references
            .iter()
            .enumerate()
            .map(|(index, reference)| RoutedReference {
                reference: *reference,
                path: self.reference_path(index),
            })
    }
}

/// One conjunctive step in an interned export-reference route.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) enum ReferencePathNode {
    /// One ordinary module-load hop.
    Hop {
        /// Previous conjunctive step, or `None` for a direct load.
        parent: Option<ReferencePathId>,
        /// Module loaded by this hop.
        target: FileId,
        /// Runtime mechanism used by this hop.
        mechanism: ModuleLoadMechanism,
    },
    /// One existential traversal through a compact namespace transition graph.
    Route {
        /// Previous conjunctive step, used when a namespace route is followed
        /// by another namespace segment.
        parent: Option<ReferencePathId>,
        /// Canonical transition graph containing `start` and `terminal`.
        graph: ReferenceRouteGraphId,
        /// Local graph node where traversal begins.
        start: ReferenceRouteNodeId,
        /// Local graph node that must be reachable.
        terminal: ReferenceRouteNodeId,
        /// Mechanism used by the consumer to load `start`. `None` means the
        /// reference source already owns the start module (entry points and
        /// concatenated route segments).
        start_mechanism: Option<ModuleLoadMechanism>,
    },
}

impl ReferencePathNode {
    pub(crate) const fn parent(self) -> Option<ReferencePathId> {
        match self {
            Self::Hop { parent, .. } | Self::Route { parent, .. } => parent,
        }
    }

    fn remap_parent(&mut self, remap: &[ReferencePathId]) {
        match self {
            Self::Hop { parent, .. } | Self::Route { parent, .. } => {
                *parent = parent.map(|path| remap[path.index()]);
            }
        }
    }
}

/// Build-time identifier for one compact namespace transition graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct ReferenceRouteGraphId(pub(crate) u32);

/// Node identifier local to one namespace transition graph.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub(crate) struct ReferenceRouteNodeId(pub(crate) u32);

/// One canonical node in a build-time namespace transition graph.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct ReferenceRouteNodeSpec {
    target: FileId,
    mechanism: ModuleLoadMechanism,
    successors: Vec<ReferenceRouteNodeId>,
}

impl ReferenceRouteNodeSpec {
    pub(crate) fn new(
        target: FileId,
        mechanism: ModuleLoadMechanism,
        mut successors: Vec<ReferenceRouteNodeId>,
    ) -> Self {
        successors.sort_unstable_by_key(|successor| successor.0);
        successors.dedup();
        Self {
            target,
            mechanism,
            successors,
        }
    }
}

/// Canonical build-time representation of one namespace transition graph.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct ReferenceRouteGraphSpec {
    nodes: Vec<ReferenceRouteNodeSpec>,
}

impl ReferenceRouteGraphSpec {
    pub(crate) fn new(nodes: Vec<ReferenceRouteNodeSpec>) -> Self {
        debug_assert!(nodes.iter().all(|node| {
            node.successors
                .iter()
                .all(|successor| successor.0 < nodes.len() as u32)
        }));
        Self { nodes }
    }
}

/// Persisted range for one canonical namespace transition graph.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct ReferenceRouteGraph {
    pub(crate) nodes: Range<u32>,
}

/// One persisted namespace transition node.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct ReferenceRouteNode {
    pub(crate) target: FileId,
    pub(crate) mechanism: ModuleLoadMechanism,
    pub(crate) successors: Range<u32>,
}

/// Cache-friendly persisted namespace transition graphs.
#[derive(Debug, Default, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct ReferenceRoutes {
    pub(crate) graphs: Vec<ReferenceRouteGraph>,
    pub(crate) nodes: Vec<ReferenceRouteNode>,
    pub(crate) edges: Vec<ReferenceRouteNodeId>,
}

impl ReferenceRoutes {
    #[cfg(test)]
    pub(crate) fn canonical_hops(
        &self,
        graph_id: ReferenceRouteGraphId,
        start: ReferenceRouteNodeId,
        terminal: ReferenceRouteNodeId,
        start_mechanism: Option<ModuleLoadMechanism>,
    ) -> Vec<(FileId, ModuleLoadMechanism)> {
        let Some(graph) = self.graphs.get(graph_id.0 as usize) else {
            return Vec::new();
        };
        let node_count = graph.nodes.end.saturating_sub(graph.nodes.start) as usize;
        let start_index = start.0 as usize;
        let terminal_index = terminal.0 as usize;
        if start_index >= node_count || terminal_index >= node_count {
            return Vec::new();
        }

        let mut predecessor = vec![None; node_count];
        let mut visited = vec![false; node_count];
        let mut queue = std::collections::VecDeque::from([start_index]);
        visited[start_index] = true;
        while let Some(local_index) = queue.pop_front() {
            if local_index == terminal_index {
                break;
            }
            let Some(node) = self.nodes.get(graph.nodes.start as usize + local_index) else {
                return Vec::new();
            };
            let Some(successors) = self
                .edges
                .get(node.successors.start as usize..node.successors.end as usize)
            else {
                return Vec::new();
            };
            for successor in successors {
                let successor_index = successor.0 as usize;
                if successor_index >= node_count || visited[successor_index] {
                    continue;
                }
                visited[successor_index] = true;
                predecessor[successor_index] = Some(local_index);
                queue.push_back(successor_index);
            }
        }
        if !visited[terminal_index] {
            return Vec::new();
        }

        let mut hops = Vec::new();
        let mut current = terminal_index;
        loop {
            let node = &self.nodes[graph.nodes.start as usize + current];
            if current != start_index {
                hops.push((node.target, node.mechanism));
            } else {
                if let Some(mechanism) = start_mechanism {
                    hops.push((node.target, mechanism));
                }
                break;
            }
            let Some(parent) = predecessor[current] else {
                return Vec::new();
            };
            current = parent;
        }
        hops
    }
}

/// Finalized linear paths plus compact namespace transition graphs.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct FinalizedReferencePaths {
    pub(crate) paths: Vec<ReferencePathNode>,
    pub(crate) routes: ReferenceRoutes,
}

/// Build-time interner for shared linked reference paths.
pub(crate) struct ReferencePathInterner {
    track_provenance: bool,
    nodes: Vec<ReferencePathNode>,
    metadata: Vec<ReferencePathMetadata>,
    ids: FxHashMap<ReferencePathNode, ReferencePathId>,
    route_graphs: Vec<ReferenceRouteGraphSpec>,
    route_graph_ids: FxHashMap<ReferenceRouteGraphSpec, ReferenceRouteGraphId>,
}

#[derive(Clone, Copy)]
struct ReferencePathMetadata {
    depth: usize,
    hop_target_bounds: Option<(FileId, FileId)>,
}

impl Default for ReferencePathInterner {
    fn default() -> Self {
        Self::new(true)
    }
}

impl ReferencePathInterner {
    pub(crate) fn new(track_provenance: bool) -> Self {
        Self {
            track_provenance,
            nodes: Vec::new(),
            metadata: Vec::new(),
            ids: FxHashMap::default(),
            route_graphs: Vec::new(),
            route_graph_ids: FxHashMap::default(),
        }
    }

    pub(crate) const fn tracks_provenance(&self) -> bool {
        self.track_provenance
    }

    /// Intern a direct consumer-to-target path.
    pub(crate) fn direct(
        &mut self,
        target: FileId,
        mechanism: ModuleLoadMechanism,
    ) -> Option<ReferencePathId> {
        self.track_provenance.then(|| {
            self.intern(ReferencePathNode::Hop {
                parent: None,
                target,
                mechanism,
            })
        })
    }

    /// Append one typed hop to an existing path.
    pub(crate) fn extend(
        &mut self,
        parent: Option<ReferencePathId>,
        target: FileId,
        mechanism: ModuleLoadMechanism,
    ) -> Option<ReferencePathId> {
        if !self.track_provenance {
            debug_assert!(parent.is_none());
            return None;
        }
        let Some(parent) = parent else {
            debug_assert!(false, "tracked reference paths require an interned parent");
            return None;
        };
        let may_contain_target = self
            .metadata
            .get(parent.index())
            .and_then(|metadata| metadata.hop_target_bounds)
            .is_some_and(|(minimum, maximum)| target.0 >= minimum.0 && target.0 <= maximum.0);
        if may_contain_target && self.contains_target(parent, target) {
            return Some(parent);
        }
        Some(self.intern(ReferencePathNode::Hop {
            parent: Some(parent),
            target,
            mechanism,
        }))
    }

    /// Intern one compact namespace transition graph.
    pub(crate) fn intern_route_graph(
        &mut self,
        graph: ReferenceRouteGraphSpec,
    ) -> ReferenceRouteGraphId {
        debug_assert!(self.track_provenance);
        if let Some(id) = self.route_graph_ids.get(&graph) {
            return *id;
        }
        let id = ReferenceRouteGraphId(self.route_graphs.len() as u32);
        self.route_graphs.push(graph.clone());
        self.route_graph_ids.insert(graph, id);
        id
    }

    /// Intern one existential traversal through a compact route graph.
    pub(crate) fn route(
        &mut self,
        parent: Option<ReferencePathId>,
        graph: ReferenceRouteGraphId,
        start: ReferenceRouteNodeId,
        terminal: ReferenceRouteNodeId,
        start_mechanism: Option<ModuleLoadMechanism>,
    ) -> Option<ReferencePathId> {
        if !self.track_provenance {
            return None;
        }
        Some(self.intern(ReferencePathNode::Route {
            parent,
            graph,
            start,
            terminal,
            start_mechanism,
        }))
    }

    fn contains_target(&self, mut path: ReferencePathId, target: FileId) -> bool {
        loop {
            let Some(node) = self.nodes.get(path.index()) else {
                return false;
            };
            if let ReferencePathNode::Hop {
                target: hop_target, ..
            } = node
                && *hop_target == target
            {
                return true;
            }
            let Some(parent) = node.parent() else {
                return false;
            };
            path = parent;
        }
    }

    fn intern(&mut self, node: ReferencePathNode) -> ReferencePathId {
        if let Some(path) = self.ids.get(&node) {
            return *path;
        }
        let path = ReferencePathId::from_index(self.nodes.len());
        let parent_metadata = node
            .parent()
            .and_then(|parent| self.metadata.get(parent.index()).copied());
        let depth = parent_metadata.map_or(0, |metadata| metadata.depth + 1);
        let hop_target_bounds = match node {
            ReferencePathNode::Hop { target, .. } => Some(
                parent_metadata
                    .and_then(|metadata| metadata.hop_target_bounds)
                    .map_or((target, target), |(minimum, maximum)| {
                        (
                            FileId(minimum.0.min(target.0)),
                            FileId(maximum.0.max(target.0)),
                        )
                    }),
            ),
            ReferencePathNode::Route { .. } => {
                parent_metadata.and_then(|metadata| metadata.hop_target_bounds)
            }
        };
        self.nodes.push(node);
        self.metadata.push(ReferencePathMetadata {
            depth,
            hop_target_bounds,
        });
        self.ids.insert(node, path);
        path
    }

    /// Finalize cache-friendly storage and assign canonical IDs.
    ///
    /// Paths are ordered depth-by-depth so every parent already has its final
    /// ID before its children are sorted. This keeps serialized graphs stable
    /// when equivalent imports or re-exports are discovered in another order.
    pub(crate) fn finalize(self, modules: &mut [ModuleNode]) -> FinalizedReferencePaths {
        if self.nodes.is_empty() && self.route_graphs.is_empty() {
            return FinalizedReferencePaths {
                paths: Vec::new(),
                routes: ReferenceRoutes::default(),
            };
        }

        let (routes, route_remap) = finalize_route_graphs(&self.route_graphs);
        let max_depth = self
            .metadata
            .iter()
            .map(|metadata| metadata.depth)
            .max()
            .unwrap_or(0);

        let mut paths_by_depth = vec![Vec::new(); max_depth.saturating_add(1)];
        for (old_index, metadata) in self.metadata.iter().enumerate() {
            paths_by_depth[metadata.depth].push(old_index);
        }

        let mut remap = vec![ReferencePathId::from_index(0); self.nodes.len()];
        let mut finalized = Vec::with_capacity(self.nodes.len());
        for mut paths in paths_by_depth {
            paths.sort_unstable_by(|&left, &right| {
                compare_path_nodes(self.nodes[left], self.nodes[right], &remap, &route_remap)
            });
            for old_index in paths {
                let mut node = self.nodes[old_index];
                node.remap_parent(&remap);
                if let ReferencePathNode::Route { graph, .. } = &mut node {
                    *graph = route_remap[graph.0 as usize];
                }
                let canonical = ReferencePathId::from_index(finalized.len());
                remap[old_index] = canonical;
                finalized.push(node);
            }
        }

        for path in modules
            .iter_mut()
            .flat_map(|module| &mut module.exports)
            .flat_map(|export| &mut export.reference_paths)
        {
            if let Some(existing) = *path {
                *path = Some(remap[existing.index()]);
            }
        }

        FinalizedReferencePaths {
            paths: finalized,
            routes,
        }
    }
}

fn compare_path_nodes(
    left: ReferencePathNode,
    right: ReferencePathNode,
    path_remap: &[ReferencePathId],
    route_remap: &[ReferenceRouteGraphId],
) -> Ordering {
    let left_parent = left.parent().map(|parent| path_remap[parent.index()].0);
    let right_parent = right.parent().map(|parent| path_remap[parent.index()].0);
    left_parent
        .cmp(&right_parent)
        .then_with(|| match (left, right) {
            (
                ReferencePathNode::Hop {
                    target: left_target,
                    mechanism: left_mechanism,
                    ..
                },
                ReferencePathNode::Hop {
                    target: right_target,
                    mechanism: right_mechanism,
                    ..
                },
            ) => {
                (left_target.0, left_mechanism as u8).cmp(&(right_target.0, right_mechanism as u8))
            }
            (ReferencePathNode::Hop { .. }, ReferencePathNode::Route { .. }) => Ordering::Less,
            (ReferencePathNode::Route { .. }, ReferencePathNode::Hop { .. }) => Ordering::Greater,
            (
                ReferencePathNode::Route {
                    graph: left_graph,
                    start: left_start,
                    terminal: left_terminal,
                    start_mechanism: left_mechanism,
                    ..
                },
                ReferencePathNode::Route {
                    graph: right_graph,
                    start: right_start,
                    terminal: right_terminal,
                    start_mechanism: right_mechanism,
                    ..
                },
            ) => (
                route_remap[left_graph.0 as usize].0,
                left_start.0,
                left_terminal.0,
                left_mechanism.map(|mechanism| mechanism as u8),
            )
                .cmp(&(
                    route_remap[right_graph.0 as usize].0,
                    right_start.0,
                    right_terminal.0,
                    right_mechanism.map(|mechanism| mechanism as u8),
                )),
        })
}

fn compare_route_graph_specs(
    left: &ReferenceRouteGraphSpec,
    right: &ReferenceRouteGraphSpec,
) -> Ordering {
    left.nodes.len().cmp(&right.nodes.len()).then_with(|| {
        left.nodes
            .iter()
            .zip(&right.nodes)
            .find_map(|(left_node, right_node)| {
                let ordering = (
                    left_node.target.0,
                    left_node.mechanism as u8,
                    &left_node.successors,
                )
                    .cmp(&(
                        right_node.target.0,
                        right_node.mechanism as u8,
                        &right_node.successors,
                    ));
                (ordering != Ordering::Equal).then_some(ordering)
            })
            .unwrap_or(Ordering::Equal)
    })
}

fn finalize_route_graphs(
    graphs: &[ReferenceRouteGraphSpec],
) -> (ReferenceRoutes, Vec<ReferenceRouteGraphId>) {
    let mut order: Vec<usize> = (0..graphs.len()).collect();
    order
        .sort_unstable_by(|&left, &right| compare_route_graph_specs(&graphs[left], &graphs[right]));

    let mut remap = vec![ReferenceRouteGraphId(0); graphs.len()];
    let mut finalized = ReferenceRoutes::default();
    for old_index in order {
        let graph_id = ReferenceRouteGraphId(finalized.graphs.len() as u32);
        remap[old_index] = graph_id;
        let node_start = finalized.nodes.len() as u32;
        for node in &graphs[old_index].nodes {
            let edge_start = finalized.edges.len() as u32;
            finalized.edges.extend_from_slice(&node.successors);
            finalized.nodes.push(ReferenceRouteNode {
                target: node.target,
                mechanism: node.mechanism,
                successors: edge_start..finalized.edges.len() as u32,
            });
        }
        finalized.graphs.push(ReferenceRouteGraph {
            nodes: node_start..finalized.nodes.len() as u32,
        });
    }
    (finalized, remap)
}

impl ReferencePathId {
    fn from_index(index: usize) -> Self {
        let Some(encoded) = u32::try_from(index)
            .ok()
            .and_then(|index| index.checked_add(1))
            .and_then(NonZeroU32::new)
        else {
            panic!("a process cannot allocate more than u32::MAX reference path nodes");
        };
        Self(encoded)
    }

    pub(crate) const fn index(self) -> usize {
        (self.0.get() - 1) as usize
    }
}

/// How an export is referenced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ReferenceKind {
    /// A named import (`import { foo }`).
    NamedImport,
    /// A default import (`import Foo`).
    DefaultImport,
    /// A namespace import (`import * as ns`).
    NamespaceImport,
    /// A re-export (`export { foo } from './bar'`).
    ReExport,
    /// A dynamic import (`import('./foo')`).
    DynamicImport,
    /// A side-effect import (`import './styles'`).
    SideEffectImport,
}

#[cfg(target_pointer_width = "64")]
const _: () = assert!(std::mem::size_of::<ExportSymbol>() == 136);
#[cfg(target_pointer_width = "64")]
const _: () = assert!(std::mem::size_of::<SymbolReference>() == 16);
#[cfg(target_pointer_width = "64")]
const _: () = assert!(std::mem::size_of::<ReExportEdge>() == 64);
#[cfg(all(target_pointer_width = "64", unix))]
const _: () = assert!(std::mem::size_of::<ModuleNode>() == 96);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::ExportNamespace;

    #[test]
    fn reference_kind_equality() {
        assert_eq!(ReferenceKind::NamedImport, ReferenceKind::NamedImport);
        assert_ne!(ReferenceKind::NamedImport, ReferenceKind::DefaultImport);
    }

    #[test]
    fn reference_kind_all_variants_are_distinct() {
        let all = [
            ReferenceKind::NamedImport,
            ReferenceKind::DefaultImport,
            ReferenceKind::NamespaceImport,
            ReferenceKind::ReExport,
            ReferenceKind::DynamicImport,
            ReferenceKind::SideEffectImport,
        ];
        for (i, a) in all.iter().enumerate() {
            for (j, b) in all.iter().enumerate() {
                if i == j {
                    assert_eq!(a, b);
                } else {
                    assert_ne!(a, b);
                }
            }
        }
    }

    #[test]
    fn reference_kind_copy() {
        let original = ReferenceKind::NamespaceImport;
        let copied = original;
        assert_eq!(original, copied);
    }

    #[test]
    fn reference_kind_debug_format() {
        let kind = ReferenceKind::DynamicImport;
        let debug_str = format!("{kind:?}");
        assert_eq!(debug_str, "DynamicImport");
    }

    fn module_with_reference_paths(paths: &[Option<ReferencePathId>]) -> ModuleNode {
        ModuleNode {
            file_id: FileId(0),
            path: PathBuf::from("/project/source.ts"),
            edge_range: 0..0,
            exports: vec![ExportSymbol {
                name: ExportName::Named("value".to_string()),
                is_type_only: false,
                is_side_effect_used: false,
                visibility: VisibilityTag::None,
                expected_unused_reason: None,
                span: oxc_span::Span::default(),
                references: paths
                    .iter()
                    .map(|_| SymbolReference {
                        from_file: FileId(0),
                        kind: ReferenceKind::NamedImport,
                        namespace: ExportNamespace::Value,
                        import_span: oxc_span::Span::default(),
                    })
                    .collect(),
                reference_paths: paths.to_vec(),
                members: Vec::new(),
            }],
            re_exports: Vec::new(),
            flags: 0,
        }
    }

    #[test]
    fn reference_path_metadata_tracks_exact_depth_and_hop_bounds() {
        let mut interner = ReferencePathInterner::default();
        let root = interner
            .direct(FileId(10), ModuleLoadMechanism::EsModule)
            .expect("tracked interner must return a path");
        let lower = interner
            .extend(Some(root), FileId(5), ModuleLoadMechanism::EsModule)
            .expect("tracked interner must extend a path");
        let upper = interner
            .extend(Some(lower), FileId(20), ModuleLoadMechanism::EsModule)
            .expect("tracked interner must extend a path");

        assert_eq!(interner.metadata[root.index()].depth, 0);
        assert_eq!(
            interner.metadata[lower.index()].hop_target_bounds,
            Some((FileId(5), FileId(10)))
        );
        assert_eq!(interner.metadata[upper.index()].depth, 2);
        assert_eq!(
            interner.metadata[upper.index()].hop_target_bounds,
            Some((FileId(5), FileId(20)))
        );

        let repeated = interner.extend(Some(upper), FileId(10), ModuleLoadMechanism::EsModule);
        assert_eq!(repeated, Some(upper));
    }

    #[test]
    fn finalized_reference_paths_are_independent_of_interning_order() {
        let mut first = ReferencePathInterner::default();
        let first_parent = first.direct(FileId(1), ModuleLoadMechanism::EsModule);
        let first_direct = first.direct(FileId(2), ModuleLoadMechanism::CommonJsRequire);
        let first_chain = first.extend(first_parent, FileId(3), ModuleLoadMechanism::EsModule);
        let mut first_modules = vec![module_with_reference_paths(&[first_direct, first_chain])];
        let first_nodes = first.finalize(&mut first_modules);

        let mut second = ReferencePathInterner::default();
        let second_direct = second.direct(FileId(2), ModuleLoadMechanism::CommonJsRequire);
        let second_parent = second.direct(FileId(1), ModuleLoadMechanism::EsModule);
        let second_chain = second.extend(second_parent, FileId(3), ModuleLoadMechanism::EsModule);
        let mut second_modules = vec![module_with_reference_paths(&[second_direct, second_chain])];
        let second_nodes = second.finalize(&mut second_modules);

        assert_eq!(first_nodes, second_nodes);
        assert_eq!(
            first_modules[0].exports[0].reference_paths,
            second_modules[0].exports[0].reference_paths
        );
    }

    fn two_hop_route(first: FileId, second: FileId) -> ReferenceRouteGraphSpec {
        ReferenceRouteGraphSpec::new(vec![
            ReferenceRouteNodeSpec::new(
                first,
                ModuleLoadMechanism::EsModule,
                vec![ReferenceRouteNodeId(1)],
            ),
            ReferenceRouteNodeSpec::new(second, ModuleLoadMechanism::EsModule, Vec::new()),
        ])
    }

    #[test]
    fn finalized_reference_routes_are_independent_of_interning_order() {
        let route_a = two_hop_route(FileId(1), FileId(2));
        let route_b = two_hop_route(FileId(3), FileId(4));

        let mut first = ReferencePathInterner::default();
        let first_a = first.intern_route_graph(route_a.clone());
        let first_b = first.intern_route_graph(route_b.clone());
        let first_b_path = first.route(
            None,
            first_b,
            ReferenceRouteNodeId(0),
            ReferenceRouteNodeId(1),
            Some(ModuleLoadMechanism::CommonJsRequire),
        );
        let first_a_path = first.route(
            None,
            first_a,
            ReferenceRouteNodeId(0),
            ReferenceRouteNodeId(1),
            Some(ModuleLoadMechanism::EsModule),
        );
        let mut first_modules = vec![module_with_reference_paths(&[first_b_path, first_a_path])];
        let first_paths = first.finalize(&mut first_modules);

        let mut second = ReferencePathInterner::default();
        let second_b = second.intern_route_graph(route_b);
        let second_a = second.intern_route_graph(route_a);
        let second_b_path = second.route(
            None,
            second_b,
            ReferenceRouteNodeId(0),
            ReferenceRouteNodeId(1),
            Some(ModuleLoadMechanism::CommonJsRequire),
        );
        let second_a_path = second.route(
            None,
            second_a,
            ReferenceRouteNodeId(0),
            ReferenceRouteNodeId(1),
            Some(ModuleLoadMechanism::EsModule),
        );
        let mut second_modules = vec![module_with_reference_paths(&[second_b_path, second_a_path])];
        let second_paths = second.finalize(&mut second_modules);

        assert_eq!(first_paths, second_paths);
        assert_eq!(
            first_modules[0].exports[0].reference_paths,
            second_modules[0].exports[0].reference_paths
        );
    }

    #[test]
    fn symbol_reference_construction() {
        let reference = SymbolReference {
            from_file: FileId(42),
            kind: ReferenceKind::NamedImport,
            namespace: ExportNamespace::Value,
            import_span: oxc_span::Span::new(10, 30),
        };
        assert_eq!(reference.from_file, FileId(42));
        assert_eq!(reference.kind, ReferenceKind::NamedImport);
        assert_eq!(reference.import_span.start, 10);
        assert_eq!(reference.import_span.end, 30);
    }

    #[test]
    fn symbol_reference_copy_preserves_all_fields() {
        let reference = SymbolReference {
            from_file: FileId(7),
            kind: ReferenceKind::ReExport,
            namespace: ExportNamespace::Value,
            import_span: oxc_span::Span::new(5, 25),
        };
        let copied = reference;
        assert_eq!(copied.from_file, reference.from_file);
        assert_eq!(copied.kind, reference.kind);
        assert_eq!(copied.import_span.start, reference.import_span.start);
        assert_eq!(copied.import_span.end, reference.import_span.end);
    }

    #[test]
    fn re_export_edge_construction() {
        let edge = ReExportEdge {
            source_file: FileId(3),
            imported_name: "*".to_string(),
            exported_name: "*".to_string(),
            is_type_only: false,
            span: oxc_span::Span::default(),
        };
        assert_eq!(edge.source_file, FileId(3));
        assert_eq!(edge.imported_name, "*");
        assert_eq!(edge.exported_name, "*");
        assert!(!edge.is_type_only);
    }

    #[test]
    fn re_export_edge_type_only() {
        let edge = ReExportEdge {
            source_file: FileId(1),
            imported_name: "MyType".to_string(),
            exported_name: "MyType".to_string(),
            is_type_only: true,
            span: oxc_span::Span::default(),
        };
        assert!(edge.is_type_only);
    }

    #[test]
    fn re_export_edge_renamed() {
        let edge = ReExportEdge {
            source_file: FileId(2),
            imported_name: "internal".to_string(),
            exported_name: "public".to_string(),
            is_type_only: false,
            span: oxc_span::Span::default(),
        };
        assert_ne!(edge.imported_name, edge.exported_name);
        assert_eq!(edge.imported_name, "internal");
        assert_eq!(edge.exported_name, "public");
    }

    #[test]
    fn export_symbol_named() {
        let sym = ExportSymbol {
            name: ExportName::Named("myFunction".to_string()),
            is_type_only: false,
            is_side_effect_used: false,
            visibility: VisibilityTag::None,
            expected_unused_reason: None,
            span: oxc_span::Span::new(0, 50),
            references: vec![],
            reference_paths: Vec::new(),
            members: vec![],
        };
        assert!(matches!(sym.name, ExportName::Named(ref n) if n == "myFunction"));
        assert!(!sym.is_type_only);
        assert_eq!(sym.visibility, VisibilityTag::None);
    }

    #[test]
    fn export_symbol_default() {
        let sym = ExportSymbol {
            name: ExportName::Default,
            is_type_only: false,
            is_side_effect_used: false,
            visibility: VisibilityTag::None,
            expected_unused_reason: None,
            span: oxc_span::Span::new(0, 20),
            references: vec![],
            reference_paths: Vec::new(),
            members: vec![],
        };
        assert!(matches!(sym.name, ExportName::Default));
    }

    #[test]
    fn export_symbol_public_tag() {
        let sym = ExportSymbol {
            name: ExportName::Named("api".to_string()),
            is_type_only: false,
            is_side_effect_used: false,
            visibility: VisibilityTag::Public,
            expected_unused_reason: None,
            span: oxc_span::Span::new(0, 10),
            references: vec![],
            reference_paths: Vec::new(),
            members: vec![],
        };
        assert_eq!(sym.visibility, VisibilityTag::Public);
    }

    #[test]
    fn export_symbol_type_only() {
        let sym = ExportSymbol {
            name: ExportName::Named("MyInterface".to_string()),
            is_type_only: true,
            is_side_effect_used: false,
            visibility: VisibilityTag::None,
            expected_unused_reason: None,
            span: oxc_span::Span::new(0, 30),
            references: vec![],
            reference_paths: Vec::new(),
            members: vec![],
        };
        assert!(sym.is_type_only);
    }

    #[test]
    fn export_symbol_with_references() {
        let sym = ExportSymbol {
            name: ExportName::Named("helper".to_string()),
            is_type_only: false,
            is_side_effect_used: false,
            visibility: VisibilityTag::None,
            expected_unused_reason: None,
            span: oxc_span::Span::new(0, 20),
            references: vec![
                SymbolReference {
                    from_file: FileId(1),
                    kind: ReferenceKind::NamedImport,
                    namespace: ExportNamespace::Value,
                    import_span: oxc_span::Span::new(0, 10),
                },
                SymbolReference {
                    from_file: FileId(2),
                    kind: ReferenceKind::ReExport,
                    namespace: ExportNamespace::Value,
                    import_span: oxc_span::Span::new(5, 15),
                },
            ],
            reference_paths: vec![
                Some(ReferencePathId::from_index(0)),
                Some(ReferencePathId::from_index(1)),
            ],
            members: vec![],
        };
        assert_eq!(sym.references.len(), 2);
        assert_eq!(sym.references[0].from_file, FileId(1));
        assert_eq!(sym.references[1].kind, ReferenceKind::ReExport);
    }

    #[test]
    fn push_reference_without_paths_never_allocates_the_side_table() {
        let mut export = ExportSymbol {
            name: ExportName::Named("value".to_string()),
            is_type_only: false,
            is_side_effect_used: false,
            visibility: VisibilityTag::None,
            expected_unused_reason: None,
            span: oxc_span::Span::default(),
            references: Vec::new(),
            reference_paths: Vec::new(),
            members: Vec::new(),
        };
        for id in 0..3 {
            export.push_reference(
                SymbolReference {
                    from_file: FileId(id),
                    kind: ReferenceKind::NamedImport,
                    namespace: ExportNamespace::Value,
                    import_span: oxc_span::Span::default(),
                },
                None,
            );
        }
        assert_eq!(export.references.len(), 3);
        assert!(export.reference_paths.is_empty());
        assert_eq!(export.reference_paths.capacity(), 0);
        assert_eq!(export.reference_path(1), None);
        assert!(export.has_reference_from(FileId(1), None, ExportNamespace::Value));
        assert!(!export.has_reference_from(FileId(9), None, ExportNamespace::Value));
    }

    #[test]
    fn push_reference_backfills_the_side_table_on_the_first_tracked_path() {
        let mut export = ExportSymbol {
            name: ExportName::Named("value".to_string()),
            is_type_only: false,
            is_side_effect_used: false,
            visibility: VisibilityTag::None,
            expected_unused_reason: None,
            span: oxc_span::Span::default(),
            references: Vec::new(),
            reference_paths: Vec::new(),
            members: Vec::new(),
        };
        let reference = SymbolReference {
            from_file: FileId(0),
            kind: ReferenceKind::NamedImport,
            namespace: ExportNamespace::Value,
            import_span: oxc_span::Span::default(),
        };
        export.push_reference(reference, None);
        let tracked = ReferencePathId::from_index(4);
        export.push_reference(reference, Some(tracked));
        export.push_reference(reference, None);

        assert_eq!(export.reference_paths, vec![None, Some(tracked), None]);
        assert_eq!(export.reference_path(0), None);
        assert_eq!(export.reference_path(1), Some(tracked));
        assert!(export.has_reference_from(FileId(0), Some(tracked), ExportNamespace::Value));
        assert!(!export.has_reference_from(
            FileId(0),
            Some(ReferencePathId::from_index(7)),
            ExportNamespace::Value
        ));
    }

    #[test]
    fn module_node_construction() {
        let mut node = ModuleNode {
            file_id: FileId(0),
            path: PathBuf::from("/project/src/index.ts"),
            edge_range: 0..5,
            exports: vec![],
            re_exports: vec![],
            flags: ModuleNode::flags_from(true, true, false),
        };
        node.set_reachable(true);
        assert_eq!(node.file_id, FileId(0));
        assert!(node.is_entry_point());
        assert!(node.is_reachable());
        assert!(node.is_runtime_reachable());
        assert!(!node.is_test_reachable());
        assert!(!node.has_cjs_exports());
        assert_eq!(node.edge_range, 0..5);
    }

    #[test]
    fn module_node_non_entry_unreachable() {
        let node = ModuleNode {
            file_id: FileId(5),
            path: PathBuf::from("/project/src/orphan.ts"),
            edge_range: 0..0,
            exports: vec![],
            re_exports: vec![],
            flags: ModuleNode::flags_from(false, false, false),
        };
        assert!(!node.is_entry_point());
        assert!(!node.is_reachable());
        assert!(!node.is_runtime_reachable());
        assert!(!node.is_test_reachable());
        assert!(node.edge_range.is_empty());
    }

    #[test]
    fn module_node_cjs_exports() {
        let mut node = ModuleNode {
            file_id: FileId(2),
            path: PathBuf::from("/project/lib/legacy.js"),
            edge_range: 3..7,
            exports: vec![],
            re_exports: vec![],
            flags: ModuleNode::flags_from(false, true, true),
        };
        node.set_reachable(true);
        assert!(node.has_cjs_exports());
        assert!(node.is_runtime_reachable());
        assert_eq!(node.edge_range.len(), 4);
    }

    #[test]
    fn module_node_with_exports_and_re_exports() {
        let node = ModuleNode {
            file_id: FileId(1),
            path: PathBuf::from("/project/src/barrel.ts"),
            edge_range: 0..3,
            exports: vec![ExportSymbol {
                name: ExportName::Named("localFn".to_string()),
                is_type_only: false,
                is_side_effect_used: false,
                visibility: VisibilityTag::None,
                expected_unused_reason: None,
                span: oxc_span::Span::new(0, 20),
                references: vec![],
                reference_paths: Vec::new(),
                members: vec![],
            }],
            re_exports: vec![ReExportEdge {
                source_file: FileId(2),
                imported_name: "*".to_string(),
                exported_name: "*".to_string(),
                is_type_only: false,
                span: oxc_span::Span::default(),
            }],
            flags: ModuleNode::flags_from(false, true, false),
        };
        assert_eq!(node.exports.len(), 1);
        assert_eq!(node.re_exports.len(), 1);
        assert_eq!(node.re_exports[0].source_file, FileId(2));
    }
}
