//! Thin-wrapper / passthrough-component detection: a React/Preact component
//! whose ENTIRE body is structural indirection. It returns exactly one
//! capitalized component element that forwards the component's own props via a
//! bare spread (`return <Child {...props}/>`), with no host wrapper, no extra
//! children, no named attributes, no hooks, no branching, and no other
//! statements. Such a component adds nothing: it is a candidate for inlining at
//! its call sites or deleting.
//!
//! Two-layer, mirroring the prop-drilling architecture: the extraction layer
//! sets `ComponentFunction.is_pure_passthrough` (proven from the component's own
//! AST, byte-identity-safe so it caches), and this cross-graph analyzer joins
//! that flag with a hook-density / cyclomatic belt-and-suspenders check and the
//! resolved single render edge to emit a located finding.
//!
//! Pure static analysis (ADR-001): no type resolution. The single child is
//! resolved to a defining component via the same-file component set first, then
//! the resolved import map; an unresolvable / ambiguous / member-expression
//! child abstains.
//!
//! Abstain set (zero-FP doctrine, each drops the wrapper):
//! - `forwardRef` / `memo` wrappers (`ForwardRefWrapper` / `MemoWrapper`): the
//!   sanctioned ways to make a child ref-able or to set a perf boundary. The
//!   single biggest legitimate-spread-forwarding class.
//! - exported components: a public-API re-brand / encapsulation indirection.
//! - context provider wrappers (`renders_provider`, or a `*.Provider` child).
//! - `cloneElement` (reflection injects props; the forward is not actually pure).
//! - render-prop / children-as-function (`has_children_as_function`).
//! - a complexity MISMATCH (the flag says passthrough but the function carries a
//!   hook or branching): abstain, never a wrong finding.
//! - a self-render (the single child resolves to / is named the wrapper itself).
//! - an unresolved / member-expression / `.Provider` child.
//!
//! Dormant by default: the `thin-wrapper` rule defaults to `off`. The detector
//! runs only when the rule is enabled. The finding is framed as a health
//! CANDIDATE ("inline at call sites or delete"), never a correctness error.

use std::path::Path;

use rustc_hash::{FxHashMap, FxHashSet};

use fallow_types::extract::{
    ComponentFunction, ComponentFunctionKind, FunctionComplexity, ModuleInfo, RenderEdge,
};

use crate::discover::FileId;
use crate::graph::ModuleGraph;
use crate::resolve::ResolvedModule;
use crate::results::ThinWrapper;

use super::react_resolve::ChildResolver;
use super::{LineOffsetsMap, byte_offset_to_line_col};

/// Result of the thin-wrapper scan: the located wrappers plus the number of
/// React components inspected (for the observability diagnostic, so a silent
/// dep-gate / silent abstain is visible).
#[derive(Debug, Default)]
pub struct ThinWrapperScan {
    /// One located record per thin wrapper.
    pub wrappers: Vec<ThinWrapper>,
    /// React components inspected across all reachable JSX modules.
    pub components_scanned: usize,
}

