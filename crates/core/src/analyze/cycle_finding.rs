//! The `circular-dependency` finding of one detected cycle.

use super::{LineOffsetsMap, cycle_edge_line_col};
use crate::discover::FileId;
use crate::graph::ModuleGraph;
use crate::results::{CircularDependency, CircularDependencyEdge};

pub(super) fn circular_dependency_from_cycle(
    graph: &ModuleGraph,
    line_offsets_map: &LineOffsetsMap<'_>,
    cycle: &[FileId],
) -> CircularDependency {
    // One anchor per hop in cycle order: `edges[i]` is the import in
    // `cycle[i]` pointing to `cycle[i + 1]`. Always populated for every
    // hop (fallback `(1, 0)` if the span is somehow missing) so
    // `edges.len() == files.len()` regardless of URL-resolvability on
    // the consumer side. The LSP renders one squiggly per edge.
    let edges: Vec<CircularDependencyEdge> = (0..cycle.len())
        .map(|edge_index| {
            let from = cycle[edge_index];
            let (line, col) =
                cycle_edge_line_col(graph, line_offsets_map, cycle, edge_index).unwrap_or((1, 0));
            CircularDependencyEdge {
                path: graph.modules[from.0 as usize].path.clone(),
                line,
                col,
            }
        })
        .collect();

    let files: Vec<std::path::PathBuf> = edges.iter().map(|edge| edge.path.clone()).collect();
    let length = files.len();
    // Top-level `line`/`col` remain the first hop's anchor for backward
    // compatibility with consumers that predate `edges`.
    let (line, col) = edges.first().map_or((1, 0), |edge| (edge.line, edge.col));
    CircularDependency {
        files,
        length,
        line,
        col,
        edges,
        is_cross_package: false,
    }
}
