//! Component render fan-in: a DESCRIPTIVE blast-radius signal counting how many
//! JSX render SITES (and distinct parent components) render a given component
//! across the whole project.
//!
//! This is the COMPONENT-graph analogue of the module-level fan-in /
//! coupling-concentration metric (`FileHealthScore.fan_in` counts importing
//! MODULES; render fan-in counts JSX render CALL SITES). It is NOVEL because a
//! shared `<Button>` is rendered in far more places than it is imported: a single
//! parent that renders `<Button>` five times is five edit-ripple points, which
//! distinct-parents and distinct-files both collapse to one.
//!
//! NOT a rule, finding, IssueKind, threshold, or severity. It extends the
//! existing hotspot/coupling MACHINERY (a computed metric + a percentile,
//! surfaced as context), mirroring `compute_coupling_concentration`.
//!
//! Mechanism (no new IR, no CACHE bump): derive from the already-extracted
//! `module.render_edges`. Each `RenderEdge` is exactly one JSX render site
//! (`crates/extract/src/visitor/react.rs` pushes one edge per capitalized /
//! member JSX tag). Resolve each edge's `child_component_name` to a concrete
//! component via the shared [`ChildResolver`] and bucket the incoming count per
//! component. Gated on `react` / `react-dom` / `next` / `preact`; non-React
//! projects compute nothing (the gate fails AND `render_edges` is empty).
//!
//! UNDERCOUNT is the documented safe direction: a child rendered via a JSX
//! spread, a dynamic `createElement(var)`, or a member-expression tag
//! (`<Lib.Button/>`) resolves to `None` and increments no component's fan-in. A
//! true high-fan-in component can only be undersold, never falsely flagged.

use rustc_hash::{FxHashMap, FxHashSet};

use fallow_types::extract::ModuleInfo;

use crate::discover::FileId;
use crate::graph::ModuleGraph;
use crate::resolve::ResolvedModule;
use crate::results::{RenderFanInComponent, RenderFanInMetric};

use super::react_resolve::{ChildResolver, CompKey};

/// The same concentration floor `compute_coupling_concentration` uses
/// (`vital_signs.rs`): the high-fan-in threshold is `max(p95, FLOOR)`, so there
/// is NO new tunable constant beyond the one coupling already carries.
const CONCENTRATION_FLOOR: u32 = 10;

