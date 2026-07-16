//! Phase 4: Re-export chain resolution, propagate references through barrel files.

mod propagate;
#[cfg(test)]
mod tests;

use std::collections::VecDeque;
use std::path::PathBuf;

use rustc_hash::{FxHashMap, FxHashSet};

#[cfg(test)]
use std::cell::{Cell, RefCell};

use fallow_types::discover::FileId;

use crate::resolve::ResolvedModule;

use super::{Edge, ModuleGraph};

use propagate::{
    NamedImportOriginIndex, NamedReExportPropagation, StarReExportPropagation,
    propagate_named_re_export, propagate_star_re_export,
};

#[cfg(test)]
thread_local! {
    static PROPAGATION_VISITS: RefCell<Option<Vec<(FileId, FileId)>>> =
        const { RefCell::new(None) };
    static DIFFERENTIAL_CHECK_ENABLED: Cell<bool> = const { Cell::new(false) };
}

#[cfg(test)]
fn record_propagation_visit(entry: &ReExportTuple) {
    PROPAGATION_VISITS.with(|visits| {
        if let Some(visits) = visits.borrow_mut().as_mut() {
            visits.push((entry.barrel, entry.source));
        }
    });
}

#[cfg(test)]
fn capture_propagation_visits<T>(run: impl FnOnce() -> T) -> (T, Vec<(FileId, FileId)>) {
    PROPAGATION_VISITS.with(|visits| *visits.borrow_mut() = Some(Vec::new()));
    let result = run();
    let visits = PROPAGATION_VISITS.with(|visits| visits.borrow_mut().take().unwrap_or_default());
    (result, visits)
}

#[cfg(test)]
fn with_re_export_differential_check<T>(run: impl FnOnce() -> T) -> T {
    DIFFERENTIAL_CHECK_ENABLED.with(|enabled| {
        let previous = enabled.replace(true);
        let result = run();
        enabled.set(previous);
        result
    })
}

/// A re-export cycle or self-loop detected during Phase 4 chain resolution.
///
/// The graph-layer mirror of `fallow_types::results::ReExportCycle`. Kept in
/// the graph crate so the types crate does not need a dependency arrow back
/// into graph for the conversion. The analysis backend performs the
/// `GraphReExportCycle` to `ReExportCycle` mapping by reading `is_self_loop`
/// and routing to the matching `ReExportCycleKind` variant.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GraphReExportCycle {
    /// Member files participating in the cycle, sorted lexicographically by
    /// the `Path::display()` form (matches the existing diagnostic-output
    /// sort). For a self-loop, exactly one entry.
    pub files: Vec<PathBuf>,
    /// Parallel array to `files`: the FileId for each member. Kept alongside
    /// the paths so the core-layer detector can call
    /// `suppressions.is_file_suppressed(id, IssueKind::ReExportCycle)`
    /// without an extra path-to-FileId lookup.
    pub file_ids: Vec<FileId>,
    /// `true` for single-file self-re-exports (`export * from './'`), `false`
    /// for multi-node strongly connected components.
    pub is_self_loop: bool,
}

/// A single re-export edge collected from the module graph.
///
/// Replaces an earlier ad-hoc 5-tuple so the propagation loop is more
/// readable and the new `is_type_only` field carried into
/// [`propagate_star_re_export`] does not get lost in tuple-index plumbing.
struct ReExportTuple {
    barrel: FileId,
    source: FileId,
    imported_name: String,
    exported_name: String,
    /// `true` when the triggering re-export edge is `export type * from ...`
    /// or `export type { foo } from ...`. Threaded into star propagation so
    /// any synthetic stub created on the source module reflects the chain's
    /// type-only-ness instead of defaulting to `false`.
    is_type_only: bool,
}

struct ReExportContext<'a> {
    entry_star_targets: &'a FxHashSet<FileId>,
    edges_by_target: &'a FxHashMap<FileId, Vec<usize>>,
    named_import_origin_index: &'a NamedImportOriginIndex,
    module_by_id: &'a FxHashMap<FileId, &'a ResolvedModule>,
    existing_refs: &'a mut FxHashSet<FileId>,
    synthetic_stubs: &'a mut FxHashSet<(FileId, String, bool)>,
}

