//! Test reach of one export, read from the existing test reachability index.
//!
//! The classification runs on demand for the exports a caller asks about. It
//! adds no stored state: the mocked-only check replays the replacement masks
//! along the recorded reference paths.

use fallow_types::discover::FileId;
use fallow_types::extract::ModuleLoadMechanism;
use rustc_hash::FxHashSet;

use super::types::{
    ReferencePathId, ReferencePathNode, ReferenceRouteGraphId, ReferenceRouteNodeId,
};
use super::{ExportNamespace, ExportSymbol, ModuleGraph, TestReachabilityIndex};

/// How tests reach one export.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExportTestReach {
    /// A test file references the export, and a test root covers that
    /// reference path.
    Direct,
    /// A covered reference exists, but no test file names the export.
    Transitive,
    /// No reference is covered, and a mock in a test root replaces a module
    /// on at least one reference path.
    MockedOnly,
    /// A test root reaches the declaring file, but no reference is covered.
    None,
    /// No test root reaches the declaring file.
    NoneFile,
}

/// Why replacement masking is or is not active for test reachability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TestMaskingState {
    /// At least one test root replaces a module with a closed factory mock.
    Active,
    /// No test root declares a closed factory mock.
    NoMocks,
    /// Too many distinct mock sets: reachability ignores the masks, so a mocked
    /// module counts as reached.
    Capped,
}

/// The test reach of one export, with the files that support the class.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportTestReachResult {
    /// The reach class.
    pub reach: ExportTestReach,
    /// Test files that name the export. For `Direct`, the test files whose
    /// references are covered. For `MockedOnly`, the test files whose
    /// references a mask removed. Empty for the other classes. Sorted by id.
    pub tests: Vec<FileId>,
    /// For `MockedOnly`: the masked modules on the reference paths. The
    /// masked module can be a barrel in front of the declaring file. Sorted by
    /// id.
    pub mocked_targets: Vec<FileId>,
}

/// Profile bits of one reference that a mask removed, and the masked modules.
#[derive(Default)]
struct MaskedBits {
    mocked: bool,
    targets: FxHashSet<FileId>,
}

impl ModuleGraph {
    /// Report whether test reachability applies replacement masks.
    #[must_use]
    pub fn test_masking_state(&self) -> TestMaskingState {
        if self.test_reachability_index.profile_count > 0 {
            TestMaskingState::Active
        } else if self.test_reachability_index.masking_capped {
            TestMaskingState::Capped
        } else {
            TestMaskingState::NoMocks
        }
    }

    /// Classify how tests reach `export` of `file`.
    ///
    /// Only value references count: an `import type` gives no test reach.
    /// `is_test_file` decides which referencing files are tests. The checks
    /// run in this order: a covered reference from a test file, any covered
    /// reference, a reference that a mask removed, then file reach.
    #[must_use]
    pub fn export_test_reach(
        &self,
        file: FileId,
        export: &ExportSymbol,
        is_test_file: &dyn Fn(FileId) -> bool,
    ) -> ExportTestReachResult {
        let value_references = || {
            export
                .references
                .iter()
                .enumerate()
                .filter(|(_, reference)| reference.namespace == ExportNamespace::Value)
        };

        let mut covered = false;
        let mut direct_tests: Vec<FileId> = Vec::new();
        for (index, reference) in value_references() {
            if !self.is_test_reference_covered(export, index) {
                continue;
            }
            covered = true;
            if is_test_file(reference.from_file) {
                direct_tests.push(reference.from_file);
            }
        }
        if !direct_tests.is_empty() {
            return ExportTestReachResult {
                reach: ExportTestReach::Direct,
                tests: sorted_unique(direct_tests),
                mocked_targets: Vec::new(),
            };
        }
        if covered {
            return ExportTestReachResult {
                reach: ExportTestReach::Transitive,
                tests: Vec::new(),
                mocked_targets: Vec::new(),
            };
        }

        if self.test_masking_state() == TestMaskingState::Active {
            let mut mocked_tests = Vec::new();
            let mut mocked_targets = FxHashSet::default();
            for (index, reference) in value_references() {
                let Some(path) = export.reference_path(index) else {
                    continue;
                };
                let masked = self.test_reachability_index.masked_reference_bits(
                    reference.from_file,
                    path,
                    self,
                );
                if !masked.mocked {
                    continue;
                }
                mocked_targets.extend(masked.targets);
                if is_test_file(reference.from_file) {
                    mocked_tests.push(reference.from_file);
                }
            }
            if !mocked_targets.is_empty() {
                return ExportTestReachResult {
                    reach: ExportTestReach::MockedOnly,
                    tests: sorted_unique(mocked_tests),
                    mocked_targets: sorted_unique(mocked_targets.into_iter().collect()),
                };
            }
        }

        let reach = if self.is_test_reachable(file) {
            ExportTestReach::None
        } else {
            ExportTestReach::NoneFile
        };
        ExportTestReachResult {
            reach,
            tests: Vec::new(),
            mocked_targets: Vec::new(),
        }
    }
}

fn sorted_unique(mut files: Vec<FileId>) -> Vec<FileId> {
    files.sort_unstable_by_key(|file| file.0);
    files.dedup();
    files
}

