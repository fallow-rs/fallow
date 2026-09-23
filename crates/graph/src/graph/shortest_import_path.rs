//! Shortest import path between two modules.
//!
//! `impact_closure` answers "what does a change reach"; this answers "HOW does
//! one module reach another". It is a plain FIFO breadth-first search over the
//! forward import edges, so the first path found is a shortest one.
//!
//! Determinism is a contract, not a coincidence: the JSON must stay
//! byte-identical across runs and platforms. Successors are expanded in
//! ascending `FileId` order and a node keeps the predecessor that discovered it
//! first, which makes the reported route the lexicographically smallest
//! `FileId` sequence among all shortest routes. The proof is inductive: the
//! level-0 frontier is the single source, and a level is enqueued while its
//! predecessors are dequeued in that same order, so within every level the
//! queue order equals the lexicographic order of the routes reaching it.
//!
//! Type-only hops are REPORTED, never skipped. An `import type` chain is a real
//! compile-time coupling, and silently dropping it would answer "unreachable"
//! for a route a reader can see in the source.

use std::collections::VecDeque;

use fallow_types::discover::FileId;
use fixedbitset::FixedBitSet;
use rustc_hash::FxHashMap;

use super::ModuleGraph;

/// One import edge on a shortest import path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImportPathHop {
    /// The importing module.
    pub from: FileId,
    /// The imported module.
    pub to: FileId,
    /// Whether every symbol on this edge is type-only, so the hop exists at
    /// compile time only. Reported rather than skipped.
    pub all_type_only: bool,
    /// Byte offset in `from` of the imported binding that creates this edge,
    /// as [`ModuleGraph::outgoing_edge_summaries`] reports it: the first
    /// value-carrying symbol, or the first symbol when every symbol is
    /// type-only. The caller owns the source text and resolves it to a line.
    pub import_span_start: Option<u32>,
}

impl ModuleGraph {
    /// The shortest import path from `from` to `to`, following import edges in
    /// the import direction.
    ///
    /// Returns `None` when `to` is not reachable from `from`, and
    /// `Some(empty)` when `from` and `to` are the same module: zero hops is a
    /// real answer, distinct from no answer at all. Out-of-range file ids have
    /// no outgoing edges and therefore reach nothing.
    #[must_use]
    pub fn shortest_import_path(&self, from: FileId, to: FileId) -> Option<Vec<ImportPathHop>> {
        if from == to {
            return Some(Vec::new());
        }
        let capacity = self.modules.len();
        if from.0 as usize >= capacity || to.0 as usize >= capacity {
            return None;
        }

        let mut visited = FixedBitSet::with_capacity(capacity);
        visited.insert(from.0 as usize);
        let mut predecessor: FxHashMap<FileId, ImportPathHop> = FxHashMap::default();
        let mut queue: VecDeque<FileId> = VecDeque::new();
        queue.push_back(from);

        while let Some(current) = queue.pop_front() {
            for hop in self.ordered_outgoing_hops(current) {
                let idx = hop.to.0 as usize;
                if idx >= capacity || visited.contains(idx) {
                    continue;
                }
                visited.insert(idx);
                predecessor.insert(hop.to, hop);
                if hop.to == to {
                    return Some(rebuild_path(&predecessor, from, to));
                }
                queue.push_back(hop.to);
            }
        }
        None
    }

    /// Outgoing edges of `file_id` as hops, in ascending target order and with
    /// one hop per target. When a target is reachable over both a value edge
    /// and a type-only edge, the value edge wins: it is the hop a reader can
    /// follow at runtime.
    fn ordered_outgoing_hops(&self, file_id: FileId) -> Vec<ImportPathHop> {
        let mut hops: Vec<ImportPathHop> = self
            .outgoing_edge_summaries(file_id)
            .filter(|&(target, _, _)| target != file_id)
            .map(|(target, all_type_only, import_span_start)| ImportPathHop {
                from: file_id,
                to: target,
                all_type_only,
                import_span_start,
            })
            .collect();
        hops.sort_unstable_by_key(|hop| (hop.to.0, hop.all_type_only, hop.import_span_start));
        hops.dedup_by_key(|hop| hop.to);
        hops
    }
}

