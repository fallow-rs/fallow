//! React component intelligence: a DESCRIPTIVE per-component summary of render
//! sites, props, and hooks, surfaced as ambient editor context (an LSP code
//! lens above each component plus per-prop hovers).
//!
//! NOT a rule, finding, IssueKind, severity, or `total_issues` input. It is the
//! component-level sibling of the descriptive [`compute_render_fan_in`] metric:
//! both read the already-extracted React IR (`component_functions`,
//! `react_props`, `hook_uses`, `render_edges`) and derive per-component context
//! the editor surfaces. The result rides the `#[serde(skip)]`
//! `AnalysisResults::react_component_intel` carrier (in-process LSP only), so
//! bare `fallow` / `audit` and the JSON / schema surface are untouched.
//!
//! Counts are HONEST (the render-fan-in incident lesson): test / spec / story /
//! fixture render sites are EXCLUDED from `render_sites`, `distinct_parents`,
//! and per-prop `passed_from_sites`, and `distinct_parents` (not the
//! repeat-inflated `render_sites`) is the headline. Resolution reuses the shared
//! [`ChildResolver`], so a member-expression tag, a spread-only / dynamic child,
//! or an unresolved import resolves to `None` and credits nothing (undercount,
//! the safe direction).
//!
//! [`compute_render_fan_in`]: super::render_fan_in::compute_render_fan_in

use std::path::Path;

use rustc_hash::{FxHashMap, FxHashSet};

use fallow_types::extract::{HookUseKind, ModuleInfo};

use crate::discover::FileId;
use crate::graph::ModuleGraph;
use crate::resolve::ResolvedModule;
use crate::results::{ReactComponentIntel, ReactHookSummary, ReactPropIntel};

use super::react_resolve::{ChildResolver, CompKey};
use super::{LineOffsetsMap, byte_offset_to_line_col};

/// Compute the per-component React intelligence for every reachable React
/// component. Returns an empty vec unless the project declares `react` /
/// `react-dom` / `next` / `preact` (the same dep gate the other React analyzers
/// use); non-React projects compute nothing.
#[must_use]
pub fn compute_react_component_intel(
    graph: &ModuleGraph,
    modules: &[ModuleInfo],
    resolved_modules: &[ResolvedModule],
    declared_deps: &FxHashSet<String>,
    root: &Path,
    line_offsets_by_file: &LineOffsetsMap<'_>,
) -> Vec<ReactComponentIntel> {
    if !project_declares_react(declared_deps) {
        return Vec::new();
    }

    let modules_by_id: FxHashMap<FileId, &ModuleInfo> =
        modules.iter().map(|m| (m.file_id, m)).collect();
    let resolved_by_id: FxHashMap<FileId, &ResolvedModule> =
        resolved_modules.iter().map(|m| (m.file_id, m)).collect();
    let resolver = ChildResolver::new(graph, &modules_by_id, &resolved_by_id);

    // Per-component render aggregation (test/spec/story render sites excluded),
    // plus the per-prop pass-count map keyed by `(child_key, prop_name)`.
    let render = aggregate_render_edges(graph, &modules_by_id, &resolver, root);

    let mut intel = Vec::new();
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
        collect_module_intel(
            node.file_id,
            node.path.as_path(),
            module,
            &render,
            line_offsets_by_file,
            &mut intel,
        );
    }

    // Deterministic ordering (path, component) so the carrier is stable across
    // runs (FxHashMap iteration order is not).
    intel.sort_by(|a, b| {
        a.path
            .cmp(&b.path)
            .then_with(|| a.component_name.cmp(&b.component_name))
    });
    intel
}

fn project_declares_react(declared_deps: &FxHashSet<String>) -> bool {
    declared_deps.contains("react")
        || declared_deps.contains("react-dom")
        || declared_deps.contains("next")
        || declared_deps.contains("preact")
}

/// Per-component render aggregation: total sites, distinct parents, and the
/// per-prop pass counts, all keyed by the resolved child [`CompKey`].
struct RenderAggregate {
    /// `child_key -> (render_sites, distinct_parent_keys)`.
    per_component: FxHashMap<CompKey, RenderAccum>,
    /// `(child_key, prop_name) -> count of NON-test render sites passing it`.
    prop_passes: FxHashMap<(CompKey, String), u32>,
}

#[derive(Default)]
struct RenderAccum {
    render_sites: u32,
    parents: FxHashSet<(FileId, String)>,
}

