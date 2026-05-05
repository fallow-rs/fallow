//! Circular dependency detection via Tarjan's SCC algorithm + elementary cycle enumeration.

use std::ops::Range;

use fixedbitset::FixedBitSet;
use rustc_hash::FxHashSet;

use fallow_types::discover::FileId;

use super::ModuleGraph;
use super::types::ModuleNode;

impl ModuleGraph {
    /// Find all circular dependency cycles in the module graph.
    ///
    /// Uses an iterative implementation of Tarjan's strongly connected components
    /// algorithm (O(V + E)) to find all SCCs with 2 or more nodes. Each such SCC
    /// represents a set of files involved in a circular dependency.
    ///
    /// Returns cycles sorted by length (shortest first), with files within each
    /// cycle sorted by path for deterministic output.
    ///
    /// # Panics
    ///
    /// Panics if the internal file-to-path lookup is inconsistent with the module list.
    #[must_use]
    #[expect(
        clippy::excessive_nesting,
        reason = "Tarjan's SCC requires deep nesting"
    )]
    #[expect(
        clippy::cast_possible_truncation,
        reason = "file count is bounded by project size, well under u32::MAX"
    )]
    pub fn find_cycles(&self) -> Vec<Vec<FileId>> {
        let n = self.modules.len();
        if n == 0 {
            return Vec::new();
        }

        // Tarjan's SCC state
        let mut index_counter: u32 = 0;
        let mut indices: Vec<u32> = vec![u32::MAX; n]; // u32::MAX = undefined
        let mut lowlinks: Vec<u32> = vec![0; n];
        let mut on_stack = FixedBitSet::with_capacity(n);
        let mut stack: Vec<usize> = Vec::new();
        let mut sccs: Vec<Vec<FileId>> = Vec::new();

        // Iterative DFS stack frame
        struct Frame {
            node: usize,
            succ_pos: usize,
            succ_end: usize,
        }

        // Pre-collect all successors (deduplicated) into a flat vec for cache-friendly access.
        let mut all_succs: Vec<usize> = Vec::with_capacity(self.edges.len());
        let mut succ_ranges: Vec<Range<usize>> = Vec::with_capacity(n);
        let mut seen_set = FxHashSet::default();
        for module in &self.modules {
            let start = all_succs.len();
            seen_set.clear();
            for edge in &self.edges[module.edge_range.clone()] {
                // Skip edges where all imports are type-only (`import type`).
                // Type-only imports are erased at compile time and cannot cause
                // runtime circular dependency issues.
                if edge.symbols.iter().all(|s| s.is_type_only) {
                    continue;
                }
                let target = edge.target.0 as usize;
                if target < n && seen_set.insert(target) {
                    all_succs.push(target);
                }
            }
            let end = all_succs.len();
            succ_ranges.push(start..end);
        }

        let mut dfs_stack: Vec<Frame> = Vec::new();

        for start_node in 0..n {
            if indices[start_node] != u32::MAX {
                continue;
            }

            // Push the starting node
            indices[start_node] = index_counter;
            lowlinks[start_node] = index_counter;
            index_counter += 1;
            on_stack.insert(start_node);
            stack.push(start_node);

            let range = &succ_ranges[start_node];
            dfs_stack.push(Frame {
                node: start_node,
                succ_pos: range.start,
                succ_end: range.end,
            });

            while let Some(frame) = dfs_stack.last_mut() {
                if frame.succ_pos < frame.succ_end {
                    let w = all_succs[frame.succ_pos];
                    frame.succ_pos += 1;

                    if indices[w] == u32::MAX {
                        // Tree edge: push w onto the DFS stack
                        indices[w] = index_counter;
                        lowlinks[w] = index_counter;
                        index_counter += 1;
                        on_stack.insert(w);
                        stack.push(w);

                        let range = &succ_ranges[w];
                        dfs_stack.push(Frame {
                            node: w,
                            succ_pos: range.start,
                            succ_end: range.end,
                        });
                    } else if on_stack.contains(w) {
                        // Back edge: update lowlink
                        let v = frame.node;
                        lowlinks[v] = lowlinks[v].min(indices[w]);
                    }
                } else {
                    // All successors processed — pop this frame
                    let v = frame.node;
                    let v_lowlink = lowlinks[v];
                    let v_index = indices[v];
                    dfs_stack.pop();

                    // Update parent's lowlink
                    if let Some(parent) = dfs_stack.last_mut() {
                        lowlinks[parent.node] = lowlinks[parent.node].min(v_lowlink);
                    }

                    // If v is a root node, pop the SCC
                    if v_lowlink == v_index {
                        let mut scc = Vec::new();
                        loop {
                            let w = stack.pop().expect("SCC stack should not be empty");
                            on_stack.set(w, false);
                            scc.push(FileId(w as u32));
                            if w == v {
                                break;
                            }
                        }
                        // Only report cycles of length >= 2
                        if scc.len() >= 2 {
                            sccs.push(scc);
                        }
                    }
                }
            }
        }

        self.enumerate_cycles_from_sccs(&sccs, &all_succs, &succ_ranges)
    }

    /// Enumerate individual elementary cycles from SCCs and return sorted results.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "file count is bounded by project size, well under u32::MAX"
    )]
    fn enumerate_cycles_from_sccs(
        &self,
        sccs: &[Vec<FileId>],
        all_succs: &[usize],
        succ_ranges: &[Range<usize>],
    ) -> Vec<Vec<FileId>> {
        const MAX_CYCLES_PER_SCC: usize = 20;

        let succs = SuccessorMap {
            all_succs,
            succ_ranges,
            modules: &self.modules,
        };

        let mut result: Vec<Vec<FileId>> = Vec::new();
        let mut seen_cycles: FxHashSet<Vec<u32>> = FxHashSet::default();

        for scc in sccs {
            if scc.len() == 2 {
                let mut cycle = vec![scc[0].0 as usize, scc[1].0 as usize];
                // Canonical: smallest path first
                if self.modules[cycle[1]].path < self.modules[cycle[0]].path {
                    cycle.swap(0, 1);
                }
                let key: Vec<u32> = cycle.iter().map(|&n| n as u32).collect();
                if seen_cycles.insert(key) {
                    result.push(cycle.into_iter().map(|n| FileId(n as u32)).collect());
                }
                continue;
            }

            let scc_nodes: Vec<usize> = scc.iter().map(|id| id.0 as usize).collect();
            let elementary = enumerate_elementary_cycles(&scc_nodes, &succs, MAX_CYCLES_PER_SCC);

            for cycle in elementary {
                let key: Vec<u32> = cycle.iter().map(|&n| n as u32).collect();
                if seen_cycles.insert(key) {
                    result.push(cycle.into_iter().map(|n| FileId(n as u32)).collect());
                }
            }
        }

        // Sort: shortest first, then by first file path
        result.sort_by(|a, b| {
            a.len().cmp(&b.len()).then_with(|| {
                self.modules[a[0].0 as usize]
                    .path
                    .cmp(&self.modules[b[0].0 as usize].path)
            })
        });

        result
    }
}

/// Rotate a cycle so the node with the smallest path is first (canonical form for dedup).
fn canonical_cycle(cycle: &[usize], modules: &[ModuleNode]) -> Vec<usize> {
    if cycle.is_empty() {
        return Vec::new();
    }
    let min_pos = cycle
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| modules[**a].path.cmp(&modules[**b].path))
        .map_or(0, |(i, _)| i);
    let mut result = cycle[min_pos..].to_vec();
    result.extend_from_slice(&cycle[..min_pos]);
    result
}

/// DFS frame for iterative cycle finding.
struct CycleFrame {
    succ_pos: usize,
    succ_end: usize,
}

