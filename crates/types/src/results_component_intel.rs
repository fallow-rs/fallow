//! Render fan-in counts and the React hook, prop drill and component
//! summaries of a run.

use super::*;

/// Per-component render fan-in counts plus the precomputed concentration aggregates.
///
/// DESCRIPTIVE blast-radius signal (NOT a rule, finding, or threshold): the
/// component-graph analogue of module-level fan-in. Module fan-in counts
/// importing MODULES; render fan-in counts JSX render CALL SITES (a shared
/// `<Button>` is rendered in far more places than it is imported).
///
/// `per_component` is the internal carrier (keyed for hotspot path annotation),
/// `#[serde(skip)]` on [`AnalysisResults`] so it never appears under bare
/// `fallow` / `audit`; the aggregates feed the descriptive `VitalSigns` block
/// (`p95_render_fan_in` / `render_fan_in_high_pct` / `max_render_fan_in`).
///
/// UNDERCOUNT is the documented safe direction: a child rendered via a JSX
/// spread, a dynamic / `createElement(var)` form, or a member-expression tag
/// (`<Lib.Button/>`) is not resolved by the shared `ChildResolver` and so
/// increments no component's fan-in. A true high-fan-in component can only be
/// undersold, never falsely flagged. A rare name-collision over-credit is
/// possible via the default-import sole-component fallback (inherited verbatim
/// from the prop-drilling / thin-wrapper resolver); low-harm for a descriptive,
/// non-gating metric.
#[derive(Debug, Clone, Default)]
pub struct RenderFanInMetric {
    /// Per-component render-site + distinct-parent counts. Keyed by
    /// `(component file path, component name)` so the hotspot surface can map a
    /// file back to its top component's fan-in. Components rendered nowhere ARE
    /// included as a real `0` so the percentile distribution is not skewed.
    pub per_component: Vec<RenderFanInComponent>,
    /// 95th-percentile DISTINCT-PARENTS render fan-in across components (the
    /// per-component distribution analogue of the module-fan-in p95). `None` on
    /// an empty population. Mirrors `compute_coupling_concentration`.
    pub p95_distinct_parents: Option<u32>,
    /// Percentage of components whose distinct-parents render fan-in exceeds the
    /// `max(p95, 10)` threshold (the same floor coupling concentration uses).
    /// `None` on an empty population.
    pub high_pct: Option<f64>,
    /// The single highest distinct-parent count across all components (the
    /// headline blast-radius number: the most distinct render locations any one
    /// component is rendered from, the honest edit-ripple count). `None` on an
    /// empty population. `render_sites` (incl. repeats) is secondary per-component
    /// context, never the headline.
    pub max_distinct_parents: Option<u32>,
}

/// One component's render fan-in detail: how many JSX render SITES target it and
/// how many DISTINCT parent components render it.
#[derive(Debug, Clone)]
pub struct RenderFanInComponent {
    /// Absolute path of the file declaring the component.
    pub file: PathBuf,
    /// The component name.
    pub component: String,
    /// Total JSX render sites that resolve to this component across the project
    /// (each capitalized / member JSX tag is one site). SECONDARY context ("incl.
    /// repeats"): a single parent rendering one child five times is five sites but
    /// one distinct parent, so render_sites overcounts blast radius.
    pub render_sites: u32,
    /// Distinct `(parent_file, parent_component)` keys that render this
    /// component. The HEADLINE blast-radius axis: the honest count of distinct
    /// render LOCATIONS, the percentiled distribution analogue of "distinct importers".
    pub distinct_parents: u32,
}

/// Per-kind hook counts for a React component, summarized from `hook_uses`.
/// DESCRIPTIVE editor context (the LSP code-lens hook breakdown), never a
/// finding, severity, or `total_issues` input. `custom` collects every
/// `use*`-named call that is not one of the four built-ins.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReactHookSummary {
    /// `useState(...)` call count.
    pub state: u16,
    /// `useEffect(...)` call count.
    pub effect: u16,
    /// `useMemo(...)` call count.
    pub memo: u16,
    /// `useCallback(...)` call count.
    pub callback: u16,
    /// Count of any other `use*`-named call (a custom hook).
    pub custom: u16,
}

