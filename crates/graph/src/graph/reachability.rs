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

        let runtime_same = runtime_entry_points == entry_points;
        let runtime_visited = if runtime_same {
            None
        } else {
            Some(self.collect_reachable(runtime_entry_points, total_capacity))
        };

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
                        expected_unused_reason: None,
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
                    semantic_facts: Box::default(),
                    whole_object_uses: Box::default(),
                    has_cjs_exports: false,
                    has_angular_component_template_url: false,
                    unused_import_bindings: FxHashSet::default(),
                    type_referenced_import_bindings: vec![],
                    value_referenced_import_bindings: vec![],
                    namespace_object_aliases: vec![],
                    exported_factory_returns: Box::default(),
                    type_member_types: Box::default(),
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

    #[test]
    fn entry_point_is_reachable() {
        let graph = build_reachability_graph(1, &[], &[0], &[]);
        assert!(graph.modules[0].is_reachable());
    }

    #[test]
    fn direct_dependency_is_reachable() {
        let graph = build_reachability_graph(2, &[(0, 1)], &[0], &[]);
        assert!(graph.modules[0].is_reachable());
        assert!(graph.modules[1].is_reachable());
    }

    #[test]
    fn chain_reachability_a_b_c() {
        let graph = build_reachability_graph(3, &[(0, 1), (1, 2)], &[0], &[]);
        assert!(graph.modules[0].is_reachable());
        assert!(graph.modules[1].is_reachable());
        assert!(graph.modules[2].is_reachable());
    }

    #[test]
    fn deep_chain_all_reachable() {
        let graph = build_reachability_graph(5, &[(0, 1), (1, 2), (2, 3), (3, 4)], &[0], &[]);
        for i in 0..5 {
            assert!(
                graph.modules[i].is_reachable(),
                "file{i} should be reachable through chain"
            );
        }
    }

    #[test]
    fn disconnected_file_is_unreachable() {
        let graph = build_reachability_graph(3, &[(0, 1)], &[0], &[]);
        assert!(graph.modules[0].is_reachable());
        assert!(graph.modules[1].is_reachable());
        assert!(!graph.modules[2].is_reachable());
    }

    #[test]
    fn no_entry_points_all_unreachable() {
        let graph = build_reachability_graph(2, &[(0, 1)], &[], &[]);
        assert!(!graph.modules[0].is_reachable());
        assert!(!graph.modules[1].is_reachable());
    }

    #[test]
    fn cycle_both_reachable_when_entry() {
        let graph = build_reachability_graph(2, &[(0, 1), (1, 0)], &[0], &[]);
        assert!(graph.modules[0].is_reachable());
        assert!(graph.modules[1].is_reachable());
    }

    #[test]
    fn three_node_cycle_all_reachable() {
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
        let graph = build_reachability_graph(3, &[(1, 2), (2, 1)], &[0], &[]);
        assert!(graph.modules[0].is_reachable());
        assert!(!graph.modules[1].is_reachable());
        assert!(!graph.modules[2].is_reachable());
    }

    #[test]
    fn runtime_reachable_only_from_runtime_entries() {
        let graph = build_reachability_graph(4, &[(0, 1), (2, 3)], &[0], &[2]);
        assert!(graph.modules[0].is_runtime_reachable());
        assert!(graph.modules[1].is_runtime_reachable());
        assert!(!graph.modules[2].is_runtime_reachable());
        assert!(!graph.modules[3].is_runtime_reachable());
    }

    #[test]
    fn test_reachable_only_from_test_entries() {
        let graph = build_reachability_graph(4, &[(0, 1), (2, 3)], &[0], &[2]);
        assert!(!graph.modules[0].is_test_reachable());
        assert!(!graph.modules[1].is_test_reachable());
        assert!(graph.modules[2].is_test_reachable());
        assert!(graph.modules[3].is_test_reachable());
    }

    #[test]
    fn overall_reachable_is_union_of_runtime_and_test() {
        let graph = build_reachability_graph(4, &[(0, 1), (2, 3)], &[0], &[2]);
        for i in 0..4 {
            assert!(
                graph.modules[i].is_reachable(),
                "file{i} should be overall-reachable"
            );
        }
    }

    #[test]
    fn shared_dependency_is_both_runtime_and_test_reachable() {
        let graph = build_reachability_graph(3, &[(0, 2), (1, 2)], &[0], &[1]);
        assert!(graph.modules[2].is_runtime_reachable());
        assert!(graph.modules[2].is_test_reachable());
        assert!(graph.modules[2].is_reachable());
    }

    #[test]
    fn runtime_same_as_overall_reuses_bfs() {
        let graph = build_reachability_graph(3, &[(0, 1), (1, 2)], &[0], &[]);
        for i in 0..3 {
            assert_eq!(
                graph.modules[i].is_reachable(),
                graph.modules[i].is_runtime_reachable(),
                "file{i}: reachable and runtime_reachable should match when runtime==overall"
            );
        }
    }

    #[test]
    fn empty_test_entries_none_test_reachable() {
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
        let graph = build_reachability_graph(2, &[(0, 1)], &[], &[0]);
        assert!(graph.modules[0].is_test_reachable());
        assert!(graph.modules[1].is_test_reachable());
        assert!(!graph.modules[0].is_runtime_reachable());
        assert!(!graph.modules[1].is_runtime_reachable());
        assert!(graph.modules[0].is_reachable());
        assert!(graph.modules[1].is_reachable());
    }

    #[test]
    fn diamond_dependency_all_reachable() {
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
