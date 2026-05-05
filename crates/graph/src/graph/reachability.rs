//! Phase 3: BFS reachability from entry points.

use std::collections::VecDeque;

use fixedbitset::FixedBitSet;

use super::ModuleGraph;

impl ModuleGraph {
    fn collect_reachable(
        &self,
        entry_points: &rustc_hash::FxHashSet<fallow_types::discover::FileId>,
        total_capacity: usize,
    ) -> FixedBitSet {
        let mut visited = FixedBitSet::with_capacity(total_capacity);
        let mut queue = VecDeque::new();

        for &ep_id in entry_points {
            if (ep_id.0 as usize) < total_capacity {
                visited.insert(ep_id.0 as usize);
                queue.push_back(ep_id);
            }
        }

        while let Some(file_id) = queue.pop_front() {
            if (file_id.0 as usize) >= self.modules.len() {
                continue;
            }
            let module = &self.modules[file_id.0 as usize];
            for edge in &self.edges[module.edge_range.clone()] {
                let target_idx = edge.target.0 as usize;
                if target_idx < total_capacity && !visited.contains(target_idx) {
                    visited.insert(target_idx);
                    queue.push_back(edge.target);
                }
            }
        }

        visited
    }

    /// Mark modules reachable from overall, runtime, and test entry points via BFS.
    ///
    /// Skips redundant BFS passes when entry point sets are identical or empty.
    pub(super) fn mark_reachable(
        &mut self,
        entry_points: &rustc_hash::FxHashSet<fallow_types::discover::FileId>,
        runtime_entry_points: &rustc_hash::FxHashSet<fallow_types::discover::FileId>,
        test_entry_points: &rustc_hash::FxHashSet<fallow_types::discover::FileId>,
        total_capacity: usize,
    ) {
        let visited = self.collect_reachable(entry_points, total_capacity);

        // Reuse the overall BFS result when runtime roots are the same set.
        let runtime_same = runtime_entry_points == entry_points;
        let runtime_visited = if runtime_same {
            None
        } else {
            Some(self.collect_reachable(runtime_entry_points, total_capacity))
        };

        // Skip BFS entirely when there are no test entry points.
        let test_visited = if test_entry_points.is_empty() {
            None
        } else {
            Some(self.collect_reachable(test_entry_points, total_capacity))
        };

        for (idx, module) in self.modules.iter_mut().enumerate() {
            module.set_reachable(visited.contains(idx));
            module.set_runtime_reachable(
                runtime_visited
                    .as_ref()
                    .map_or_else(|| visited.contains(idx), |rv| rv.contains(idx)),
            );
            module.set_test_reachable(test_visited.as_ref().is_some_and(|tv| tv.contains(idx)));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use rustc_hash::FxHashSet;

    use crate::graph::ModuleGraph;
    use crate::resolve::{ResolveResult, ResolvedImport, ResolvedModule};
    use fallow_types::discover::{DiscoveredFile, EntryPoint, EntryPointSource, FileId};
    use fallow_types::extract::{ExportName, ImportInfo, ImportedName, VisibilityTag};

    /// Build a graph with separate runtime and test entry point sets.
    ///
    /// `file_count` nodes are created, `edges_spec` defines directed edges,
    /// `runtime_eps` and `test_eps` are file indices for each entry point
    /// category. All entry points (union of runtime + test) form the overall
    /// entry set.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "test file counts are trivially small"
    )]
    fn build_reachability_graph(
        file_count: usize,
        edges_spec: &[(u32, u32)],
        runtime_eps: &[u32],
        test_eps: &[u32],
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

        let runtime_entry_points: Vec<EntryPoint> = runtime_eps
            .iter()
            .map(|&i| EntryPoint {
                path: PathBuf::from(format!("/project/file{i}.ts")),
                source: EntryPointSource::PackageJsonMain,
            })
            .collect();

        let test_entry_points: Vec<EntryPoint> = test_eps
            .iter()
            .map(|&i| EntryPoint {
                path: PathBuf::from(format!("/project/file{i}.ts")),
                source: EntryPointSource::TestFile,
            })
            .collect();

        // Overall entry points = runtime + test (union)
        let mut all_entry_points = runtime_entry_points.clone();
        all_entry_points.extend(test_entry_points.iter().cloned());

        ModuleGraph::build_with_reachability_roots(
            &resolved_modules,
            &all_entry_points,
            &runtime_entry_points,
            &test_entry_points,
            &files,
        )
    }

