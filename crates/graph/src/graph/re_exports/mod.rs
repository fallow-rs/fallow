//! Phase 4: Re-export chain resolution, propagate references through barrel files.

mod propagate;
#[cfg(test)]
mod tests;

use std::collections::VecDeque;
use std::path::PathBuf;

use fixedbitset::FixedBitSet;
use rustc_hash::{FxHashMap, FxHashSet};

#[cfg(test)]
use std::cell::{Cell, RefCell};

use crate::resolve::ResolvedModule;
use fallow_types::discover::FileId;

use super::types::{ReferencePathInterner, RoutedReferenceKey};
use super::{Edge, ModuleGraph};

use propagate::{
    EffectiveDeclarationRouteCache, ImportBindingUsageIndex, NamedPropagationScratch,
    NamedReExportPropagation, StarReExportPropagation, propagate_named_re_export,
    propagate_star_re_export,
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
    binding_usage: &'a ImportBindingUsageIndex,
    effective_exports: &'a super::effective_exports::EffectiveExportIndex,
    existing_refs: &'a mut FxHashSet<RoutedReferenceKey>,
    synthetic_stubs: &'a mut FxHashSet<(FileId, String, bool)>,
    declaration_routes: &'a mut EffectiveDeclarationRouteCache,
    scratch: &'a mut NamedPropagationScratch,
    reference_paths: &'a mut ReferencePathInterner,
}

/// How much of a closure member the consumers that cannot be enumerated see.
///
/// The two differ on `default` alone, because a plain `export *` forwards
/// every named export of its source and never the source's `default`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Exposure {
    /// Reached through a plain `export *`: every export except `default`.
    StarSurface,
    /// The whole namespace object is observed: every export, `default`
    /// included.
    NamespaceObject,
}

/// The exposed namespace closure: every module whose names reach consumers
/// the graph cannot enumerate, with the part of its export surface they see.
///
/// Built by [`ModuleGraph::collect_exposed_namespace_targets`], computed once
/// per graph build and read by Phase 2c (namespace re-export propagation) and
/// Phase 4 (the entry-star seed).
pub(in crate::graph) struct ExposedNamespaceTargets {
    members: FxHashMap<FileId, Exposure>,
}

impl ExposedNamespaceTargets {
    /// Whether the closure has no members at all.
    pub(in crate::graph) fn is_empty(&self) -> bool {
        self.members.is_empty()
    }

    /// Whether the member exposes `exported_name`.
    ///
    /// A member reached through a plain `export *` does not expose `default`:
    /// the star that carried its names onward never forwards it, so an
    /// `export * as default` declared on such a member hands its target's
    /// namespace object to nobody.
    pub(in crate::graph) fn exposes_name(&self, file_id: FileId, exported_name: &str) -> bool {
        match self.members.get(&file_id) {
            Some(Exposure::NamespaceObject) => true,
            Some(Exposure::StarSurface) => exported_name != "default",
            None => false,
        }
    }

    /// Every member, at either exposure.
    ///
    /// Phase 4 star propagation credits the named exports of a member's
    /// `export *` sources and never their `default`, which both exposures
    /// forward alike, so it reads the membership alone.
    fn files(&self) -> impl Iterator<Item = FileId> + '_ {
        self.members.keys().copied()
    }

    /// Record a member, re-walking it when a wider exposure than a previous
    /// visit arrives. Each member is walked at most twice.
    fn record(&mut self, stack: &mut Vec<(FileId, Exposure)>, file_id: FileId, exposure: Exposure) {
        match self.members.entry(file_id) {
            std::collections::hash_map::Entry::Occupied(mut slot) => {
                if *slot.get() == Exposure::StarSurface && exposure == Exposure::NamespaceObject {
                    slot.insert(exposure);
                    stack.push((file_id, exposure));
                }
            }
            std::collections::hash_map::Entry::Vacant(slot) => {
                slot.insert(exposure);
                stack.push((file_id, exposure));
            }
        }
    }
}