#[cfg(test)]
struct LegacyReExportFullScan<'a> {
    modules: &'a mut [super::types::ModuleNode],
    edges: &'a [Edge],
    re_export_info: &'a [ReExportTuple],
    entry_star_targets: &'a FxHashSet<FileId>,
    edges_by_target: &'a FxHashMap<FileId, Vec<usize>>,
    named_import_origin_index: &'a NamedImportOriginIndex,
    module_by_id: &'a FxHashMap<FileId, &'a ResolvedModule>,
}

/// Deterministic scheduler for monotone re-export propagation.
///
/// Each tuple reads export state from `barrel` and may add references or
/// synthetic exports to `source`. When `source` changes, only tuples whose
/// `barrel` is that module can observe the new state, so those tuple indices
/// are re-enqueued in their original stable order.
struct ReExportPropagationPlan {
    observers_by_module: FxHashMap<FileId, Vec<usize>>,
    queue: VecDeque<usize>,
    enqueued: Vec<bool>,
}

impl ReExportPropagationPlan {
    fn new(re_export_info: &[ReExportTuple]) -> Self {
        let mut observers_by_module: FxHashMap<FileId, Vec<usize>> = FxHashMap::default();
        for (idx, entry) in re_export_info.iter().enumerate() {
            observers_by_module
                .entry(entry.barrel)
                .or_default()
                .push(idx);
        }

        Self {
            observers_by_module,
            queue: (0..re_export_info.len()).collect(),
            enqueued: vec![true; re_export_info.len()],
        }
    }

    fn pop_front(&mut self) -> Option<usize> {
        let idx = self.queue.pop_front()?;
        self.enqueued[idx] = false;
        Some(idx)
    }

    fn enqueue_observers(&mut self, changed_module: FileId) {
        let Some(observers) = self.observers_by_module.get(&changed_module) else {
            return;
        };
        for &idx in observers {
            if !self.enqueued[idx] {
                self.enqueued[idx] = true;
                self.queue.push_back(idx);
            }
        }
    }
}

impl ModuleGraph {
    /// Resolve re-export chains: when module A re-exports from B,
    /// any reference to A's re-exported symbol should also count as a reference
    /// to B's original export (and transitively through the chain).
    ///
    /// Returns the list of re-export cycles and self-loops detected during
    /// the upfront Tarjan SCC pass. The caller stores this on the
    /// `ModuleGraph` so the `re-export-cycle` finding type can surface them
    /// to users instead of relying on `RUST_LOG=warn` (see issue #515).
    pub(super) fn resolve_re_export_chains(
        &mut self,
        module_by_id: &FxHashMap<FileId, &ResolvedModule>,
    ) -> Vec<GraphReExportCycle> {
        let re_export_info = self.collect_re_export_tuples();

        if re_export_info.is_empty() {
            return Vec::new();
        }

        let cycles = find_re_export_cycles(&self.modules, &re_export_info);

        let entry_star_targets = self.collect_entry_star_targets();
        let edges_by_target = self.build_edges_by_target();
        let named_import_origin_index =
            if self.needs_named_import_origin_index(&re_export_info, &entry_star_targets) {
                NamedImportOriginIndex::from_edges(&self.edges)
            } else {
                NamedImportOriginIndex::default()
            };

        self.run_re_export_fixpoint(
            &re_export_info,
            &entry_star_targets,
            &edges_by_target,
            &named_import_origin_index,
            module_by_id,
        );

        cycles
    }

    /// Flatten every module's re-export edges into a single tuple list.
    fn collect_re_export_tuples(&self) -> Vec<ReExportTuple> {
        self.modules
            .iter()
            .flat_map(|m| {
                m.re_exports.iter().map(move |re| ReExportTuple {
                    barrel: m.file_id,
                    source: re.source_file,
                    imported_name: re.imported_name.clone(),
                    exported_name: re.exported_name.clone(),
                    is_type_only: re.is_type_only,
                })
            })
            .collect()
    }