/// Find thin-wrapper / passthrough components. Returns an empty scan unless the
/// project declares `react` / `react-dom` / `next` / `preact`.
#[must_use]
pub fn find_thin_wrappers(
    graph: &ModuleGraph,
    modules: &[ModuleInfo],
    resolved_modules: &[ResolvedModule],
    declared_deps: &FxHashSet<String>,
    line_offsets_by_file: &LineOffsetsMap<'_>,
) -> ThinWrapperScan {
    let gated = declared_deps.contains("react")
        || declared_deps.contains("react-dom")
        || declared_deps.contains("next")
        || declared_deps.contains("preact");
    if !gated {
        return ThinWrapperScan::default();
    }

    let modules_by_id: FxHashMap<FileId, &ModuleInfo> =
        modules.iter().map(|m| (m.file_id, m)).collect();
    let resolved_by_id: FxHashMap<FileId, &ResolvedModule> =
        resolved_modules.iter().map(|m| (m.file_id, m)).collect();

    // Per-file set of component names + the sole-component map, for child
    // resolution. Built once over reachable React modules.
    let resolver = ChildResolver::new(graph, &modules_by_id, &resolved_by_id);

    let mut scan = ThinWrapperScan::default();
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
        scan.components_scanned += module.component_functions.len();

        // Index render edges by parent component name (a file may declare
        // several components).
        let mut edges_by_parent: FxHashMap<&str, Vec<&RenderEdge>> = FxHashMap::default();
        for edge in &module.render_edges {
            edges_by_parent
                .entry(edge.parent_component.as_str())
                .or_default()
                .push(edge);
        }
        // Complexity is gated by `need_complexity` and is absent in the dead-code
        // pipeline, so the cyclomatic belt below is enforced ONLY when an entry
        // exists (health / audit). The hook-density belt uses `hook_uses`, which
        // the React structural walk always populates regardless of complexity.
        let complexity_by_name: FxHashMap<&str, &FunctionComplexity> = module
            .complexity
            .iter()
            .map(|c| (c.name.as_str(), c))
            .collect();
        let hook_counts = component_hook_counts(module);

        let ctx = FileContext {
            file: node.file_id,
            path: &node.path,
            edges_by_parent: &edges_by_parent,
            complexity_by_name: &complexity_by_name,
            hook_counts: &hook_counts,
        };
        for func in &module.component_functions {
            if let Some(wrapper) =
                classify_thin_wrapper(func, &ctx, &resolver, line_offsets_by_file)
            {
                scan.wrappers.push(wrapper);
            }
        }
    }

    scan.wrappers.sort_by(|a, b| {
        a.file
            .cmp(&b.file)
            .then(a.line.cmp(&b.line))
            .then(a.component.cmp(&b.component))
    });
    scan
}

/// Count, per component (keyed by its `span_start`), the React hook calls that
/// fall inside that component's body. Each hook is attributed to the component
/// with the largest `span_start` not exceeding the hook's offset (the innermost
/// enclosing component in source order). Used as the always-available
/// hook-density belt, since `ComponentFunction` carries no `span_end`.
fn component_hook_counts(module: &ModuleInfo) -> FxHashMap<u32, usize> {
    let mut counts: FxHashMap<u32, usize> = FxHashMap::default();
    if module.hook_uses.is_empty() {
        return counts;
    }
    let mut starts: Vec<u32> = module
        .component_functions
        .iter()
        .map(|c| c.span_start)
        .collect();
    starts.sort_unstable();
    for hook in &module.hook_uses {
        // The owning component is the one with the largest start <= hook offset.
        let owner = starts
            .partition_point(|&s| s <= hook.span_start)
            .checked_sub(1)
            .map(|idx| starts[idx]);
        if let Some(owner) = owner {
            *counts.entry(owner).or_insert(0) += 1;
        }
    }
    counts
}

/// The per-file lookup state the classifier needs: the file id + path plus the
/// render-edge / complexity / hook-count indices built once per module. Bundled
/// to keep the classifier's argument count within the unit-interfacing ceiling.
struct FileContext<'a> {
    file: FileId,
    path: &'a Path,
    edges_by_parent: &'a FxHashMap<&'a str, Vec<&'a RenderEdge>>,
    complexity_by_name: &'a FxHashMap<&'a str, &'a FunctionComplexity>,
    hook_counts: &'a FxHashMap<u32, usize>,
}

