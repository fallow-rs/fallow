//! Phase 3: BFS reachability from entry points.

use std::collections::VecDeque;

use fixedbitset::FixedBitSet;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::resolve::ResolvedReplacedModuleTarget;
use fallow_types::discover::FileId;
use fallow_types::extract::ModuleLoadMechanism;

use super::{ModuleGraph, TestReachabilityIndex};

/// Upper bound on distinct replacement-mask profiles before profiled
/// reachability falls back to the legacy coarse bitset pass.
///
/// Follows the `re_export_transition_safety_cap` precedent of bounding an
/// otherwise unbounded blowup. `TestReachabilityIndex` storage is
/// O(files * profile_count / 64) u64 words and worklist propagation touches
/// every profile word per edge visit, so at 1024 profiles each file costs 16
/// words (128 bytes): about 12 MiB of index for a 100k-file monorepo. Beyond
/// that the memory and propagation cost outweigh the masking precision, and
/// the legacy pass is a safe over-approximation (fail-open, pre-#2068
/// behavior).
const PROFILE_COUNT_SAFETY_CAP: usize = 1024;

#[derive(Debug, PartialEq, Eq)]
pub(super) struct TestReachabilityProfile {
    masked_targets: Vec<FileId>,
    roots: Vec<FileId>,
}

pub(super) enum TestReachabilityPlan<'roots> {
    NoRoots,
    Legacy {
        roots: &'roots FxHashSet<FileId>,
    },
    Profiled {
        profiles: Vec<TestReachabilityProfile>,
    },
}

impl<'roots> TestReachabilityPlan<'roots> {
    pub(super) fn new(
        test_entry_points: &'roots FxHashSet<FileId>,
        replaced_module_targets: &[ResolvedReplacedModuleTarget],
        total_capacity: usize,
    ) -> Self {
        if test_entry_points.is_empty() {
            return Self::NoRoots;
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
            return Self::Legacy {
                roots: test_entry_points,
            };
        }

        let mut roots_by_mask: FxHashMap<Vec<FileId>, Vec<FileId>> = FxHashMap::default();
        for &root in test_entry_points {
            let mask = targets_by_root.remove(&root).unwrap_or_default();
            roots_by_mask.entry(mask).or_default().push(root);
        }

        if roots_by_mask.len() > PROFILE_COUNT_SAFETY_CAP {
            tracing::warn!(
                profile_count = roots_by_mask.len(),
                safety_cap = PROFILE_COUNT_SAFETY_CAP,
                "Test-reachability profile count exceeded its safety cap; \
                 falling back to coarse test reachability without replacement \
                 masking. Mocked modules stay test-reachable (fail-open)."
            );
            return Self::Legacy {
                roots: test_entry_points,
            };
        }

        let mut profiles: Vec<_> = roots_by_mask
            .into_iter()
            .map(|(masked_targets, mut roots)| {
                roots.sort_unstable_by_key(|root| root.0);
                roots.dedup();
                TestReachabilityProfile {
                    masked_targets,
                    roots,
                }
            })
            .collect();
        profiles.sort_by(|left, right| {
            left.masked_targets
                .iter()
                .map(|file_id| file_id.0)
                .cmp(right.masked_targets.iter().map(|file_id| file_id.0))
        });

        Self::Profiled { profiles }
    }

    pub(super) const fn requires_reference_provenance(&self) -> bool {
        matches!(self, Self::Profiled { .. })
    }
}

enum TestReachability {
    NoRoots,
    Legacy(FixedBitSet),
    Profiled {
        reachable: FixedBitSet,
        index: TestReachabilityIndex,
        #[cfg(test)]
        dirty_word_pops: usize,
    },
}

impl TestReachability {
    fn into_parts(self) -> (Option<FixedBitSet>, TestReachabilityIndex) {
        match self {
            Self::NoRoots => (None, TestReachabilityIndex::default()),
            Self::Legacy(reachable) => (Some(reachable), TestReachabilityIndex::default()),
            Self::Profiled {
                reachable, index, ..
            } => (Some(reachable), index),
        }
    }

    #[cfg(test)]
    const fn traversal_count(&self) -> usize {
        match self {
            Self::NoRoots => 0,
            Self::Legacy(_) | Self::Profiled { .. } => 1,
        }
    }

