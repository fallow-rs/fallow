//! Load closures of one entry module, split by when each module loads, and
//! the single imports that dominate the eager closure.
//!
//! The eager closure follows only edges with a static, value-carrying symbol:
//! the code that loads before the entry runs. The deferred closure adds
//! `import()` and pattern edges, and the out-of-thread closure adds worker,
//! fork and loader-hook edges. Type-only symbols never load a module.
//!
//! A dominating import is an edge `importer -> target` that is the only way
//! into `target` from outside the part of the eager closure that `target`
//! dominates. If that edge became an `import()`, the whole dominator subtree
//! of `target` would leave the eager closure. Every result is a pure function
//! of the edge set, and every list is ordered by `FileId`, so repeated runs
//! give identical output.

use fallow_types::discover::FileId;
use fixedbitset::FixedBitSet;
use rustc_hash::FxHashMap;

use super::{ImportedSymbol, ModuleGraph};
use fallow_types::extract::ImportLoadKind;

/// The modules that one entry reaches, split by when they load.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EntryLoadClosure {
    /// Modules that load before the entry runs, the entry included, in
    /// ascending `FileId` order.
    pub eager: Vec<FileId>,
    /// Modules that load on demand on the same thread (`import()` or a lazy
    /// pattern) and are not eager, in ascending `FileId` order.
    pub deferred: Vec<FileId>,
    /// Modules that only an out-of-thread load reaches (a worker, a fork, a
    /// loader hook), in ascending `FileId` order.
    pub out_of_thread: Vec<FileId>,
}

/// One import edge that alone keeps a subtree of the eager closure eager.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DominatingImport {
    /// The module that contains the import.
    pub importer: FileId,
    /// The imported module; the root of the subtree that the edge keeps eager.
    pub target: FileId,
    /// Byte offset of the first static, value-carrying binding on the edge.
    /// `None` for an edge without a binding span, such as a re-export or an
    /// eager glob match.
    pub import_span_start: Option<u32>,
    /// Modules that leave the eager closure if the edge becomes an `import()`.
    pub exclusive_modules: usize,
    /// Summed weight of those modules.
    pub exclusive_weight: u64,
}

const NO_NODE: u32 = u32::MAX;

impl ModuleGraph {
    /// Split the modules that `entry` reaches by when they load.
    ///
    /// Returns an empty closure for an out-of-range `entry`.
    #[must_use]
    pub fn entry_load_closure(&self, entry: FileId) -> EntryLoadClosure {
        if entry.0 as usize >= self.modules.len() {
            return EntryLoadClosure::default();
        }
        let eager = self.symbol_closure(&[entry], ImportedSymbol::is_eager_value);
        let eager_ids = set_members(&eager);
        let same_thread = self.symbol_closure(&eager_ids, |symbol| {
            !symbol.is_type_only && symbol.load_kind() != ImportLoadKind::OutOfThread
        });
        let same_thread_ids = set_members(&same_thread);
        let everything = self.symbol_closure(&same_thread_ids, |symbol| !symbol.is_type_only);

        let mut deferred = same_thread.clone();
        deferred.difference_with(&eager);
        let mut out_of_thread = everything;
        out_of_thread.difference_with(&same_thread);

        EntryLoadClosure {
            eager: eager_ids,
            deferred: set_members(&deferred),
            out_of_thread: set_members(&out_of_thread),
        }
    }