impl TestReachabilityIndex {
    /// Replay one reference path from the referencing file to the export and
    /// collect the profiles that a mask removes on the way.
    ///
    /// The mask step runs before the reachable step at each hop, because a
    /// profile never sets the reachable bit of a target that it masks. A
    /// CommonJS hop never masks: replacement mocks do not intercept
    /// `require()`.
    fn masked_reference_bits(
        &self,
        source: FileId,
        path: ReferencePathId,
        graph: &ModuleGraph,
    ) -> MaskedBits {
        let mut result = MaskedBits::default();
        let Some(source_profiles) = self.profiles_for(&self.reachable_profiles, source) else {
            return result;
        };
        let mut chain: Vec<ReferencePathNode> = Vec::new();
        let mut next = Some(path);
        while let Some(path_id) = next {
            let Some(&node) = graph.reference_paths.get(path_id.index()) else {
                return result;
            };
            next = node.parent();
            chain.push(node);
        }
        chain.reverse();

        for (word_index, &source_word) in source_profiles.iter().enumerate() {
            let mut alive = source_word;
            for node in &chain {
                if alive == 0 {
                    break;
                }
                alive = match *node {
                    ReferencePathNode::Hop {
                        target, mechanism, ..
                    } => self.masked_hop(target, mechanism, word_index, alive, &mut result),
                    ReferencePathNode::Route {
                        graph: route_graph,
                        start,
                        terminal,
                        start_mechanism,
                        ..
                    } => self.masked_route(
                        graph,
                        RouteQuery {
                            graph_id: route_graph,
                            start,
                            terminal,
                            start_mechanism,
                            word_index,
                        },
                        alive,
                        &mut result,
                    ),
                };
            }
        }
        result
    }

    /// Move the masked profiles of one ESM hop into `result`, then keep the
    /// profiles that reach the target.
    fn masked_hop(
        &self,
        target: FileId,
        mechanism: ModuleLoadMechanism,
        word_index: usize,
        alive: u64,
        result: &mut MaskedBits,
    ) -> u64 {
        let mut alive = alive;
        if matches!(mechanism, ModuleLoadMechanism::EsModule)
            && let Some(masked_word) = self
                .masked_profiles_for(target)
                .and_then(|profiles| profiles.get(word_index))
        {
            let masked = alive & masked_word;
            if masked != 0 {
                result.mocked = true;
                result.targets.insert(target);
                alive &= !masked_word;
            }
        }
        let reachable = self
            .profiles_for(&self.reachable_profiles, target)
            .and_then(|profiles| profiles.get(word_index))
            .copied()
            .unwrap_or(0);
        alive & reachable
    }

    /// Mirror `active_route_profiles` over one namespace transition graph and
    /// collect the masked profiles of every visited ESM node.
    fn masked_route(
        &self,
        graph: &ModuleGraph,
        query: RouteQuery,
        candidate_profiles: u64,
        result: &mut MaskedBits,
    ) -> u64 {
        let routes = &graph.reference_routes;
        let Some(route_graph) = routes.graphs.get(query.graph_id.0 as usize) else {
            return 0;
        };
        let node_count = route_graph
            .nodes
            .end
            .saturating_sub(route_graph.nodes.start) as usize;
        let start_index = query.start.0 as usize;
        let terminal_index = query.terminal.0 as usize;
        if start_index >= node_count || terminal_index >= node_count {
            return 0;
        }

        let mut attempted = vec![0_u64; node_count];
        let mut pending = vec![0_u64; node_count];
        let mut queued = vec![false; node_count];
        let mut queue = std::collections::VecDeque::from([start_index]);
        pending[start_index] = candidate_profiles;
        queued[start_index] = true;
        let mut successful_profiles = 0_u64;

        while let Some(local_index) = queue.pop_front() {
            queued[local_index] = false;
            let incoming = pending[local_index] & !attempted[local_index];
            pending[local_index] = 0;
            attempted[local_index] |= incoming;
            if incoming == 0 {
                continue;
            }
            let Some(node) = routes
                .nodes
                .get(route_graph.nodes.start as usize + local_index)
            else {
                return 0;
            };
            let mechanism = if local_index == start_index {
                query.start_mechanism
            } else {
                Some(node.mechanism)
            };
            // A start node without a mechanism is the module that the
            // reference source already owns, so no load (and no mask) applies.
            let active = match mechanism {
                Some(mechanism) => {
                    self.masked_hop(node.target, mechanism, query.word_index, incoming, result)
                }
                None => incoming,
            };
            if active == 0 {
                continue;
            }
            if local_index == terminal_index {
                successful_profiles |= active;
                continue;
            }
            let Some(successors) = routes
                .edges
                .get(node.successors.start as usize..node.successors.end as usize)
            else {
                return 0;
            };
            for successor in successors {
                let successor_index = successor.0 as usize;
                if successor_index >= node_count {
                    return 0;
                }
                let new_profiles = active & !attempted[successor_index] & !pending[successor_index];
                if new_profiles == 0 {
                    continue;
                }
                pending[successor_index] |= new_profiles;
                if !queued[successor_index] {
                    queued[successor_index] = true;
                    queue.push_back(successor_index);
                }
            }
        }
        successful_profiles
    }
}

/// One namespace route evaluation for one profile word.
#[derive(Clone, Copy)]
struct RouteQuery {
    graph_id: ReferenceRouteGraphId,
    start: ReferenceRouteNodeId,
    terminal: ReferenceRouteNodeId,
    start_mechanism: Option<ModuleLoadMechanism>,
    word_index: usize,
}