    // ── Basic reachability from entry points ────────────────────

    #[test]
    fn entry_point_is_reachable() {
        // Single entry point, no edges.
        let graph = build_reachability_graph(1, &[], &[0], &[]);
        assert!(graph.modules[0].is_reachable());
    }

    #[test]
    fn direct_dependency_is_reachable() {
        // A -> B, A is entry.
        let graph = build_reachability_graph(2, &[(0, 1)], &[0], &[]);
        assert!(graph.modules[0].is_reachable());
        assert!(graph.modules[1].is_reachable());
    }

    // ── Chain reachability ──────────────────────────────────────

    #[test]
    fn chain_reachability_a_b_c() {
        // A -> B -> C, A is entry. B and C should be reachable.
        let graph = build_reachability_graph(3, &[(0, 1), (1, 2)], &[0], &[]);
        assert!(graph.modules[0].is_reachable());
        assert!(graph.modules[1].is_reachable());
        assert!(graph.modules[2].is_reachable());
    }

    #[test]
    fn deep_chain_all_reachable() {
        // 0 -> 1 -> 2 -> 3 -> 4
        let graph = build_reachability_graph(5, &[(0, 1), (1, 2), (2, 3), (3, 4)], &[0], &[]);
        for i in 0..5 {
            assert!(
                graph.modules[i].is_reachable(),
                "file{i} should be reachable through chain"
            );
        }
    }

    // ── Unreachable files ───────────────────────────────────────

    #[test]
    fn disconnected_file_is_unreachable() {
        // A -> B, C is disconnected. A is entry.
        let graph = build_reachability_graph(3, &[(0, 1)], &[0], &[]);
        assert!(graph.modules[0].is_reachable());
        assert!(graph.modules[1].is_reachable());
        assert!(!graph.modules[2].is_reachable());
    }

    #[test]
    fn no_entry_points_all_unreachable() {
        // Two files, an edge, but no entry points.
        let graph = build_reachability_graph(2, &[(0, 1)], &[], &[]);
        assert!(!graph.modules[0].is_reachable());
        assert!(!graph.modules[1].is_reachable());
    }

    // ── Cycle handling ──────────────────────────────────────────

    #[test]
    fn cycle_both_reachable_when_entry() {
        // A -> B -> A, A is entry. Both should be reachable.
        let graph = build_reachability_graph(2, &[(0, 1), (1, 0)], &[0], &[]);
        assert!(graph.modules[0].is_reachable());
        assert!(graph.modules[1].is_reachable());
    }

    #[test]
    fn three_node_cycle_all_reachable() {
        // A -> B -> C -> A, A is entry.
        let graph = build_reachability_graph(3, &[(0, 1), (1, 2), (2, 0)], &[0], &[]);
        for i in 0..3 {
            assert!(
                graph.modules[i].is_reachable(),
                "file{i} in cycle should be reachable"
            );
        }
    }

    #[test]
    fn cycle_not_reachable_from_entry() {
        // 0 is entry (no edges to cycle). 1 -> 2 -> 1 form a disconnected cycle.
        let graph = build_reachability_graph(3, &[(1, 2), (2, 1)], &[0], &[]);
        assert!(graph.modules[0].is_reachable());
        assert!(!graph.modules[1].is_reachable());
        assert!(!graph.modules[2].is_reachable());
    }

    // ── Runtime vs test entry point separation ──────────────────

    #[test]
    fn runtime_reachable_only_from_runtime_entries() {
        // 0 (runtime) -> 1, 2 (test) -> 3
        let graph = build_reachability_graph(4, &[(0, 1), (2, 3)], &[0], &[2]);
        // File 0 and 1: runtime-reachable
        assert!(graph.modules[0].is_runtime_reachable());
        assert!(graph.modules[1].is_runtime_reachable());
        // File 2 and 3: not runtime-reachable
        assert!(!graph.modules[2].is_runtime_reachable());
        assert!(!graph.modules[3].is_runtime_reachable());
    }