/// Pre-collected, deduplicated successor data for cache-friendly graph traversal.
struct SuccessorMap<'a> {
    all_succs: &'a [usize],
    succ_ranges: &'a [Range<usize>],
    modules: &'a [ModuleNode],
}

/// Record a cycle in canonical form if not already seen.
#[expect(
    clippy::cast_possible_truncation,
    reason = "file count is bounded by project size, well under u32::MAX"
)]
fn try_record_cycle(
    path: &[usize],
    modules: &[ModuleNode],
    seen: &mut FxHashSet<Vec<u32>>,
    cycles: &mut Vec<Vec<usize>>,
) {
    let canonical = canonical_cycle(path, modules);
    let key: Vec<u32> = canonical.iter().map(|&n| n as u32).collect();
    if seen.insert(key) {
        cycles.push(canonical);
    }
}

/// Run a bounded DFS from `start`, looking for elementary cycles of exactly `depth_limit` nodes.
///
/// Appends any newly found cycles to `cycles` (deduped via `seen`).
/// Stops early once `cycles.len() >= max_cycles`.
fn dfs_find_cycles_from(
    start: usize,
    depth_limit: usize,
    scc_set: &FxHashSet<usize>,
    succs: &SuccessorMap<'_>,
    max_cycles: usize,
    seen: &mut FxHashSet<Vec<u32>>,
    cycles: &mut Vec<Vec<usize>>,
) {
    let mut path: Vec<usize> = vec![start];
    let mut path_set = FixedBitSet::with_capacity(succs.modules.len());
    path_set.insert(start);

    let range = &succs.succ_ranges[start];
    let mut dfs: Vec<CycleFrame> = vec![CycleFrame {
        succ_pos: range.start,
        succ_end: range.end,
    }];

    while let Some(frame) = dfs.last_mut() {
        if cycles.len() >= max_cycles {
            return;
        }

        if frame.succ_pos >= frame.succ_end {
            // Backtrack: all successors exhausted for this frame
            dfs.pop();
            if path.len() > 1 {
                let removed = path.pop().unwrap();
                path_set.set(removed, false);
            }
            continue;
        }

        let w = succs.all_succs[frame.succ_pos];
        frame.succ_pos += 1;

        // Only follow edges within this SCC
        if !scc_set.contains(&w) {
            continue;
        }

        // Found an elementary cycle at exactly this depth
        if w == start && path.len() >= 2 && path.len() == depth_limit {
            try_record_cycle(&path, succs.modules, seen, cycles);
            continue;
        }

        // Skip if already on current path or beyond depth limit
        if path_set.contains(w) || path.len() >= depth_limit {
            continue;
        }

        // Extend path
        path.push(w);
        path_set.insert(w);

        let range = &succs.succ_ranges[w];
        dfs.push(CycleFrame {
            succ_pos: range.start,
            succ_end: range.end,
        });
    }
}

/// Enumerate individual elementary cycles within an SCC using depth-limited DFS.
///
/// Uses iterative deepening: first finds all 2-node cycles, then 3-node, etc.
/// This ensures the shortest, most actionable cycles are always found first.
/// Stops after `max_cycles` total cycles to bound work on dense SCCs.
fn enumerate_elementary_cycles(
    scc_nodes: &[usize],
    succs: &SuccessorMap<'_>,
    max_cycles: usize,
) -> Vec<Vec<usize>> {
    let scc_set: FxHashSet<usize> = scc_nodes.iter().copied().collect();
    let mut cycles: Vec<Vec<usize>> = Vec::new();
    let mut seen: FxHashSet<Vec<u32>> = FxHashSet::default();

    // Sort start nodes by path for deterministic enumeration order
    let mut sorted_nodes: Vec<usize> = scc_nodes.to_vec();
    sorted_nodes.sort_by(|a, b| succs.modules[*a].path.cmp(&succs.modules[*b].path));

    // Iterative deepening: increase max depth from 2 up to SCC size
    let max_depth = scc_nodes.len().min(12); // Cap depth to avoid very long cycles
    for depth_limit in 2..=max_depth {
        if cycles.len() >= max_cycles {
            break;
        }

        for &start in &sorted_nodes {
            if cycles.len() >= max_cycles {
                break;
            }

            dfs_find_cycles_from(
                start,
                depth_limit,
                &scc_set,
                succs,
                max_cycles,
                &mut seen,
                &mut cycles,
            );
        }
    }

    cycles
}

#[cfg(test)]
mod tests {
    use std::ops::Range;
    use std::path::PathBuf;

    use rustc_hash::FxHashSet;

    use crate::graph::types::ModuleNode;
    use crate::resolve::{ResolveResult, ResolvedImport, ResolvedModule};
    use fallow_types::discover::{DiscoveredFile, EntryPoint, EntryPointSource, FileId};
    use fallow_types::extract::{ExportName, ImportInfo, ImportedName, VisibilityTag};

    use super::{
        ModuleGraph, SuccessorMap, canonical_cycle, dfs_find_cycles_from,
        enumerate_elementary_cycles, try_record_cycle,
    };

