//! Phase 3: BFS reachability from entry points.

use std::collections::VecDeque;

use fixedbitset::FixedBitSet;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::resolve::ResolvedReplacedModuleTarget;
use fallow_types::discover::FileId;
use fallow_types::extract::ModuleLoadMechanism;

use super::{ModuleGraph, TestReachabilityIndex};

enum TestReachability {
    NoRoots,
    Legacy(FixedBitSet),
    Profiled {
        reachable: FixedBitSet,
        index: TestReachabilityIndex,
    },
}

impl TestReachability {
    fn into_parts(self) -> (Option<FixedBitSet>, TestReachabilityIndex) {
        match self {
            Self::NoRoots => (None, TestReachabilityIndex::default()),
            Self::Legacy(reachable) => (Some(reachable), TestReachabilityIndex::default()),
            Self::Profiled { reachable, index } => (Some(reachable), index),
        }
    }

    #[cfg(test)]
    const fn traversal_count(&self) -> usize {
        match self {
            Self::NoRoots => 0,
            Self::Legacy(_) => 1,
            Self::Profiled { index, .. } => index.profile_count,
        }
    }
}

impl ModuleGraph {
    fn collect_reachable(
        &self,
        entry_points: &FxHashSet<FileId>,
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

    fn collect_reachable_with_mask(
        &self,
        entry_points: &FxHashSet<FileId>,
        masked_targets: &FixedBitSet,
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
                let target_is_replaced = masked_targets.contains(target_idx)
                    && edge.symbols.iter().all(|symbol| {
                        !matches!(symbol.mechanism, ModuleLoadMechanism::CommonJsRequire)
                    });
                if target_is_replaced {
                    continue;
                }
                if target_idx < total_capacity && !visited.contains(target_idx) {
                    visited.insert(target_idx);
                    queue.push_back(edge.target);
                }
            }
        }

