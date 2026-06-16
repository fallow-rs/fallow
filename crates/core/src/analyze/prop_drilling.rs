//! Prop-drilling detection (Phase 3): a React/Preact prop forwarded unchanged
//! through `>= N` intermediate pass-through components until a component that
//! substantively consumes it.
//!
//! The high-confidence signal is "the received identifier is used ONLY as the
//! root of forwarded child-JSX attribute values" (`ComponentProp.used_in_script
//! && !used_outside_forward`), NOT the attribute name matching. This catches the
//! `<Child userName={user.name}/>` projection case while abstaining on
//! transforms.
//!
//! Pure static analysis (ADR-001): no type resolution. The chain walk resolves a
//! rendered child component name to its defining component via the same-file
//! component set first, then the resolved import map; an ambiguous or
//! unresolvable hop abstains the chain.
//!
//! Abstain set (any one drops the WHOLE chain, zero-FP doctrine):
//! - a JSX spread (`{...props}`) on any render edge in the path;
//! - `cloneElement` anywhere in a component on the path;
//! - an element-as-prop / render-prop / children-as-function forward;
//! - a `*.Provider` rendered in a component on the path (downgrade/abstain);
//! - a dynamic / computed attribute or component name (already absent at
//!   extraction);
//! - a child component that resolves ambiguously or not at all.
//!
//! Dormant by default: the `prop-drilling` rule defaults to `off`. The detector
//! runs only when the rule is enabled; the located per-chain records feed CI /
//! agents and a small capped health penalty.

use std::path::Path;

use rustc_hash::{FxHashMap, FxHashSet};

use fallow_types::extract::{ComponentFunction, ComponentProp, ModuleInfo, RenderEdge};

use crate::discover::FileId;
use crate::graph::ModuleGraph;
use crate::resolve::ResolvedModule;
use crate::results::{PropDrillHop, PropDrillingChain};

use super::react_resolve::{ChildResolver, CompKey};
use super::{LineOffsetsMap, byte_offset_to_line_col};

/// The minimum number of forwarding hops (source + pass-throughs + consumer) a
/// chain must span to be reported. N=3 = source -> 2 pass-throughs -> consumer.
const MIN_CHAIN_DEPTH: usize = 3;

/// A bound on the chain walk to terminate on pathological graphs (deep
/// component trees, mutual renders). A drilling chain longer than this is still
/// reported (capped at the bound), but the walk never loops.
const MAX_CHAIN_DEPTH: usize = 32;

/// The forward-vs-consume classification of a received prop in its component.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PropRole {
    /// Referenced ONLY as the root of forwarded child-JSX attribute values: a
    /// pure pass-through (the prop-drilling middle-of-chain signal).
    PassThrough,
    /// Referenced at least once outside a forwarded attribute value: a
    /// substantive consumption (the chain terminus).
    Consumer,
}

/// One component's prop-drilling-relevant state.
struct CompState<'a> {
    /// Source path (for the located hop records).
    path: &'a Path,
    /// The component definition span start (anchors the hop line).
    span_start: u32,
    /// `true` when ANY abstain signal is present on this component (clone-element,
    /// provider render, render-prop / children-as-function, an unharvestable
    /// props signature, or a spread on any of its render edges). A chain passing
    /// through an abstaining component is dropped.
    abstain: bool,
    /// Per-prop role (the prop LOCAL name -> role). Only props that are
    /// referenced (`used_in_script`) appear; unused props are not chain links.
    prop_roles: FxHashMap<String, PropRole>,
    /// The declared name and span of each harvested prop, keyed by local. Used
    /// for the source-hop anchor + the reported prop name.
    prop_decls: FxHashMap<String, PropDecl>,
    /// Forwarding edges: for each prop LOCAL forwarded by this component, the set
    /// of `(child_component_name, child_attr_name)` it is passed to.
    forwards: FxHashMap<String, Vec<ForwardTarget>>,
}

/// A declared prop: its written name and declaration span (the source anchor).
struct PropDecl {
    name: String,
    span_start: u32,
}

/// A forward target: the child component name (as written) and the attribute the
/// prop is passed as.
struct ForwardTarget {
    child_name: String,
    child_attr: String,
}

/// Result of the prop-drilling scan: the located chains plus the number of React
/// components inspected (for the observability diagnostic, so a silent
/// dep-gate / silent abstain is visible).
#[derive(Debug, Default)]
pub struct PropDrillingScan {
    /// One located chain per drilled prop.
    pub chains: Vec<PropDrillingChain>,
    /// React components inspected across all reachable JSX modules.
    pub components_scanned: usize,
}