/// Apply the full abstain ladder to one component and emit a located
/// [`ThinWrapper`] when it is a proven pure passthrough that resolves to a real,
/// distinct child component. `None` on any abstain.
fn classify_thin_wrapper(
    func: &ComponentFunction,
    ctx: &FileContext<'_>,
    resolver: &ChildResolver<'_>,
    line_offsets_by_file: &LineOffsetsMap<'_>,
) -> Option<ThinWrapper> {
    // The extraction flag is the necessary precondition: the body is exactly a
    // bare spread-forwarded single child element (proven per-file).
    if !func.is_pure_passthrough {
        return None;
    }

    // forwardRef / memo wrappers are sanctioned indirection: ALWAYS abstain.
    if matches!(
        func.kind,
        ComponentFunctionKind::ForwardRefWrapper | ComponentFunctionKind::MemoWrapper
    ) {
        return None;
    }
    // Exported components are public-API indirection (re-brand / encapsulation).
    if func.is_exported {
        return None;
    }
    // Context provider wrappers add value even when they spread props through.
    if func.renders_provider {
        return None;
    }
    // cloneElement reflection / render-prop forwards are not pure.
    if func.uses_clone_element || func.has_children_as_function {
        return None;
    }

    let name = func.name.as_str();

    // Belt-and-suspenders hook-density check: a pure passthrough has ZERO hooks.
    // `hook_uses` is always populated by the React structural walk (independent
    // of `need_complexity`), so this belt runs in every pipeline. A MISMATCH
    // (the extraction flag says passthrough but a hook falls in the component
    // body) is treated as an ABSTAIN, not a finding, so a future extraction bug
    // surfaces as a silent false-negative rather than a wrong finding.
    if ctx.hook_counts.get(&func.span_start).copied().unwrap_or(0) != 0 {
        tracing::debug!(
            component = name,
            "thin-wrapper: is_pure_passthrough set but the component owns a hook; abstaining"
        );
        return None;
    }
    // Belt-and-suspenders cyclomatic check, enforced ONLY when a complexity
    // entry exists (the dead-code pipeline runs without it; the extraction proof
    // already guarantees cyclomatic 1 via the single-return / no-branch shape).
    // When present, a cyclomatic != 1 mismatch ABSTAINS.
    if let Some(complexity) = ctx.complexity_by_name.get(name)
        && (complexity.cyclomatic != 1 || complexity.react_hook_count != 0)
    {
        tracing::debug!(
            component = name,
            cyclomatic = complexity.cyclomatic,
            react_hook_count = complexity.react_hook_count,
            "thin-wrapper: is_pure_passthrough disagrees with complexity join; abstaining"
        );
        return None;
    }

    // The component must have exactly one render edge (the single forwarded
    // child). The extraction flag already proved the return shape is a single
    // element, so a different edge count means the body had additional renders
    // the flag did not account for: abstain.
    let edges = ctx.edges_by_parent.get(name)?;
    let [edge] = edges.as_slice() else {
        return None;
    };
    let child_name = edge.child_component_name.as_str();

    // A member-expression tag (`Foo.Bar`) or `*.Provider` child is unprovable /
    // a provider; abstain.
    if child_name.contains('.') {
        return None;
    }
    // Self-render guard: a wrapper whose single child is named the wrapper itself
    // is infinite recursion / an HOC-result shadow, never a thin wrapper.
    if child_name == name {
        return None;
    }
    // The child must resolve to a real component (same-file or via the import
    // map) OR be a known imported binding; an unresolvable child abstains.
    let resolved = resolver.resolve(ctx.file, child_name);
    if resolved.is_none() && !resolver.is_imported_binding(ctx.file, child_name) {
        return None;
    }
    // A resolved child that is the wrapper itself (cross-file self-shadow) also
    // abstains.
    if let Some(target) = resolved
        && target.file == ctx.file
        && target.name == name
    {
        return None;
    }

    let (line, _col) = byte_offset_to_line_col(line_offsets_by_file, ctx.file, func.span_start);
    Some(ThinWrapper {
        file: ctx.path.to_path_buf(),
        line,
        component: name.to_string(),
        child_component: child_name.to_string(),
    })
}

/// Whether the path is a React/Preact JSX module (`.jsx` / `.tsx`).
fn is_react_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("jsx" | "tsx")
    )
}