/// `export * as ns from './x'`: the barrel exposes x's namespace object under
/// a single name instead of forwarding x's names.
fn is_namespace_re_export(re: &super::types::ReExportEdge) -> bool {
    re.imported_name == "*" && re.exported_name != "*"
}

/// Reverse index of the re-export edges that carry one exported name outward,
/// from the module that declares it toward the barrels that forward it.
///
/// A namespace re-export (`export * as ns`) is not a forwarder: it bundles the
/// source's names into one object instead of passing them along.
struct NameForwarders<'a> {
    /// `(source, imported name)` to the barrels re-exporting it, each with the
    /// name it exports it under, so renames are followed exactly.
    named: FxHashMap<(FileId, &'a str), Vec<(FileId, &'a str)>>,
    /// Source file to the barrels that re-export all of its names.
    stars: FxHashMap<FileId, Vec<FileId>>,
}

/// What [`NameForwarders::reaches_surface`] measures a state against.
struct SurfaceReach {
    /// Entry points plus every barrel they reach through plain `export *`.
    star_surface: FxHashSet<FileId>,
    /// Every module some star-surface module re-exports from, transitively,
    /// names ignored.
    ///
    /// A name can only travel from a module to the entry surface along
    /// forwarding edges, so a module outside this set answers the search in
    /// constant time however deep its own chains run.
    may_reach: FxHashSet<FileId>,
}