    /// The import edges that each keep a subtree of the eager closure eager,
    /// ordered by exclusive weight (heaviest first), then exclusive module
    /// count, then importer and target `FileId`.
    ///
    /// `eager` must be the eager closure of `entry` from
    /// [`Self::entry_load_closure`]. `weight` gives the weight of one module,
    /// for example its source size in bytes.
    #[must_use]
    pub fn eager_dominating_imports(
        &self,
        entry: FileId,
        eager: &[FileId],
        weight: impl Fn(FileId) -> u64,
    ) -> Vec<DominatingImport> {
        let subgraph = EagerSubgraph::new(self, entry, eager);
        let Some(subgraph) = subgraph else {
            return Vec::new();
        };
        let tree = DominatorTree::new(&subgraph);

        let mut subtree_modules = vec![1_usize; subgraph.nodes.len()];
        let mut subtree_weight: Vec<u64> = subgraph.nodes.iter().map(|&id| weight(id)).collect();
        for &node in tree.reverse_postorder.iter().rev() {
            let parent = tree.idom[node as usize];
            if parent == node || parent == NO_NODE {
                continue;
            }
            subtree_modules[parent as usize] += subtree_modules[node as usize];
            subtree_weight[parent as usize] =
                subtree_weight[parent as usize].saturating_add(subtree_weight[node as usize]);
        }

        let mut imports = Vec::new();
        for (node, predecessors) in subgraph.predecessors.iter().enumerate() {
            if node as u32 == subgraph.root {
                continue;
            }
            let mut external = predecessors
                .iter()
                .filter(|(pred, _)| !tree.dominates(node as u32, *pred));
            let (Some(&(importer, span)), None) = (external.next(), external.next()) else {
                continue;
            };
            imports.push(DominatingImport {
                importer: subgraph.nodes[importer as usize],
                target: subgraph.nodes[node],
                import_span_start: span,
                exclusive_modules: subtree_modules[node],
                exclusive_weight: subtree_weight[node],
            });
        }
        imports.sort_by(|a, b| {
            b.exclusive_weight
                .cmp(&a.exclusive_weight)
                .then_with(|| b.exclusive_modules.cmp(&a.exclusive_modules))
                .then_with(|| a.importer.0.cmp(&b.importer.0))
                .then_with(|| a.target.0.cmp(&b.target.0))
        });
        imports
    }

    /// Modules reachable from `seeds` over edges with at least one symbol
    /// that `follows` accepts. The seeds are part of the result.
    fn symbol_closure(
        &self,
        seeds: &[FileId],
        follows: impl Fn(&ImportedSymbol) -> bool,
    ) -> FixedBitSet {
        let capacity = self.modules.len();
        let mut visited = FixedBitSet::with_capacity(capacity);
        let mut stack: Vec<FileId> = Vec::new();
        for &seed in seeds {
            let idx = seed.0 as usize;
            if idx < capacity && !visited.contains(idx) {
                visited.insert(idx);
                stack.push(seed);
            }
        }
        while let Some(current) = stack.pop() {
            let range = self.modules[current.0 as usize].edge_range.clone();
            for edge in &self.edges[range] {
                let idx = edge.target.0 as usize;
                if idx < capacity && !visited.contains(idx) && edge.symbols.iter().any(&follows) {
                    visited.insert(idx);
                    stack.push(edge.target);
                }
            }
        }
        visited
    }
}

fn set_members(set: &FixedBitSet) -> Vec<FileId> {
    set.ones()
        .map(|idx| FileId(u32::try_from(idx).unwrap_or(u32::MAX)))
        .collect()
}

/// The eager closure as a compact graph with local node indices.
struct EagerSubgraph {
    /// Local index to `FileId`, in ascending `FileId` order.
    nodes: Vec<FileId>,
    /// Local index of the entry.
    root: u32,
    /// Eager successors of each node, in ascending order.
    successors: Vec<Vec<u32>>,
    /// Eager predecessors of each node with the binding span of the edge.
    predecessors: Vec<Vec<(u32, Option<u32>)>>,
}

impl EagerSubgraph {
    fn new(graph: &ModuleGraph, entry: FileId, eager: &[FileId]) -> Option<Self> {
        let local: FxHashMap<FileId, u32> = eager
            .iter()
            .enumerate()
            .map(|(idx, &id)| (id, u32::try_from(idx).unwrap_or(NO_NODE)))
            .collect();
        let root = *local.get(&entry)?;
        let mut successors = vec![Vec::new(); eager.len()];
        let mut predecessors = vec![Vec::new(); eager.len()];
        for (source_idx, &source) in eager.iter().enumerate() {
            let Some(module) = graph.modules.get(source.0 as usize) else {
                continue;
            };
            for edge in &graph.edges[module.edge_range.clone()] {
                let Some(&target_idx) = local.get(&edge.target) else {
                    continue;
                };
                let Some(symbol) = edge.symbols.iter().find(|s| s.is_eager_value()) else {
                    continue;
                };
                if target_idx as usize == source_idx {
                    continue;
                }
                let span = (symbol.import_span.end > symbol.import_span.start)
                    .then_some(symbol.import_span.start);
                let source_local = u32::try_from(source_idx).unwrap_or(NO_NODE);
                successors[source_idx].push(target_idx);
                predecessors[target_idx as usize].push((source_local, span));
            }
        }
        for list in &mut successors {
            list.sort_unstable();
            list.dedup();
        }
        for list in &mut predecessors {
            list.sort_unstable_by_key(|(pred, _)| *pred);
            list.dedup_by_key(|(pred, _)| *pred);
        }
        Some(Self {
            nodes: eager.to_vec(),
            root,
            successors,
            predecessors,
        })
    }
}