/// Find prop-drilling chains. Returns an empty scan unless the project declares
/// `react` / `react-dom` / `next` / `preact`.
#[must_use]
pub fn find_prop_drilling_chains(
    graph: &ModuleGraph,
    modules: &[ModuleInfo],
    resolved_modules: &[ResolvedModule],
    declared_deps: &FxHashSet<String>,
    line_offsets_by_file: &LineOffsetsMap<'_>,
) -> PropDrillingScan {
    let gated = declared_deps.contains("react")
        || declared_deps.contains("react-dom")
        || declared_deps.contains("next")
        || declared_deps.contains("preact");
    if !gated {
        return PropDrillingScan::default();
    }

    let modules_by_id: FxHashMap<FileId, &ModuleInfo> =
        modules.iter().map(|m| (m.file_id, m)).collect();
    let resolved_by_id: FxHashMap<FileId, &ResolvedModule> =
        resolved_modules.iter().map(|m| (m.file_id, m)).collect();

    // Build per-component state across every reachable JSX module.
    let mut states: FxHashMap<CompKey, CompState<'_>> = FxHashMap::default();
    let mut components_scanned = 0usize;
    for node in &graph.modules {
        if !node.is_reachable() || !is_react_file(&node.path) {
            continue;
        }
        let Some(module) = modules_by_id.get(&node.file_id) else {
            continue;
        };
        if module.component_functions.is_empty() {
            continue;
        }
        components_scanned += module.component_functions.len();
        build_component_states(node.file_id, &node.path, module, &mut states);
    }

    // Resolve every render edge's child component name to a CompKey once, so the
    // chain walk is pure map lookups. The shared resolver builds
    // `components_per_file` from graph.modules + component_functions (the same
    // set the `states` map's keys are derived from), so the migration off the
    // old states-based constructor is behavior-preserving.
    let resolver = ChildResolver::new(graph, &modules_by_id, &resolved_by_id);

    let chains = walk_chains(&states, &resolver, line_offsets_by_file);

    PropDrillingScan {
        chains,
        components_scanned,
    }
}

/// Build the [`CompState`] for every component declared in one module.
fn build_component_states<'a>(
    file: FileId,
    path: &'a Path,
    module: &'a ModuleInfo,
    states: &mut FxHashMap<CompKey, CompState<'a>>,
) {
    // Index render edges + abstain flags by parent component.
    let mut edges_by_parent: FxHashMap<&str, Vec<&RenderEdge>> = FxHashMap::default();
    for edge in &module.render_edges {
        edges_by_parent
            .entry(edge.parent_component.as_str())
            .or_default()
            .push(edge);
    }
    // Index harvested props by component.
    let mut props_by_comp: FxHashMap<&str, Vec<&ComponentProp>> = FxHashMap::default();
    for prop in &module.react_props {
        props_by_comp
            .entry(prop.component.as_str())
            .or_default()
            .push(prop);
    }

    for func in &module.component_functions {
        let name = func.name.as_str();
        let edges = edges_by_parent.get(name).cloned().unwrap_or_default();
        let component_abstain = component_has_abstain(func, &edges);

        let mut prop_roles = FxHashMap::default();
        let mut prop_decls = FxHashMap::default();
        let mut forwards: FxHashMap<String, Vec<ForwardTarget>> = FxHashMap::default();

        if let Some(props) = props_by_comp.get(name) {
            for prop in props {
                if !prop.used_in_script {
                    // Unused props are not chain links (they are the
                    // `unused-component-prop` finding's concern, Phase 1).
                    continue;
                }
                let role = if prop.used_outside_forward {
                    PropRole::Consumer
                } else {
                    PropRole::PassThrough
                };
                prop_roles.insert(prop.local.clone(), role);
                prop_decls.insert(
                    prop.local.clone(),
                    PropDecl {
                        name: prop.name.clone(),
                        span_start: prop.span_start,
                    },
                );
            }
        }

        // Forward targets: each render edge's `forward_attrs` whose root is one of
        // this component's prop locals.
        for edge in &edges {
            for fa in &edge.forward_attrs {
                if prop_roles.contains_key(&fa.root) {
                    forwards
                        .entry(fa.root.clone())
                        .or_default()
                        .push(ForwardTarget {
                            child_name: edge.child_component_name.clone(),
                            child_attr: fa.attr.clone(),
                        });
                }
            }
        }

        states.insert(
            CompKey {
                file,
                name: name.to_string(),
            },
            CompState {
                path,
                span_start: func.span_start,
                abstain: component_abstain,
                prop_roles,
                prop_decls,
                forwards,
            },
        );
    }
}