/// Walk every render edge, resolve its child, and credit one render site + the
/// distinct parent key + each passed attribute name. Test / spec / story /
/// fixture PARENT files are skipped wholesale so test-local render loops never
/// inflate any component's headline (the render-fan-in incident lesson).
fn aggregate_render_edges(
    graph: &ModuleGraph,
    modules_by_id: &FxHashMap<FileId, &ModuleInfo>,
    resolver: &ChildResolver<'_>,
    root: &Path,
) -> RenderAggregate {
    let mut per_component: FxHashMap<CompKey, RenderAccum> = FxHashMap::default();
    let mut prop_passes: FxHashMap<(CompKey, String), u32> = FxHashMap::default();

    for node in &graph.modules {
        if !node.is_reachable() || !is_react_file(&node.path) {
            continue;
        }
        // A render SITE whose PARENT file is a test/spec/story/fixture file must
        // not count toward render_sites, distinct_parents, or pass counts.
        if is_project_test_path(&node.path, root) {
            continue;
        }
        let Some(module) = modules_by_id.get(&node.file_id) else {
            continue;
        };
        for edge in &module.render_edges {
            let Some(child_key) = resolver.resolve(node.file_id, &edge.child_component_name) else {
                // Spread-only / dynamic / member-expression child: undercount.
                continue;
            };
            let accum = per_component.entry(child_key.clone()).or_default();
            accum.render_sites += 1;
            accum
                .parents
                .insert((node.file_id, edge.parent_component.clone()));
            for attr in &edge.attr_names {
                *prop_passes
                    .entry((child_key.clone(), attr.clone()))
                    .or_default() += 1;
            }
        }
    }

    RenderAggregate {
        per_component,
        prop_passes,
    }
}

fn collect_module_intel(
    file_id: FileId,
    path: &Path,
    module: &ModuleInfo,
    render: &RenderAggregate,
    line_offsets_by_file: &LineOffsetsMap<'_>,
    intel: &mut Vec<ReactComponentIntel>,
) {
    for component in &module.component_functions {
        let key = CompKey {
            file: file_id,
            name: component.name.clone(),
        };
        let (render_sites, distinct_parents) =
            render.per_component.get(&key).map_or((0, 0), |accum| {
                (
                    accum.render_sites,
                    u32::try_from(accum.parents.len()).unwrap_or(u32::MAX),
                )
            });

        let props = collect_component_props(
            &key,
            component.name.as_str(),
            module,
            render,
            file_id,
            line_offsets_by_file,
        );

        let hooks = summarize_hooks(component.name.as_str(), module);
        let (anchor_line, anchor_col) =
            byte_offset_to_line_col(line_offsets_by_file, file_id, component.span_start);

        intel.push(ReactComponentIntel {
            path: path.to_path_buf(),
            component_name: component.name.clone(),
            anchor_line,
            anchor_col,
            render_sites,
            distinct_parents,
            prop_count: u16::try_from(
                module
                    .react_props
                    .iter()
                    .filter(|p| p.component == component.name)
                    .count(),
            )
            .unwrap_or(u16::MAX),
            hooks,
            props,
        });
    }
}

fn collect_component_props(
    key: &CompKey,
    component_name: &str,
    module: &ModuleInfo,
    render: &RenderAggregate,
    file_id: FileId,
    line_offsets_by_file: &LineOffsetsMap<'_>,
) -> Vec<ReactPropIntel> {
    module
        .react_props
        .iter()
        .filter(|p| p.component == component_name)
        .map(|prop| {
            let passed_from_sites = render
                .prop_passes
                .get(&(key.clone(), prop.name.clone()))
                .copied()
                .unwrap_or(0);
            let (anchor_line, anchor_col) =
                byte_offset_to_line_col(line_offsets_by_file, file_id, prop.span_start);
            ReactPropIntel {
                name: prop.name.clone(),
                anchor_line,
                anchor_col,
                // React arm: `used_in_script` is the set-in-body signal.
                used_in_body: prop.used_in_script,
                passed_from_sites,
            }
        })
        .collect()
}

/// Count `hook_uses` belonging to `component_name` into a per-kind summary.
/// Hook uses carry no enclosing-component name in the IR, so a file with one
/// component attributes every hook to it; a file with several components shares
/// the file's hook uses across them. This is descriptive context, not a gated
/// finding, so the over-attribution in the multi-component-per-file case is
/// acceptable (it never produces a false positive, there being no positive).
fn summarize_hooks(component_name: &str, module: &ModuleInfo) -> ReactHookSummary {
    // When the file declares exactly one component, every hook is its. When it
    // declares several, the hook IR has no per-component attribution, so the
    // summary is left empty rather than attributing a hook to the wrong
    // component (undercount, the honest direction).
    let single_component = module.component_functions.len() == 1
        && module.component_functions[0].name == component_name;
    if !single_component {
        return ReactHookSummary::default();
    }

    let mut summary = ReactHookSummary::default();
    for hook in &module.hook_uses {
        match hook.kind {
            HookUseKind::UseState => summary.state = summary.state.saturating_add(1),
            HookUseKind::UseEffect => summary.effect = summary.effect.saturating_add(1),
            HookUseKind::UseMemo => summary.memo = summary.memo.saturating_add(1),
            HookUseKind::UseCallback => summary.callback = summary.callback.saturating_add(1),
            HookUseKind::Custom => summary.custom = summary.custom.saturating_add(1),
        }
    }
    summary
}

// The test-file predicate must run on the project-relative path, not the
// absolute node path (mirrors `render_fan_in::is_project_test_path`).
fn is_project_test_path(path: &Path, root: &Path) -> bool {
    let rel = path.strip_prefix(root).unwrap_or(path);
    super::predicates::is_test_or_spec_file(rel)
}

/// Whether the path is a React/Preact JSX module (`.jsx` / `.tsx`).
fn is_react_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("jsx" | "tsx")
    )
}