/// Immediate dominators by the iterative Cooper, Harvey and Kennedy method,
/// plus tree intervals for a constant-time dominance test.
struct DominatorTree {
    idom: Vec<u32>,
    reverse_postorder: Vec<u32>,
    entry_time: Vec<u32>,
    exit_time: Vec<u32>,
}

impl DominatorTree {
    fn new(graph: &EagerSubgraph) -> Self {
        let count = graph.nodes.len();
        let reverse_postorder = reverse_postorder(graph);
        let mut order = vec![NO_NODE; count];
        for (position, &node) in reverse_postorder.iter().enumerate() {
            order[node as usize] = u32::try_from(position).unwrap_or(NO_NODE);
        }

        let mut idom = vec![NO_NODE; count];
        idom[graph.root as usize] = graph.root;
        let mut changed = true;
        while changed {
            changed = false;
            for &node in reverse_postorder.iter().skip(1) {
                let mut new_idom = NO_NODE;
                for &(pred, _) in &graph.predecessors[node as usize] {
                    if idom[pred as usize] == NO_NODE {
                        continue;
                    }
                    new_idom = if new_idom == NO_NODE {
                        pred
                    } else {
                        intersect(&idom, &order, pred, new_idom)
                    };
                }
                if new_idom != NO_NODE && idom[node as usize] != new_idom {
                    idom[node as usize] = new_idom;
                    changed = true;
                }
            }
        }

        let (entry_time, exit_time) = tree_intervals(graph.root, &idom, &reverse_postorder);
        Self {
            idom,
            reverse_postorder,
            entry_time,
            exit_time,
        }
    }

    /// Whether `dominator` dominates `node` (every node dominates itself).
    fn dominates(&self, dominator: u32, node: u32) -> bool {
        let (d, n) = (dominator as usize, node as usize);
        self.entry_time[d] <= self.entry_time[n] && self.exit_time[n] <= self.exit_time[d]
    }
}

fn intersect(idom: &[u32], order: &[u32], mut left: u32, mut right: u32) -> u32 {
    while left != right {
        while order[left as usize] > order[right as usize] {
            left = idom[left as usize];
        }
        while order[right as usize] > order[left as usize] {
            right = idom[right as usize];
        }
    }
    left
}

/// Reverse postorder of the nodes reachable from the root, by an iterative
/// depth-first search that visits successors in ascending order.
fn reverse_postorder(graph: &EagerSubgraph) -> Vec<u32> {
    let mut visited = FixedBitSet::with_capacity(graph.nodes.len());
    let mut postorder = Vec::with_capacity(graph.nodes.len());
    let mut stack: Vec<(u32, usize)> = vec![(graph.root, 0)];
    visited.insert(graph.root as usize);
    while let Some((node, next_child)) = stack.last_mut() {
        let children = &graph.successors[*node as usize];
        if let Some(&child) = children.get(*next_child) {
            *next_child += 1;
            if !visited.contains(child as usize) {
                visited.insert(child as usize);
                stack.push((child, 0));
            }
        } else {
            postorder.push(*node);
            stack.pop();
        }
    }
    postorder.reverse();
    postorder
}