    /// Compute the transitive closure of `export *` source files reachable from
    /// entry-point barrels.
    fn collect_entry_star_targets(&self) -> FxHashSet<FileId> {
        let mut entry_star_targets: FxHashSet<FileId> = self
            .modules
            .iter()
            .filter(|m| m.is_entry_point())
            .flat_map(|m| {
                m.re_exports
                    .iter()
                    .filter(|re| re.exported_name == "*")
                    .map(|re| re.source_file)
            })
            .collect();
        let mut entry_star_stack: Vec<FileId> = entry_star_targets.iter().copied().collect();
        while let Some(file_id) = entry_star_stack.pop() {
            let idx = file_id.0 as usize;
            if idx >= self.modules.len() {
                continue;
            }

            for re in self.modules[idx]
                .re_exports
                .iter()
                .filter(|re| re.exported_name == "*")
            {
                if entry_star_targets.insert(re.source_file) {
                    entry_star_stack.push(re.source_file);
                }
            }
        }
        entry_star_targets
    }

    /// Index every edge by its target file for fast star-propagation lookups.
    fn build_edges_by_target(&self) -> FxHashMap<FileId, Vec<usize>> {
        let mut edges_by_target: FxHashMap<FileId, Vec<usize>> = FxHashMap::default();
        for (idx, edge) in self.edges.iter().enumerate() {
            edges_by_target.entry(edge.target).or_default().push(idx);
        }
        edges_by_target
    }

    fn needs_named_import_origin_index(
        &self,
        re_export_info: &[ReExportTuple],
        entry_star_targets: &FxHashSet<FileId>,
    ) -> bool {
        re_export_info.iter().any(|entry| {
            if entry.exported_name != "*" || entry_star_targets.contains(&entry.barrel) {
                return false;
            }

            self.modules
                .get(entry.barrel.0 as usize)
                .is_some_and(|barrel| !barrel.is_entry_point())
        })
    }

    /// Run monotone propagation, revisiting only tuples affected by new state.
    fn run_re_export_fixpoint(
        &mut self,
        re_export_info: &[ReExportTuple],
        entry_star_targets: &FxHashSet<FileId>,
        edges_by_target: &FxHashMap<FileId, Vec<usize>>,
        named_import_origin_index: &NamedImportOriginIndex,
        module_by_id: &FxHashMap<FileId, &ResolvedModule>,
    ) {
        #[cfg(test)]
        let mut legacy_modules: Option<Vec<super::types::ModuleNode>> = DIFFERENTIAL_CHECK_ENABLED
            .with(|enabled| {
                enabled.get().then(|| {
                    serde_json::from_value(
                        serde_json::to_value(&self.modules)
                            .expect("module graph should serialize for differential testing"),
                    )
                    .expect("module graph should deserialize for differential testing")
                })
            });

        let safety_cap = self.re_export_transition_safety_cap(re_export_info);
        let mut processed = 0usize;
        let mut plan = ReExportPropagationPlan::new(re_export_info);
        let mut existing_refs: FxHashSet<FileId> = FxHashSet::default();
        let mut synthetic_stubs: FxHashSet<(FileId, String, bool)> = FxHashSet::default();

        while let Some(entry_idx) = plan.pop_front() {
            if processed >= safety_cap {
                tracing::error!(
                    processed,
                    safety_cap,
                    re_export_edges = re_export_info.len(),
                    "Re-export propagation exceeded its finite-state safety cap; \
                     propagation may be non-monotonic. Please file a bug at \
                     https://github.com/fallow-rs/fallow/issues with the repro."
                );
                break;
            }
            processed += 1;

            let mut context = ReExportContext {
                entry_star_targets,
                edges_by_target,
                named_import_origin_index,
                module_by_id,
                existing_refs: &mut existing_refs,
                synthetic_stubs: &mut synthetic_stubs,
            };

            let entry = &re_export_info[entry_idx];
            #[cfg(test)]
            record_propagation_visit(entry);
            if Self::propagate_re_export_entry(&mut self.modules, &self.edges, entry, &mut context)
            {
                plan.enqueue_observers(entry.source);
            }
        }

        #[cfg(test)]
        if let Some(legacy_modules) = legacy_modules.as_mut() {
            Self::run_re_export_full_scan(LegacyReExportFullScan {
                modules: legacy_modules,
                edges: &self.edges,
                re_export_info,
                entry_star_targets,
                edges_by_target,
                named_import_origin_index,
                module_by_id,
            });
            assert_eq!(
                serde_json::to_value(legacy_modules)
                    .expect("legacy module graph should serialize for comparison"),
                serde_json::to_value(&self.modules)
                    .expect("queue module graph should serialize for comparison"),
                "work-queue propagation must match the legacy full-scan fixpoint"
            );
        }
    }

