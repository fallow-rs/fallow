//! Phase 4: Re-export chain resolution, propagate references through barrel files.

mod propagate;
#[cfg(test)]
mod tests;

use rustc_hash::{FxHashMap, FxHashSet};

use fallow_types::discover::FileId;

use super::ModuleGraph;

use propagate::{propagate_named_re_export, propagate_star_re_export};

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

impl ModuleGraph {
    /// Resolve re-export chains: when module A re-exports from B,
    /// any reference to A's re-exported symbol should also count as a reference
    /// to B's original export (and transitively through the chain).
    pub(super) fn resolve_re_export_chains(&mut self) {
        let re_export_info: Vec<ReExportTuple> = self
            .modules
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
            .collect();

        if re_export_info.is_empty() {
            return;
        }

        // Surface re-export cycles up front via Tarjan SCC over the
        // re-export subgraph (barrel -> source). A cycle is almost always a
        // real bug in the barrel structure: silent termination via an
        // iteration cap hid these for years. Cycles still terminate
        // naturally via the dedup-by-`from_file` check inside each
        // propagation helper, so this pass is purely diagnostic.
        warn_on_re_export_cycles(&self.modules, &re_export_info);

        // Precompute barrels that are transitively star-re-exported from entry points.
        // These get entry-point-like treatment: all source exports are marked used.
        // Entry points often expose public APIs through multiple `export *`
        // barrels, so direct targets alone are not enough.
        // Computing this once avoids O(modules) per call inside the hot loop.
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

        // Pre-build reverse edge index: target FileId -> edge indices.
        // This avoids O(all_edges) scans per star re-export in the hot loop.
        // For barrel-heavy monorepos (Vue/Nuxt), star re-exports dominate the
        // iteration cost, without this index, each call to propagate_star_re_export
        // linearly scans all edges to find those targeting the barrel.
        let mut edges_by_target: FxHashMap<FileId, Vec<usize>> = FxHashMap::default();
        for (idx, edge) in self.edges.iter().enumerate() {
            edges_by_target.entry(edge.target).or_default().push(idx);
        }

        // For each re-export, if the barrel's exported symbol has references,
        // propagate those references to the source module's original export.
        // Iterate until no new references are added (handles chains of arbitrary depth).
        //
        // Termination: each propagation helper adds references only after a
        // dedup-by-`from_file` check, so the total monotone growth across
        // iterations is bounded by `|files| * |exports|`. Once an iteration
        // adds zero references, the fixpoint is reached and the loop exits.
        //
        // The `safety_cap` is a defensive backstop: a chain of length K
        // converges in at most K iterations, and K cannot exceed the number
        // of re-export edges. If the cap fires, something has violated
        // monotonicity, which is a real bug and warrants a loud diagnostic.
        let safety_cap = re_export_info.len().saturating_add(1);
        let mut changed = true;
        let mut iteration: usize = 0;
        // Reuse a single HashSet across iterations to avoid repeated allocations.
        // In barrel-heavy monorepos, this loop can run up to safety_cap * re_export_info.len()
        // * target_exports.len() times, reusing with .clear() avoids O(n) allocations.
        let mut existing_refs: FxHashSet<FileId> = FxHashSet::default();
        // Track every (source, exported_name) pair we synthesise a stub for so a
        // later value-bearing triggering edge can downgrade a type-only stub.
        // Real `export type Foo` declarations on the source are NOT in this set
        // and stay type-only; only synthesised bridge stubs ever flip.
        let mut synthetic_stubs: FxHashSet<(FileId, String)> = FxHashSet::default();

        while changed && iteration < safety_cap {
            changed = false;
            iteration += 1;

            for entry in &re_export_info {
                let barrel_idx = entry.barrel.0 as usize;
                let source_idx = entry.source.0 as usize;

                if barrel_idx >= self.modules.len() || source_idx >= self.modules.len() {
                    continue;
                }

                if entry.exported_name == "*" {
                    changed |= propagate_star_re_export(
                        &mut self.modules,
                        &self.edges,
                        &edges_by_target,
                        entry.barrel,
                        barrel_idx,
                        entry.source,
                        source_idx,
                        &entry_star_targets,
                        entry.is_type_only,
                        &mut synthetic_stubs,
                    );
                } else {
                    changed |= propagate_named_re_export(
                        &mut self.modules,
                        entry.barrel,
                        barrel_idx,
                        source_idx,
                        &entry.imported_name,
                        &entry.exported_name,
                        &mut existing_refs,
                    );
                }
            }
        }

        if iteration >= safety_cap && changed {
            // Should never fire in practice. If it does, propagation lost its
            // monotonicity invariant and the bug needs a loud diagnostic.
            tracing::error!(
                iterations = iteration,
                safety_cap,
                re_export_edges = re_export_info.len(),
                "Re-export chain fixpoint exceeded safety cap; \
                 propagation may be non-monotonic. Please file a bug at \
                 https://github.com/fallow-rs/fallow/issues with the repro."
            );
        }
    }
}