        visited
    }

    #[expect(
        clippy::cast_possible_truncation,
        reason = "reachable indices originate from u32 FileIds"
    )]
    fn collect_test_reachable(
        &self,
        test_entry_points: &FxHashSet<FileId>,
        replaced_module_targets: &[ResolvedReplacedModuleTarget],
        total_capacity: usize,
    ) -> TestReachability {
        if test_entry_points.is_empty() {
            return TestReachability::NoRoots;
        }

        let mut targets_by_root: FxHashMap<FileId, Vec<FileId>> = FxHashMap::default();
        for replacement in replaced_module_targets {
            if test_entry_points.contains(&replacement.source_file)
                && (replacement.target_file.0 as usize) < total_capacity
            {
                targets_by_root
                    .entry(replacement.source_file)
                    .or_default()
                    .push(replacement.target_file);
            }
        }
        for targets in targets_by_root.values_mut() {
            targets.sort_unstable_by_key(|target| target.0);
            targets.dedup();
        }

        if targets_by_root.is_empty() {
            return TestReachability::Legacy(
                self.collect_reachable(test_entry_points, total_capacity),
            );
        }

        let mut roots_by_mask: FxHashMap<Vec<FileId>, FxHashSet<FileId>> = FxHashMap::default();
        for &root in test_entry_points {
            let mask = targets_by_root.remove(&root).unwrap_or_default();
            roots_by_mask.entry(mask).or_default().insert(root);
        }

        let mut grouped_roots: Vec<_> = roots_by_mask.into_iter().collect();
        grouped_roots.sort_by(|(left_mask, _), (right_mask, _)| {
            left_mask
                .iter()
                .map(|file_id| file_id.0)
                .cmp(right_mask.iter().map(|file_id| file_id.0))
        });

        let mut all_test_reachable = FixedBitSet::with_capacity(total_capacity);
        let mut index = TestReachabilityIndex::new(total_capacity, grouped_roots.len());
        for (profile, (masked_targets, roots)) in grouped_roots.into_iter().enumerate() {
            let mut roots: Vec<_> = roots.into_iter().collect();
            roots.sort_unstable_by_key(|root| root.0);

            let mut mask = FixedBitSet::with_capacity(total_capacity);
            for target in &masked_targets {
                mask.insert(target.0 as usize);
                index.set_masked(*target, profile);
            }
            let root_set = roots.iter().copied().collect();
            let reachable = self.collect_reachable_with_mask(&root_set, &mask, total_capacity);
            all_test_reachable.union_with(&reachable);
            for file_index in reachable.ones() {
                index.set_reachable(FileId(file_index as u32), profile);
            }
        }

        TestReachability::Profiled {
            reachable: all_test_reachable,
            index,
        }
    }

    /// Mark modules reachable from overall, runtime, and test entry points via BFS.
    ///
    /// Skips redundant BFS passes when entry point sets are identical or empty.
    pub(super) fn mark_reachable(
        &mut self,
        entry_points: &FxHashSet<FileId>,
        runtime_entry_points: &FxHashSet<FileId>,
        test_entry_points: &FxHashSet<FileId>,
        replaced_module_targets: &[ResolvedReplacedModuleTarget],
        total_capacity: usize,
    ) {
        let visited = self.collect_reachable(entry_points, total_capacity);

        let runtime_same = runtime_entry_points == entry_points;
        let runtime_visited = if runtime_same {
            None
        } else {
            Some(self.collect_reachable(runtime_entry_points, total_capacity))
        };

        let (test_visited, test_reachability_index) = self
            .collect_test_reachable(test_entry_points, replaced_module_targets, total_capacity)
            .into_parts();
        self.test_reachability_index = test_reachability_index;

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
    use crate::resolve::{
        ResolveResult, ResolvedImport, ResolvedModule, ResolvedReExport,
        ResolvedReplacedModuleTarget,
    };
    use fallow_types::discover::{DiscoveredFile, EntryPoint, EntryPointSource, FileId};
    use fallow_types::extract::{
        ExportName, ImportInfo, ImportedName, ModuleLoadMechanism, ReExportInfo, VisibilityTag,
    };

    /// Build a graph with separate runtime and test entry point sets.
    ///
    /// `file_count` nodes are created, `edges_spec` defines directed edges,
    /// `runtime_eps` and `test_eps` are file indices for each entry point
    /// category. All entry points (union of runtime + test) form the overall
    /// entry set.
    fn build_reachability_graph(
        file_count: usize,
        edges_spec: &[(u32, u32)],
        runtime_eps: &[u32],
        test_eps: &[u32],
    ) -> ModuleGraph {
        let edges: Vec<_> = edges_spec
            .iter()
            .map(|&(source, target)| (source, target, false))
            .collect();
        build_reachability_graph_with_replacements(file_count, &edges, runtime_eps, test_eps, &[])
    }

    #[expect(
        clippy::cast_possible_truncation,
        reason = "test file counts are trivially small"
    )]
    fn build_reachability_graph_with_replacements(
        file_count: usize,
        edges_spec: &[(u32, u32, bool)],
        runtime_eps: &[u32],
        test_eps: &[u32],
        replacements: &[ResolvedReplacedModuleTarget],
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
                    .filter(|(source, _, _)| *source == i as u32)
                    .map(|(_, target, is_commonjs)| ResolvedImport {
                        info: ImportInfo {
                            source: format!("./file{target}"),
                            imported_name: ImportedName::Named("x".to_string()),
                            local_name: "x".to_string(),
                            is_type_only: false,
                            from_style: false,
                            span: oxc_span::Span::new(0, 10),
                            source_span: oxc_span::Span::default(),
                        },
                        target: if *is_commonjs {
                            ResolveResult::CommonJsInternalModule(FileId(*target))
                        } else {
                            ResolveResult::InternalModule(FileId(*target))
                        },
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
                    exported_factory_return_object_shapes: Box::default(),
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

        ModuleGraph::build_with_reachability_roots_and_replacements(
            &resolved_modules,
            replacements,
            &all_entry_points,
            &runtime_entry_points,
            &test_entry_points,
            &files,
        )
    }

    fn build_masked_re_export_graph(
        commonjs_to_barrel: bool,
        commonjs_to_target: bool,
    ) -> ModuleGraph {
        let files: Vec<_> = (0..3)
            .map(|id| DiscoveredFile {
                id: FileId(id),
                path: PathBuf::from(format!("/project/file{id}.ts")),
                size_bytes: 100,
            })
            .collect();
        let mut test_imports = vec![ResolvedImport {
            info: ImportInfo {
                source: "./barrel".to_string(),
                imported_name: ImportedName::Named("value".to_string()),
                local_name: "value".to_string(),
                is_type_only: false,
                from_style: false,
                span: oxc_span::Span::new(0, 10),
                source_span: oxc_span::Span::default(),
            },
            target: if commonjs_to_barrel {
                ResolveResult::CommonJsInternalModule(FileId(1))
            } else {
                ResolveResult::InternalModule(FileId(1))
            },
        }];
        if commonjs_to_target {
            test_imports.push(ResolvedImport {
                info: ImportInfo {
                    source: "./target".to_string(),
                    imported_name: ImportedName::Named("value".to_string()),
                    local_name: "requiredValue".to_string(),
                    is_type_only: false,
                    from_style: false,
                    span: oxc_span::Span::new(20, 30),
                    source_span: oxc_span::Span::default(),
                },
                target: ResolveResult::CommonJsInternalModule(FileId(2)),
            });
        }
        let modules = vec![
            ResolvedModule {
                file_id: FileId(0),
                path: files[0].path.clone(),
                resolved_imports: test_imports,
                ..ResolvedModule::default()
            },
            ResolvedModule {
                file_id: FileId(1),
                path: files[1].path.clone(),
                re_exports: vec![ResolvedReExport {
                    info: ReExportInfo {
                        source: "./target".to_string(),
                        imported_name: "value".to_string(),
                        exported_name: "value".to_string(),
                        is_type_only: false,
                        span: oxc_span::Span::new(0, 10),
                    },
                    target: ResolveResult::InternalModule(FileId(2)),
                }],
                ..ResolvedModule::default()
            },
            ResolvedModule {
                file_id: FileId(2),
                path: files[2].path.clone(),
                exports: vec![fallow_types::extract::ExportInfo {
                    name: ExportName::Named("value".to_string()),
                    local_name: Some("value".to_string()),
                    is_type_only: false,
                    visibility: VisibilityTag::None,
                    expected_unused_reason: None,
                    span: oxc_span::Span::new(0, 20),
                    members: Vec::new(),
                    is_side_effect_used: false,
                    super_class: None,
                }],
                ..ResolvedModule::default()
            },
        ];
        let test_entry_points = vec![EntryPoint {
            path: files[0].path.clone(),
            source: EntryPointSource::TestFile,
        }];
        ModuleGraph::build_with_reachability_roots_and_replacements(
            &modules,
            &[ResolvedReplacedModuleTarget {
                source_file: FileId(0),
                target_file: FileId(2),
            }],
            &test_entry_points,
            &[],
            &test_entry_points,
            &files,
        )
    }

    fn build_masked_pattern_graph(mechanisms: &[ModuleLoadMechanism]) -> ModuleGraph {
        let files: Vec<_> = (0..2)
            .map(|id| DiscoveredFile {
                id: FileId(id),
                path: PathBuf::from(format!("/project/file{id}.ts")),
                size_bytes: 100,
            })
            .collect();
        let modules = vec![
            ResolvedModule {
                file_id: FileId(0),
                path: files[0].path.clone(),
                resolved_dynamic_patterns: mechanisms
                    .iter()
                    .copied()
                    .map(|mechanism| {
                        (
                            fallow_types::extract::DynamicImportPattern {
                                prefix: "./modules/".to_string(),
                                suffix: None,
                                span: oxc_span::Span::new(0, 10),
                                mechanism,
                            },
                            vec![FileId(1)],
                        )
                    })
                    .collect(),
                ..ResolvedModule::default()
            },
            ResolvedModule {
                file_id: FileId(1),
                path: files[1].path.clone(),
                exports: vec![fallow_types::extract::ExportInfo {
                    name: ExportName::Named("value".to_string()),
                    local_name: Some("value".to_string()),
                    is_type_only: false,
                    visibility: VisibilityTag::None,
                    expected_unused_reason: None,
                    span: oxc_span::Span::new(0, 20),
                    members: Vec::new(),
                    is_side_effect_used: false,
                    super_class: None,
                }],
                ..ResolvedModule::default()
            },
        ];
        let test_entry_points = vec![EntryPoint {
            path: files[0].path.clone(),
            source: EntryPointSource::TestFile,
        }];
        ModuleGraph::build_with_reachability_roots_and_replacements(
            &modules,
            &[ResolvedReplacedModuleTarget {
                source_file: FileId(0),
                target_file: FileId(1),
            }],
            &test_entry_points,
            &[],
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
    fn esm_replacement_masks_target_only_from_test_reachability() {
        let graph = build_reachability_graph_with_replacements(
            3,
            &[(0, 1, false), (1, 2, false)],
            &[],
            &[0],
            &[ResolvedReplacedModuleTarget {
                source_file: FileId(0),
                target_file: FileId(1),
            }],
        );

        assert!(graph.modules[1].is_reachable());
        assert!(!graph.modules[1].is_test_reachable());
        assert!(!graph.modules[2].is_test_reachable());
        assert_eq!(graph.test_reachability_index.profile_count, 1);
        assert!(graph.test_reachability_index.profile_reaches(FileId(0), 0));
        assert!(!graph.test_reachability_index.profile_reaches(FileId(1), 0));
    }

    #[test]
    fn commonjs_path_preserves_a_replaced_target() {
        let graph = build_reachability_graph_with_replacements(
            2,
            &[(0, 1, true)],
            &[],
            &[0],
            &[ResolvedReplacedModuleTarget {
                source_file: FileId(0),
                target_file: FileId(1),
            }],
        );

        assert!(graph.modules[1].is_test_reachable());
    }

    #[test]
    fn mixed_commonjs_and_esm_symbols_preserve_a_replaced_target() {
        let graph = build_reachability_graph_with_replacements(
            2,
            &[(0, 1, false), (0, 1, true)],
            &[],
            &[0],
            &[ResolvedReplacedModuleTarget {
                source_file: FileId(0),
                target_file: FileId(1),
            }],
        );

        assert!(graph.modules[1].is_test_reachable());
    }

    #[test]
    fn replacement_masks_a_target_behind_an_esm_re_export() {
        let graph = build_masked_re_export_graph(false, false);
        let target_export = &graph.modules[2].exports[0];
        let reference = target_export
            .references
            .first()
            .expect("barrel consumer reference should propagate");

        assert!(graph.modules[1].is_test_reachable());
        assert!(!graph.modules[2].is_test_reachable());
        assert_eq!(reference.mechanism, ModuleLoadMechanism::EsModule);
        assert!(!graph.is_test_reference_covered(FileId(2), reference));
    }

    #[test]
    fn commonjs_loaded_barrel_does_not_bypass_its_esm_re_export() {
        let graph = build_masked_re_export_graph(true, false);
        let target_export = &graph.modules[2].exports[0];
        let reference = target_export
            .references
            .first()
            .expect("barrel consumer reference should propagate");

        assert!(graph.modules[1].is_test_reachable());
        assert!(!graph.modules[2].is_test_reachable());
        assert_eq!(reference.mechanism, ModuleLoadMechanism::EsModule);
        assert!(!graph.is_test_reference_covered(FileId(2), reference));
    }

    #[test]
    fn direct_commonjs_and_esm_re_export_retain_distinct_coverage() {
        let graph = build_masked_re_export_graph(false, true);
        let references = &graph.modules[2].exports[0].references;

        assert_eq!(references.len(), 2);
        let esm = references
            .iter()
            .find(|reference| reference.mechanism == ModuleLoadMechanism::EsModule)
            .expect("ESM re-export reference");
        let commonjs = references
            .iter()
            .find(|reference| reference.mechanism == ModuleLoadMechanism::CommonJsRequire)
            .expect("direct CommonJS reference");
        assert!(!graph.is_test_reference_covered(FileId(2), esm));
        assert!(graph.is_test_reference_covered(FileId(2), commonjs));
    }

    #[test]
    fn require_context_keeps_a_replaced_target_covered() {
        let graph = build_masked_pattern_graph(&[ModuleLoadMechanism::CommonJsRequire]);
        let reference = &graph.modules[1].exports[0].references[0];

        assert!(graph.modules[1].is_test_reachable());
        assert_eq!(reference.mechanism, ModuleLoadMechanism::CommonJsRequire);
        assert!(graph.is_test_reference_covered(FileId(1), reference));
    }

    #[test]
    fn overlapping_patterns_preserve_exact_esm_and_commonjs_coverage() {
        let graph = build_masked_pattern_graph(&[
            ModuleLoadMechanism::EsModule,
            ModuleLoadMechanism::CommonJsRequire,
        ]);
        let references = &graph.modules[1].exports[0].references;

        assert!(graph.modules[1].is_test_reachable());
        assert_eq!(references.len(), 2);
        let esm = references
            .iter()
            .find(|reference| reference.mechanism == ModuleLoadMechanism::EsModule)
            .expect("ESM pattern reference");
        let commonjs = references
            .iter()
            .find(|reference| reference.mechanism == ModuleLoadMechanism::CommonJsRequire)
            .expect("CommonJS pattern reference");
        assert!(!graph.is_test_reference_covered(FileId(1), esm));
        assert!(graph.is_test_reference_covered(FileId(1), commonjs));
    }

    #[test]
    fn replacement_stops_before_a_masked_cycle_member() {
        let graph = build_reachability_graph_with_replacements(
            3,
            &[(0, 1, false), (1, 2, false), (2, 1, false)],
            &[],
            &[0],
            &[ResolvedReplacedModuleTarget {
                source_file: FileId(0),
                target_file: FileId(2),
            }],
        );

        assert!(graph.modules[0].is_test_reachable());
        assert!(graph.modules[1].is_test_reachable());
        assert!(!graph.modules[2].is_test_reachable());
    }

    #[test]
    fn identical_masks_share_one_reachability_profile() {
        let graph = build_reachability_graph_with_replacements(
            3,
            &[(0, 2, false), (1, 2, false)],
            &[],
            &[0, 1],
            &[
                ResolvedReplacedModuleTarget {
                    source_file: FileId(1),
                    target_file: FileId(2),
                },
                ResolvedReplacedModuleTarget {
                    source_file: FileId(0),
                    target_file: FileId(2),
                },
            ],
        );

        assert_eq!(graph.test_reachability_index.profile_count, 1);
        assert!(graph.test_reachability_index.profile_masks(FileId(2), 0));
        assert!(graph.test_reachability_index.profile_reaches(FileId(0), 0));
        assert!(graph.test_reachability_index.profile_reaches(FileId(1), 0));
        assert!(!graph.modules[2].is_test_reachable());
    }

    #[test]
    fn an_unmasked_root_keeps_the_shared_target_test_reachable() {
        let graph = build_reachability_graph_with_replacements(
            3,
            &[(0, 2, false), (1, 2, false)],
            &[],
            &[0, 1],
            &[ResolvedReplacedModuleTarget {
                source_file: FileId(0),
                target_file: FileId(2),
            }],
        );

        assert_eq!(graph.test_reachability_index.profile_count, 2);
        assert!(graph.modules[2].is_test_reachable());
        assert!(!graph.test_reachability_index.profile_masks(FileId(2), 0));
        assert!(graph.test_reachability_index.profile_reaches(FileId(1), 0));
        assert!(graph.test_reachability_index.profile_reaches(FileId(2), 0));
        assert!(graph.test_reachability_index.profile_masks(FileId(2), 1));
        assert!(graph.test_reachability_index.profile_reaches(FileId(0), 1));
        assert!(!graph.test_reachability_index.shares_profile(
            FileId(2),
            FileId(0),
            ModuleLoadMechanism::EsModule,
        ));
        assert!(graph.test_reachability_index.shares_profile(
            FileId(2),
            FileId(1),
            ModuleLoadMechanism::EsModule,
        ));
    }

    #[test]
    fn replacement_declared_outside_a_test_root_keeps_the_fast_path() {
        let graph = build_reachability_graph_with_replacements(
            3,
            &[(0, 1, false)],
            &[],
            &[0],
            &[ResolvedReplacedModuleTarget {
                source_file: FileId(2),
                target_file: FileId(1),
            }],
        );

        assert_eq!(graph.test_reachability_index.profile_count, 0);
        assert!(graph.modules[1].is_test_reachable());
    }

    #[test]
    fn replacement_profiles_survive_graph_cache_serialization() {
        let graph = build_reachability_graph_with_replacements(
            2,
            &[(0, 1, false)],
            &[],
            &[0],
            &[ResolvedReplacedModuleTarget {
                source_file: FileId(0),
                target_file: FileId(1),
            }],
        );

        let encoded = postcard::to_allocvec(&graph).expect("encode graph");
        let decoded: ModuleGraph = postcard::from_bytes(&encoded).expect("decode graph");

        assert_eq!(decoded.test_reachability_index.profile_count, 1);
        assert!(decoded.test_reachability_index.profile_masks(FileId(1), 0));
        assert!(
            decoded
                .test_reachability_index
                .profile_reaches(FileId(0), 0)
        );
        assert!(
            !decoded
                .test_reachability_index
                .profile_reaches(FileId(1), 0)
        );
    }

    #[test]
    fn many_unmasked_roots_use_one_test_traversal() {
        let test_roots: Vec<u32> = (0..512).collect();
        let graph = build_reachability_graph(512, &[], &[], &test_roots);
        let root_set: FxHashSet<_> = test_roots.iter().copied().map(FileId).collect();

        let reachability = graph.collect_test_reachable(&root_set, &[], 512);

        assert_eq!(reachability.traversal_count(), 1);
    }

    #[test]
    fn many_roots_use_one_traversal_per_distinct_mask() {
        let test_roots: Vec<u32> = (0..128).collect();
        let edges: Vec<_> = test_roots
            .iter()
            .copied()
            .map(|root| (root, 128, false))
            .collect();
        let replacements: Vec<_> = (0..64)
            .map(|root| ResolvedReplacedModuleTarget {
                source_file: FileId(root),
                target_file: FileId(128),
            })
            .collect();
        let graph = build_reachability_graph_with_replacements(
            129,
            &edges,
            &[],
            &test_roots,
            &replacements,
        );
        let root_set: FxHashSet<_> = test_roots.iter().copied().map(FileId).collect();

        let reachability = graph.collect_test_reachable(&root_set, &replacements, 129);

        assert_eq!(reachability.traversal_count(), 2);
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