    /// Bound scheduler work by the finite set of exports, synthetic names, and
    /// reference source modules that monotone propagation can add.
    fn re_export_transition_safety_cap(&self, re_export_info: &[ReExportTuple]) -> usize {
        let initial_exports = self
            .modules
            .iter()
            .map(|module| module.exports.len())
            .sum::<usize>();
        let named_inputs = self
            .edges
            .iter()
            .flat_map(|edge| &edge.symbols)
            .filter(|symbol| {
                matches!(
                    &symbol.imported_name,
                    fallow_types::extract::ImportedName::Named(_)
                )
            })
            .count()
            .saturating_add(initial_exports)
            .saturating_add(re_export_info.len());

        let module_count = self.modules.len();
        let synthetic_export_hosts = self
            .modules
            .iter()
            .filter(|module| {
                module
                    .re_exports
                    .iter()
                    .any(|re_export| re_export.exported_name == "*")
            })
            .count();
        let synthetic_exports = synthetic_export_hosts
            .saturating_mul(named_inputs)
            .saturating_mul(2);
        let max_exports = initial_exports.saturating_add(synthetic_exports);
        let reference_additions = max_exports.saturating_mul(module_count);
        let state_changes = synthetic_exports.saturating_add(reference_additions);

        re_export_info
            .len()
            .saturating_add(state_changes.saturating_mul(re_export_info.len()))
            .max(re_export_info.len())
    }

    /// Propagate references for one re-export edge, dispatching star vs named.
    fn propagate_re_export_entry(
        modules: &mut [super::types::ModuleNode],
        edges: &[Edge],
        entry: &ReExportTuple,
        context: &mut ReExportContext<'_>,
    ) -> bool {
        let barrel_idx = entry.barrel.0 as usize;
        let source_idx = entry.source.0 as usize;

        if barrel_idx >= modules.len() || source_idx >= modules.len() {
            return false;
        }

        if entry.exported_name == "*" {
            propagate_star_re_export(StarReExportPropagation {
                modules,
                edges,
                edges_by_target: context.edges_by_target,
                named_import_origin_index: context.named_import_origin_index,
                module_by_id: context.module_by_id,
                barrel_id: entry.barrel,
                barrel_idx,
                source_id: entry.source,
                source_idx,
                entry_star_targets: context.entry_star_targets,
                triggering_is_type_only: entry.is_type_only,
                synthetic_stubs: context.synthetic_stubs,
            })
        } else {
            propagate_named_re_export(NamedReExportPropagation {
                modules,
                barrel_id: entry.barrel,
                barrel_idx,
                source_idx,
                imported_name: &entry.imported_name,
                exported_name: &entry.exported_name,
                existing_refs: context.existing_refs,
            })
        }
    }

    #[cfg(test)]
    fn run_re_export_full_scan(input: LegacyReExportFullScan<'_>) {
        let LegacyReExportFullScan {
            modules,
            edges,
            re_export_info,
            entry_star_targets,
            edges_by_target,
            named_import_origin_index,
            module_by_id,
        } = input;
        let max_iterations = re_export_info.len().saturating_add(1);
        let mut existing_refs: FxHashSet<FileId> = FxHashSet::default();
        let mut synthetic_stubs: FxHashSet<(FileId, String, bool)> = FxHashSet::default();

        for _ in 0..max_iterations {
            let mut changed = false;
            for entry in re_export_info {
                let mut context = ReExportContext {
                    entry_star_targets,
                    edges_by_target,
                    named_import_origin_index,
                    module_by_id,
                    existing_refs: &mut existing_refs,
                    synthetic_stubs: &mut synthetic_stubs,
                };
                changed |= Self::propagate_re_export_entry(modules, edges, entry, &mut context);
            }
            if !changed {
                break;
            }
        }
    }
}