/// Whether a component carries any prop-drilling abstain signal.
fn component_has_abstain(func: &ComponentFunction, edges: &[&RenderEdge]) -> bool {
    func.uses_clone_element
        || func.renders_provider
        || func.has_children_as_function
        || func.has_unharvestable_props
        || edges.iter().any(|e| e.has_spread || e.has_complex_forward)
}

/// Compute every `(child_key, child_local)` that is fed a pass-through forward by
/// a non-abstaining pass-through parent. Such a component is an INTERMEDIATE hop
/// of a longer chain, never a fresh source, so the walk skips it as an origin and
/// reports only the maximal chain.
fn non_maximal_origins(
    states: &FxHashMap<CompKey, CompState<'_>>,
    resolver: &ChildResolver<'_>,
) -> FxHashSet<(CompKey, String)> {
    let mut non_maximal: FxHashSet<(CompKey, String)> = FxHashSet::default();
    for (parent_key, parent_state) in states {
        if parent_state.abstain {
            continue;
        }
        for (parent_local, targets) in &parent_state.forwards {
            // Only a PASS-THROUGH parent prop extends a chain into its child; a
            // consumer prop that also happens to forward is a chain end.
            if parent_state.prop_roles.get(parent_local) != Some(&PropRole::PassThrough) {
                continue;
            }
            for target in targets {
                let Some(child_key) = resolver.resolve(parent_key.file, &target.child_name) else {
                    continue;
                };
                let Some(child_state) = states.get(&child_key) else {
                    continue;
                };
                if let Some(child_local) = local_for_attr(child_state, &target.child_attr) {
                    non_maximal.insert((child_key, child_local));
                }
            }
        }
    }
    non_maximal
}

/// Walk every chain origin and emit located chains of depth `>= MIN_CHAIN_DEPTH`.
fn walk_chains(
    states: &FxHashMap<CompKey, CompState<'_>>,
    resolver: &ChildResolver<'_>,
    line_offsets_by_file: &LineOffsetsMap<'_>,
) -> Vec<PropDrillingChain> {
    let mut chains: Vec<PropDrillingChain> = Vec::new();
    // Dedup identical chains (the same source prop can be reached via the dedup
    // of forward targets; keep one record per (source key, prop)).
    let mut seen: FxHashSet<(FileId, String, String)> = FxHashSet::default();

    // Only MAXIMAL chains are reported: an origin that is itself fed the prop by
    // a pass-through parent is an INTERMEDIATE hop of a longer chain, not a fresh
    // source. `non_maximal` holds every `(child_key, child_local)` that receives
    // a pass-through forward, so a sub-chain (e.g. Layout->Sidebar->Profile when
    // Page->Layout->... already exists) is not double-reported.
    let non_maximal = non_maximal_origins(states, resolver);

    for (key, state) in states {
        if state.abstain {
            continue;
        }
        // A chain ORIGIN is a pass-through prop on a NON-abstaining component:
        // the component owns the prop and only forwards it. (A consumer prop is a
        // chain terminus, not an origin; an unused prop is not a chain link.)
        for (local, role) in &state.prop_roles {
            if *role != PropRole::PassThrough {
                continue;
            }
            if non_maximal.contains(&(key.clone(), local.clone())) {
                continue;
            }
            let Some(decl) = state.prop_decls.get(local) else {
                continue;
            };
            let dedup_key = (key.file, key.name.clone(), decl.name.clone());
            if seen.contains(&dedup_key) {
                continue;
            }
            if let Some(hops) = follow_chain(key, local, states, resolver, line_offsets_by_file)
                && hops.len() >= MIN_CHAIN_DEPTH
            {
                seen.insert(dedup_key);
                let depth = u32::try_from(hops.len()).unwrap_or(u32::MAX);
                chains.push(PropDrillingChain {
                    prop: decl.name.clone(),
                    depth,
                    hops,
                });
            }
        }
    }

    chains.sort_by(|a, b| {
        let a_src = a.hops.first();
        let b_src = b.hops.first();
        a_src
            .map(|h| &h.file)
            .cmp(&b_src.map(|h| &h.file))
            .then_with(|| a_src.map(|h| h.line).cmp(&b_src.map(|h| h.line)))
            .then(a.prop.cmp(&b.prop))
    });
    chains
}