/// Walk the predecessor chain back from `to` and return it in import order.
fn rebuild_path(
    predecessor: &FxHashMap<FileId, ImportPathHop>,
    from: FileId,
    to: FileId,
) -> Vec<ImportPathHop> {
    let mut reversed = Vec::new();
    let mut cursor = to;
    while cursor != from {
        let Some(&hop) = predecessor.get(&cursor) else {
            break;
        };
        reversed.push(hop);
        cursor = hop.from;
    }
    reversed.reverse();
    reversed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolve::{ResolveResult, ResolvedImport, ResolvedModule};
    use fallow_types::discover::{DiscoveredFile, EntryPoint, EntryPointSource};
    use fallow_types::extract::{ExportInfo, ExportName, ImportInfo, ImportedName, VisibilityTag};
    use std::path::PathBuf;

    fn module_path(id: u32) -> PathBuf {
        PathBuf::from(format!("/p/src/m{id}.ts"))
    }

    fn import(target: u32, span_start: u32, type_only: bool) -> ResolvedImport {
        ResolvedImport {
            info: ImportInfo {
                source: format!("./m{target}"),
                imported_name: ImportedName::Named("value".to_string()),
                local_name: "value".to_string(),
                is_type_only: type_only,
                is_type_only_star: false,
                from_style: false,
                span: oxc_span::Span::new(span_start, span_start + 10),
                source_span: oxc_span::Span::default(),
            },
            target: ResolveResult::InternalModule(FileId(target)),
        }
    }

    fn value_export() -> ExportInfo {
        ExportInfo {
            name: ExportName::Named("value".to_string()),
            local_name: Some("value".to_string()),
            is_type_only: false,
            visibility: VisibilityTag::None,
            expected_unused_reason: None,
            span: oxc_span::Span::new(0, 20),
            members: vec![],
            is_side_effect_used: false,
            super_class: None,
            deprecated: false,
            deprecated_reason: None,
        }
    }

    /// Build a graph from `(source, targets)` pairs. Every edge carries one
    /// value symbol whose import span starts at `source * 100 + target`, so a
    /// hop's reported span identifies the edge it came from.
    fn graph_with_edges(module_count: u32, edges: &[(u32, &[u32])]) -> ModuleGraph {
        graph_with_typed_edges(module_count, edges, &[])
    }

    /// Same, with `type_only_edges` naming the `(source, target)` pairs whose
    /// import is spelled `import type`.
    fn graph_with_typed_edges(
        module_count: u32,
        edges: &[(u32, &[u32])],
        type_only_edges: &[(u32, u32)],
    ) -> ModuleGraph {
        let files: Vec<DiscoveredFile> = (0..module_count)
            .map(|id| DiscoveredFile {
                id: FileId(id),
                path: module_path(id),
                size_bytes: 10,
            })
            .collect();
        let entry_points = vec![EntryPoint {
            path: module_path(0),
            source: EntryPointSource::PackageJsonMain,
        }];
        let resolved: Vec<ResolvedModule> = (0..module_count)
            .map(|id| {
                let resolved_imports = edges
                    .iter()
                    .find(|(source, _)| *source == id)
                    .map(|(source, targets)| {
                        targets
                            .iter()
                            .map(|&target| {
                                import(
                                    target,
                                    source * 100 + target,
                                    type_only_edges.contains(&(*source, target)),
                                )
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                ResolvedModule {
                    file_id: FileId(id),
                    path: module_path(id),
                    resolved_imports,
                    exports: vec![value_export()].into(),
                    ..Default::default()
                }
            })
            .collect();
        ModuleGraph::build(&resolved, &entry_points, &files)
    }

    #[test]
    fn same_module_is_zero_hops_not_unreachable() {
        let graph = graph_with_edges(2, &[(0, &[1])]);
        assert_eq!(
            graph.shortest_import_path(FileId(0), FileId(0)),
            Some(vec![])
        );
    }

    #[test]
    fn unreachable_target_has_no_path() {
        let graph = graph_with_edges(3, &[(0, &[1])]);
        assert_eq!(graph.shortest_import_path(FileId(0), FileId(2)), None);
        // Edges are directed: the importer is not reachable from the imported.
        assert_eq!(graph.shortest_import_path(FileId(1), FileId(0)), None);
    }

    #[test]
    fn direct_import_is_one_hop_with_its_import_span() {
        let graph = graph_with_edges(2, &[(0, &[1])]);
        let path = graph
            .shortest_import_path(FileId(0), FileId(1))
            .expect("direct import is reachable");
        assert_eq!(path.len(), 1);
        assert_eq!(path[0].from, FileId(0));
        assert_eq!(path[0].to, FileId(1));
        assert!(!path[0].all_type_only);
        assert_eq!(path[0].import_span_start, Some(1));
    }

    #[test]
    fn breadth_first_prefers_the_shorter_route() {
        // Two routes to 5: two hops through 1, four hops through 2. A LIFO walk
        // drains the deeper branch first and reports the four-hop detour.
        let graph = graph_with_edges(
            6,
            &[(0, &[1, 2]), (1, &[5]), (2, &[3]), (3, &[4]), (4, &[5])],
        );
        let path = graph
            .shortest_import_path(FileId(0), FileId(5))
            .expect("target is reachable");
        assert_eq!(
            path.iter().map(|hop| hop.to).collect::<Vec<_>>(),
            vec![FileId(1), FileId(5)]
        );
    }

    #[test]
    fn equal_length_routes_resolve_to_the_smallest_file_id_sequence() {
        // Two two-hop routes to 3: through 2 and through 1. The smaller
        // intermediate wins regardless of the order the edges were declared in.
        let graph = graph_with_edges(4, &[(0, &[2, 1]), (1, &[3]), (2, &[3])]);
        let path = graph
            .shortest_import_path(FileId(0), FileId(3))
            .expect("target is reachable");
        assert_eq!(
            path.iter().map(|hop| hop.to).collect::<Vec<_>>(),
            vec![FileId(1), FileId(3)]
        );
    }

    #[test]
    fn a_cycle_does_not_stall_the_walk() {
        let graph = graph_with_edges(4, &[(0, &[1]), (1, &[2]), (2, &[1, 3])]);
        let path = graph
            .shortest_import_path(FileId(0), FileId(3))
            .expect("target is reachable behind a cycle");
        assert_eq!(path.len(), 3);
    }

    #[test]
    fn out_of_range_ids_reach_nothing() {
        let graph = graph_with_edges(2, &[(0, &[1])]);
        assert_eq!(graph.shortest_import_path(FileId(0), FileId(9)), None);
        assert_eq!(graph.shortest_import_path(FileId(9), FileId(1)), None);
    }

    #[test]
    fn a_type_only_hop_is_reported_not_skipped() {
        let graph = graph_with_typed_edges(2, &[(0, &[1])], &[(0, 1)]);
        let path = graph
            .shortest_import_path(FileId(0), FileId(1))
            .expect("a type-only import is still a route");
        assert_eq!(path.len(), 1);
        assert!(path[0].all_type_only);
    }
}