/// Find SCCs of size >= 2 in the re-export subgraph and self-re-export
/// edges, emit one `tracing::warn!` per cycle, AND return structured cycle
/// data for the user-visible `re-export-cycle` finding type.
///
/// The `tracing::warn!` emissions remain unchanged from #442 (RUST_LOG=warn
/// operators still see them). The returned `Vec<GraphReExportCycle>` is the
/// structured surface that the analysis backend consumes and wraps in typed
/// `ReExportCycleFinding`s for end-user output. See issue #515.
fn find_re_export_cycles(
    modules: &[super::types::ModuleNode],
    re_export_info: &[ReExportTuple],
) -> Vec<GraphReExportCycle> {
    let mut cycles: Vec<GraphReExportCycle> = Vec::new();

    let (node_index, nodes) = build_re_export_node_index(re_export_info);
    let n = nodes.len();
    if n == 0 {
        return cycles;
    }

    let adj = build_re_export_adjacency(re_export_info, &node_index, modules, &mut cycles);

    let sccs = tarjan_scc(n, &adj);

    for scc in &sccs {
        if scc.len() < 2 {
            continue;
        }
        cycles.push(build_multi_node_cycle(scc, &nodes, modules));
    }

    cycles
}

/// Assign a dense node index to every distinct barrel / source file id.
fn build_re_export_node_index(
    re_export_info: &[ReExportTuple],
) -> (FxHashMap<FileId, usize>, Vec<FileId>) {
    let mut node_index: FxHashMap<FileId, usize> = FxHashMap::default();
    let mut nodes: Vec<FileId> = Vec::new();
    for entry in re_export_info {
        for &id in &[entry.barrel, entry.source] {
            node_index.entry(id).or_insert_with(|| {
                let idx = nodes.len();
                nodes.push(id);
                idx
            });
        }
    }
    (node_index, nodes)
}

/// Build the adjacency list for the re-export subgraph, emitting a self-loop
/// `GraphReExportCycle` for any barrel that re-exports from itself.
fn build_re_export_adjacency(
    re_export_info: &[ReExportTuple],
    node_index: &FxHashMap<FileId, usize>,
    modules: &[super::types::ModuleNode],
    cycles: &mut Vec<GraphReExportCycle>,
) -> Vec<Vec<usize>> {
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); node_index.len()];
    let mut seen_edge: FxHashSet<(usize, usize)> = FxHashSet::default();
    let mut seen_self_loop: FxHashSet<FileId> = FxHashSet::default();
    for entry in re_export_info {
        let from = node_index[&entry.barrel];
        let to = node_index[&entry.source];
        if from == to {
            if seen_self_loop.insert(entry.barrel) {
                cycles.push(build_self_loop_cycle(entry.barrel, modules));
            }
            continue;
        }
        if seen_edge.insert((from, to)) {
            adj[from].push(to);
        }
    }
    adj
}

/// Emit the `tracing::warn!` and structured cycle for a self-re-export edge.
fn build_self_loop_cycle(
    barrel: FileId,
    modules: &[super::types::ModuleNode],
) -> GraphReExportCycle {
    let (path_buf, path_display) = module_path_and_display(barrel, modules);
    tracing::warn!(
        file = path_display.as_str(),
        "Re-export self-loop detected: this file re-exports from \
         itself. Chain propagation is structurally a no-op for \
         these edges. Inspect the barrel for an accidental \
         `export * from './<this-file>'` after a rename or move."
    );
    GraphReExportCycle {
        files: vec![path_buf],
        file_ids: vec![barrel],
        is_self_loop: true,
    }
}

/// Emit the `tracing::warn!` and structured cycle for a multi-node SCC.
fn build_multi_node_cycle(
    scc: &[usize],
    nodes: &[FileId],
    modules: &[super::types::ModuleNode],
) -> GraphReExportCycle {
    let mut triples: Vec<(PathBuf, String, FileId)> = scc
        .iter()
        .map(|&idx| {
            let file_id = nodes[idx];
            let (path, display) = module_path_and_display(file_id, modules);
            (path, display, file_id)
        })
        .collect();
    triples.sort_by(|a, b| a.1.cmp(&b.1));
    let members = triples
        .iter()
        .map(|(_, d, _)| d.as_str())
        .collect::<Vec<_>>()
        .join(" <-> ");
    tracing::warn!(
        cycle_size = scc.len(),
        members = members.as_str(),
        "Re-export cycle detected: chain propagation may be incomplete \
         for symbols on this barrel loop. Break the cycle to restore \
         full reachability analysis."
    );
    let (files, file_ids) = triples.into_iter().fold(
        (Vec::new(), Vec::new()),
        |(mut paths, mut ids), (p, _, id)| {
            paths.push(p);
            ids.push(id);
            (paths, ids)
        },
    );
    GraphReExportCycle {
        files,
        file_ids,
        is_self_loop: false,
    }
}