/// Follow a chain from `(key, local)` through pass-through hops to a consumer.
///
/// Returns the located hop trail (source first, consumer last) when the chain
/// terminates at a consumer through ONLY pass-through links, or `None` when it
/// abstains: an abstaining hop, an unresolvable / ambiguous child, a forward
/// that fans out to multiple distinct children (a join, not a single chain), a
/// child that does not receive the prop, or a cycle.
fn follow_chain(
    origin: &CompKey,
    origin_local: &str,
    states: &FxHashMap<CompKey, CompState<'_>>,
    resolver: &ChildResolver<'_>,
    line_offsets_by_file: &LineOffsetsMap<'_>,
) -> Option<Vec<PropDrillHop>> {
    let mut hops: Vec<PropDrillHop> = Vec::new();
    let mut visited: FxHashSet<CompKey> = FxHashSet::default();

    // The source hop anchors at the prop DECLARATION (so a jump-to-source lands
    // on the drilled prop), not the component definition.
    let origin_state = states.get(origin)?;
    let decl = origin_state.prop_decls.get(origin_local)?;
    hops.push(hop_at(
        origin,
        origin_state.path,
        decl.span_start,
        line_offsets_by_file,
    ));
    visited.insert(origin.clone());

    let mut current_key = origin.clone();
    let mut current_local = origin_local.to_string();

    loop {
        if hops.len() > MAX_CHAIN_DEPTH {
            // Pathological depth: stop (still a valid drilled chain, capped).
            return Some(hops);
        }
        let state = states.get(&current_key)?;
        // The current component must FORWARD this prop to exactly one child the
        // chain can follow. A forward to >= 2 distinct children is a fan-out (not
        // a single drilling line); abstain to stay high-confidence.
        let targets = state.forwards.get(&current_local)?;
        let resolved: Vec<(CompKey, &ForwardTarget)> = targets
            .iter()
            .filter_map(|t| {
                resolver
                    .resolve(current_key.file, &t.child_name)
                    .map(|k| (k, t))
            })
            .collect();
        // Every written target must resolve (an unresolvable hop abstains), and
        // they must all point at the SAME child receiving the SAME attr (a
        // genuine single-line forward, possibly written twice).
        if resolved.len() != targets.len() {
            return None;
        }
        let (child_key, target) = single_child(&resolved)?;
        if !visited.insert(child_key.clone()) {
            // Cycle: abstain.
            return None;
        }
        let child_state = states.get(&child_key)?;
        if child_state.abstain {
            return None;
        }
        // The child's prop LOCAL receiving this forward = the local whose declared
        // NAME equals the forwarded attribute name.
        let Some(child_local) = local_for_attr(child_state, &target.child_attr) else {
            // The child does not declare a harvestable prop for this attr (it may
            // spread, rest, or take an unharvestable signature). Abstain.
            return None;
        };
        let role = *child_state.prop_roles.get(&child_local)?;
        hops.push(hop_at(
            &child_key,
            child_state.path,
            child_state.span_start,
            line_offsets_by_file,
        ));
        match role {
            PropRole::Consumer => return Some(hops),
            PropRole::PassThrough => {
                current_key = child_key;
                current_local = child_local;
            }
        }
    }
}

/// Collapse a set of resolved forward targets to a single child when they all
/// point at the same `(child component, attr)`; `None` on a fan-out.
fn single_child<'t>(
    resolved: &'t [(CompKey, &'t ForwardTarget)],
) -> Option<(CompKey, &'t ForwardTarget)> {
    let (first_key, first_target) = resolved.first()?;
    if resolved
        .iter()
        .all(|(k, t)| k == first_key && t.child_attr == first_target.child_attr)
    {
        Some((first_key.clone(), first_target))
    } else {
        None
    }
}

/// The child component's prop LOCAL whose declared NAME equals `attr`.
fn local_for_attr(state: &CompState<'_>, attr: &str) -> Option<String> {
    state
        .prop_decls
        .iter()
        .find(|(_, decl)| decl.name == attr)
        .map(|(local, _)| local.clone())
}

/// Build a located hop record at a byte offset.
fn hop_at(
    key: &CompKey,
    path: &Path,
    span_start: u32,
    line_offsets_by_file: &LineOffsetsMap<'_>,
) -> PropDrillHop {
    let (line, _col) = byte_offset_to_line_col(line_offsets_by_file, key.file, span_start);
    PropDrillHop {
        file: path.to_path_buf(),
        line,
        component: key.name.clone(),
    }
}

/// Whether the path is a React/Preact JSX module (`.jsx` / `.tsx`).
fn is_react_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("jsx" | "tsx")
    )
}