/// Find SCCs of size >= 2 in the re-export subgraph and emit one
/// `tracing::warn!` per cycle with the member file paths.
///
/// Diagnostics are RUST_LOG-gated today; surfacing them as user-visible
/// `AnalysisResults` findings is tracked as a follow-up so #442 stays
/// scoped to the graph crate.
fn warn_on_re_export_cycles(
    modules: &[super::types::ModuleNode],
    re_export_info: &[ReExportTuple],
) {
    // Collect unique nodes (FileIds appearing as either endpoint).
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

    let n = nodes.len();
    if n == 0 {
        return;
    }

    // Build adjacency list: barrel -> source. Dedup parallel edges (same
    // pair via multiple re-exports) so the SCC walk doesn't revisit.
    // Self-edges (a barrel re-exporting from itself) are pathological in
    // their own right; warn separately and exclude from the SCC pass so
    // the cycle diagnostic stays focused on barrel-to-barrel loops.
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut seen_edge: FxHashSet<(usize, usize)> = FxHashSet::default();
    let mut seen_self_loop: FxHashSet<FileId> = FxHashSet::default();
    for entry in re_export_info {
        let from = node_index[&entry.barrel];
        let to = node_index[&entry.source];
        if from == to {
            if seen_self_loop.insert(entry.barrel) {
                let i = entry.barrel.0 as usize;
                let path = if i < modules.len() {
                    modules[i].path.display().to_string()
                } else {
                    format!("<file id {i}>")
                };
                tracing::warn!(
                    file = path.as_str(),
                    "Re-export self-loop detected: this file re-exports from \
                     itself. Chain propagation is structurally a no-op for \
                     these edges. Inspect the barrel for an accidental \
                     `export * from './<this-file>'` after a rename or move."
                );
            }
            continue;
        }
        if seen_edge.insert((from, to)) {
            adj[from].push(to);
        }
    }

    // Iterative Tarjan's SCC over the re-export subgraph.
    let sccs = tarjan_scc(n, &adj);

    for scc in &sccs {
        if scc.len() < 2 {
            continue;
        }
        let mut paths: Vec<String> = scc
            .iter()
            .map(|&idx| {
                let file_id = nodes[idx];
                let i = file_id.0 as usize;
                if i < modules.len() {
                    modules[i].path.display().to_string()
                } else {
                    format!("<file id {i}>")
                }
            })
            .collect();
        paths.sort();
        let members = paths.join(" <-> ");
        tracing::warn!(
            cycle_size = scc.len(),
            members = members.as_str(),
            "Re-export cycle detected: chain propagation may be incomplete \
             for symbols on this barrel loop. Break the cycle to restore \
             full reachability analysis."
        );
    }
}

/// Iterative Tarjan's strongly connected components, returns SCCs that
/// contain at least one node. The graph is given as adjacency-by-index;
/// the caller maps node indices back to FileIds.
fn tarjan_scc(n: usize, adj: &[Vec<usize>]) -> Vec<Vec<usize>> {
    use fixedbitset::FixedBitSet;

    let mut index_counter: u32 = 0;
    let mut indices: Vec<u32> = vec![u32::MAX; n];
    let mut lowlinks: Vec<u32> = vec![0; n];
    let mut on_stack = FixedBitSet::with_capacity(n);
    let mut stack: Vec<usize> = Vec::new();
    let mut sccs: Vec<Vec<usize>> = Vec::new();

    struct Frame {
        node: usize,
        next_succ: usize,
    }

    for start in 0..n {
        if indices[start] != u32::MAX {
            continue;
        }
        let mut dfs: Vec<Frame> = vec![Frame {
            node: start,
            next_succ: 0,
        }];
        indices[start] = index_counter;
        lowlinks[start] = index_counter;
        index_counter = index_counter.saturating_add(1);
        stack.push(start);
        on_stack.insert(start);

        while let Some(frame) = dfs.last_mut() {
            let v = frame.node;
            if frame.next_succ < adj[v].len() {
                let w = adj[v][frame.next_succ];
                frame.next_succ = frame.next_succ.saturating_add(1);
                if indices[w] == u32::MAX {
                    indices[w] = index_counter;
                    lowlinks[w] = index_counter;
                    index_counter = index_counter.saturating_add(1);
                    stack.push(w);
                    on_stack.insert(w);
                    dfs.push(Frame {
                        node: w,
                        next_succ: 0,
                    });
                } else if on_stack.contains(w) {
                    lowlinks[v] = lowlinks[v].min(indices[w]);
                }
            } else {
                if lowlinks[v] == indices[v] {
                    let mut scc = Vec::new();
                    while let Some(w) = stack.pop() {
                        on_stack.remove(w);
                        scc.push(w);
                        if w == v {
                            break;
                        }
                    }
                    sccs.push(scc);
                }
                dfs.pop();
                if let Some(parent) = dfs.last_mut() {
                    let pv = parent.node;
                    lowlinks[pv] = lowlinks[pv].min(lowlinks[v]);
                }
            }
        }
    }

    sccs
}