/// Reusable state for [`NameForwarders::reaches_surface`].
///
/// `failed` outlives a single search: an exhausted search proves that every
/// state it visited fails too, so a forwarding chain shared by many namespace
/// edges is walked once instead of once per edge.
#[derive(Default)]
struct NameSearchScratch<'a> {
    visited: FxHashSet<(FileId, &'a str)>,
    frontier: Vec<(FileId, &'a str)>,
    failed: FxHashSet<(FileId, &'a str)>,
}

impl<'a> NameForwarders<'a> {
    fn build(modules: &'a [super::types::ModuleNode]) -> Self {
        let mut named: FxHashMap<(FileId, &'a str), Vec<(FileId, &'a str)>> = FxHashMap::default();
        let mut stars: FxHashMap<FileId, Vec<FileId>> = FxHashMap::default();
        for module in modules {
            for re in &module.re_exports {
                if re.imported_name == "*" {
                    if re.exported_name == "*" {
                        stars
                            .entry(re.source_file)
                            .or_default()
                            .push(module.file_id);
                    }
                } else {
                    named
                        .entry((re.source_file, re.imported_name.as_str()))
                        .or_default()
                        .push((module.file_id, re.exported_name.as_str()));
                }
            }
        }
        Self { named, stars }
    }

    /// Whether `name`, as exported by `file`, reaches the entry point's own
    /// export surface through named and plain-star re-exports.
    ///
    /// Each hop must really forward the binding: a barrel that declares its
    /// own `name`, or that receives it from two stars at once, exports a
    /// different binding under that name and the chain stops there. A plain
    /// `export *` also never carries `default`, so a `default`-named state
    /// takes named hops only.
    fn reaches_surface(
        &self,
        graph: &ModuleGraph,
        file: FileId,
        name: &'a str,
        surfaces: &SurfaceReach,
        scratch: &mut NameSearchScratch<'a>,
    ) -> bool {
        let SurfaceReach {
            star_surface,
            may_reach,
        } = surfaces;
        if !may_reach.contains(&file) || scratch.failed.contains(&(file, name)) {
            return false;
        }
        scratch.visited.clear();
        scratch.frontier.clear();
        scratch.visited.insert((file, name));
        scratch.frontier.push((file, name));
        while let Some((current, current_name)) = scratch.frontier.pop() {
            if graph.surface_exposes(star_surface, current, current_name) {
                return true;
            }
            if let Some(barrels) = self.named.get(&(current, current_name)) {
                for &(barrel, exported_name) in barrels {
                    if may_reach.contains(&barrel)
                        && graph.forwards_binding(current, current_name, barrel, exported_name)
                        && !scratch.failed.contains(&(barrel, exported_name))
                        && scratch.visited.insert((barrel, exported_name))
                    {
                        scratch.frontier.push((barrel, exported_name));
                    }
                }
            }
            if current_name == "default" {
                continue;
            }
            if let Some(barrels) = self.stars.get(&current) {
                for &barrel in barrels {
                    if may_reach.contains(&barrel)
                        && graph.forwards_binding(current, current_name, barrel, current_name)
                        && !scratch.failed.contains(&(barrel, current_name))
                        && scratch.visited.insert((barrel, current_name))
                    {
                        scratch.frontier.push((barrel, current_name));
                    }
                }
            }
        }
        scratch.failed.extend(scratch.visited.iter().copied());
        false
    }
}

struct ReExportFixpointInput<'a> {
    re_export_info: &'a [ReExportTuple],
    entry_star_targets: &'a FxHashSet<FileId>,
    edges_by_target: &'a FxHashMap<FileId, Vec<usize>>,
    module_by_id: &'a FxHashMap<FileId, &'a ResolvedModule>,
    reference_paths: &'a mut ReferencePathInterner,
}

#[cfg(test)]
struct LegacyReExportFullScan<'a> {
    modules: &'a mut [super::types::ModuleNode],
    edges: &'a [Edge],
    re_export_info: &'a [ReExportTuple],
    entry_star_targets: &'a FxHashSet<FileId>,
    edges_by_target: &'a FxHashMap<FileId, Vec<usize>>,
    module_by_id: &'a FxHashMap<FileId, &'a ResolvedModule>,
    effective_exports: &'a super::effective_exports::EffectiveExportIndex,
    reference_paths: &'a mut ReferencePathInterner,
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
        exposed_namespace_targets: &ExposedNamespaceTargets,
        reference_paths: &mut ReferencePathInterner,
    ) -> Vec<GraphReExportCycle> {
        let re_export_info = self.collect_re_export_tuples();

        if re_export_info.is_empty() {
            return Vec::new();
        }

        let cycles = find_re_export_cycles(&self.modules, &re_export_info);

        let entry_star_targets = self.collect_entry_star_targets(exposed_namespace_targets);
        let edges_by_target = self.build_edges_by_target();

        self.run_re_export_fixpoint(ReExportFixpointInput {
            re_export_info: &re_export_info,
            entry_star_targets: &entry_star_targets,
            edges_by_target: &edges_by_target,
            module_by_id,
            reference_paths,
        });

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

    /// Compute the transitive closure of `export *` source files whose every
    /// named export is credited: star sources of entry-point barrels, closed
    /// over plain `export *` chains, plus every member of the exposed
    /// namespace closure (`collect_exposed_namespace_targets`, computed once
    /// per build and threaded in).
    fn collect_entry_star_targets(
        &self,
        exposed_namespace_targets: &ExposedNamespaceTargets,
    ) -> FxHashSet<FileId> {
        let mut entry_star_targets: FxHashSet<FileId> = exposed_namespace_targets.files().collect();
        entry_star_targets.extend(self.modules.iter().filter(|m| m.is_entry_point()).flat_map(
            |m| {
                m.re_exports
                    .iter()
                    .filter(|re| re.exported_name == "*")
                    .map(|re| re.source_file)
            },
        ));
        self.extend_plain_star_closure(&mut entry_star_targets);
        entry_star_targets
    }

    /// Every module whose full namespace object is handed to consumers the
    /// graph cannot enumerate per name (issues #2357, #2372, #2373).
    ///
    /// The seeds are the targets whose whole namespace object Phase 2
    /// observed (`whole_module_targets`: an ambient-module star, a
    /// dynamic-import pattern match, or a namespace import the graph could not
    /// narrow because it is used as a whole object, handed on without member
    /// access, or re-exported from a non-entry module) plus the `export * as
    /// ns` sources an entry point's own export surface exposes. Every such
    /// consumer sees every name on the namespace object, including the names
    /// that only arrive through the target's own `export *` and
    /// `export * as ns` chains, and per-name propagation cannot credit those
    /// because no name is ever imported. The closure therefore follows both
    /// chain forms: star propagation treats each member like an entry barrel
    /// for its `export *` sources (named exports, never `default`), and
    /// namespace re-export propagation credits every export of each member's
    /// `export * as ns` sources (`default` included, because the namespace
    /// object exposes it).
    ///
    /// A member reached through a plain `export *` carries the weaker
    /// [`Exposure::StarSurface`]: the star forwarded its named exports and not
    /// its `default`, so an `export * as default` declared on it exposes
    /// nothing and stops the walk.
    ///
    /// `entry_reachable` is the entry-point reachability bitset. A module no
    /// entry point reaches is already reported as an unused file, so crediting
    /// its chain would only add unused-export rows underneath that row; the
    /// only consumers left to observe it are unreachable themselves.
    ///
    /// Computed once per graph build and threaded into both phases that read
    /// it; it depends only on `re_exports`, the entry-point flags, and
    /// reachability, none of which any later phase mutates.
    pub(in crate::graph) fn collect_exposed_namespace_targets(
        &self,
        whole_module_targets: &FxHashSet<FileId>,
        entry_reachable: &FixedBitSet,
    ) -> ExposedNamespaceTargets {
        let mut seeds = whole_module_targets.clone();
        self.seed_entry_exposed_namespaces(&mut seeds);

        let mut closure = ExposedNamespaceTargets {
            members: FxHashMap::default(),
        };
        let mut stack: Vec<(FileId, Exposure)> = Vec::new();
        for seed in seeds {
            if entry_reachable.contains(seed.0 as usize) {
                closure.record(&mut stack, seed, Exposure::NamespaceObject);
            }
        }
        while let Some((file_id, exposure)) = stack.pop() {
            let Some(module) = self.modules.get(file_id.0 as usize) else {
                continue;
            };
            for re in &module.re_exports {
                if re.imported_name != "*" || !entry_reachable.contains(re.source_file.0 as usize) {
                    continue;
                }
                let next = if re.exported_name == "*" {
                    Exposure::StarSurface
                } else if exposure == Exposure::NamespaceObject || re.exported_name != "default" {
                    Exposure::NamespaceObject
                } else {
                    continue;
                };
                closure.record(&mut stack, re.source_file, next);
            }
        }
        closure
    }

    /// Add every `export * as ns` source whose namespace object an entry
    /// point's own export surface exposes.
    ///
    /// The surface is tracked by name, so a barrel that only forwards one
    /// binding never exposes the rest of its module. `ns` reaches an entry
    /// point three ways: the namespace re-export sits on the entry point
    /// itself, it sits on a barrel the entry reaches through plain `export *`
    /// (the star forwards `ns` along with every other name), or an entry
    /// point re-exports `ns` by name through a chain of named and star
    /// re-exports, renames included (`export { sub } from './barrel'`). All
    /// three hand `ns` to consumers the graph cannot enumerate, so the
    /// target's own chains must be credited. `export * as default` is the one
    /// name a plain `export *` leaves behind, so it only counts on an entry
    /// point itself or through a named hop that mentions it.
    ///
    /// The search runs outward from each namespace edge rather than inward
    /// from every entry-point export, so its cost scales with the number of
    /// `export * as` edges (usually a handful) instead of with the number of
    /// names an entry point re-exports.
    fn seed_entry_exposed_namespaces(&self, targets: &mut FxHashSet<FileId>) {
        let namespace_edges: Vec<(FileId, &str, FileId)> = self
            .modules
            .iter()
            .flat_map(|m| {
                m.re_exports
                    .iter()
                    .filter(|re| is_namespace_re_export(re))
                    .map(move |re| (m.file_id, re.exported_name.as_str(), re.source_file))
            })
            .collect();
        if namespace_edges.is_empty() {
            return;
        }

        let mut star_surface: FxHashSet<FileId> = self
            .modules
            .iter()
            .filter(|m| m.is_entry_point())
            .map(|m| m.file_id)
            .collect();
        self.extend_plain_star_closure(&mut star_surface);

        let mut off_surface: Vec<(FileId, &str, FileId)> = Vec::new();
        for (barrel, exported_name, source) in namespace_edges {
            if self.surface_exposes(&star_surface, barrel, exported_name) {
                targets.insert(source);
            } else {
                off_surface.push((barrel, exported_name, source));
            }
        }
        if off_surface.is_empty() {
            return;
        }

        let surfaces = SurfaceReach {
            may_reach: self.collect_surface_forwarding_sources(&star_surface),
            star_surface,
        };
        let forwarders = NameForwarders::build(&self.modules);
        let mut search = NameSearchScratch::default();
        for (barrel, exported_name, source) in off_surface {
            if !targets.contains(&source)
                && forwarders.reaches_surface(self, barrel, exported_name, &surfaces, &mut search)
            {
                targets.insert(source);
            }
        }
    }

    /// Every module a star-surface module re-exports from, transitively,
    /// following named and plain-star edges and ignoring names.
    ///
    /// `export * as ns` is left out: it bundles its source's names into one
    /// object instead of forwarding them, so it never carries a name outward.
    fn collect_surface_forwarding_sources(
        &self,
        star_surface: &FxHashSet<FileId>,
    ) -> FxHashSet<FileId> {
        let mut may_reach = star_surface.clone();
        let mut stack: Vec<FileId> = star_surface.iter().copied().collect();
        while let Some(barrel) = stack.pop() {
            let Some(module) = self.modules.get(barrel.0 as usize) else {
                continue;
            };
            for re in &module.re_exports {
                if !is_namespace_re_export(re) && may_reach.insert(re.source_file) {
                    stack.push(re.source_file);
                }
            }
        }
        may_reach
    }

    /// Whether a module on the entry-point star surface really exposes `name`
    /// on that surface.
    ///
    /// An entry point exposes every name it exports, `default` included: it is
    /// the public API. A barrel an entry point only reaches through plain
    /// `export *` exposes every name except `default`, which no plain
    /// `export *` forwards.
    fn surface_exposes(
        &self,
        star_surface: &FxHashSet<FileId>,
        file_id: FileId,
        name: &str,
    ) -> bool {
        star_surface.contains(&file_id) && (name != "default" || self.is_entry_point_file(file_id))
    }

    /// Whether `barrel` re-exports under `barrel_name` the very binding
    /// `source` exports under `source_name`.
    ///
    /// A local declaration on the barrel, or the same name arriving from two
    /// stars at once, shadows a star-forwarded name: the barrel then exports a
    /// different binding and the name never travels further. The value
    /// namespace decides whenever the source exports the name there (a
    /// namespace object is a value binding); a type-only surface falls back to
    /// the type namespace. Phase 2c's own walk applies the same rule through
    /// `uniquely_forwards_binding`.
    fn forwards_binding(
        &self,
        source: FileId,
        source_name: &str,
        barrel: FileId,
        barrel_name: &str,
    ) -> bool {
        for namespace in [super::ExportNamespace::Value, super::ExportNamespace::Type] {
            let super::EffectiveExportResolution::Unique(origin) =
                self.resolve_export(source, source_name, namespace)
            else {
                continue;
            };
            return matches!(
                self.resolve_export(barrel, barrel_name, namespace),
                super::EffectiveExportResolution::Unique(forwarded) if forwarded == origin
            );
        }
        false
    }

    /// Whether the file is an entry point of this graph.
    fn is_entry_point_file(&self, file_id: FileId) -> bool {
        self.modules
            .get(file_id.0 as usize)
            .is_some_and(super::types::ModuleNode::is_entry_point)
    }

    /// Extend `targets` with every module its members reach through plain
    /// `export *` chains, transitively.
    fn extend_plain_star_closure(&self, targets: &mut FxHashSet<FileId>) {
        let mut stack: Vec<FileId> = targets.iter().copied().collect();
        while let Some(file_id) = stack.pop() {
            let Some(module) = self.modules.get(file_id.0 as usize) else {
                continue;
            };
            for re in module
                .re_exports
                .iter()
                .filter(|re| re.imported_name == "*" && re.exported_name == "*")
            {
                if targets.insert(re.source_file) {
                    stack.push(re.source_file);
                }
            }
        }
    }

    /// Index every edge by its target file for fast star-propagation lookups.
    fn build_edges_by_target(&self) -> FxHashMap<FileId, Vec<usize>> {
        let mut edges_by_target: FxHashMap<FileId, Vec<usize>> = FxHashMap::default();
        for (idx, edge) in self.edges.iter().enumerate() {
            edges_by_target.entry(edge.target).or_default().push(idx);
        }
        edges_by_target
    }

    /// Run monotone propagation, revisiting only tuples affected by new state.
    fn run_re_export_fixpoint(&mut self, input: ReExportFixpointInput<'_>) {
        let ReExportFixpointInput {
            re_export_info,
            entry_star_targets,
            edges_by_target,
            module_by_id,
            reference_paths,
        } = input;
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
        let mut existing_refs: FxHashSet<RoutedReferenceKey> = FxHashSet::default();
        let mut synthetic_stubs: FxHashSet<(FileId, String, bool)> = FxHashSet::default();
        let binding_usage = ImportBindingUsageIndex::build(module_by_id);
        let mut declaration_routes = EffectiveDeclarationRouteCache::default();
        let mut scratch = NamedPropagationScratch::default();

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
                binding_usage: &binding_usage,
                effective_exports: &self.effective_exports,
                existing_refs: &mut existing_refs,
                synthetic_stubs: &mut synthetic_stubs,
                declaration_routes: &mut declaration_routes,
                scratch: &mut scratch,
                reference_paths,
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
                module_by_id,
                effective_exports: &self.effective_exports,
                reference_paths,
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
    /// interned reference paths that monotone propagation can add.
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
        let reference_additions = max_exports.saturating_mul(module_count).saturating_mul(2);
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
                binding_usage: context.binding_usage,
                effective_exports: context.effective_exports,
                barrel_id: entry.barrel,
                barrel_idx,
                source_id: entry.source,
                source_idx,
                entry_star_targets: context.entry_star_targets,
                triggering_is_type_only: entry.is_type_only,
                synthetic_stubs: context.synthetic_stubs,
                reference_paths: context.reference_paths,
            })
        } else {
            propagate_named_re_export(NamedReExportPropagation {
                modules,
                effective_exports: context.effective_exports,
                barrel_id: entry.barrel,
                barrel_idx,
                source_id: entry.source,
                source_idx,
                imported_name: &entry.imported_name,
                exported_name: &entry.exported_name,
                is_type_only: entry.is_type_only,
                existing_refs: context.existing_refs,
                declaration_routes: context.declaration_routes,
                scratch: context.scratch,
                reference_paths: context.reference_paths,
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
            module_by_id,
            effective_exports,
            reference_paths,
        } = input;
        let max_iterations = re_export_info.len().saturating_add(1);
        let mut existing_refs: FxHashSet<RoutedReferenceKey> = FxHashSet::default();
        let mut synthetic_stubs: FxHashSet<(FileId, String, bool)> = FxHashSet::default();
        let binding_usage = ImportBindingUsageIndex::build(module_by_id);
        let mut declaration_routes = EffectiveDeclarationRouteCache::default();
        let mut scratch = NamedPropagationScratch::default();

        for _ in 0..max_iterations {
            let mut changed = false;
            for entry in re_export_info {
                let mut context = ReExportContext {
                    entry_star_targets,
                    edges_by_target,
                    binding_usage: &binding_usage,
                    effective_exports,
                    existing_refs: &mut existing_refs,
                    synthetic_stubs: &mut synthetic_stubs,
                    declaration_routes: &mut declaration_routes,
                    scratch: &mut scratch,
                    reference_paths,
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