    #[test]
    fn test_reachable_only_from_test_entries() {
        // 0 (runtime) -> 1, 2 (test) -> 3
        let graph = build_reachability_graph(4, &[(0, 1), (2, 3)], &[0], &[2]);
        // File 0 and 1: not test-reachable
        assert!(!graph.modules[0].is_test_reachable());
        assert!(!graph.modules[1].is_test_reachable());
        // File 2 and 3: test-reachable
        assert!(graph.modules[2].is_test_reachable());
        assert!(graph.modules[3].is_test_reachable());
    }

    #[test]
    fn overall_reachable_is_union_of_runtime_and_test() {
        // 0 (runtime) -> 1, 2 (test) -> 3
        let graph = build_reachability_graph(4, &[(0, 1), (2, 3)], &[0], &[2]);
        // All four files are overall-reachable (union of runtime + test)
        for i in 0..4 {
            assert!(
                graph.modules[i].is_reachable(),
                "file{i} should be overall-reachable"
            );
        }
    }

    #[test]
    fn shared_dependency_is_both_runtime_and_test_reachable() {
        // 0 (runtime) -> 2, 1 (test) -> 2
        let graph = build_reachability_graph(3, &[(0, 2), (1, 2)], &[0], &[1]);
        assert!(graph.modules[2].is_runtime_reachable());
        assert!(graph.modules[2].is_test_reachable());
        assert!(graph.modules[2].is_reachable());
    }

    // ── Short-circuit: runtime_entry_points == entry_points ─────

    #[test]
    fn runtime_same_as_overall_reuses_bfs() {
        // When there are no test entry points, runtime == overall.
        // All reachable files should be both overall-reachable and runtime-reachable.
        let graph = build_reachability_graph(3, &[(0, 1), (1, 2)], &[0], &[]);
        for i in 0..3 {
            assert_eq!(
                graph.modules[i].is_reachable(),
                graph.modules[i].is_runtime_reachable(),
                "file{i}: reachable and runtime_reachable should match when runtime==overall"
            );
        }
    }

    // ── test_entry_points.is_empty() fast-path ──────────────────

    #[test]
    fn empty_test_entries_none_test_reachable() {
        // No test entry points: no file should be test-reachable.
        let graph = build_reachability_graph(3, &[(0, 1), (1, 2)], &[0], &[]);
        for i in 0..3 {
            assert!(
                !graph.modules[i].is_test_reachable(),
                "file{i} should not be test-reachable when no test entries exist"
            );
        }
    }

    #[test]
    fn only_test_entries_runtime_unreachable() {
        // Only test entry points, no runtime entries.
        let graph = build_reachability_graph(2, &[(0, 1)], &[], &[0]);
        // Test reachability
        assert!(graph.modules[0].is_test_reachable());
        assert!(graph.modules[1].is_test_reachable());
        // Runtime: no runtime entries, so nothing is runtime-reachable
        assert!(!graph.modules[0].is_runtime_reachable());
        assert!(!graph.modules[1].is_runtime_reachable());
        // Overall reachability comes from test entries
        assert!(graph.modules[0].is_reachable());
        assert!(graph.modules[1].is_reachable());
    }

    // ── Diamond and branching topologies ─────────────────────────

    #[test]
    fn diamond_dependency_all_reachable() {
        // 0 -> 1, 0 -> 2, 1 -> 3, 2 -> 3 (diamond: 0 at top, 3 at bottom)
        let graph = build_reachability_graph(4, &[(0, 1), (0, 2), (1, 3), (2, 3)], &[0], &[]);
        for i in 0..4 {
            assert!(
                graph.modules[i].is_reachable(),
                "file{i} in diamond should be reachable"
            );
        }
    }

    #[test]
    fn multiple_entry_points_reach_disjoint_subtrees() {
        // 0 -> 1, 2 -> 3. Both 0 and 2 are runtime entries.
        let graph = build_reachability_graph(4, &[(0, 1), (2, 3)], &[0, 2], &[]);
        for i in 0..4 {
            assert!(
                graph.modules[i].is_reachable(),
                "file{i} should be reachable from one of the entry points"
            );
        }
    }

    #[test]
    fn empty_graph_no_panics() {
        let graph = build_reachability_graph(0, &[], &[], &[]);
        assert_eq!(graph.module_count(), 0);
    }
}