    #[cfg(test)]
    const fn dirty_word_pop_count(&self) -> usize {
        match self {
            Self::Profiled {
                dirty_word_pops, ..
            } => *dirty_word_pops,
            Self::NoRoots | Self::Legacy(_) => 0,
        }
    }
}

/// Owns the transient state for one bit-parallel profiled reachability pass.
///
/// Only `index` and `all_reachable` survive the pass. Pending deltas, queue
/// membership, and the target-sparse mask map remain local to propagation.
struct ProfileWorklist {
    index: TestReachabilityIndex,
    masked_profiles: FxHashMap<FileId, Vec<u64>>,
    all_reachable: FixedBitSet,
    pending_profiles: Vec<u64>,
    queued: FixedBitSet,
    queue: VecDeque<(FileId, usize)>,
    dirty_word_pops: usize,
}

impl ProfileWorklist {
    fn new(profiles: &[TestReachabilityProfile], total_capacity: usize) -> Self {
        let profile_count = profiles.len();
        let index = TestReachabilityIndex::new(total_capacity, profile_count);
        let words_per_file = index.words_per_file;
        let mut masked_profiles: FxHashMap<FileId, Vec<u64>> = FxHashMap::default();

        for (profile, reachability_profile) in profiles.iter().enumerate() {
            let word_index = profile / u64::BITS as usize;
            let profile_bit = 1_u64 << (profile % u64::BITS as usize);
            for &target in &reachability_profile.masked_targets {
                masked_profiles
                    .entry(target)
                    .or_insert_with(|| vec![0; words_per_file])[word_index] |= profile_bit;
            }
        }

        let pending_profiles = vec![0_u64; index.reachable_profiles.len()];
        let mut worklist = Self {
            index,
            masked_profiles,
            all_reachable: FixedBitSet::with_capacity(total_capacity),
            pending_profiles,
            queued: FixedBitSet::with_capacity(total_capacity.saturating_mul(words_per_file)),
            queue: VecDeque::new(),
            dirty_word_pops: 0,
        };

        for (profile, reachability_profile) in profiles.iter().enumerate() {
            let word_index = profile / u64::BITS as usize;
            let profile_bit = 1_u64 << (profile % u64::BITS as usize);
            for &root in &reachability_profile.roots {
                let root_index = root.0 as usize;
                if root_index >= total_capacity {
                    continue;
                }
                let Some(slot) = root_index
                    .checked_mul(words_per_file)
                    .and_then(|start| start.checked_add(word_index))
                else {
                    continue;
                };
                let Some(reachable_word) = worklist.index.reachable_profiles.get_mut(slot) else {
                    continue;
                };
                if *reachable_word & profile_bit != 0 {
                    continue;
                }
                *reachable_word |= profile_bit;
                worklist.pending_profiles[slot] |= profile_bit;
                worklist.all_reachable.insert(root_index);
                if !worklist.queued.contains(slot) {
                    worklist.queued.insert(slot);
                    worklist.queue.push_back((root, word_index));
                }
            }
        }

        worklist
    }