/// A prop-drilling trace for a prop at the ROOT of a forwarding chain.
/// DESCRIPTIVE ambient editor context (the LSP per-prop hover): the prop is
/// forwarded unchanged through `depth` components before a component
/// substantively consumes it. Reuses the `prop-drilling` chain machinery's
/// abstain ladder (spread / `cloneElement` / dynamic / provider-in-subtree drop
/// the whole chain), so the trace is honest. NOT a finding (the opt-in
/// `prop-drilling` rule owns the finding); this rides the `#[serde(skip)]`
/// `ReactComponentIntel` carrier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReactPropDrill {
    /// The chain depth = number of components the prop is forwarded THROUGH
    /// (source + intermediates + consumer), matching `PropDrillingChain.depth`.
    pub depth: u32,
    /// The ordered component names from source to consumer (`hops[0]` owns the
    /// prop, the last consumes it).
    pub hops: Vec<String>,
}

/// Per-prop usage intelligence for one React component prop. DESCRIPTIVE editor
/// context (the LSP per-prop hover): whether the prop is read in the component
/// body and how many render sites pass it. NOT a finding (the
/// `unused-component-prop` React arm owns the deadness rule); this is ambient
/// signal. `anchor_line` / `anchor_col` follow the same convention the React
/// `unused-component-prop` findings use (1-based line, byte-derived col from
/// `byte_offset_to_line_col`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReactPropIntel {
    /// The declared prop name.
    pub name: String,
    /// 1-based line of the prop declaration (anchors the hover).
    pub anchor_line: u32,
    /// Column of the prop declaration (byte-derived, matching the React
    /// `unused-component-prop` finding convention).
    pub anchor_col: u32,
    /// Whether the prop is referenced in the component body (`used_in_script`
    /// for the React arm: a resolved reference to the destructured local).
    pub used_in_body: bool,
    /// Count of render sites (test/spec/story/fixture files excluded) whose
    /// passed-attribute set contains this prop name.
    pub passed_from_sites: u32,
    /// A prop-drilling trace, present only when this prop is the ROOT of a
    /// forwarding chain that reaches a consumer through `>= N` pass-through
    /// components. `None` for an ordinary prop. Test/spec/story/fixture source
    /// components never carry a drill trace.
    pub drill: Option<ReactPropDrill>,
}

/// Per-component render + prop + hook intelligence for one React component.
/// DESCRIPTIVE ambient editor context surfaced by the LSP (a component summary
/// code lens plus per-prop hovers), NOT a finding, IssueKind, severity, or
/// `total_issues` input. Carried in-process on the `#[serde(skip)]`
/// `AnalysisResults::react_component_intel` field (like
/// [`RenderFanInMetric`]); never serialized, so bare `fallow` / `audit` and the
/// JSON / schema surface are untouched.
///
/// Counts are HONEST: test/spec/story/fixture render sites are excluded from
/// `render_sites`, `distinct_parents`, and per-prop `passed_from_sites`, and
/// `distinct_parents` (not the repeat-inflated `render_sites`) is the headline,
/// mirroring the render-fan-in metric's discipline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReactComponentIntel {
    /// Absolute path of the file declaring the component.
    pub path: PathBuf,
    /// The component name.
    pub component_name: String,
    /// 1-based line of the component definition (anchors the code lens).
    pub anchor_line: u32,
    /// Column of the component definition (byte-derived).
    pub anchor_col: u32,
    /// Total JSX render SITES that resolve to this component (each capitalized /
    /// member JSX tag is one site). SECONDARY context: a single parent rendering
    /// the child five times is five sites but one distinct parent.
    pub render_sites: u32,
    /// Distinct `(parent_file, parent_component)` keys rendering this component.
    /// The HEADLINE blast-radius count (never the repeat-inflated site count).
    pub distinct_parents: u32,
    /// Number of declared props on this component.
    pub prop_count: u16,
    /// Per-kind hook counts.
    pub hooks: ReactHookSummary,
    /// Per-prop usage intelligence (one entry per declared prop).
    pub props: Vec<ReactPropIntel>,
}