/// Resolve a `FileId` to its `(PathBuf, display string)`, falling back to a
/// placeholder when the id is outside the module list.
fn module_path_and_display(
    file_id: FileId,
    modules: &[super::types::ModuleNode],
) -> (PathBuf, String) {
    let i = file_id.0 as usize;
    if i < modules.len() {
        let p = modules[i].path.clone();
        let d = p.display().to_string();
        (p, d)
    } else {
        let placeholder = format!("<file id {i}>");
        (PathBuf::from(&placeholder), placeholder)
    }
}

struct TarjanFrame {
    node: usize,
    next_succ: usize,
}

/// Mutable Tarjan SCC state shared across the iterative DFS.
struct TarjanState {
    index_counter: u32,
    indices: Vec<u32>,
    lowlinks: Vec<u32>,
    on_stack: fixedbitset::FixedBitSet,
    stack: Vec<usize>,
    sccs: Vec<Vec<usize>>,
}

impl TarjanState {
    fn new(n: usize) -> Self {
        Self {
            index_counter: 0,
            indices: vec![u32::MAX; n],
            lowlinks: vec![0; n],
            on_stack: fixedbitset::FixedBitSet::with_capacity(n),
            stack: Vec::new(),
            sccs: Vec::new(),
        }
    }

    /// Assign the next DFS index to `node` and push it onto the SCC stack.
    fn discover(&mut self, node: usize) {
        self.indices[node] = self.index_counter;
        self.lowlinks[node] = self.index_counter;
        self.index_counter = self.index_counter.saturating_add(1);
        self.stack.push(node);
        self.on_stack.insert(node);
    }

    /// Advance one successor of the current frame, pushing a child frame when a
    /// new node is discovered. Returns the child node to descend into, if any.
    fn step_successor(&mut self, frame: &mut TarjanFrame, adj: &[Vec<usize>]) -> Option<usize> {
        let v = frame.node;
        let w = adj[v][frame.next_succ];
        frame.next_succ = frame.next_succ.saturating_add(1);
        if self.indices[w] == u32::MAX {
            self.discover(w);
            Some(w)
        } else {
            if self.on_stack.contains(w) {
                self.lowlinks[v] = self.lowlinks[v].min(self.indices[w]);
            }
            None
        }
    }

    /// Finish the current frame: emit its SCC if it is a root, then propagate
    /// its lowlink to the parent frame.
    fn finish_frame(&mut self, v: usize, parent: Option<usize>) {
        if self.lowlinks[v] == self.indices[v] {
            let mut scc = Vec::new();
            while let Some(w) = self.stack.pop() {
                self.on_stack.remove(w);
                scc.push(w);
                if w == v {
                    break;
                }
            }
            self.sccs.push(scc);
        }
        if let Some(pv) = parent {
            self.lowlinks[pv] = self.lowlinks[pv].min(self.lowlinks[v]);
        }
    }
}

/// Iterative Tarjan's strongly connected components, returns SCCs that
/// contain at least one node. The graph is given as adjacency-by-index;
/// the caller maps node indices back to FileIds.
fn tarjan_scc(n: usize, adj: &[Vec<usize>]) -> Vec<Vec<usize>> {
    let mut state = TarjanState::new(n);

    for start in 0..n {
        if state.indices[start] != u32::MAX {
            continue;
        }
        state.discover(start);
        let mut dfs: Vec<TarjanFrame> = vec![TarjanFrame {
            node: start,
            next_succ: 0,
        }];

        while let Some(frame) = dfs.last_mut() {
            let v = frame.node;
            if frame.next_succ < adj[v].len() {
                if let Some(child) = state.step_successor(frame, adj) {
                    dfs.push(TarjanFrame {
                        node: child,
                        next_succ: 0,
                    });
                }
            } else {
                dfs.pop();
                state.finish_frame(v, dfs.last().map(|parent| parent.node));
            }
        }
    }

    state.sccs
}
