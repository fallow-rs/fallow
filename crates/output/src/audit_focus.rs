//! Audit focus-map output contracts.

use serde::Serialize;

/// The focus label for a review unit. `Skip` is the SAFE explicit-skip label and
/// is runtime-backed ONLY: it is producible solely on the paid runtime path
/// and solely for a unit runtime-proves cold with zero risk signals. Free mode
/// (no runtime evidence) emits only `ReviewHere` / `NotPrioritized`, never `Skip`
/// (the build-focus-map labeller cannot reach the `Skip` arm without runtime
/// input), so the free-tier "rank but never skip" stance holds by construction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum FocusLabel {
    /// Review this unit.
    ReviewHere,
    /// Not prioritized, but still visible in the escape-hatch list.
    NotPrioritized,
    /// Safe to skip: runtime evidence proves the unit cold (only `safe_to_delete`
    /// findings, no hot path) AND it carries no risk signal. Runtime-backed only.
    Skip,
}

impl FocusLabel {
    /// The wire token.
    #[must_use]
    pub const fn token(self) -> &'static str {
        match self {
            Self::ReviewHere => "review-here",
            Self::NotPrioritized => "not-prioritized",
            Self::Skip => "skip",
        }
    }
}

/// A per-unit confidence flag. The EXACT panel-decided strings: a dynamically-
/// wired or re-export-heavy unit carries one so its static-reachability signal is
/// not trusted as complete (the anti-silent-de-prioritization guard). The flag
/// NEVER lowers the score; it is advisory provenance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum ConfidenceFlag {
    /// The unit is dynamically wired.
    DynamicDispatch,
    /// The unit's reachability runs through re-export barrels.
    ReExportIndirection,
}

impl ConfidenceFlag {
    /// The wire message for this flag.
    #[must_use]
    pub const fn message(self) -> &'static str {
        match self {
            Self::DynamicDispatch => "low: dynamic dispatch detected",
            Self::ReExportIndirection => "low: re-export indirection",
        }
    }
}

/// The composite attention score, with the deterministic component sub-scores
/// kept on the wire so the runtime layer adds its weight without recomputing the
/// signals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct FocusScore {
    /// Fan-in/out blast-radius component.
    pub fan_io: u32,
    /// Security source -> sink taint-touch component (0 until a security pass is
    /// threaded onto the brief path; the seam is built and tested).
    pub security_taint: u32,
    /// Risk-zone component (boundary / public-API / security-sensitive).
    pub risk_zone: u32,
    /// Change-shape component (new/widened export, signature change proxy).
    pub change_shape: u32,
    /// Runtime-weight component (paid): a hot path (runtime evidence of high
    /// invocation) adds an invocation-bucketed weight so it amplifies the blast
    /// and outranks an otherwise-equal cold unit. `0` in free mode (no runtime
    /// input), so the free-tier total stays the four deterministic components and
    /// is byte-identical to the no-runtime baseline.
    #[serde(default, skip_serializing_if = "is_zero")]
    #[cfg_attr(feature = "schema", schemars(default))]
    pub runtime: u32,
    /// The summed total of every present component (the four deterministic ones
    /// plus the runtime weight).
    pub total: u32,
}

/// Whether a `u32` is zero (serde skip predicate so the `runtime` component is
/// omitted from the wire in free mode, keeping the no-runtime JSON byte-identical).
#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde skip predicate signature"
)]
fn is_zero(value: &u32) -> bool {
    *value == 0
}

/// One review unit on the focus map: its file, composite score, label, human
/// reason, and any confidence flags.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct FocusUnit {
    /// Root-relative path of the changed file this unit covers.
    pub file: String,
    /// The composite attention score and its component breakdown.
    pub score: FocusScore,
    /// The focus label (`review-here` / `not-prioritized`, or the runtime-backed
    /// `skip` on the paid path).
    pub label: FocusLabel,
    /// A human-readable reason for the label, built from the present signals.
    pub reason: String,
    /// Confidence flags (advisory; never lower the score). Sorted, deduped.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub confidence: Vec<ConfidenceFlag>,
}

/// The weighted focus map: the ranked `review-here` units plus the FULL
/// `deprioritized` escape-hatch list, so nothing is hidden.
///
/// Completeness invariant (the escape-hatch done-condition): the two lists
/// partition the unit set, so `review_here.len() + deprioritized.len()` equals
/// the total unit count by construction.
#[derive(Debug, Clone, Default, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct FocusMap {
    /// Units labeled `review-here`, ranked by composite score (descending), ties
    /// broken by path for determinism.
    pub review_here: Vec<FocusUnit>,
    /// EVERY de-prioritized unit (`not-prioritized`, plus runtime-backed `skip`
    /// units on the paid path) -- the escape hatch. Always present and fully
    /// enumerated so a reviewer can always "show me what you de-prioritized"; the
    /// human brief collapses it by default and re-expands under
    /// `--show-deprioritized`. Nothing is ever hidden, including a `skip`.
    pub deprioritized: Vec<FocusUnit>,
}

impl FocusMap {
    /// Total number of units.
    #[must_use]
    pub fn total_units(&self) -> usize {
        self.review_here.len() + self.deprioritized.len()
    }
}