    /// Helper: build a graph from files+edges, no entry points needed for cycle detection.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "test file counts are trivially small"
    )]
    fn build_cycle_graph(file_count: usize, edges_spec: &[(u32, u32)]) -> ModuleGraph {
        let files: Vec<DiscoveredFile> = (0..file_count)
            .map(|i| DiscoveredFile {
                id: FileId(i as u32),
                path: PathBuf::from(format!("/project/file{i}.ts")),
                size_bytes: 100,
            })
            .collect();

        let resolved_modules: Vec<ResolvedModule> = (0..file_count)
            .map(|i| {
                let imports: Vec<ResolvedImport> = edges_spec
                    .iter()
                    .filter(|(src, _)| *src == i as u32)
                    .map(|(_, tgt)| ResolvedImport {
                        info: ImportInfo {
                            source: format!("./file{tgt}"),
                            imported_name: ImportedName::Named("x".to_string()),
                            local_name: "x".to_string(),
                            is_type_only: false,
                            from_style: false,
                            span: oxc_span::Span::new(0, 10),
                            source_span: oxc_span::Span::default(),
                        },
                        target: ResolveResult::InternalModule(FileId(*tgt)),
                    })
                    .collect();

                ResolvedModule {
                    file_id: FileId(i as u32),
                    path: PathBuf::from(format!("/project/file{i}.ts")),
                    exports: vec![fallow_types::extract::ExportInfo {
                        name: ExportName::Named("x".to_string()),
                        local_name: Some("x".to_string()),
                        is_type_only: false,
                        visibility: VisibilityTag::None,
                        span: oxc_span::Span::new(0, 20),
                        members: vec![],
                        is_side_effect_used: false,
                        super_class: None,
                    }],
                    re_exports: vec![],
                    resolved_imports: imports,
                    resolved_dynamic_imports: vec![],
                    resolved_dynamic_patterns: vec![],
                    member_accesses: vec![],
                    whole_object_uses: vec![],
                    has_cjs_exports: false,
                    unused_import_bindings: FxHashSet::default(),
                    type_referenced_import_bindings: vec![],
                    value_referenced_import_bindings: vec![],
                }
            })
            .collect();

        let entry_points = vec![EntryPoint {
            path: PathBuf::from("/project/file0.ts"),
            source: EntryPointSource::PackageJsonMain,
        }];

        ModuleGraph::build(&resolved_modules, &entry_points, &files)
    }

    #[test]
    fn find_cycles_empty_graph() {
        let graph = ModuleGraph::build(&[], &[], &[]);
        assert!(graph.find_cycles().is_empty());
    }

    #[test]
    fn find_cycles_no_cycles() {
        // A -> B -> C (no back edges)
        let graph = build_cycle_graph(3, &[(0, 1), (1, 2)]);
        assert!(graph.find_cycles().is_empty());
    }

    #[test]
    fn find_cycles_simple_two_node_cycle() {
        // A -> B -> A
        let graph = build_cycle_graph(2, &[(0, 1), (1, 0)]);
        let cycles = graph.find_cycles();
        assert_eq!(cycles.len(), 1);
        assert_eq!(cycles[0].len(), 2);
    }

    #[test]
    fn find_cycles_three_node_cycle() {
        // A -> B -> C -> A
        let graph = build_cycle_graph(3, &[(0, 1), (1, 2), (2, 0)]);
        let cycles = graph.find_cycles();
        assert_eq!(cycles.len(), 1);
        assert_eq!(cycles[0].len(), 3);
    }

    #[test]
    fn find_cycles_self_import_ignored() {
        // A -> A (self-import, should NOT be reported as a cycle).
        // Reason: Tarjan's SCC only reports components with >= 2 nodes,
        // so a single-node self-edge never forms a reportable cycle.
        let graph = build_cycle_graph(1, &[(0, 0)]);
        let cycles = graph.find_cycles();
        assert!(
            cycles.is_empty(),
            "self-imports should not be reported as cycles"
        );
    }

    #[test]
    fn find_cycles_multiple_independent_cycles() {
        // Cycle 1: A -> B -> A
        // Cycle 2: C -> D -> C
        // No connection between cycles
        let graph = build_cycle_graph(4, &[(0, 1), (1, 0), (2, 3), (3, 2)]);
        let cycles = graph.find_cycles();
        assert_eq!(cycles.len(), 2);
        // Both cycles should have length 2
        assert!(cycles.iter().all(|c| c.len() == 2));
    }

    #[test]
    fn find_cycles_linear_chain_with_back_edge() {
        // A -> B -> C -> D -> B (cycle is B-C-D)
        let graph = build_cycle_graph(4, &[(0, 1), (1, 2), (2, 3), (3, 1)]);
        let cycles = graph.find_cycles();
        assert_eq!(cycles.len(), 1);
        assert_eq!(cycles[0].len(), 3);
        // The cycle should contain files 1, 2, 3
        let ids: Vec<u32> = cycles[0].iter().map(|f| f.0).collect();
        assert!(ids.contains(&1));
        assert!(ids.contains(&2));
        assert!(ids.contains(&3));
        assert!(!ids.contains(&0));
    }

    #[test]
    fn find_cycles_overlapping_cycles_enumerated() {
        // A -> B -> A, B -> C -> B => SCC is {A, B, C} but should report 2 elementary cycles
        let graph = build_cycle_graph(3, &[(0, 1), (1, 0), (1, 2), (2, 1)]);
        let cycles = graph.find_cycles();
        assert_eq!(
            cycles.len(),
            2,
            "should find 2 elementary cycles, not 1 SCC"
        );
        assert!(
            cycles.iter().all(|c| c.len() == 2),
            "both cycles should have length 2"
        );
    }

    #[test]
    fn find_cycles_deterministic_ordering() {
        // Run twice with the same graph — results should be identical
        let graph1 = build_cycle_graph(3, &[(0, 1), (1, 2), (2, 0)]);
        let graph2 = build_cycle_graph(3, &[(0, 1), (1, 2), (2, 0)]);
        let cycles1 = graph1.find_cycles();
        let cycles2 = graph2.find_cycles();
        assert_eq!(cycles1.len(), cycles2.len());
        for (c1, c2) in cycles1.iter().zip(cycles2.iter()) {
            let paths1: Vec<&PathBuf> = c1
                .iter()
                .map(|f| &graph1.modules[f.0 as usize].path)
                .collect();
            let paths2: Vec<&PathBuf> = c2
                .iter()
                .map(|f| &graph2.modules[f.0 as usize].path)
                .collect();
            assert_eq!(paths1, paths2);
        }
    }

    #[test]
    fn find_cycles_sorted_by_length() {
        // Two cycles: A-B (len 2) and C-D-E (len 3)
        let graph = build_cycle_graph(5, &[(0, 1), (1, 0), (2, 3), (3, 4), (4, 2)]);
        let cycles = graph.find_cycles();
        assert_eq!(cycles.len(), 2);
        assert!(
            cycles[0].len() <= cycles[1].len(),
            "cycles should be sorted by length"
        );
    }

    #[test]
    fn find_cycles_large_cycle() {
        // Chain of 10 nodes forming a single cycle: 0->1->2->...->9->0
        let edges: Vec<(u32, u32)> = (0..10).map(|i| (i, (i + 1) % 10)).collect();
        let graph = build_cycle_graph(10, &edges);
        let cycles = graph.find_cycles();
        assert_eq!(cycles.len(), 1);
        assert_eq!(cycles[0].len(), 10);
    }

    #[test]
    fn find_cycles_complex_scc_multiple_elementary() {
        // A square: A->B, B->C, C->D, D->A, plus diagonal A->C
        // Elementary cycles: A->B->C->D->A, A->C->D->A, and A->B->C->...
        let graph = build_cycle_graph(4, &[(0, 1), (1, 2), (2, 3), (3, 0), (0, 2)]);
        let cycles = graph.find_cycles();
        // Should find multiple elementary cycles, not just one SCC of 4
        assert!(
            cycles.len() >= 2,
            "should find at least 2 elementary cycles, got {}",
            cycles.len()
        );
        // All cycles should be shorter than the full SCC
        assert!(cycles.iter().all(|c| c.len() <= 4));
    }

    #[test]
    fn find_cycles_no_duplicate_cycles() {
        // Triangle: A->B->C->A — should find exactly 1 cycle, not duplicates
        // from different DFS start points
        let graph = build_cycle_graph(3, &[(0, 1), (1, 2), (2, 0)]);
        let cycles = graph.find_cycles();
        assert_eq!(cycles.len(), 1, "triangle should produce exactly 1 cycle");
        assert_eq!(cycles[0].len(), 3);
    }

    // -----------------------------------------------------------------------
    // Unit-level helpers for testing extracted functions directly
    // -----------------------------------------------------------------------

    /// Build lightweight `ModuleNode` stubs and successor data for unit tests.
    ///
    /// `edges_spec` is a list of (source, target) pairs (0-indexed).
    /// Returns (modules, all_succs, succ_ranges) suitable for constructing a `SuccessorMap`.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "test file counts are trivially small"
    )]
    fn build_test_succs(
        file_count: usize,
        edges_spec: &[(usize, usize)],
    ) -> (Vec<ModuleNode>, Vec<usize>, Vec<Range<usize>>) {
        let modules: Vec<ModuleNode> = (0..file_count)
            .map(|i| {
                let mut node = ModuleNode {
                    file_id: FileId(i as u32),
                    path: PathBuf::from(format!("/project/file{i}.ts")),
                    edge_range: 0..0,
                    exports: vec![],
                    re_exports: vec![],
                    flags: ModuleNode::flags_from(i == 0, true, false),
                };
                node.set_reachable(true);
                node
            })
            .collect();

        let mut all_succs: Vec<usize> = Vec::new();
        let mut succ_ranges: Vec<Range<usize>> = Vec::with_capacity(file_count);
        for src in 0..file_count {
            let start = all_succs.len();
            let mut seen = FxHashSet::default();
            for &(s, t) in edges_spec {
                if s == src && t < file_count && seen.insert(t) {
                    all_succs.push(t);
                }
            }
            let end = all_succs.len();
            succ_ranges.push(start..end);
        }

        (modules, all_succs, succ_ranges)
    }

    // -----------------------------------------------------------------------
    // canonical_cycle tests
    // -----------------------------------------------------------------------

    #[test]
    fn canonical_cycle_empty() {
        let modules: Vec<ModuleNode> = vec![];
        assert!(canonical_cycle(&[], &modules).is_empty());
    }

    #[test]
    fn canonical_cycle_rotates_to_smallest_path() {
        let (modules, _, _) = build_test_succs(3, &[]);
        // Cycle [2, 0, 1] — file0 has the smallest path, so canonical is [0, 1, 2]
        let result = canonical_cycle(&[2, 0, 1], &modules);
        assert_eq!(result, vec![0, 1, 2]);
    }

    #[test]
    fn canonical_cycle_already_canonical() {
        let (modules, _, _) = build_test_succs(3, &[]);
        let result = canonical_cycle(&[0, 1, 2], &modules);
        assert_eq!(result, vec![0, 1, 2]);
    }

    #[test]
    fn canonical_cycle_single_node() {
        let (modules, _, _) = build_test_succs(1, &[]);
        let result = canonical_cycle(&[0], &modules);
        assert_eq!(result, vec![0]);
    }

    // -----------------------------------------------------------------------
    // try_record_cycle tests
    // -----------------------------------------------------------------------

    #[test]
    fn try_record_cycle_inserts_new_cycle() {
        let (modules, _, _) = build_test_succs(3, &[]);
        let mut seen = FxHashSet::default();
        let mut cycles = Vec::new();

        try_record_cycle(&[0, 1, 2], &modules, &mut seen, &mut cycles);
        assert_eq!(cycles.len(), 1);
        assert_eq!(cycles[0], vec![0, 1, 2]);
    }

    #[test]
    fn try_record_cycle_deduplicates_rotated_cycle() {
        // Same cycle in two rotations: [0,1,2] and [1,2,0]
        // Both should canonicalize to the same key, so only one is recorded.
        let (modules, _, _) = build_test_succs(3, &[]);
        let mut seen = FxHashSet::default();
        let mut cycles = Vec::new();

        try_record_cycle(&[0, 1, 2], &modules, &mut seen, &mut cycles);
        try_record_cycle(&[1, 2, 0], &modules, &mut seen, &mut cycles);
        try_record_cycle(&[2, 0, 1], &modules, &mut seen, &mut cycles);

        assert_eq!(
            cycles.len(),
            1,
            "rotations of the same cycle should be deduped"
        );
    }

    #[test]
    fn try_record_cycle_single_node_self_loop() {
        // A single-node "cycle" (self-loop) — should be recorded if passed in
        let (modules, _, _) = build_test_succs(1, &[]);
        let mut seen = FxHashSet::default();
        let mut cycles = Vec::new();

        try_record_cycle(&[0], &modules, &mut seen, &mut cycles);
        assert_eq!(cycles.len(), 1);
        assert_eq!(cycles[0], vec![0]);
    }

    #[test]
    fn try_record_cycle_distinct_cycles_both_recorded() {
        // Two genuinely different cycles
        let (modules, _, _) = build_test_succs(4, &[]);
        let mut seen = FxHashSet::default();
        let mut cycles = Vec::new();

        try_record_cycle(&[0, 1], &modules, &mut seen, &mut cycles);
        try_record_cycle(&[2, 3], &modules, &mut seen, &mut cycles);

        assert_eq!(cycles.len(), 2);
    }

    // -----------------------------------------------------------------------
    // SuccessorMap construction tests
    // -----------------------------------------------------------------------

    #[test]
    fn successor_map_empty_graph() {
        let (modules, all_succs, succ_ranges) = build_test_succs(0, &[]);
        let succs = SuccessorMap {
            all_succs: &all_succs,
            succ_ranges: &succ_ranges,
            modules: &modules,
        };
        assert!(succs.all_succs.is_empty());
        assert!(succs.succ_ranges.is_empty());
    }

    #[test]
    fn successor_map_single_node_self_edge() {
        let (modules, all_succs, succ_ranges) = build_test_succs(1, &[(0, 0)]);
        let succs = SuccessorMap {
            all_succs: &all_succs,
            succ_ranges: &succ_ranges,
            modules: &modules,
        };
        assert_eq!(succs.all_succs.len(), 1);
        assert_eq!(succs.all_succs[0], 0);
        assert_eq!(succs.succ_ranges[0], 0..1);
    }

    #[test]
    fn successor_map_deduplicates_edges() {
        // Two edges from 0 to 1 — should be deduped
        let (modules, all_succs, succ_ranges) = build_test_succs(2, &[(0, 1), (0, 1)]);
        let succs = SuccessorMap {
            all_succs: &all_succs,
            succ_ranges: &succ_ranges,
            modules: &modules,
        };
        let range = &succs.succ_ranges[0];
        assert_eq!(
            range.end - range.start,
            1,
            "duplicate edges should be deduped"
        );
    }

    #[test]
    fn successor_map_multiple_successors() {
        let (modules, all_succs, succ_ranges) = build_test_succs(4, &[(0, 1), (0, 2), (0, 3)]);
        let succs = SuccessorMap {
            all_succs: &all_succs,
            succ_ranges: &succ_ranges,
            modules: &modules,
        };
        let range = &succs.succ_ranges[0];
        assert_eq!(range.end - range.start, 3);
        // Node 1, 2, 3 have no successors
        for i in 1..4 {
            let r = &succs.succ_ranges[i];
            assert_eq!(r.end - r.start, 0);
        }
    }

    // -----------------------------------------------------------------------
    // dfs_find_cycles_from tests
    // -----------------------------------------------------------------------

    #[test]
    fn dfs_find_cycles_from_isolated_node() {
        // Node 0 with no successors — should find no cycles
        let (modules, all_succs, succ_ranges) = build_test_succs(1, &[]);
        let succs = SuccessorMap {
            all_succs: &all_succs,
            succ_ranges: &succ_ranges,
            modules: &modules,
        };
        let scc_set: FxHashSet<usize> = std::iter::once(0).collect();
        let mut seen = FxHashSet::default();
        let mut cycles = Vec::new();

        dfs_find_cycles_from(0, 2, &scc_set, &succs, 10, &mut seen, &mut cycles);
        assert!(cycles.is_empty(), "isolated node should have no cycles");
    }

    #[test]
    fn dfs_find_cycles_from_simple_two_cycle() {
        // 0 -> 1, 1 -> 0, both in SCC
        let (modules, all_succs, succ_ranges) = build_test_succs(2, &[(0, 1), (1, 0)]);
        let succs = SuccessorMap {
            all_succs: &all_succs,
            succ_ranges: &succ_ranges,
            modules: &modules,
        };
        let scc_set: FxHashSet<usize> = [0, 1].into_iter().collect();
        let mut seen = FxHashSet::default();
        let mut cycles = Vec::new();

        dfs_find_cycles_from(0, 2, &scc_set, &succs, 10, &mut seen, &mut cycles);
        assert_eq!(cycles.len(), 1);
        assert_eq!(cycles[0].len(), 2);
    }

    #[test]
    fn dfs_find_cycles_from_diamond_graph() {
        // Diamond: 0->1, 0->2, 1->3, 2->3, 3->0 (all in SCC)
        // At depth 3: 0->1->3->0 and 0->2->3->0
        // At depth 4: 0->1->3->?->0 — but 3 only goes to 0, so no 4-cycle
        let (modules, all_succs, succ_ranges) =
            build_test_succs(4, &[(0, 1), (0, 2), (1, 3), (2, 3), (3, 0)]);
        let succs = SuccessorMap {
            all_succs: &all_succs,
            succ_ranges: &succ_ranges,
            modules: &modules,
        };
        let scc_set: FxHashSet<usize> = [0, 1, 2, 3].into_iter().collect();
        let mut seen = FxHashSet::default();
        let mut cycles = Vec::new();

        // Depth 3: should find two 3-node cycles
        dfs_find_cycles_from(0, 3, &scc_set, &succs, 10, &mut seen, &mut cycles);
        assert_eq!(cycles.len(), 2, "diamond should have two 3-node cycles");
        assert!(cycles.iter().all(|c| c.len() == 3));
    }

    #[test]
    fn dfs_find_cycles_from_depth_limit_prevents_longer_cycles() {
        // 0->1->2->3->0 forms a 4-cycle
        // With depth_limit=3, the DFS should NOT find this 4-cycle
        let (modules, all_succs, succ_ranges) =
            build_test_succs(4, &[(0, 1), (1, 2), (2, 3), (3, 0)]);
        let succs = SuccessorMap {
            all_succs: &all_succs,
            succ_ranges: &succ_ranges,
            modules: &modules,
        };
        let scc_set: FxHashSet<usize> = [0, 1, 2, 3].into_iter().collect();
        let mut seen = FxHashSet::default();
        let mut cycles = Vec::new();

        dfs_find_cycles_from(0, 3, &scc_set, &succs, 10, &mut seen, &mut cycles);
        assert!(
            cycles.is_empty(),
            "depth_limit=3 should prevent finding a 4-node cycle"
        );
    }

    #[test]
    fn dfs_find_cycles_from_depth_limit_exact_match() {
        // 0->1->2->3->0 forms a 4-cycle
        // With depth_limit=4, the DFS should find it
        let (modules, all_succs, succ_ranges) =
            build_test_succs(4, &[(0, 1), (1, 2), (2, 3), (3, 0)]);
        let succs = SuccessorMap {
            all_succs: &all_succs,
            succ_ranges: &succ_ranges,
            modules: &modules,
        };
        let scc_set: FxHashSet<usize> = [0, 1, 2, 3].into_iter().collect();
        let mut seen = FxHashSet::default();
        let mut cycles = Vec::new();

        dfs_find_cycles_from(0, 4, &scc_set, &succs, 10, &mut seen, &mut cycles);
        assert_eq!(
            cycles.len(),
            1,
            "depth_limit=4 should find the 4-node cycle"
        );
        assert_eq!(cycles[0].len(), 4);
    }

    #[test]
    fn dfs_find_cycles_from_respects_max_cycles() {
        // Dense graph: complete graph of 4 nodes — many cycles
        let edges: Vec<(usize, usize)> = (0..4)
            .flat_map(|i| (0..4).filter(move |&j| i != j).map(move |j| (i, j)))
            .collect();
        let (modules, all_succs, succ_ranges) = build_test_succs(4, &edges);
        let succs = SuccessorMap {
            all_succs: &all_succs,
            succ_ranges: &succ_ranges,
            modules: &modules,
        };
        let scc_set: FxHashSet<usize> = (0..4).collect();
        let mut seen = FxHashSet::default();
        let mut cycles = Vec::new();

        // max_cycles = 2: should stop after finding 2
        dfs_find_cycles_from(0, 2, &scc_set, &succs, 2, &mut seen, &mut cycles);
        assert!(
            cycles.len() <= 2,
            "should respect max_cycles limit, got {}",
            cycles.len()
        );
    }

    #[test]
    fn dfs_find_cycles_from_ignores_nodes_outside_scc() {
        // 0->1->2->0 but only {0, 1} in SCC set — node 2 should be ignored
        let (modules, all_succs, succ_ranges) = build_test_succs(3, &[(0, 1), (1, 2), (2, 0)]);
        let succs = SuccessorMap {
            all_succs: &all_succs,
            succ_ranges: &succ_ranges,
            modules: &modules,
        };
        let scc_set: FxHashSet<usize> = [0, 1].into_iter().collect();
        let mut seen = FxHashSet::default();
        let mut cycles = Vec::new();

        for depth in 2..=3 {
            dfs_find_cycles_from(0, depth, &scc_set, &succs, 10, &mut seen, &mut cycles);
        }
        assert!(
            cycles.is_empty(),
            "should not find cycles through nodes outside the SCC set"
        );
    }

    // -----------------------------------------------------------------------
    // enumerate_elementary_cycles tests
    // -----------------------------------------------------------------------

    #[test]
    fn enumerate_elementary_cycles_empty_scc() {
        let (modules, all_succs, succ_ranges) = build_test_succs(0, &[]);
        let succs = SuccessorMap {
            all_succs: &all_succs,
            succ_ranges: &succ_ranges,
            modules: &modules,
        };
        let cycles = enumerate_elementary_cycles(&[], &succs, 10);
        assert!(cycles.is_empty());
    }

    #[test]
    fn enumerate_elementary_cycles_max_cycles_limit() {
        // Complete graph of 4 nodes — many elementary cycles
        let edges: Vec<(usize, usize)> = (0..4)
            .flat_map(|i| (0..4).filter(move |&j| i != j).map(move |j| (i, j)))
            .collect();
        let (modules, all_succs, succ_ranges) = build_test_succs(4, &edges);
        let succs = SuccessorMap {
            all_succs: &all_succs,
            succ_ranges: &succ_ranges,
            modules: &modules,
        };
        let scc_nodes: Vec<usize> = (0..4).collect();

        let cycles = enumerate_elementary_cycles(&scc_nodes, &succs, 3);
        assert!(
            cycles.len() <= 3,
            "should respect max_cycles=3 limit, got {}",
            cycles.len()
        );
    }

    #[test]
    fn enumerate_elementary_cycles_finds_all_in_triangle() {
        // 0->1->2->0 — single elementary cycle
        let (modules, all_succs, succ_ranges) = build_test_succs(3, &[(0, 1), (1, 2), (2, 0)]);
        let succs = SuccessorMap {
            all_succs: &all_succs,
            succ_ranges: &succ_ranges,
            modules: &modules,
        };
        let scc_nodes: Vec<usize> = vec![0, 1, 2];

        let cycles = enumerate_elementary_cycles(&scc_nodes, &succs, 20);
        assert_eq!(cycles.len(), 1);
        assert_eq!(cycles[0].len(), 3);
    }

    #[test]
    fn enumerate_elementary_cycles_iterative_deepening_order() {
        // SCC with both 2-node and 3-node cycles
        // 0->1->0 (2-cycle) and 0->1->2->0 (3-cycle)
        let (modules, all_succs, succ_ranges) =
            build_test_succs(3, &[(0, 1), (1, 0), (1, 2), (2, 0)]);
        let succs = SuccessorMap {
            all_succs: &all_succs,
            succ_ranges: &succ_ranges,
            modules: &modules,
        };
        let scc_nodes: Vec<usize> = vec![0, 1, 2];

        let cycles = enumerate_elementary_cycles(&scc_nodes, &succs, 20);
        assert!(cycles.len() >= 2, "should find at least 2 cycles");
        // Iterative deepening: shorter cycles should come first
        assert!(
            cycles[0].len() <= cycles[cycles.len() - 1].len(),
            "shorter cycles should be found before longer ones"
        );
    }

    // -----------------------------------------------------------------------
    // Integration-level edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn find_cycles_max_cycles_per_scc_respected() {
        // Dense SCC (complete graph of 5 nodes) — should cap at MAX_CYCLES_PER_SCC (20)
        let edges: Vec<(u32, u32)> = (0..5)
            .flat_map(|i| (0..5).filter(move |&j| i != j).map(move |j| (i, j)))
            .collect();
        let graph = build_cycle_graph(5, &edges);
        let cycles = graph.find_cycles();
        // K5 has many elementary cycles, but we cap at 20 per SCC
        assert!(
            cycles.len() <= 20,
            "should cap at MAX_CYCLES_PER_SCC, got {}",
            cycles.len()
        );
        assert!(
            !cycles.is_empty(),
            "dense graph should still find some cycles"
        );
    }

    #[test]
    fn find_cycles_graph_with_no_cycles_returns_empty() {
        // Star topology: center -> all leaves, no cycles possible
        let graph = build_cycle_graph(5, &[(0, 1), (0, 2), (0, 3), (0, 4)]);
        assert!(graph.find_cycles().is_empty());
    }

    #[test]
    fn find_cycles_diamond_no_cycle() {
        // Diamond without back-edge: A->B, A->C, B->D, C->D — no cycle
        let graph = build_cycle_graph(4, &[(0, 1), (0, 2), (1, 3), (2, 3)]);
        assert!(graph.find_cycles().is_empty());
    }

    #[test]
    fn find_cycles_diamond_with_back_edge() {
        // Diamond with back-edge: A->B, A->C, B->D, C->D, D->A
        let graph = build_cycle_graph(4, &[(0, 1), (0, 2), (1, 3), (2, 3), (3, 0)]);
        let cycles = graph.find_cycles();
        assert!(
            cycles.len() >= 2,
            "diamond with back-edge should have at least 2 elementary cycles, got {}",
            cycles.len()
        );
        // Shortest cycles should be length 3 (A->B->D->A and A->C->D->A)
        assert_eq!(cycles[0].len(), 3);
    }

    // -----------------------------------------------------------------------
    // Additional canonical_cycle tests
    // -----------------------------------------------------------------------

    #[test]
    fn canonical_cycle_non_sequential_indices() {
        // Cycle with non-sequential node indices [3, 1, 4] — file1 has smallest path
        let (modules, _, _) = build_test_succs(5, &[]);
        let result = canonical_cycle(&[3, 1, 4], &modules);
        // file1 has path "/project/file1.ts" which is smallest, so rotation starts there
        assert_eq!(result, vec![1, 4, 3]);
    }

    #[test]
    fn canonical_cycle_different_starting_points_same_result() {
        // The same logical cycle [0, 1, 2, 3] presented from four different starting points
        // should always canonicalize to [0, 1, 2, 3] since file0 has the smallest path.
        let (modules, _, _) = build_test_succs(4, &[]);
        let r1 = canonical_cycle(&[0, 1, 2, 3], &modules);
        let r2 = canonical_cycle(&[1, 2, 3, 0], &modules);
        let r3 = canonical_cycle(&[2, 3, 0, 1], &modules);
        let r4 = canonical_cycle(&[3, 0, 1, 2], &modules);
        assert_eq!(r1, r2);
        assert_eq!(r2, r3);
        assert_eq!(r3, r4);
        assert_eq!(r1, vec![0, 1, 2, 3]);
    }

    #[test]
    fn canonical_cycle_two_node_both_rotations() {
        // Two-node cycle: [0, 1] and [1, 0] should both canonicalize to [0, 1]
        let (modules, _, _) = build_test_succs(2, &[]);
        assert_eq!(canonical_cycle(&[0, 1], &modules), vec![0, 1]);
        assert_eq!(canonical_cycle(&[1, 0], &modules), vec![0, 1]);
    }

    // -----------------------------------------------------------------------
    // Self-loop unit-level tests
    // -----------------------------------------------------------------------

    #[test]
    fn dfs_find_cycles_from_self_loop_not_found() {
        // Node 0 has a self-edge (0->0). The DFS requires path.len() >= 2 for a cycle,
        // so a self-loop should not be detected as a cycle.
        let (modules, all_succs, succ_ranges) = build_test_succs(1, &[(0, 0)]);
        let succs = SuccessorMap {
            all_succs: &all_succs,
            succ_ranges: &succ_ranges,
            modules: &modules,
        };
        let scc_set: FxHashSet<usize> = std::iter::once(0).collect();
        let mut seen = FxHashSet::default();
        let mut cycles = Vec::new();

        for depth in 1..=3 {
            dfs_find_cycles_from(0, depth, &scc_set, &succs, 10, &mut seen, &mut cycles);
        }
        assert!(
            cycles.is_empty(),
            "self-loop should not be detected as a cycle by dfs_find_cycles_from"
        );
    }

    #[test]
    fn enumerate_elementary_cycles_self_loop_not_found() {
        // Single node with self-edge — enumerate should find no elementary cycles
        let (modules, all_succs, succ_ranges) = build_test_succs(1, &[(0, 0)]);
        let succs = SuccessorMap {
            all_succs: &all_succs,
            succ_ranges: &succ_ranges,
            modules: &modules,
        };
        let cycles = enumerate_elementary_cycles(&[0], &succs, 20);
        assert!(
            cycles.is_empty(),
            "self-loop should not produce elementary cycles"
        );
    }

    // -----------------------------------------------------------------------
    // Two overlapping cycles sharing an edge
    // -----------------------------------------------------------------------

    #[test]
    fn find_cycles_two_cycles_sharing_edge() {
        // A->B->C->A and A->B->D->A share edge A->B
        // Should find exactly 2 elementary cycles, both of length 3
        let graph = build_cycle_graph(4, &[(0, 1), (1, 2), (2, 0), (1, 3), (3, 0)]);
        let cycles = graph.find_cycles();
        assert_eq!(
            cycles.len(),
            2,
            "two cycles sharing edge A->B should both be found, got {}",
            cycles.len()
        );
        assert!(
            cycles.iter().all(|c| c.len() == 3),
            "both cycles should have length 3"
        );
    }

    #[test]
    fn enumerate_elementary_cycles_shared_edge() {
        // Same topology at the unit level: 0->1->2->0 and 0->1->3->0 share edge 0->1
        let (modules, all_succs, succ_ranges) =
            build_test_succs(4, &[(0, 1), (1, 2), (2, 0), (1, 3), (3, 0)]);
        let succs = SuccessorMap {
            all_succs: &all_succs,
            succ_ranges: &succ_ranges,
            modules: &modules,
        };
        let scc_nodes: Vec<usize> = vec![0, 1, 2, 3];
        let cycles = enumerate_elementary_cycles(&scc_nodes, &succs, 20);
        assert_eq!(
            cycles.len(),
            2,
            "should find exactly 2 elementary cycles sharing edge 0->1, got {}",
            cycles.len()
        );
    }

    // -----------------------------------------------------------------------
    // Large SCC with multiple elementary cycles — verify all found
    // -----------------------------------------------------------------------

    #[test]
    fn enumerate_elementary_cycles_pentagon_with_chords() {
        // Pentagon 0->1->2->3->4->0 plus chords 0->2 and 0->3
        // Elementary cycles include:
        //   len 3: 0->2->3->4->... no, let's enumerate:
        //   0->1->2->3->4->0 (len 5)
        //   0->2->3->4->0 (len 4, via chord 0->2)
        //   0->3->4->0 (len 3, via chord 0->3)
        //   0->1->2->... subsets through chords
        let (modules, all_succs, succ_ranges) =
            build_test_succs(5, &[(0, 1), (1, 2), (2, 3), (3, 4), (4, 0), (0, 2), (0, 3)]);
        let succs = SuccessorMap {
            all_succs: &all_succs,
            succ_ranges: &succ_ranges,
            modules: &modules,
        };
        let scc_nodes: Vec<usize> = vec![0, 1, 2, 3, 4];
        let cycles = enumerate_elementary_cycles(&scc_nodes, &succs, 20);

        // Should find at least 3 distinct elementary cycles (the pentagon + two chord-shortened)
        assert!(
            cycles.len() >= 3,
            "pentagon with chords should have at least 3 elementary cycles, got {}",
            cycles.len()
        );
        // All cycles should be unique (no duplicates)
        let unique: FxHashSet<Vec<usize>> = cycles.iter().cloned().collect();
        assert_eq!(
            unique.len(),
            cycles.len(),
            "all enumerated cycles should be unique"
        );
        // Shortest cycle should be length 3 (0->3->4->0)
        assert_eq!(
            cycles[0].len(),
            3,
            "shortest cycle in pentagon with chords should be length 3"
        );
    }

    #[test]
    fn find_cycles_large_scc_complete_graph_k6() {
        // Complete graph K6: every node connects to every other node
        // This creates a dense SCC with many elementary cycles
        let edges: Vec<(u32, u32)> = (0..6)
            .flat_map(|i| (0..6).filter(move |&j| i != j).map(move |j| (i, j)))
            .collect();
        let graph = build_cycle_graph(6, &edges);
        let cycles = graph.find_cycles();

        // K6 has a huge number of elementary cycles; we should find many but cap at 20
        assert!(
            cycles.len() <= 20,
            "should cap at MAX_CYCLES_PER_SCC (20), got {}",
            cycles.len()
        );
        assert_eq!(
            cycles.len(),
            20,
            "K6 has far more than 20 elementary cycles, so we should hit the cap"
        );
        // Shortest cycles should be 2-node cycles (since every pair has bidirectional edges)
        assert_eq!(cycles[0].len(), 2, "shortest cycles in K6 should be 2-node");
    }

    // -----------------------------------------------------------------------
    // Depth limit enforcement in enumerate_elementary_cycles
    // -----------------------------------------------------------------------

    #[test]
    fn enumerate_elementary_cycles_respects_depth_cap_of_12() {
        // Build a single long cycle of 15 nodes: 0->1->2->...->14->0
        // enumerate_elementary_cycles caps depth at min(scc.len(), 12) = 12
        // So the 15-node cycle should NOT be found.
        let edges: Vec<(usize, usize)> = (0..15).map(|i| (i, (i + 1) % 15)).collect();
        let (modules, all_succs, succ_ranges) = build_test_succs(15, &edges);
        let succs = SuccessorMap {
            all_succs: &all_succs,
            succ_ranges: &succ_ranges,
            modules: &modules,
        };
        let scc_nodes: Vec<usize> = (0..15).collect();
        let cycles = enumerate_elementary_cycles(&scc_nodes, &succs, 20);

        assert!(
            cycles.is_empty(),
            "a pure 15-node cycle should not be found with depth cap of 12, got {} cycles",
            cycles.len()
        );
    }

    #[test]
    fn enumerate_elementary_cycles_finds_cycle_at_depth_cap_boundary() {
        // Build a single cycle of exactly 12 nodes: 0->1->...->11->0
        // depth cap = min(12, 12) = 12, so this cycle should be found.
        let edges: Vec<(usize, usize)> = (0..12).map(|i| (i, (i + 1) % 12)).collect();
        let (modules, all_succs, succ_ranges) = build_test_succs(12, &edges);
        let succs = SuccessorMap {
            all_succs: &all_succs,
            succ_ranges: &succ_ranges,
            modules: &modules,
        };
        let scc_nodes: Vec<usize> = (0..12).collect();
        let cycles = enumerate_elementary_cycles(&scc_nodes, &succs, 20);

        assert_eq!(
            cycles.len(),
            1,
            "a pure 12-node cycle should be found at the depth cap boundary"
        );
        assert_eq!(cycles[0].len(), 12);
    }

    #[test]
    fn enumerate_elementary_cycles_13_node_pure_cycle_not_found() {
        // 13-node pure cycle: depth cap = min(13, 12) = 12, so the 13-node cycle is skipped
        let edges: Vec<(usize, usize)> = (0..13).map(|i| (i, (i + 1) % 13)).collect();
        let (modules, all_succs, succ_ranges) = build_test_succs(13, &edges);
        let succs = SuccessorMap {
            all_succs: &all_succs,
            succ_ranges: &succ_ranges,
            modules: &modules,
        };
        let scc_nodes: Vec<usize> = (0..13).collect();
        let cycles = enumerate_elementary_cycles(&scc_nodes, &succs, 20);

        assert!(
            cycles.is_empty(),
            "13-node pure cycle exceeds depth cap of 12"
        );
    }

    // -----------------------------------------------------------------------
    // MAX_CYCLES_PER_SCC enforcement at integration level
    // -----------------------------------------------------------------------

    #[test]
    fn find_cycles_max_cycles_per_scc_enforced_on_k7() {
        // K7 complete graph: enormous number of elementary cycles
        // Should still be capped at 20 per SCC
        let edges: Vec<(u32, u32)> = (0..7)
            .flat_map(|i| (0..7).filter(move |&j| i != j).map(move |j| (i, j)))
            .collect();
        let graph = build_cycle_graph(7, &edges);
        let cycles = graph.find_cycles();

        assert!(
            cycles.len() <= 20,
            "K7 should cap at MAX_CYCLES_PER_SCC (20), got {}",
            cycles.len()
        );
        assert_eq!(
            cycles.len(),
            20,
            "K7 has far more than 20 elementary cycles, should hit the cap exactly"
        );
    }

    #[test]
    fn find_cycles_two_dense_sccs_each_capped() {
        // Two separate complete subgraphs K4 (nodes 0-3) and K4 (nodes 4-7)
        // Each has many elementary cycles; total should be capped at 20 per SCC
        let mut edges: Vec<(u32, u32)> = Vec::new();
        // First K4: nodes 0-3
        for i in 0..4 {
            for j in 0..4 {
                if i != j {
                    edges.push((i, j));
                }
            }
        }
        // Second K4: nodes 4-7
        for i in 4..8 {
            for j in 4..8 {
                if i != j {
                    edges.push((i, j));
                }
            }
        }
        let graph = build_cycle_graph(8, &edges);
        let cycles = graph.find_cycles();

        // Each K4 has 2-cycles: C(4,2)=6, plus 3-cycles and 4-cycles
        // Both SCCs contribute cycles, but each is independently capped at 20
        assert!(!cycles.is_empty(), "two dense SCCs should produce cycles");
        // Total can be up to 40 (20 per SCC), but K4 has fewer than 20 elementary cycles
        // K4 elementary cycles: 6 two-cycles + 8 three-cycles + 3 four-cycles = 17
        // So we should get all from both SCCs
        assert!(
            cycles.len() > 2,
            "should find multiple cycles across both SCCs, got {}",
            cycles.len()
        );
    }

    mod proptests {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            /// A DAG (directed acyclic graph) should always have zero cycles.
            /// We construct a DAG by only allowing edges from lower to higher node indices.
            #[test]
            fn dag_has_no_cycles(
                file_count in 2..20usize,
                edge_pairs in prop::collection::vec((0..19u32, 0..19u32), 0..30),
            ) {
                // Filter to only forward edges (i < j) to guarantee a DAG
                let dag_edges: Vec<(u32, u32)> = edge_pairs
                    .into_iter()
                    .filter(|(a, b)| (*a as usize) < file_count && (*b as usize) < file_count && a < b)
                    .collect();

                let graph = build_cycle_graph(file_count, &dag_edges);
                let cycles = graph.find_cycles();
                prop_assert!(
                    cycles.is_empty(),
                    "DAG should have no cycles, but found {}",
                    cycles.len()
                );
            }

            /// Adding mutual edges A->B->A should always detect a cycle.
            #[test]
            fn mutual_edges_always_detect_cycle(extra_nodes in 0..10usize) {
                let file_count = 2 + extra_nodes;
                let graph = build_cycle_graph(file_count, &[(0, 1), (1, 0)]);
                let cycles = graph.find_cycles();
                prop_assert!(
                    !cycles.is_empty(),
                    "A->B->A should always produce at least one cycle"
                );
                // The cycle should contain both nodes 0 and 1
                let has_pair_cycle = cycles.iter().any(|c| {
                    c.contains(&FileId(0)) && c.contains(&FileId(1))
                });
                prop_assert!(has_pair_cycle, "Should find a cycle containing nodes 0 and 1");
            }

            /// All cycle members should be valid FileId indices.
            #[test]
            fn cycle_members_are_valid_indices(
                file_count in 2..15usize,
                edge_pairs in prop::collection::vec((0..14u32, 0..14u32), 1..20),
            ) {
                let edges: Vec<(u32, u32)> = edge_pairs
                    .into_iter()
                    .filter(|(a, b)| (*a as usize) < file_count && (*b as usize) < file_count && a != b)
                    .collect();

                let graph = build_cycle_graph(file_count, &edges);
                let cycles = graph.find_cycles();
                for cycle in &cycles {
                    prop_assert!(cycle.len() >= 2, "Cycles must have at least 2 nodes");
                    for file_id in cycle {
                        prop_assert!(
                            (file_id.0 as usize) < file_count,
                            "FileId {} exceeds file count {}",
                            file_id.0, file_count
                        );
                    }
                }
            }

            /// Cycles should be sorted by length (shortest first).
            #[test]
            fn cycles_sorted_by_length(
                file_count in 3..12usize,
                edge_pairs in prop::collection::vec((0..11u32, 0..11u32), 2..25),
            ) {
                let edges: Vec<(u32, u32)> = edge_pairs
                    .into_iter()
                    .filter(|(a, b)| (*a as usize) < file_count && (*b as usize) < file_count && a != b)
                    .collect();

                let graph = build_cycle_graph(file_count, &edges);
                let cycles = graph.find_cycles();
                for window in cycles.windows(2) {
                    prop_assert!(
                        window[0].len() <= window[1].len(),
                        "Cycles should be sorted by length: {} > {}",
                        window[0].len(), window[1].len()
                    );
                }
            }
        }
    }

    // ── Type-only cycle tests ────────────────────────────────────

    /// Build a cycle graph where specific edges are type-only.
    fn build_cycle_graph_with_type_only(
        file_count: usize,
        edges_spec: &[(u32, u32, bool)], // (source, target, is_type_only)
    ) -> ModuleGraph {
        let files: Vec<DiscoveredFile> = (0..file_count)
            .map(|i| DiscoveredFile {
                id: FileId(i as u32),
                path: PathBuf::from(format!("/project/file{i}.ts")),
                size_bytes: 100,
            })
            .collect();

        let resolved_modules: Vec<ResolvedModule> = (0..file_count)
            .map(|i| {
                let imports: Vec<ResolvedImport> = edges_spec
                    .iter()
                    .filter(|(src, _, _)| *src == i as u32)
                    .map(|(_, tgt, type_only)| ResolvedImport {
                        info: ImportInfo {
                            source: format!("./file{tgt}"),
                            imported_name: ImportedName::Named("x".to_string()),
                            local_name: "x".to_string(),
                            is_type_only: *type_only,
                            from_style: false,
                            span: oxc_span::Span::new(0, 10),
                            source_span: oxc_span::Span::default(),
                        },
                        target: ResolveResult::InternalModule(FileId(*tgt)),
                    })
                    .collect();

                ResolvedModule {
                    file_id: FileId(i as u32),
                    path: PathBuf::from(format!("/project/file{i}.ts")),
                    exports: vec![fallow_types::extract::ExportInfo {
                        name: ExportName::Named("x".to_string()),
                        local_name: Some("x".to_string()),
                        is_type_only: false,
                        visibility: VisibilityTag::None,
                        span: oxc_span::Span::new(0, 20),
                        members: vec![],
                        is_side_effect_used: false,
                        super_class: None,
                    }],
                    re_exports: vec![],
                    resolved_imports: imports,
                    resolved_dynamic_imports: vec![],
                    resolved_dynamic_patterns: vec![],
                    member_accesses: vec![],
                    whole_object_uses: vec![],
                    has_cjs_exports: false,
                    unused_import_bindings: FxHashSet::default(),
                    type_referenced_import_bindings: vec![],
                    value_referenced_import_bindings: vec![],
                }
            })
            .collect();

        let entry_points = vec![EntryPoint {
            path: PathBuf::from("/project/file0.ts"),
            source: EntryPointSource::PackageJsonMain,
        }];

        ModuleGraph::build(&resolved_modules, &entry_points, &files)
    }

    #[test]
    fn type_only_bidirectional_import_not_a_cycle() {
        // A imports type from B, B imports type from A — not a runtime cycle
        let graph = build_cycle_graph_with_type_only(2, &[(0, 1, true), (1, 0, true)]);
        let cycles = graph.find_cycles();
        assert!(
            cycles.is_empty(),
            "type-only bidirectional imports should not be reported as cycles"
        );
    }

    #[test]
    fn mixed_type_and_value_import_not_a_cycle() {
        // A value-imports B, B type-imports A — NOT a runtime cycle.
        // B's import of A is type-only (erased at compile time), so the runtime
        // dependency is one-directional: A→B only.
        let graph = build_cycle_graph_with_type_only(2, &[(0, 1, false), (1, 0, true)]);
        let cycles = graph.find_cycles();
        assert!(
            cycles.is_empty(),
            "A->B (value) + B->A (type-only) is not a runtime cycle"
        );
    }

    #[test]
    fn both_value_imports_with_one_type_still_a_cycle() {
        // A value-imports B AND type-imports B. B value-imports A.
        // A->B has a non-type-only symbol, B->A has a non-type-only symbol = real cycle.
        let graph = build_cycle_graph_with_type_only(2, &[(0, 1, false), (1, 0, false)]);
        let cycles = graph.find_cycles();
        assert!(
            !cycles.is_empty(),
            "bidirectional value imports should be reported as a cycle"
        );
    }

    #[test]
    fn all_value_imports_still_a_cycle() {
        // A value-imports B, B value-imports A — still a cycle
        let graph = build_cycle_graph_with_type_only(2, &[(0, 1, false), (1, 0, false)]);
        let cycles = graph.find_cycles();
        assert_eq!(cycles.len(), 1);
    }

    #[test]
    fn three_node_type_only_cycle_not_reported() {
        // A -> B -> C -> A, all type-only
        let graph =
            build_cycle_graph_with_type_only(3, &[(0, 1, true), (1, 2, true), (2, 0, true)]);
        let cycles = graph.find_cycles();
        assert!(
            cycles.is_empty(),
            "three-node type-only cycle should not be reported"
        );
    }

    #[test]
    fn three_node_cycle_one_value_edge_still_reported() {
        // A -value-> B -type-> C -type-> A
        // B->C and C->A are type-only, but A->B is a value edge.
        // This still forms a cycle because Tarjan's considers all non-type-only successors.
        // However, since B only has type-only successors (B->C is type-only),
        // B has no runtime successors, so no SCC with B will form.
        let graph =
            build_cycle_graph_with_type_only(3, &[(0, 1, false), (1, 2, true), (2, 0, true)]);
        let cycles = graph.find_cycles();
        // B has no runtime successors (B->C is type-only), so the cycle is broken
        assert!(
            cycles.is_empty(),
            "cycle broken by type-only edge in the middle should not be reported"
        );
    }
}