/// Pre- and post-order times of the dominator tree, for the interval test.
fn tree_intervals(root: u32, idom: &[u32], reverse_postorder: &[u32]) -> (Vec<u32>, Vec<u32>) {
    let count = idom.len();
    let mut children = vec![Vec::new(); count];
    for &node in reverse_postorder {
        let parent = idom[node as usize];
        if node != root && parent != NO_NODE {
            children[parent as usize].push(node);
        }
    }
    let mut entry_time = vec![u32::MAX; count];
    let mut exit_time = vec![0_u32; count];
    let mut clock = 0_u32;
    let mut stack: Vec<(u32, usize)> = vec![(root, 0)];
    entry_time[root as usize] = clock;
    while let Some((node, next_child)) = stack.last_mut() {
        if let Some(&child) = children[*node as usize].get(*next_child) {
            *next_child += 1;
            clock += 1;
            entry_time[child as usize] = clock;
            stack.push((child, 0));
        } else {
            clock += 1;
            exit_time[*node as usize] = clock;
            stack.pop();
        }
    }
    (entry_time, exit_time)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use fallow_types::discover::{DiscoveredFile, EntryPoint, EntryPointSource, FileId};
    use fallow_types::extract::{ImportInfo, ImportedName};

    use crate::graph::ModuleGraph;
    use crate::resolve::{ResolveResult, ResolvedImport, ResolvedModule};

    fn import(to: u32, start: u32) -> ResolvedImport {
        ResolvedImport {
            info: ImportInfo {
                source: format!("./m{to}"),
                imported_name: ImportedName::SideEffect,
                local_name: String::new(),
                is_type_only: false,
                is_type_only_star: false,
                from_style: false,
                span: oxc_span::Span::new(start, start + 10),
                source_span: oxc_span::Span::default(),
            },
            target: ResolveResult::InternalModule(FileId(to)),
        }
    }

    /// Static edges only; `edges[i]` lists the targets of module `i`.
    fn graph(edges: &[&[u32]]) -> ModuleGraph {
        let path = |i: usize| PathBuf::from(format!("/p/m{i}.ts"));
        let files: Vec<DiscoveredFile> = (0..edges.len())
            .map(|i| DiscoveredFile {
                id: FileId(u32::try_from(i).unwrap_or(u32::MAX)),
                path: path(i),
                size_bytes: 10,
            })
            .collect();
        let modules: Vec<ResolvedModule> = edges
            .iter()
            .enumerate()
            .map(|(i, targets)| ResolvedModule {
                file_id: FileId(u32::try_from(i).unwrap_or(u32::MAX)),
                path: path(i),
                resolved_imports: targets
                    .iter()
                    .zip(0_u32..)
                    .map(|(&to, n)| import(to, n * 20))
                    .collect(),
                ..Default::default()
            })
            .collect();
        let entry = vec![EntryPoint {
            path: path(0),
            source: EntryPointSource::PackageJsonMain,
        }];
        ModuleGraph::build(&modules, &entry, &files)
    }

    #[test]
    fn a_cycle_back_into_a_subtree_does_not_hide_its_dominating_import() {
        // m0 -> m1 -> m2 -> m1: the back edge m2 -> m1 comes from inside the
        // subtree that m1 dominates, so m0 -> m1 still removes m1 and m2.
        let graph = graph(&[&[1], &[2], &[1]]);
        let closure = graph.entry_load_closure(FileId(0));
        let imports = graph.eager_dominating_imports(FileId(0), &closure.eager, |_| 10);
        let first = imports.first().expect("m0 -> m1 dominates the cycle");
        assert_eq!((first.importer, first.target), (FileId(0), FileId(1)));
        assert_eq!(first.exclusive_modules, 2);
        assert_eq!(first.exclusive_weight, 20);
    }

    #[test]
    fn a_diamond_has_no_single_dominating_import_for_the_shared_module() {
        // m0 -> m1 -> m3 and m0 -> m2 -> m3: two eager importers keep m3.
        let graph = graph(&[&[1, 2], &[3], &[3], &[]]);
        let closure = graph.entry_load_closure(FileId(0));
        let imports = graph.eager_dominating_imports(FileId(0), &closure.eager, |_| 10);
        assert!(imports.iter().all(|import| import.target != FileId(3)));
        assert_eq!(
            imports.len(),
            2,
            "m0 -> m1 and m0 -> m2 each remove one module"
        );
    }

    #[test]
    fn an_out_of_range_entry_has_an_empty_closure() {
        let graph = graph(&[&[]]);
        assert_eq!(graph.entry_load_closure(FileId(9)).eager, Vec::new());
    }
}
