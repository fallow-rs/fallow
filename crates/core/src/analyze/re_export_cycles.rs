//! Re-export cycle detector: maps graph-layer `GraphReExportCycle` entries to
//! typed `ReExportCycleFinding` entries on `AnalysisResults`, applying file-
//! level suppression so any member file carrying
//! `// fallow-ignore-file re-export-cycle` short-circuits the whole cycle.

use crate::graph::ModuleGraph;
use crate::suppress::{IssueKind, SuppressionContext};
use fallow_types::output_dead_code::ReExportCycleFinding;
use fallow_types::results::{ReExportCycle, ReExportCycleKind};

/// Walk `graph.re_export_cycles` and produce one `ReExportCycleFinding` per
/// surviving cycle. A cycle is dropped when ANY member file carries a
/// file-level suppression for `IssueKind::ReExportCycle`; a single
/// `// fallow-ignore-file re-export-cycle` on the alphabetically-first member
/// is enough to silence the entire finding, matching the action's primary
/// suggestion.
///
/// The graph layer already sorts `files` lexicographically and pairs each
/// path with its `FileId` in `file_ids`, so suppression checks are O(members)
/// without any path-to-FileId lookup.
pub fn find_re_export_cycles(
    graph: &ModuleGraph,
    suppressions: &SuppressionContext<'_>,
) -> Vec<ReExportCycleFinding> {
    graph
        .re_export_cycles
        .iter()
        .filter_map(|cycle| {
            let any_suppressed = cycle
                .file_ids
                .iter()
                .any(|id| suppressions.is_file_suppressed(*id, IssueKind::ReExportCycle));
            if any_suppressed {
                return None;
            }
            let kind = if cycle.is_self_loop {
                ReExportCycleKind::SelfLoop
            } else {
                ReExportCycleKind::MultiNode
            };
            Some(ReExportCycleFinding::with_actions(ReExportCycle {
                files: cycle.files.clone(),
                kind,
            }))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discover::FileId;
    use crate::graph::GraphReExportCycle;
    use crate::graph::ModuleGraph;
    use std::path::PathBuf;

    fn empty_graph_with_cycles(cycles: Vec<GraphReExportCycle>) -> ModuleGraph {
        let mut graph = ModuleGraph::build(&[], &[], &[]);
        graph.re_export_cycles = cycles;
        graph
    }

    #[test]
    fn empty_graph_returns_empty() {
        let graph = ModuleGraph::build(&[], &[], &[]);
        let suppressions = SuppressionContext::new(&[]);
        let findings = find_re_export_cycles(&graph, &suppressions);
        assert!(findings.is_empty());
    }

    #[test]
    fn self_loop_maps_to_self_loop_kind() {
        let graph = empty_graph_with_cycles(vec![GraphReExportCycle {
            files: vec![PathBuf::from("/p/barrel.ts")],
            file_ids: vec![FileId(0)],
            is_self_loop: true,
        }]);
        let suppressions = SuppressionContext::new(&[]);
        let findings = find_re_export_cycles(&graph, &suppressions);
        assert_eq!(findings.len(), 1);
        assert!(matches!(
            findings[0].cycle.kind,
            ReExportCycleKind::SelfLoop
        ));
        assert_eq!(findings[0].cycle.files.len(), 1);
        assert!(!findings[0].actions.is_empty());
    }

    #[test]
    fn multi_node_cycle_maps_to_multi_node_kind() {
        let graph = empty_graph_with_cycles(vec![GraphReExportCycle {
            files: vec![PathBuf::from("/p/a.ts"), PathBuf::from("/p/b.ts")],
            file_ids: vec![FileId(0), FileId(1)],
            is_self_loop: false,
        }]);
        let suppressions = SuppressionContext::new(&[]);
        let findings = find_re_export_cycles(&graph, &suppressions);
        assert_eq!(findings.len(), 1);
        assert!(matches!(
            findings[0].cycle.kind,
            ReExportCycleKind::MultiNode
        ));
        assert_eq!(findings[0].cycle.files.len(), 2);
        assert!(!findings[0].actions.is_empty());
    }
}