    fn run(mut self, graph: &ModuleGraph) -> (FixedBitSet, TestReachabilityIndex, usize) {
        let words_per_file = self.index.words_per_file;

        while let Some((file_id, word_index)) = self.queue.pop_front() {
            self.dirty_word_pops += 1;
            let file_index = file_id.0 as usize;
            let Some(source_slot) = file_index
                .checked_mul(words_per_file)
                .and_then(|start| start.checked_add(word_index))
            else {
                continue;
            };
            self.queued.remove(source_slot);
            let Some(source_delta) = self.pending_profiles.get_mut(source_slot) else {
                continue;
            };
            let delta_word = *source_delta;
            *source_delta = 0;
            if delta_word == 0 {
                continue;
            }

            let Some(module) = graph.modules.get(file_index) else {
                continue;
            };
            for edge in &graph.edges[module.edge_range.clone()] {
                let target_idx = edge.target.0 as usize;
                if target_idx >= self.all_reachable.len() {
                    continue;
                }
                let Some(target_slot) = target_idx
                    .checked_mul(words_per_file)
                    .and_then(|start| start.checked_add(word_index))
                else {
                    continue;
                };
                let edge_is_esm_only = edge.symbols.iter().all(|symbol| {
                    !matches!(symbol.mechanism, ModuleLoadMechanism::CommonJsRequire)
                });
                let edge_mask = if edge_is_esm_only {
                    self.masked_profiles.get(&edge.target)
                } else {
                    None
                };

                let propagated = edge_mask
                    .and_then(|mask| mask.get(word_index))
                    .map_or(delta_word, |mask_word| delta_word & !mask_word);
                let Some(reachable_word) = self.index.reachable_profiles.get_mut(target_slot)
                else {
                    continue;
                };
                let new_profiles = propagated & !*reachable_word;
                if new_profiles == 0 {
                    continue;
                }
                *reachable_word |= new_profiles;
                self.pending_profiles[target_slot] |= new_profiles;

                self.all_reachable.insert(target_idx);
                if !self.queued.contains(target_slot) {
                    self.queued.insert(target_slot);
                    self.queue.push_back((edge.target, word_index));
                }
            }
        }

        self.index.set_sparse_masks(self.masked_profiles);
        (self.all_reachable, self.index, self.dirty_word_pops)
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

    fn collect_profiled_reachable(
        &self,
        profiles: &[TestReachabilityProfile],
        total_capacity: usize,
    ) -> (FixedBitSet, TestReachabilityIndex, usize) {
        ProfileWorklist::new(profiles, total_capacity).run(self)
    }

    fn collect_test_reachable(
        &self,
        plan: TestReachabilityPlan<'_>,
        total_capacity: usize,
    ) -> TestReachability {
        match plan {
            TestReachabilityPlan::NoRoots => TestReachability::NoRoots,
            TestReachabilityPlan::Legacy { roots } => {
                TestReachability::Legacy(self.collect_reachable(roots, total_capacity))
            }
            TestReachabilityPlan::Profiled { profiles } => {
                let (reachable, index, dirty_word_pops) =
                    self.collect_profiled_reachable(&profiles, total_capacity);
                #[cfg(not(test))]
                let _ = dirty_word_pops;

                TestReachability::Profiled {
                    reachable,
                    index,
                    #[cfg(test)]
                    dirty_word_pops,
                }
            }
        }
    }

    /// Mark modules reachable from overall, runtime, and test entry points via BFS.
    ///
    /// Skips redundant BFS passes when entry point sets are identical or empty.
    pub(super) fn mark_reachable(
        &mut self,
        entry_points: &FxHashSet<FileId>,
        runtime_entry_points: &FxHashSet<FileId>,
        test_reachability_plan: TestReachabilityPlan<'_>,
        total_capacity: usize,
    ) {
        let visited = self.collect_reachable(entry_points, total_capacity);

        let runtime_same = runtime_entry_points == entry_points;
        let runtime_visited = if runtime_same {
            None
        } else {
            Some(self.collect_reachable(runtime_entry_points, total_capacity))
        };

        let requires_reference_provenance = test_reachability_plan.requires_reference_provenance();
        let (test_visited, test_reachability_index) = self
            .collect_test_reachable(test_reachability_plan, total_capacity)
            .into_parts();
        debug_assert_eq!(
            requires_reference_provenance,
            test_reachability_index.profile_count > 0
        );
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

    use super::{TestReachabilityPlan, TestReachabilityProfile};
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

    fn intermediate_import(
        source: &str,
        target: FileId,
        imported_name: ImportedName,
        commonjs: bool,
    ) -> ResolvedImport {
        ResolvedImport {
            info: ImportInfo {
                source: source.to_string(),
                imported_name,
                local_name: "value".to_string(),
                is_type_only: false,
                from_style: false,
                span: oxc_span::Span::new(0, 10),
                source_span: oxc_span::Span::default(),
            },
            target: if commonjs {
                ResolveResult::CommonJsInternalModule(target)
            } else {
                ResolveResult::InternalModule(target)
            },
        }
    }

    fn intermediate_modules(
        files: &[DiscoveredFile],
        commonjs_to_barrel: bool,
        include_cycle: bool,
    ) -> Vec<ResolvedModule> {
        let mut target_re_exports = Vec::new();
        if include_cycle {
            target_re_exports.push(ResolvedReExport {
                info: ReExportInfo {
                    source: "./barrel".to_string(),
                    imported_name: "*".to_string(),
                    exported_name: "*".to_string(),
                    is_type_only: false,
                    span: oxc_span::Span::new(20, 30),
                },
                target: ResolveResult::InternalModule(FileId(1)),
            });
        }
        vec![
            ResolvedModule {
                file_id: FileId(0),
                path: files[0].path.clone(),
                resolved_imports: vec![
                    intermediate_import(
                        "./barrel",
                        FileId(1),
                        ImportedName::Named("value".to_string()),
                        commonjs_to_barrel,
                    ),
                    intermediate_import("./alternate", FileId(3), ImportedName::SideEffect, false),
                ],
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
                re_exports: target_re_exports,
                ..ResolvedModule::default()
            },
            ResolvedModule {
                file_id: FileId(3),
                path: files[3].path.clone(),
                resolved_imports: vec![intermediate_import(
                    "./target",
                    FileId(2),
                    ImportedName::SideEffect,
                    false,
                )],
                ..ResolvedModule::default()
            },
        ]
    }

    fn build_intermediate_replacement_graph(
        commonjs_to_barrel: bool,
        include_cycle: bool,
        mask_final_target: bool,
    ) -> ModuleGraph {
        let files: Vec<_> = (0..4)
            .map(|id| DiscoveredFile {
                id: FileId(id),
                path: PathBuf::from(format!("/project/file{id}.ts")),
                size_bytes: 100,
            })
            .collect();
        let modules = intermediate_modules(&files, commonjs_to_barrel, include_cycle);
        let test_entry_points = vec![EntryPoint {
            path: files[0].path.clone(),
            source: EntryPointSource::TestFile,
        }];
        let mut replacements = vec![ResolvedReplacedModuleTarget {
            source_file: FileId(0),
            target_file: FileId(1),
        }];
        if mask_final_target {
            replacements.push(ResolvedReplacedModuleTarget {
                source_file: FileId(0),
                target_file: FileId(2),
            });
        }

        ModuleGraph::build_with_reachability_roots_and_replacements(
            &modules,
            &replacements,
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
        assert!(
            !target_export.references.is_empty(),
            "barrel consumer reference should propagate"
        );

        assert!(graph.modules[1].is_test_reachable());
        assert!(!graph.modules[2].is_test_reachable());
        assert_eq!(
            graph.reference_path_hops(target_export, 0)[0].1,
            ModuleLoadMechanism::EsModule
        );
        assert!(!graph.is_test_reference_covered(target_export, 0));
    }

    #[test]
    fn commonjs_loaded_barrel_does_not_bypass_its_esm_re_export() {
        let graph = build_masked_re_export_graph(true, false);
        let target_export = &graph.modules[2].exports[0];
        assert!(
            !target_export.references.is_empty(),
            "barrel consumer reference should propagate"
        );

        assert!(graph.modules[1].is_test_reachable());
        assert!(!graph.modules[2].is_test_reachable());
        assert_eq!(
            graph.reference_path_hops(target_export, 0)[0].1,
            ModuleLoadMechanism::EsModule
        );
        assert!(!graph.is_test_reference_covered(target_export, 0));
    }

    #[test]
    fn direct_commonjs_and_esm_re_export_retain_distinct_coverage() {
        let graph = build_masked_re_export_graph(false, true);
        let export = &graph.modules[2].exports[0];

        assert_eq!(export.references.len(), 2);
        let esm = (0..export.references.len())
            .find(|&index| {
                graph.reference_path_hops(export, index)[0].1 == ModuleLoadMechanism::EsModule
            })
            .expect("ESM re-export reference");
        let commonjs = (0..export.references.len())
            .find(|&index| {
                graph.reference_path_hops(export, index)[0].1
                    == ModuleLoadMechanism::CommonJsRequire
            })
            .expect("direct CommonJS reference");
        assert!(!graph.is_test_reference_covered(export, esm));
        assert!(graph.is_test_reference_covered(export, commonjs));
    }

    #[test]
    fn mocked_intermediate_barrel_blocks_a_reference_despite_an_alternate_target_route() {
        let graph = build_intermediate_replacement_graph(false, false, false);
        let export = &graph.modules[2].exports[0];
        let hops = graph.reference_path_hops(export, 0);

        assert!(graph.modules[2].is_test_reachable());
        assert_eq!(
            hops,
            vec![
                (FileId(2), ModuleLoadMechanism::EsModule),
                (FileId(1), ModuleLoadMechanism::EsModule),
            ]
        );
        assert!(!graph.is_test_reference_covered(export, 0));
    }

    #[test]
    fn mocked_re_export_cycle_member_blocks_the_cyclic_reference_path() {
        let graph = build_intermediate_replacement_graph(false, true, false);
        let export = &graph.modules[2].exports[0];

        assert!(
            graph
                .re_export_cycles
                .iter()
                .any(|cycle| cycle.file_ids == vec![FileId(1), FileId(2)])
        );
        assert!(graph.modules[2].is_test_reachable());
        assert!(!graph.is_test_reference_covered(export, 0));
    }

    #[test]
    fn commonjs_barrel_hop_bypasses_only_its_own_replacement() {
        let graph = build_intermediate_replacement_graph(true, false, false);
        let export = &graph.modules[2].exports[0];

        assert_eq!(
            graph.reference_path_hops(export, 0),
            vec![
                (FileId(2), ModuleLoadMechanism::EsModule),
                (FileId(1), ModuleLoadMechanism::CommonJsRequire),
            ]
        );
        assert!(graph.is_test_reference_covered(export, 0));
    }

    #[test]
    fn commonjs_barrel_hop_does_not_bypass_a_replaced_esm_target() {
        let graph = build_intermediate_replacement_graph(true, false, true);
        let export = &graph.modules[2].exports[0];

        assert!(!graph.is_test_reference_covered(export, 0));
    }

    #[test]
    fn reference_paths_survive_graph_cache_serialization() {
        let graph = build_intermediate_replacement_graph(false, false, false);
        let encoded = postcard::to_allocvec(&graph).expect("encode graph");
        let decoded: ModuleGraph = postcard::from_bytes(&encoded).expect("decode graph");
        let export = &decoded.modules[2].exports[0];

        assert_eq!(
            decoded.reference_path_hops(export, 0),
            vec![
                (FileId(2), ModuleLoadMechanism::EsModule),
                (FileId(1), ModuleLoadMechanism::EsModule),
            ]
        );
        assert!(!decoded.is_test_reference_covered(export, 0));
    }

    #[test]
    fn require_context_keeps_a_replaced_target_covered() {
        let graph = build_masked_pattern_graph(&[ModuleLoadMechanism::CommonJsRequire]);
        let export = &graph.modules[1].exports[0];

        assert!(graph.modules[1].is_test_reachable());
        assert_eq!(
            graph.reference_path_hops(export, 0)[0].1,
            ModuleLoadMechanism::CommonJsRequire
        );
        assert!(graph.is_test_reference_covered(export, 0));
    }

    #[test]
    fn overlapping_patterns_preserve_exact_esm_and_commonjs_coverage() {
        let graph = build_masked_pattern_graph(&[
            ModuleLoadMechanism::EsModule,
            ModuleLoadMechanism::CommonJsRequire,
        ]);
        let export = &graph.modules[1].exports[0];

        assert!(graph.modules[1].is_test_reachable());
        assert_eq!(export.references.len(), 2);
        let esm = (0..export.references.len())
            .find(|&index| {
                graph.reference_path_hops(export, index)[0].1 == ModuleLoadMechanism::EsModule
            })
            .expect("ESM pattern reference");
        let commonjs = (0..export.references.len())
            .find(|&index| {
                graph.reference_path_hops(export, index)[0].1
                    == ModuleLoadMechanism::CommonJsRequire
            })
            .expect("CommonJS pattern reference");
        assert!(!graph.is_test_reference_covered(export, esm));
        assert!(graph.is_test_reference_covered(export, commonjs));
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
        assert!(!graph.test_reachability_index.covers_path(
            FileId(0),
            &std::iter::once((FileId(2), ModuleLoadMechanism::EsModule)),
        ));
        assert!(graph.test_reachability_index.covers_path(
            FileId(1),
            &std::iter::once((FileId(2), ModuleLoadMechanism::EsModule)),
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
    fn test_reachability_plan_uses_no_roots_without_test_entries() {
        let test_roots = FxHashSet::default();

        let plan = TestReachabilityPlan::new(&test_roots, &[], 3);

        assert!(matches!(&plan, TestReachabilityPlan::NoRoots));
        assert!(!plan.requires_reference_provenance());
    }

    #[test]
    fn test_reachability_plan_ignores_replacements_outside_test_roots() {
        let test_roots = FxHashSet::from_iter([FileId(0)]);
        let replacements = [ResolvedReplacedModuleTarget {
            source_file: FileId(1),
            target_file: FileId(2),
        }];

        let plan = TestReachabilityPlan::new(&test_roots, &replacements, 3);

        assert!(matches!(&plan, TestReachabilityPlan::Legacy { .. }));
        assert!(!plan.requires_reference_provenance());
    }

    #[test]
    fn test_reachability_plan_ignores_out_of_capacity_targets() {
        let test_roots = FxHashSet::from_iter([FileId(0)]);
        let replacements = [ResolvedReplacedModuleTarget {
            source_file: FileId(0),
            target_file: FileId(3),
        }];

        let plan = TestReachabilityPlan::new(&test_roots, &replacements, 3);

        assert!(matches!(&plan, TestReachabilityPlan::Legacy { .. }));
        assert!(!plan.requires_reference_provenance());
    }

    #[test]
    fn test_reachability_plan_groups_roots_by_normalized_replacement_mask() {
        let test_roots = FxHashSet::from_iter([FileId(0), FileId(1), FileId(2)]);
        let replacements = [
            ResolvedReplacedModuleTarget {
                source_file: FileId(0),
                target_file: FileId(4),
            },
            ResolvedReplacedModuleTarget {
                source_file: FileId(0),
                target_file: FileId(4),
            },
            ResolvedReplacedModuleTarget {
                source_file: FileId(1),
                target_file: FileId(4),
            },
        ];

        let plan = TestReachabilityPlan::new(&test_roots, &replacements, 5);

        assert!(plan.requires_reference_provenance());
        let TestReachabilityPlan::Profiled { profiles } = plan else {
            panic!("valid test-root replacements must use profiled reachability");
        };
        assert_eq!(
            profiles,
            vec![
                TestReachabilityProfile {
                    masked_targets: Vec::new(),
                    roots: vec![FileId(2)],
                },
                TestReachabilityProfile {
                    masked_targets: vec![FileId(4)],
                    roots: vec![FileId(0), FileId(1)],
                },
            ]
        );
    }

    type UniqueMaskInputs = (
        Vec<u32>,
        Vec<(u32, u32, bool)>,
        Vec<ResolvedReplacedModuleTarget>,
    );

    /// Build roots 0..count, each importing and mocking a unique target at
    /// count + root, so every root lands in its own mask profile.
    fn unique_mask_graph_inputs(count: u32) -> UniqueMaskInputs {
        let test_roots: Vec<u32> = (0..count).collect();
        let mut edges = Vec::with_capacity(test_roots.len());
        let mut replacements = Vec::with_capacity(test_roots.len());
        for &root in &test_roots {
            let masked_target = count + root;
            edges.push((root, masked_target, false));
            replacements.push(ResolvedReplacedModuleTarget {
                source_file: FileId(root),
                target_file: FileId(masked_target),
            });
        }
        (test_roots, edges, replacements)
    }

    #[test]
    #[cfg_attr(
        miri,
        ignore = "thousands of synthetic profiles are too slow under miri"
    )]
    fn profile_count_at_the_safety_cap_keeps_profiled_masking() {
        let count = u32::try_from(super::PROFILE_COUNT_SAFETY_CAP).expect("cap fits in u32");
        let total = count as usize * 2;
        let (test_roots, edges, replacements) = unique_mask_graph_inputs(count);
        let graph = build_reachability_graph_with_replacements(
            total,
            &edges,
            &[],
            &test_roots,
            &replacements,
        );

        assert_eq!(
            graph.test_reachability_index.profile_count,
            super::PROFILE_COUNT_SAFETY_CAP
        );
        // Profiles sort by mask, so profile 0 belongs to root 0 whose unique
        // target is FileId(count).
        assert!(
            graph
                .test_reachability_index
                .profile_masks(FileId(count), 0)
        );
        assert!(graph.modules[0].is_test_reachable());
        // Masking still applies under the cap: the mocked target is only
        // imported by the root that replaces it, so it stays unreachable.
        assert!(!graph.modules[count as usize].is_test_reachable());
    }

    #[test]
    #[cfg_attr(
        miri,
        ignore = "thousands of synthetic profiles are too slow under miri"
    )]
    fn profile_count_above_the_safety_cap_falls_back_to_coarse_reachability() {
        let count = u32::try_from(super::PROFILE_COUNT_SAFETY_CAP * 4).expect("cap fits in u32");
        let total = count as usize * 2;
        let (test_roots, edges, replacements) = unique_mask_graph_inputs(count);
        let graph = build_reachability_graph_with_replacements(
            total,
            &edges,
            &[],
            &test_roots,
            &replacements,
        );
        let root_set: FxHashSet<_> = test_roots.iter().copied().map(FileId).collect();

        let plan = TestReachabilityPlan::new(&root_set, &replacements, total);
        assert!(matches!(&plan, TestReachabilityPlan::Legacy { .. }));
        assert!(!plan.requires_reference_provenance());

        // Fail-open: no profiles survive, and mocked targets stay
        // test-reachable exactly as the pre-profile coarse pass reported them.
        assert_eq!(graph.test_reachability_index.profile_count, 0);
        for &root in &test_roots {
            assert!(graph.modules[root as usize].is_test_reachable());
            assert!(graph.modules[(count + root) as usize].is_test_reachable());
        }
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

        let plan = TestReachabilityPlan::new(&root_set, &[], 512);
        let reachability = graph.collect_test_reachable(plan, 512);

        assert_eq!(reachability.traversal_count(), 1);
    }

    #[test]
    fn many_roots_share_one_profile_worklist() {
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

        let plan = TestReachabilityPlan::new(&root_set, &replacements, 129);
        let reachability = graph.collect_test_reachable(plan, 129);

        assert_eq!(reachability.traversal_count(), 1);
        assert_eq!(reachability.dirty_word_pop_count(), 129);
    }

    #[test]
    fn unique_masks_cross_profile_words_in_one_deterministic_worklist() {
        let test_roots: Vec<u32> = (0..65).collect();
        let mut edges = Vec::with_capacity(test_roots.len() * 2);
        let mut replacements = Vec::with_capacity(test_roots.len());
        for &root in &test_roots {
            let masked_target = 65 + root;
            edges.push((root, masked_target, false));
            edges.push((root, 130, false));
            replacements.push(ResolvedReplacedModuleTarget {
                source_file: FileId(root),
                target_file: FileId(masked_target),
            });
        }
        let graph = build_reachability_graph_with_replacements(
            131,
            &edges,
            &[],
            &test_roots,
            &replacements,
        );
        let root_set: FxHashSet<_> = test_roots.iter().copied().map(FileId).collect();

        let plan = TestReachabilityPlan::new(&root_set, &replacements, 131);
        let reachability = graph.collect_test_reachable(plan, 131);

        assert_eq!(reachability.traversal_count(), 1);
        assert_eq!(reachability.dirty_word_pop_count(), 67);
        assert_eq!(graph.test_reachability_index.profile_count, 65);
        assert_eq!(graph.test_reachability_index.words_per_file, 2);
        assert_eq!(graph.test_reachability_index.masked_profiles.len(), 65);
        assert!(
            graph
                .test_reachability_index
                .profile_reaches(FileId(130), 0)
        );
        assert!(
            graph
                .test_reachability_index
                .profile_reaches(FileId(130), 64)
        );
        assert!(graph.test_reachability_index.profile_masks(FileId(65), 0));
        assert!(graph.test_reachability_index.profile_masks(FileId(129), 64));
        assert!(!graph.test_reachability_index.profile_reaches(FileId(65), 0));
        assert!(
            !graph
                .test_reachability_index
                .profile_reaches(FileId(129), 64)
        );

        let encoded = postcard::to_allocvec(&graph).expect("encode graph");
        let decoded: ModuleGraph = postcard::from_bytes(&encoded).expect("decode graph");
        assert_eq!(decoded.test_reachability_index.profile_count, 65);
        assert_eq!(decoded.test_reachability_index.words_per_file, 2);
        assert_eq!(decoded.test_reachability_index.masked_profiles.len(), 65);
        assert!(
            decoded
                .test_reachability_index
                .profile_reaches(FileId(130), 64)
        );
        assert!(
            decoded
                .test_reachability_index
                .profile_masks(FileId(129), 64)
        );
    }

    #[test]
    fn staggered_profile_words_propagate_without_clean_word_scans() {
        const PROFILE_COUNT: u32 = 65;
        const MASK_START: u32 = PROFILE_COUNT;
        const FANOUT_START: u32 = MASK_START + PROFILE_COUNT;
        const FANOUT_COUNT: u32 = 8;

        let test_roots: Vec<u32> = (0..PROFILE_COUNT).collect();
        let mut edges = Vec::new();
        for root in 1..PROFILE_COUNT {
            edges.push((root, root - 1, false));
        }
        for target in FANOUT_START..FANOUT_START + FANOUT_COUNT {
            edges.push((0, target, false));
        }
        let replacements: Vec<_> = test_roots
            .iter()
            .copied()
            .map(|root| ResolvedReplacedModuleTarget {
                source_file: FileId(root),
                target_file: FileId(MASK_START + root),
            })
            .collect();
        let file_count = (FANOUT_START + FANOUT_COUNT) as usize;
        let graph = build_reachability_graph_with_replacements(
            file_count,
            &edges,
            &[],
            &test_roots,
            &replacements,
        );
        let root_set: FxHashSet<_> = test_roots.iter().copied().map(FileId).collect();

        let plan = TestReachabilityPlan::new(&root_set, &replacements, file_count);
        let reachability = graph.collect_test_reachable(plan, file_count);

        assert_eq!(graph.test_reachability_index.words_per_file, 2);
        let expected_dirty_word_pops =
            (PROFILE_COUNT * (PROFILE_COUNT + 1) / 2 + FANOUT_COUNT * PROFILE_COUNT) as usize;
        assert_eq!(
            reachability.dirty_word_pop_count(),
            expected_dirty_word_pops
        );
        for target in FANOUT_START..FANOUT_START + FANOUT_COUNT {
            assert!(
                graph
                    .test_reachability_index
                    .profile_reaches(FileId(target), 0)
            );
            assert!(
                graph
                    .test_reachability_index
                    .profile_reaches(FileId(target), 64)
            );
        }
    }

    #[test]
    fn no_profile_coverage_omits_unused_reference_provenance() {
        const CHAIN_LENGTH: u32 = 128;

        let files: Vec<_> = (0..=CHAIN_LENGTH)
            .map(|id| DiscoveredFile {
                id: FileId(id),
                path: PathBuf::from(format!("/project/file{id}.ts")),
                size_bytes: 100,
            })
            .collect();
        let mut modules = Vec::with_capacity(files.len());
        modules.push(ResolvedModule {
            file_id: FileId(0),
            path: files[0].path.clone(),
            resolved_imports: vec![intermediate_import(
                "./file1",
                FileId(1),
                ImportedName::Named("value".to_string()),
                false,
            )],
            ..ResolvedModule::default()
        });
        for id in 1..CHAIN_LENGTH {
            modules.push(ResolvedModule {
                file_id: FileId(id),
                path: files[id as usize].path.clone(),
                re_exports: vec![ResolvedReExport {
                    info: ReExportInfo {
                        source: format!("./file{}", id + 1),
                        imported_name: "value".to_string(),
                        exported_name: "value".to_string(),
                        is_type_only: false,
                        span: oxc_span::Span::new(0, 10),
                    },
                    target: ResolveResult::InternalModule(FileId(id + 1)),
                }],
                ..ResolvedModule::default()
            });
        }
        modules.push(ResolvedModule {
            file_id: FileId(CHAIN_LENGTH),
            path: files[CHAIN_LENGTH as usize].path.clone(),
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
        });
        let test_entry_points = vec![EntryPoint {
            path: files[0].path.clone(),
            source: EntryPointSource::TestFile,
        }];
        let graph = ModuleGraph::build_with_reachability_roots_and_replacements(
            &modules,
            &[],
            &test_entry_points,
            &[],
            &test_entry_points,
            &files,
        );
        let export = &graph.modules[CHAIN_LENGTH as usize].exports[0];

        assert_eq!(graph.test_reachability_index.profile_count, 0);
        assert!(
            export.reference_paths.is_empty(),
            "the provenance side table must stay unallocated on the fast path"
        );
        assert!(graph.reference_paths.is_empty());
        assert!(graph.is_test_reference_covered(export, 0));
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