/// Compute the project-wide render fan-in metric. Returns `None` unless the
/// project declares `react` / `react-dom` / `next` / `preact` (the same dep gate
/// the other React analyzers use); non-React projects compute nothing.
#[must_use]
pub fn compute_render_fan_in(
    graph: &ModuleGraph,
    modules: &[ModuleInfo],
    resolved_modules: &[ResolvedModule],
    declared_deps: &FxHashSet<String>,
    root: &std::path::Path,
) -> Option<RenderFanInMetric> {
    let gated = declared_deps.contains("react")
        || declared_deps.contains("react-dom")
        || declared_deps.contains("next")
        || declared_deps.contains("preact");
    if !gated {
        return None;
    }

    // The test-file predicate must run on the PROJECT-RELATIVE path, not the
    // absolute node path: the project root itself can contain a `tests/`
    // segment (fallow's own fixtures live under `tests/fixtures/...`), and the
    // `**/test/**` / `**/tests/**` globs anchor at any segment, so an absolute
    // path would match every file. Strip the root first; fall back to the full
    // path when the file is not under it (defensive, never expected).
    let is_test_anchor = |path: &std::path::Path| -> bool {
        let rel = path.strip_prefix(root).unwrap_or(path);
        super::predicates::is_test_or_spec_file(rel)
    };

    let modules_by_id: FxHashMap<FileId, &ModuleInfo> =
        modules.iter().map(|m| (m.file_id, m)).collect();
    let resolved_by_id: FxHashMap<FileId, &ResolvedModule> =
        resolved_modules.iter().map(|m| (m.file_id, m)).collect();

    let resolver = ChildResolver::new(graph, &modules_by_id, &resolved_by_id);

    // Seed every declared component with a zero entry so a component rendered
    // nowhere is a REAL 0 in the distribution (not absent), and so the
    // per-component path-annotation map has an entry for every component the
    // hotspot surface might query.
    let mut counts: FxHashMap<CompKey, FanInAccum> = FxHashMap::default();
    for node in &graph.modules {
        if !node.is_reachable() || !is_react_file(&node.path) {
            continue;
        }
        // A component DEFINED in a test/spec file must not be a fan-in target
        // (its headline is test-local noise, dominated by render loops in the
        // test body). Excluded from the seeded population so the percentile
        // distribution and the top-N are over non-test components only.
        if is_test_anchor(&node.path) {
            continue;
        }
        let Some(module) = modules_by_id.get(&node.file_id) else {
            continue;
        };
        for func in &module.component_functions {
            counts
                .entry(CompKey {
                    file: node.file_id,
                    name: func.name.clone(),
                })
                .or_default();
        }
    }

    // Walk every render edge, resolve its child to a concrete component, and
    // credit one render SITE plus the distinct parent key. The parent key is
    // `(parent_file, parent_component)`; an edge with an empty parent (a
    // top-level render expression) still counts as a render site, keyed to the
    // module file with an empty parent name so it collapses correctly.
    for node in &graph.modules {
        if !node.is_reachable() || !is_react_file(&node.path) {
            continue;
        }
        // A render SITE whose PARENT file is a test/spec file must not count: a
        // test that renders `<Page>` 146 times is not real blast radius. Skip
        // the whole module's render edges so test-local render loops never
        // inflate any component's fan-in.
        if is_test_anchor(&node.path) {
            continue;
        }
        let Some(module) = modules_by_id.get(&node.file_id) else {
            continue;
        };
        for edge in &module.render_edges {
            let Some(child_key) = resolver.resolve(node.file_id, &edge.child_component_name) else {
                // Spread-only / dynamic / member-expression child: undercount
                // (the safe direction). This site credits nothing.
                continue;
            };
            let accum = counts.entry(child_key).or_default();
            accum.render_sites += 1;
            accum
                .parents
                .insert((node.file_id, edge.parent_component.clone()));
        }
    }

    // Build the per-component detail (keyed back to file paths for the hotspot
    // surface) and the distinct-parents distribution for the percentile.
    let path_by_id: FxHashMap<FileId, &std::path::Path> = graph
        .modules
        .iter()
        .map(|node| (node.file_id, node.path.as_path()))
        .collect();

    let mut per_component: Vec<RenderFanInComponent> = Vec::with_capacity(counts.len());
    let mut distinct_parents_dist: Vec<u32> = Vec::with_capacity(counts.len());
    let mut max_distinct_parents: u32 = 0;
    for (key, accum) in &counts {
        let Some(path) = path_by_id.get(&key.file) else {
            continue;
        };
        let distinct_parents = u32::try_from(accum.parents.len()).unwrap_or(u32::MAX);
        // The HEADLINE is the max DISTINCT-PARENTS (honest blast radius = how many
        // distinct render LOCATIONS edit-ripple to), not the max render_sites
        // (which is inflated by a single parent rendering one child in a loop).
        max_distinct_parents = max_distinct_parents.max(distinct_parents);
        distinct_parents_dist.push(distinct_parents);
        per_component.push(RenderFanInComponent {
            file: (*path).to_path_buf(),
            component: key.name.clone(),
            render_sites: accum.render_sites,
            distinct_parents,
        });
    }

    if per_component.is_empty() {
        // React declared but no inspectable components: nothing to surface.
        return None;
    }

    // Deterministic ordering (path, component) so the carrier is stable across
    // runs (FxHashMap iteration order is not).
    per_component.sort_by(|a, b| {
        a.file
            .cmp(&b.file)
            .then_with(|| a.component.cmp(&b.component))
    });

    let (p95_distinct_parents, high_pct) = concentration(&distinct_parents_dist);

    Some(RenderFanInMetric {
        per_component,
        p95_distinct_parents,
        high_pct,
        max_distinct_parents: Some(max_distinct_parents),
    })
}

/// Running accumulator per component during the edge walk.
#[derive(Default)]
struct FanInAccum {
    /// Total JSX render SITES (one per resolved render edge).
    render_sites: u32,
    /// Distinct `(parent_file, parent_component)` keys rendering this component.
    parents: FxHashSet<(FileId, String)>,
}

/// Compute the distinct-parents concentration: `(p95, high_pct)`, mirroring
/// `compute_coupling_concentration` in `crates/cli/src/vital_signs.rs` verbatim
/// (p95 over the per-component distinct-parents distribution; `high_pct` = the
/// percent of components above the `max(p95, FLOOR)` threshold). `(None, None)`
/// on an empty population (the percentile-on-tiny-population caveat coupling
/// already carries). A singleton population is fine (its single value is the
/// p95).
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    reason = "distinct-parents values are bounded by project size; mirrors compute_coupling_concentration"
)]
fn concentration(distinct_parents: &[u32]) -> (Option<u32>, Option<f64>) {
    if distinct_parents.is_empty() {
        return (None, None);
    }
    let mut sorted: Vec<u32> = distinct_parents.to_vec();
    sorted.sort_unstable();
    let idx = (sorted.len() as f64 * 0.95).ceil() as usize;
    let idx = idx.min(sorted.len()) - 1;
    let p95 = sorted[idx];

    let threshold = p95.max(CONCENTRATION_FLOOR);
    let high_count = sorted.iter().filter(|&&fi| fi > threshold).count();
    let high_pct = (high_count as f64 / sorted.len() as f64 * 1000.0).round() / 10.0;

    (Some(p95), Some(high_pct))
}

/// Whether the path is a React/Preact JSX module (`.jsx` / `.tsx`).
fn is_react_file(path: &std::path::Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("jsx" | "tsx")
    )
}
