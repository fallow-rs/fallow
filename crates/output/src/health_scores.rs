//! Score types, grade boundaries, file health metrics, and findings.

use crate::CoverageModel;

/// Minimum churn-times-complexity hotspot score for an entry to count toward
/// the vital-signs `hotspot_count`; lower-scoring entries still appear in the
/// hotspot list but do not feed the health-score hotspot penalty.
pub const HOTSPOT_SCORE_THRESHOLD: f64 = 50.0;

/// Cognitive complexity at or above which a function is flagged as an
/// extraction candidate in refactor targets and cited as a contributing
/// factor on file health scores.
pub const COGNITIVE_EXTRACTION_THRESHOLD: u16 = 30;

/// Default cognitive complexity threshold for "high" finding severity;
/// crossing it upgrades a complexity finding from moderate to high.
pub const DEFAULT_COGNITIVE_HIGH: u16 = 25;

/// Default cognitive complexity threshold for "critical" finding severity.
pub const DEFAULT_COGNITIVE_CRITICAL: u16 = 40;

/// Default cyclomatic complexity threshold for "high" finding severity;
/// crossing it upgrades a complexity finding from moderate to high.
pub const DEFAULT_CYCLOMATIC_HIGH: u16 = 30;

/// Default cyclomatic complexity threshold for "critical" finding severity.
/// Also the cutoff for the v2 health score's critical-complexity density
/// penalty (share of functions at or above this value).
pub const DEFAULT_CYCLOMATIC_CRITICAL: u16 = 50;

/// Minimum lines of code for full complexity density weight in the MI formula.
pub const MI_DENSITY_MIN_LINES: f64 = 50.0;

/// Formula version for the overall health score, serialized as
/// [`HealthScore::formula_version`] so consumers can distinguish a score shift
/// caused by a formula change from one caused by an actual codebase change.
/// v2 replaced the size-dependent aggregators (average and p90 cyclomatic,
/// raw hotspot and dependency counts) with scale-invariant densities
/// (critical-complexity share, per-thousand-file dependency rates, top-1%
/// hotspot share) so scores are comparable across repository sizes; older
/// snapshots that lack the density fields fall back to the v1 aggregators.
/// See `engine::vital_signs` for the full penalty formula.
pub const HEALTH_SCORE_FORMULA_VERSION: u32 = 2;

/// Formula version for the styling-health score (the CSS / design-system axis).
/// Bumped independently of [`HEALTH_SCORE_FORMULA_VERSION`] whenever the styling
/// penalty rubric is recalibrated, so consumers can distinguish a score shift
/// caused by a weight change from one caused by an actual codebase change. v2
/// recalibrated `dead_surface` (size-stable declaration-share denominator) and
/// `token_erosion` (gently saturating arbitrary-value term) from real-project
/// evidence. v3 re-weighted the duplication family toward value DRIFT: it
/// down-weighted the exact-block `duplication` scale (exact CSS duplication is the
/// least-harmful pattern) and added a hardcoded-value-sprawl drift sub-term to
/// `token_erosion` (distinct un-tokenized `box-shadow`/`border-radius`/`line-height`
/// values). See `engine::health::styling_score` for the full rubric + calibration.
pub const STYLING_HEALTH_FORMULA_VERSION: u32 = 3;

/// `skip_serializing_if` predicate: drop a `u16` field from JSON when zero, so
/// the React descriptive counts never bloat non-React complexity findings.
#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde skip_serializing_if requires a by-reference predicate"
)]
fn is_zero_u16(value: &u16) -> bool {
    *value == 0
}

/// `skip_serializing_if` predicate: drop a `usize` field from JSON when zero,
/// so default-configuration file-score rows stay byte-identical.
#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde skip_serializing_if requires a by-reference predicate"
)]
fn is_zero_usize(value: &usize) -> bool {
    *value == 0
}

/// Overall project health score: 100 minus capped per-category penalties.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct HealthScore {
    /// Score formula version; see [`HEALTH_SCORE_FORMULA_VERSION`].
    pub formula_version: u32,
    /// Health score in `[0, 100]`; higher is healthier.
    pub score: f64,
    /// Letter grade from [`letter_grade`] (A>=85, B>=70, C>=55, D>=40, F<40).
    pub grade: &'static str,
    /// Per-component penalty breakdown.
    pub penalties: HealthScorePenalties,
}

/// Per-component penalty breakdown for the health score.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct HealthScorePenalties {
    /// Points subtracted for unreachable files; absent when dead-code data
    /// was not available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dead_files: Option<f64>,
    /// Points subtracted for unused exports; absent when dead-code data was
    /// not available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dead_exports: Option<f64>,
    /// Points subtracted for overall complexity load.
    pub complexity: f64,
    /// Points subtracted for the complexity tail (v1: p90 cyclomatic; v2:
    /// critical-complexity density).
    pub p90_complexity: f64,
    /// Points subtracted for low maintainability-index files; absent when
    /// file scores were not computed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub maintainability: Option<f64>,
    /// Points subtracted for churn-times-complexity hotspots; absent without
    /// git history.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hotspots: Option<f64>,
    /// Points subtracted for unused dependencies; absent when dead-code data
    /// was not available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unused_deps: Option<f64>,
    /// Points subtracted for circular dependency chains; absent when
    /// dead-code data was not available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub circular_deps: Option<f64>,
    /// Penalty for oversized functions, computed against fixed calibration
    /// (very-high-risk bin edge at 60 LOC). Deliberately independent of
    /// `health.maxUnitSize`, which filters the large-functions findings list
    /// only; raising that threshold empties the list without moving this
    /// penalty. `health.ignore` removes files from the score entirely.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit_size: Option<f64>,
    /// Points subtracted for fan-in coupling concentration; absent when the
    /// module graph was not available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coupling: Option<f64>,
    /// Points subtracted for duplicated code; absent when the duplication
    /// pipeline did not run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duplication: Option<f64>,
    /// Small capped penalty for prop-drilling chains. `None` unless the opt-in
    /// `prop-drilling` rule is enabled; sized like the coupling penalty (~5pt cap).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prop_drilling: Option<f64>,
}

/// Project-level styling-health score: a SECOND health axis computed purely from
/// the structural CSS analytics (`CssAnalyticsReport`), orthogonal to the JS/TS
/// code-health [`HealthScore`]. Surfaced only alongside the `--css` analytics, so
/// a plain `fallow health` run is byte-unchanged. The code score and grade stay
/// untouched: styling health is additive, never folded into the code score.
///
/// Like [`HealthScore`], the score starts at 100 and subtracts capped per-category
/// penalties; the grade reuses the shared [`letter_grade`] thresholds verbatim
/// (A>=85, B>=70, C>=55, D>=40, F<40), so the two axes are read on one scale.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct StylingHealth {
    /// Styling formula version; see [`STYLING_HEALTH_FORMULA_VERSION`].
    pub formula_version: u32,
    /// Styling-health score in `[0, 100]`; higher is healthier.
    pub score: f64,
    /// Letter grade from the shared [`letter_grade`] thresholds.
    pub grade: &'static str,
    /// Per-category penalty breakdown.
    pub penalties: StylingHealthPenalties,
    /// How much to trust the grade. `Low` in either of two cases, `High`
    /// otherwise (see `confidence_reason` for which): (1) the analyzed CSS surface
    /// is too thin for the declaration-normalized penalty rubric to be reliable
    /// (the gradeable, non-atomic declaration count is below 50); or (2) the
    /// project's CSS is predominantly flat compile-time-atomic CSS-in-JS
    /// (StyleX/Panda), whose structure is not assessable, so the grade reflects
    /// token hygiene only regardless of declaration count. This is descriptive
    /// metadata that NEVER feeds the score: `score`/`grade`/`penalties` are
    /// byte-identical whether confidence is high or low. Gate on this `confidence`
    /// flag, which is the complete signal; do NOT reconstruct it from
    /// `total_declarations`, since that summary count includes atomic declarations
    /// the grade excludes (a large all-atomic project is `Low` despite a high
    /// `total_declarations`).
    pub confidence: StylingHealthConfidence,
    /// Human-readable reason the grade is low-confidence: either the declaration
    /// and stylesheet counts a thin grade was computed from, or that structure is
    /// not assessable for compile-time-atomic CSS-in-JS. `None` when confidence is
    /// `High`. Prose, not a stable machine field: gate on `confidence`, not on
    /// this string.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence_reason: Option<String>,
}

/// Trust level for a [`StylingHealth`] grade. TWO variants (not the three-tier
/// `high`/`medium`/`low` of [`crate::Confidence`] / `FeatureFlagConfidence`) ON
/// PURPOSE: styling confidence is binary (the grade is either reliable for the
/// analyzed surface or it is not), not three distinct evidence tiers, so a
/// never-emitted `Medium` would be dead surface. Serializes lowercase (`"high"` /
/// `"low"`), matching the sibling confidence enums' vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum StylingHealthConfidence {
    /// The analyzed CSS surface is large enough, and structurally assessable
    /// enough, for the grade to be reliable.
    High,
    /// The grade is indicative rather than authoritative, for one of two reasons
    /// (named in `confidence_reason`): a thin authored-CSS surface (little to
    /// measure), or predominantly flat compile-time-atomic CSS-in-JS
    /// (StyleX/Panda) whose structure is not assessable. NOT a signal that
    /// fallow's analysis failed.
    Low,
}

/// Per-category penalty breakdown for the styling-health score. Each field is the
/// number of points subtracted from a starting 100 for one CSS signal family,
/// already capped at its category ceiling. A `0.0` field means "the signal was
/// evaluated and clean"; the whole struct is only ever built when CSS analytics
/// were produced, so there is no "missing pipeline" ambiguity to model with
/// `Option` here (the parent `StylingHealth` is itself `Option` on the report).
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct StylingHealthPenalties {
    /// Copy-paste declaration blocks (`duplicate_declaration_blocks`), scaled by
    /// total removable declarations. Capped at 20pt.
    pub duplication: f64,
    /// Dead styling surface, two independently-normalized terms summed and capped
    /// at 20pt: (a) unused `@theme` tokens as a share of the total `@theme` token
    /// population (size-independent, so a declaration-sparse Tailwind project is
    /// not penalized for a few dead tokens); plus (b) the other dead entities
    /// (unreferenced classes, unused `@property`/`@layer` at-rules, dead
    /// `@font-face` families) as a share of `total_declarations`.
    pub dead_surface: f64,
    /// Broken references: markup classes one edit from a defined class
    /// (`unresolved_class_references`) and animations referencing a `@keyframes`
    /// defined nowhere (`undefined_keyframes`). Capped at 15pt.
    pub broken_references: f64,
    /// Design-token erosion: mixed `font-size` units (`font_size_unit_mix`),
    /// Tailwind arbitrary-value bypasses (`tailwind_arbitrary_values`), and
    /// distinct HARDCODED `box-shadow`/`border-radius`/`line-height` values above
    /// per-axis baselines (the v3 value-sprawl drift sub-term; `var(--*)`-
    /// referenced values are not counted). Capped at 10pt.
    pub token_erosion: f64,
    /// Structural smells from the summary aggregates: `!important` density and
    /// deep style-rule nesting. Capped at 10pt.
    pub structural: f64,
}

/// Map a numeric score (0-100) to a letter grade.
#[must_use]
#[expect(
    clippy::cast_possible_truncation,
    reason = "score is 0-100, fits in u32"
)]
pub const fn letter_grade(score: f64) -> &'static str {
    let s = score as u32;
    if s >= 85 {
        "A"
    } else if s >= 70 {
        "B"
    } else if s >= 55 {
        "C"
    } else if s >= 40 {
        "D"
    } else {
        "F"
    }
}

/// Coverage tier classification for CRAP findings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum CoverageTier {
    /// No test coverage.
    None,
    /// Some coverage below the high watermark.
    Partial,
    /// Coverage at or above the high watermark (70%).
    High,
}

/// Coverage percentage at or above which a function is classified as `High`.
const HIGH_COVERAGE_WATERMARK: f64 = 70.0;

impl CoverageTier {
    /// Bucket a numeric coverage percentage `[0, 100]` into a tier.
    #[must_use]
    pub fn from_pct(pct: f64) -> Self {
        if pct <= 0.0 {
            Self::None
        } else if pct >= HIGH_COVERAGE_WATERMARK {
            Self::High
        } else {
            Self::Partial
        }
    }
}

/// Provenance of a CRAP finding's coverage signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum CoverageSource {
    /// Measured from Istanbul runtime coverage data.
    Istanbul,
    /// Estimated from static test reachability.
    Estimated,
    /// Estimated coverage inherited from the enclosing component.
    EstimatedComponentInherited,
}

/// Whether CRAP findings in the report used one coverage-source kind or a mix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum CoverageSourceConsistency {
    /// Every CRAP finding used the same coverage source.
    Uniform,
    /// CRAP findings mix coverage sources.
    Mixed,
}

/// Summarise the coverage-source provenance attached to CRAP findings.
#[must_use]
pub fn summarize_coverage_source_consistency(
    sources: impl IntoIterator<Item = CoverageSource>,
) -> Option<CoverageSourceConsistency> {
    let mut first = None;
    for source in sources {
        match first {
            None => first = Some(source),
            Some(existing) if existing != source => {
                return Some(CoverageSourceConsistency::Mixed);
            }
            Some(_) => {}
        }
    }
    first.map(|_| CoverageSourceConsistency::Uniform)
}

/// Per-component React hook profile derived from the cached `hook_uses` IR at
/// the health layer. Descriptive context that refines the bare
/// [`ComplexityViolation::react_hook_count`] headline with a per-kind breakdown
/// and the maximum `useEffect` dependency-array arity.
///
/// Attached only when at least one component-scope hook was attributed to the
/// function, so non-React findings stay byte-identical on the wire. The
/// per-kind counts cover hooks recorded by the React visitor (calls inside an
/// identified component); a `use*` call inside a plain helper function is
/// counted in `react_hook_count` but NOT here, so the breakdown can sum to LESS
/// than `react_hook_count`. `react_hook_count` remains the headline total; this
/// is an additive refinement.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ReactHookProfile {
    /// `useState` call count attributed to this component.
    pub state: u16,
    /// `useEffect` call count attributed to this component.
    pub effect: u16,
    /// `useMemo` call count attributed to this component.
    pub memo: u16,
    /// `useCallback` call count attributed to this component.
    pub callback: u16,
    /// Custom `use*` hook call count attributed to this component.
    pub custom: u16,
    /// Largest `useEffect` dependency-array arity over the attributed effects
    /// that carry a literal deps array. `None` when no attributed `useEffect`
    /// had a literal array (absent or non-literal deps; ADR-001 syntactic-only,
    /// so absence does NOT mean "no coupling").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_effect_dep_arity: Option<u32>,
}

impl ReactHookProfile {
    /// Total component-scope hooks attributed (state + effect + memo + callback
    /// + custom). Used to gate whether the profile is surfaced at all.
    #[must_use]
    pub fn total(&self) -> u16 {
        self.state
            .saturating_add(self.effect)
            .saturating_add(self.memo)
            .saturating_add(self.callback)
            .saturating_add(self.custom)
    }

    /// `true` when no hook was attributed, so the profile carries no signal.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.total() == 0
    }
}

/// Inner complexity-violation payload.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ComplexityViolation {
    /// File path relative to the project root.
    #[serde(serialize_with = "fallow_types::serde_path::serialize")]
    pub path: std::path::PathBuf,
    /// Function name, or a synthesized name for anonymous functions.
    pub name: String,
    /// 1-based line the function starts on.
    pub line: u32,
    /// 1-based column the function starts on.
    pub col: u32,
    /// Cyclomatic complexity of the function.
    pub cyclomatic: u16,
    /// Cognitive complexity of the function.
    pub cognitive: u16,
    /// Lines of code in the function body.
    pub line_count: u32,
    /// Number of declared parameters.
    pub param_count: u8,
    /// Number of React hook calls in this function's body (`useState` /
    /// `useEffect` / `useMemo` / `useCallback` / custom `use*`). Descriptive
    /// hotspot context for React components; omitted when zero (non-React).
    #[serde(default, skip_serializing_if = "is_zero_u16")]
    pub react_hook_count: u16,
    /// Deepest JSX element nesting reached in this function's body. Descriptive
    /// hotspot context; omitted when zero (renders no JSX).
    #[serde(default, skip_serializing_if = "is_zero_u16")]
    pub react_jsx_max_depth: u16,
    /// Number of props destructured from this component's first parameter.
    /// Descriptive hotspot context; omitted when zero.
    #[serde(default, skip_serializing_if = "is_zero_u16")]
    pub react_prop_count: u16,
    /// Per-kind React hook breakdown (state/effect/memo/callback/custom) plus
    /// the max `useEffect` dependency-array arity, derived from the cached
    /// `hook_uses` IR at the health layer. Descriptive refinement of
    /// `react_hook_count`; present only when at least one component-scope hook
    /// was attributed, so non-React findings stay byte-identical.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub react_hook_profile: Option<ReactHookProfile>,
    /// Which metric crossed its threshold.
    pub exceeded: ExceededThreshold,
    /// Finding severity derived from how far thresholds were crossed.
    pub severity: FindingSeverity,
    /// CRAP score (change risk anti-pattern), when coverage data exists.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crap: Option<f64>,
    /// Test coverage percentage (0-100) backing the CRAP score.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coverage_pct: Option<f64>,
    /// Coverage tier bucket.
    ///
    /// Derived from `coverage_pct` when coverage was measured. When
    /// `coverage_source` is estimated, `coverage_pct` is absent and the tier
    /// describes the static estimate behind the CRAP score rather than an
    /// observation, so read the two fields together.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coverage_tier: Option<CoverageTier>,
    /// Provenance of the coverage signal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coverage_source: Option<CoverageSource>,
    /// Component file the inherited coverage estimate came from, for
    /// component-inherited coverage.
    #[serde(
        default,
        serialize_with = "fallow_types::serde_path::serialize_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub inherited_from: Option<std::path::PathBuf>,
    /// Aggregate of the enclosing component's findings, when rolled up.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub component_rollup: Option<ComponentRollup>,
    /// Per-decision-point complexity breakdown explaining WHICH constructs drove
    /// the cyclomatic and cognitive scores. Populated only when the caller opts
    /// in via `health --complexity-breakdown`; empty (and omitted from JSON)
    /// otherwise so default and CI output stay lean.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub contributions: Vec<fallow_types::extract::ComplexityContribution>,
    /// Resolved thresholds used for this finding when a config override changed
    /// at least one ceiling. Omitted for findings using global thresholds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effective_thresholds: Option<HealthEffectiveThresholds>,
    /// Source of the effective thresholds. Omitted when thresholds are global.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub threshold_source: Option<ThresholdSource>,
}

impl ComplexityViolation {
    /// Ceilings this finding was actually evaluated against: the per-file
    /// `thresholdOverrides` result when an override matched, otherwise the
    /// run's global summary ceilings.
    ///
    /// Every renderer that prints or compares a threshold must go through this
    /// so a finding is never described against a ceiling it was not measured
    /// with.
    #[must_use]
    pub fn resolved_thresholds(&self, summary: &HealthSummary) -> HealthEffectiveThresholds {
        self.effective_thresholds
            .unwrap_or(HealthEffectiveThresholds {
                max_cyclomatic: summary.max_cyclomatic_threshold,
                max_cognitive: summary.max_cognitive_threshold,
                max_crap: summary.max_crap_threshold,
                max_unit_size: summary.max_unit_size_threshold,
            })
    }
}

/// Default unit-size ceiling (`health.maxUnitSize`): functions over 60 lines of
/// code are reported as oversized. Mirrors the config crate's default so
/// renderers can fill an effective-thresholds fallback without a config handle.
pub const DEFAULT_MAX_UNIT_SIZE: u32 = 60;

/// Resolved thresholds used to evaluate a health finding.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[allow(
    clippy::struct_field_names,
    reason = "target-dependent clippy lint; wire fields mirror max_* config keys"
)]
pub struct HealthEffectiveThresholds {
    /// Effective cyclomatic-complexity ceiling for the matched file.
    pub max_cyclomatic: u16,
    /// Effective cognitive-complexity ceiling for the matched file.
    pub max_cognitive: u16,
    /// Effective CRAP-score ceiling for the matched file.
    pub max_crap: f64,
    /// Effective unit-size ceiling (maximum function length in lines) for the
    /// matched file, after applying any `thresholdOverrides` on top of the
    /// global `health.maxUnitSize` default.
    pub max_unit_size: u32,
}

/// Threshold values configured by a single override entry.
#[derive(Debug, Clone, Copy, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[allow(
    clippy::struct_field_names,
    reason = "target-dependent clippy lint; wire fields mirror max_* config keys"
)]
pub struct HealthConfiguredThresholds {
    /// Cyclomatic ceiling set by the override, when it sets one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_cyclomatic: Option<u16>,
    /// Cognitive ceiling set by the override, when it sets one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_cognitive: Option<u16>,
    /// CRAP ceiling set by the override, when it sets one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_crap: Option<f64>,
    /// Unit-size ceiling set by the override, when it sets one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_unit_size: Option<u32>,
}

/// Source for a finding's effective thresholds.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ThresholdSource {
    /// A `thresholdOverrides` config entry changed at least one ceiling.
    Override,
}

/// Lifecycle state for a configured threshold override.
#[derive(Debug, Clone, Copy, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ThresholdOverrideStatus {
    /// The override matches a finding that still needs the raised ceiling.
    Active,
    /// The override is not what keeps the matched unit quiet, so it can go:
    /// either the unit passes the global thresholds on its own, or an inline
    /// suppression already covers it.
    Stale,
    /// The override raises the ceiling for this dimension but the matched code
    /// still exceeds the raised value, so the finding survives the override.
    /// Without this state the row was dropped entirely and a user saw no
    /// feedback at all on an override that was in force (issue #2163).
    Insufficient,
    /// The override matches no analyzed file or function.
    NoMatch,
}

/// Which threshold dimension a `thresholdOverrides` state row describes.
///
/// One configured override produces one row per dimension it participates in,
/// because the complexity ceilings and the CRAP ceiling are evaluated
/// independently: raising `maxCyclomatic` says nothing about whether the unit
/// still breaches `maxCrap`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ThresholdOverrideDimension {
    /// The structural ceilings: `maxCyclomatic`, `maxCognitive` and
    /// `maxUnitSize`.
    Complexity,
    /// The `maxCrap` ceiling, and only that ceiling.
    Crap,
}

/// Current complexity metrics for a matched threshold override entry.
#[derive(Debug, Clone, Copy, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ThresholdOverrideMetrics {
    /// Current cyclomatic complexity of the matched function.
    pub cyclomatic: u16,
    /// Current cognitive complexity of the matched function.
    pub cognitive: u16,
    /// Current CRAP score, when coverage data exists.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crap: Option<f64>,
    /// Measured line count of the matched unit. Present on complexity rows,
    /// where `maxUnitSize` participates in the dimension; absent on CRAP rows
    /// and `<component>` rollup rows, which are never scored on unit size.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_count: Option<u32>,
}

/// Report entry describing whether a threshold override is active, stale, or
/// no longer matching any analyzed file or function.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ThresholdOverrideState {
    /// Lifecycle state of the override.
    pub status: ThresholdOverrideStatus,
    /// Index of the entry in the configured `thresholdOverrides` array.
    /// Several rows can share one index when the override participates in more
    /// than one dimension; group on this to count configured overrides.
    pub override_index: usize,
    /// Threshold dimension this row describes.
    pub dimension: ThresholdOverrideDimension,
    /// Dimensions the matched unit still breaches despite this override,
    /// whether or not this override configures their ceilings. Non-empty means
    /// raising the ceiling did not settle the matter: a complexity or CRAP
    /// finding survived, or the unit is still longer than the resolved
    /// `maxUnitSize`, which keeps it in the large-function list without
    /// emitting a finding of its own.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub outstanding: Vec<ThresholdOverrideDimension>,
    /// Matched file path, when the override matched one.
    #[serde(
        default,
        serialize_with = "fallow_types::serde_path::serialize_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub path: Option<std::path::PathBuf>,
    /// Matched function name, for function-scoped overrides.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub function: Option<String>,
    /// 1-based line of the matched unit. Absent on `no_match` rows, which
    /// describe an entry that matched nothing. Name alone is not an identity:
    /// one file can hold several units sharing a name, so this pairs with
    /// `col` to keep their rows distinct (issue #2163).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    /// 0-based byte column of the matched unit. Absent on `no_match` rows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub col: Option<u32>,
    /// Ceilings the override entry configures.
    pub configured_thresholds: HealthConfiguredThresholds,
    /// Ceilings in effect after applying the override to the defaults.
    pub effective_thresholds: HealthEffectiveThresholds,
    /// Current complexity metrics of the matched code, when matched.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metrics: Option<ThresholdOverrideMetrics>,
    /// Human-readable explanation of the status.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl ThresholdOverrideState {
    /// Render the matched unit as `path:line:function`, given the path already
    /// formatted for the target surface.
    ///
    /// Every renderer must go through this: two units sharing a name in one
    /// file produce two rows, and without the position they print as the same
    /// line (issue #2163).
    #[must_use]
    pub fn target_label(&self, display: &str) -> String {
        let Some(name) = self.function.as_deref() else {
            return display.to_owned();
        };
        self.line.map_or_else(
            || format!("{display}:{name}"),
            |line| format!("{display}:{line}:{name}"),
        )
    }
}

/// Component-level aggregate attached to a template complexity finding,
/// pairing the template's scores with the worst class-side function.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ComponentRollup {
    /// Component name.
    pub component: String,
    /// Name of the worst-scoring function in the component class.
    pub class_worst_function: String,
    /// Cyclomatic complexity of that worst class function.
    pub class_cyclomatic: u16,
    /// Cognitive complexity of that worst class function.
    pub class_cognitive: u16,
    /// Template file path relative to the project root.
    #[serde(serialize_with = "fallow_types::serde_path::serialize")]
    pub template_path: std::path::PathBuf,
    /// Cyclomatic complexity of the template.
    pub template_cyclomatic: u16,
    /// Cognitive complexity of the template.
    pub template_cognitive: u16,
}

/// Which complexity threshold was exceeded.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ExceededThreshold {
    /// Only cyclomatic exceeded.
    Cyclomatic,
    /// Only cognitive exceeded.
    Cognitive,
    /// Both cyclomatic and cognitive exceeded (may or may not also exceed CRAP).
    Both,
    /// Only CRAP exceeded (cyclomatic and cognitive are under threshold).
    Crap,
    /// Cyclomatic and CRAP exceeded.
    CyclomaticCrap,
    /// Cognitive and CRAP exceeded.
    CognitiveCrap,
    /// Cyclomatic, cognitive, and CRAP all exceeded.
    All,
}

impl ExceededThreshold {
    /// Classify a finding from which individual thresholds were exceeded.
    ///
    /// Panics if all three bools are false; callers are expected to only
    /// construct an `ExceededThreshold` for findings that exceeded at least
    /// one threshold.
    #[must_use]
    pub fn from_bools(cyclomatic: bool, cognitive: bool, crap: bool) -> Self {
        match (cyclomatic, cognitive, crap) {
            (true, true, true) => Self::All,
            (true, true, false) => Self::Both,
            (true, false, true) => Self::CyclomaticCrap,
            (false, true, true) => Self::CognitiveCrap,
            (true, false, false) => Self::Cyclomatic,
            (false, true, false) => Self::Cognitive,
            (false, false, true) => Self::Crap,
            (false, false, false) => {
                unreachable!("ExceededThreshold requires at least one threshold exceeded")
            }
        }
    }

    /// True when the cyclomatic threshold contributed to the finding.
    #[must_use]
    pub const fn includes_cyclomatic(self) -> bool {
        matches!(
            self,
            Self::Cyclomatic | Self::Both | Self::CyclomaticCrap | Self::All
        )
    }

    /// True when the cognitive threshold contributed to the finding.
    #[must_use]
    pub const fn includes_cognitive(self) -> bool {
        matches!(
            self,
            Self::Cognitive | Self::Both | Self::CognitiveCrap | Self::All
        )
    }

    /// True when the CRAP threshold contributed to the finding.
    #[must_use]
    pub const fn includes_crap(self) -> bool {
        matches!(
            self,
            Self::Crap | Self::CyclomaticCrap | Self::CognitiveCrap | Self::All
        )
    }
}

/// Severity tier indicating how far a function exceeds complexity thresholds.
///
/// Determined by the highest tier reached across both cognitive and cyclomatic
/// scores. Default thresholds: cognitive 25/40, cyclomatic 30/50.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum FindingSeverity {
    /// Above threshold but manageable (cognitive < 25 or cyclomatic < 30).
    Moderate,
    /// Recommended for extraction (cognitive 25-39 or cyclomatic 30-49).
    High,
    /// Immediate extraction candidate (cognitive >= 40 or cyclomatic >= 50).
    Critical,
}

/// CRAP score threshold for "high" severity. CC=7 untested -> 56, CC=10 -> 110.
pub const DEFAULT_CRAP_HIGH: f64 = 50.0;

/// CRAP score threshold for "critical" severity. CC=10 untested gives 110,
/// CC=12 untested gives 156; 100 lands between the two and flags genuinely
/// dangerous combinations of high complexity and low coverage.
pub const DEFAULT_CRAP_CRITICAL: f64 = 100.0;

/// Compute the severity tier for a complexity finding.
///
/// Uses the highest tier reached across cognitive, cyclomatic, and CRAP
/// scores. Pass `None` for `crap` to skip the CRAP contribution (used when
/// the finding was triggered by complexity thresholds only).
#[expect(
    clippy::too_many_arguments,
    reason = "public library API for napi/embedders; the metric values and their high/critical threshold pairs are a stable positional contract that bundling would break"
)]
pub fn compute_finding_severity(
    cognitive: u16,
    cyclomatic: u16,
    crap: Option<f64>,
    cognitive_high: u16,
    cognitive_critical: u16,
    cyclomatic_high: u16,
    cyclomatic_critical: u16,
) -> FindingSeverity {
    let cog = if cognitive >= cognitive_critical {
        FindingSeverity::Critical
    } else if cognitive >= cognitive_high {
        FindingSeverity::High
    } else {
        FindingSeverity::Moderate
    };

    let cyc = if cyclomatic >= cyclomatic_critical {
        FindingSeverity::Critical
    } else if cyclomatic >= cyclomatic_high {
        FindingSeverity::High
    } else {
        FindingSeverity::Moderate
    };

    let crap_sev = crap.map_or(FindingSeverity::Moderate, |c| {
        if c >= DEFAULT_CRAP_CRITICAL {
            FindingSeverity::Critical
        } else if c >= DEFAULT_CRAP_HIGH {
            FindingSeverity::High
        } else {
            FindingSeverity::Moderate
        }
    });

    cog.max(cyc).max(crap_sev)
}

/// A function exceeding the very-high-risk size threshold (>60 LOC).
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct LargeFunctionEntry {
    /// File path relative to the project root.
    #[serde(serialize_with = "fallow_types::serde_path::serialize")]
    pub path: std::path::PathBuf,
    /// Function name, or a synthesized name for anonymous functions.
    pub name: String,
    /// 1-based line the function starts on.
    pub line: u32,
    /// Lines of code in the function body.
    pub line_count: u32,
}

/// Staleness of a loaded health baseline.
///
/// Reports how many saved complexity and CRAP finding entries still matched a
/// current finding on this run, so consumers can see a rotting baseline before
/// it degrades to zero overlap. Runtime-coverage suppressions and refactoring
/// target keys carried by the same baseline are not counted here. Present in
/// the summary only when a baseline was loaded.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct HealthBaselineStaleness {
    /// Complexity and CRAP finding entries carried by the loaded baseline.
    pub baseline_entries: usize,
    /// Entries that matched a current finding in the active baseline mode,
    /// including entries matched through a followed file move.
    pub matched_entries: usize,
    /// Entries that matched no current finding on this run.
    pub stale_entries: usize,
    /// Entries that matched only by following a file move in identity mode.
    /// Always zero in count mode.
    pub moved_entries: usize,
    /// True when this run analyzed a subset of the project (changed-file,
    /// diff, or workspace scoping), so the baseline was compared against a
    /// narrowed finding set and staleness cannot be judged. `stale` is always
    /// false on scoped runs.
    pub change_scoped: bool,
    /// True exactly when the run was not change-scoped, at least one current
    /// finding existed before baseline filtering, and `stale_entries` reached
    /// a quarter of `baseline_entries`. Mirrors the human warning so machine
    /// consumers do not have to reimplement the threshold.
    pub stale: bool,
}

/// Summary statistics for the health report.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct HealthSummary {
    /// Files included in the health analysis.
    pub files_analyzed: usize,
    /// Functions scored across the analyzed files.
    pub functions_analyzed: usize,
    /// Functions exceeding at least one complexity or CRAP threshold.
    pub functions_above_threshold: usize,
    /// Global cyclomatic-complexity ceiling for this run.
    pub max_cyclomatic_threshold: u16,
    /// Global cognitive-complexity ceiling for this run.
    pub max_cognitive_threshold: u16,
    /// Global CRAP-score ceiling for this run.
    pub max_crap_threshold: f64,
    /// Effective global unit-size ceiling (`health.maxUnitSize`, maximum
    /// function length in lines) for this run. Sits alongside the other three
    /// `max_*_threshold` siblings so a consumer reading the summary sees every
    /// configured threshold. Per-file `thresholdOverrides` are not reflected
    /// here; this is the global default.
    pub max_unit_size_threshold: u32,
    /// Files with a computed maintainability score; absent when file scoring
    /// did not run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files_scored: Option<usize>,
    /// Mean maintainability index over scored files (0-100).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub average_maintainability: Option<f64>,
    /// Coverage model behind the CRAP scores, when coverage was used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coverage_model: Option<CoverageModel>,
    /// Whether CRAP findings mix coverage sources.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coverage_source_consistency: Option<CoverageSourceConsistency>,
    /// Functions matched against the Istanbul coverage file, in Istanbul mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub istanbul_matched: Option<usize>,
    /// Functions in the Istanbul coverage file, in Istanbul mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub istanbul_total: Option<usize>,
    /// Analyzed files the Istanbul coverage file carried an entry for.
    /// Read against `istanbul_files_total`, this separates a coverage file
    /// that did not join from code the coverage file says nothing ran in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub istanbul_files_matched: Option<usize>,
    /// Files described by the Istanbul coverage file, joined or not.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub istanbul_files_total: Option<usize>,
    /// Findings with critical severity.
    pub severity_critical_count: usize,
    /// Findings with high severity.
    pub severity_high_count: usize,
    /// Findings with moderate severity.
    pub severity_moderate_count: usize,
    /// Baseline staleness data, present only when a baseline was loaded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_staleness: Option<HealthBaselineStaleness>,
}

impl Default for HealthSummary {
    fn default() -> Self {
        Self {
            files_analyzed: 0,
            functions_analyzed: 0,
            functions_above_threshold: 0,
            max_cyclomatic_threshold: 20,
            max_cognitive_threshold: 15,
            max_crap_threshold: 30.0,
            max_unit_size_threshold: DEFAULT_MAX_UNIT_SIZE,
            files_scored: None,
            average_maintainability: None,
            coverage_model: None,
            coverage_source_consistency: None,
            istanbul_matched: None,
            istanbul_total: None,
            istanbul_files_matched: None,
            istanbul_files_total: None,
            severity_critical_count: 0,
            severity_high_count: 0,
            severity_moderate_count: 0,
            baseline_staleness: None,
        }
    }
}

/// Per-file health score combining complexity, coupling, and dead code metrics.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct FileHealthScore {
    /// File path relative to the project root.
    #[serde(serialize_with = "fallow_types::serde_path::serialize")]
    pub path: std::path::PathBuf,
    /// Modules importing this file.
    pub fan_in: usize,
    /// Modules this file imports.
    pub fan_out: usize,
    /// Unused exports as a fraction of the file's exports, in `[0, 1]`.
    pub dead_code_ratio: f64,
    /// Total cyclomatic complexity per line of code.
    pub complexity_density: f64,
    /// Maintainability index (0-100); higher is healthier.
    pub maintainability_index: f64,
    /// Summed cyclomatic complexity over the file's functions.
    pub total_cyclomatic: u32,
    /// Summed cognitive complexity over the file's functions.
    pub total_cognitive: u32,
    /// Functions in the file.
    pub function_count: usize,
    /// Lines of code in the file.
    pub lines: u32,
    /// Highest CRAP score among the file's functions. Always the raw measured
    /// value; threshold overrides never rewrite it.
    pub crap_max: f64,
    /// Functions whose rounded CRAP score meets or exceeds their effective
    /// ceiling, resolved from `health.thresholdOverrides` over the global
    /// `maxCrap` / `--max-crap` value. Zero when CRAP enforcement is disabled
    /// (global ceiling `0`).
    pub crap_above_threshold: usize,
    /// Functions whose rounded CRAP score is at or above the canonical 30.0
    /// baseline but below their effective ceiling: the count the configuration
    /// let through. Stays `0` when the effective ceiling is stricter than 30.
    /// When CRAP enforcement is disabled (global ceiling `0`), counts every
    /// function at or above the canonical baseline. Omitted when zero.
    #[serde(default, skip_serializing_if = "is_zero_usize")]
    #[cfg_attr(feature = "schema", schemars(default))]
    pub crap_exempted: usize,
    /// Lowest effective CRAP ceiling among the file's functions, present only
    /// when it differs from the run global (`summary.max_crap_threshold`).
    /// Consumers fall back to `summary.max_crap_threshold` when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crap_effective_threshold: Option<f64>,
}

/// A hotspot: a file that is both complex and frequently changing.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct HotspotEntry {
    /// File path relative to the project root.
    #[serde(serialize_with = "fallow_types::serde_path::serialize")]
    pub path: std::path::PathBuf,
    /// Churn-times-complexity hotspot score; higher is riskier.
    pub score: f64,
    /// Commits touching the file in the analysis window.
    pub commits: u32,
    /// Recency-weighted commit count.
    pub weighted_commits: f64,
    /// Lines added to the file in the analysis window.
    pub lines_added: u32,
    /// Lines deleted from the file in the analysis window.
    pub lines_deleted: u32,
    /// Total cyclomatic complexity per line of code.
    pub complexity_density: f64,
    /// Modules importing this file.
    pub fan_in: usize,
    /// Whether churn is rising, falling, or steady over the window.
    pub trend: fallow_types::churn::ChurnTrend,
    /// Ownership metrics, when ownership analysis ran.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ownership: Option<OwnershipMetrics>,
    /// True for files matched by test-path patterns; omitted when false.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    #[cfg_attr(feature = "schema", schemars(default))]
    pub is_test_path: bool,
}

/// One contributor row in ownership metrics.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ContributorEntry {
    /// Contributor identifier, encoded per `format`.
    pub identifier: String,
    /// How `identifier` is encoded (raw, handle, anonymized, or hash).
    pub format: ContributorIdentifierFormat,
    /// Contributor's share of the file's commits, in `[0, 1]`.
    pub share: f64,
    /// Days since the contributor's last commit to the file.
    pub stale_days: u64,
    /// Contributor's commits touching the file in the window.
    pub commits: u32,
}

/// Encoding applied to a [`ContributorEntry::identifier`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum ContributorIdentifierFormat {
    /// Raw git author identity.
    Raw,
    /// Platform handle, e.g. a GitHub username.
    Handle,
    /// Anonymized label that stays stable within the report.
    Anonymized,
    /// One-way hash of the identity.
    Hash,
}

/// Ownership lifecycle state of a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum OwnershipState {
    /// A declared or de-facto owner is still actively contributing.
    Active,
    /// No owner could be resolved.
    Unowned,
    /// The declared owner has stopped contributing.
    DeclaredInactive,
    /// Recent contributions come from outside the declared ownership.
    Drifting,
}

/// Ownership metrics for a hotspot file, derived from git history and
/// CODEOWNERS declarations.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct OwnershipMetrics {
    /// Minimum contributors covering half the file's commits.
    pub bus_factor: u32,

    /// Distinct contributors touching the file in the window.
    pub contributor_count: u32,

    /// Contributor with the largest commit share.
    pub top_contributor: ContributorEntry,

    /// Contributors active in the recent window; omitted when empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[cfg_attr(feature = "schema", schemars(default))]
    pub recent_contributors: Vec<ContributorEntry>,

    /// Contributors best positioned to review changes; omitted when empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[cfg_attr(feature = "schema", schemars(default))]
    pub suggested_reviewers: Vec<ContributorEntry>,

    /// Owner declared in CODEOWNERS, when one matches the file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub declared_owner: Option<String>,

    /// Whether no owner could be resolved; `null` when ownership resolution
    /// did not run.
    pub unowned: Option<bool>,

    /// Ownership lifecycle state.
    pub ownership_state: OwnershipState,

    /// True when recent contributions drift away from the declared ownership.
    pub drift: bool,

    /// Human-readable explanation of the drift, when drifting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drift_reason: Option<String>,
}

/// Scope metadata for the hotspot analysis.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct HotspotSummary {
    /// Start of the churn window, as passed to `git log --since`.
    pub since: String,
    /// Minimum commit count for a file to qualify as a hotspot.
    pub min_commits: u32,
    /// Files with churn data in the window.
    pub files_analyzed: usize,
    /// Files excluded by test-path and ignore filters.
    pub files_excluded: usize,
    /// True when the repository is a shallow clone, so churn counts are
    /// truncated.
    pub shallow_clone: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exceeded_threshold_serializes_as_snake_case() {
        let json = serde_json::to_string(&ExceededThreshold::Both)
            .expect("threshold variant should serialize");
        assert_eq!(json, r#""both""#);

        let json = serde_json::to_string(&ExceededThreshold::Cyclomatic)
            .expect("threshold variant should serialize");
        assert_eq!(json, r#""cyclomatic""#);
    }

    #[test]
    fn exceeded_threshold_all_variants_serialize() {
        for (variant, expected) in [
            (ExceededThreshold::Cyclomatic, r#""cyclomatic""#),
            (ExceededThreshold::Cognitive, r#""cognitive""#),
            (ExceededThreshold::Both, r#""both""#),
            (ExceededThreshold::Crap, r#""crap""#),
            (ExceededThreshold::CyclomaticCrap, r#""cyclomatic_crap""#),
            (ExceededThreshold::CognitiveCrap, r#""cognitive_crap""#),
            (ExceededThreshold::All, r#""all""#),
        ] {
            let json = serde_json::to_string(&variant).expect("threshold variant should serialize");
            assert_eq!(json, expected, "wire form for {variant:?} should be stable");
        }
    }

    #[test]
    fn letter_grade_boundaries() {
        assert_eq!(letter_grade(100.0), "A");
        assert_eq!(letter_grade(85.0), "A");
        assert_eq!(letter_grade(84.9), "B");
        assert_eq!(letter_grade(70.0), "B");
        assert_eq!(letter_grade(69.9), "C");
        assert_eq!(letter_grade(55.0), "C");
        assert_eq!(letter_grade(54.9), "D");
        assert_eq!(letter_grade(40.0), "D");
        assert_eq!(letter_grade(39.9), "F");
        assert_eq!(letter_grade(0.0), "F");
    }

    #[test]
    fn coverage_tier_boundaries() {
        assert_eq!(CoverageTier::from_pct(0.0), CoverageTier::None);
        assert_eq!(CoverageTier::from_pct(0.1), CoverageTier::Partial);
        assert_eq!(CoverageTier::from_pct(69.9), CoverageTier::Partial);
        assert_eq!(CoverageTier::from_pct(70.0), CoverageTier::High);
        assert_eq!(CoverageTier::from_pct(100.0), CoverageTier::High);
    }

    #[test]
    fn hotspot_score_threshold_is_50() {
        assert!((HOTSPOT_SCORE_THRESHOLD - 50.0).abs() < f64::EPSILON);
    }

    #[test]
    fn health_score_serializes_correctly() {
        let score = HealthScore {
            formula_version: HEALTH_SCORE_FORMULA_VERSION,
            score: 78.5,
            grade: "B",
            penalties: HealthScorePenalties {
                dead_files: Some(3.1),
                dead_exports: Some(6.0),
                complexity: 0.0,
                p90_complexity: 0.0,
                maintainability: None,
                hotspots: None,
                unused_deps: Some(5.0),
                circular_deps: Some(4.0),
                unit_size: None,
                coupling: None,
                duplication: None,
                prop_drilling: None,
            },
        };
        let json = serde_json::to_string(&score).expect("health score should serialize");
        let parsed: serde_json::Value =
            serde_json::from_str(&json).expect("health score JSON should parse");
        assert_eq!(parsed["formula_version"], HEALTH_SCORE_FORMULA_VERSION);
        assert_eq!(parsed["score"], 78.5);
        assert_eq!(parsed["grade"], "B");
        assert_eq!(parsed["penalties"]["dead_files"], 3.1);
        assert!(!json.contains("maintainability"));
        assert!(!json.contains("hotspots"));
        assert!(!json.contains("duplication"));
    }

    #[test]
    fn styling_health_serializes_correctly() {
        let styling = StylingHealth {
            formula_version: STYLING_HEALTH_FORMULA_VERSION,
            score: 72.0,
            grade: "B",
            penalties: StylingHealthPenalties {
                duplication: 12.0,
                dead_surface: 8.0,
                broken_references: 4.0,
                token_erosion: 2.0,
                structural: 2.0,
            },
            confidence: StylingHealthConfidence::High,
            confidence_reason: None,
        };
        let json = serde_json::to_string(&styling).expect("styling health should serialize");
        let parsed: serde_json::Value =
            serde_json::from_str(&json).expect("styling health JSON should parse");
        assert_eq!(parsed["formula_version"], STYLING_HEALTH_FORMULA_VERSION);
        assert_eq!(parsed["score"], 72.0);
        assert_eq!(parsed["grade"], "B");
        assert_eq!(parsed["penalties"]["duplication"], 12.0);
        assert_eq!(parsed["penalties"]["dead_surface"], 8.0);
        assert_eq!(parsed["penalties"]["broken_references"], 4.0);
        assert_eq!(parsed["penalties"]["token_erosion"], 2.0);
        assert_eq!(parsed["penalties"]["structural"], 2.0);
        // `high` confidence omits the reason; the enum serializes lowercase.
        assert_eq!(parsed["confidence"], "high");
        assert!(parsed.get("confidence_reason").is_none());
    }

    #[test]
    fn styling_health_low_confidence_serializes_reason() {
        let styling = StylingHealth {
            formula_version: STYLING_HEALTH_FORMULA_VERSION,
            score: 89.0,
            grade: "A",
            penalties: StylingHealthPenalties {
                duplication: 0.0,
                dead_surface: 0.0,
                broken_references: 0.0,
                token_erosion: 0.0,
                structural: 0.0,
            },
            confidence: StylingHealthConfidence::Low,
            confidence_reason: Some("graded from only 24 declarations across 2 stylesheets".into()),
        };
        let json = serde_json::to_string(&styling).expect("styling health should serialize");
        let parsed: serde_json::Value =
            serde_json::from_str(&json).expect("styling health JSON should parse");
        assert_eq!(parsed["confidence"], "low");
        assert_eq!(
            parsed["confidence_reason"],
            "graded from only 24 declarations across 2 stylesheets"
        );
    }

    #[test]
    fn coverage_model_serializes_as_snake_case() {
        let json = serde_json::to_string(&CoverageModel::StaticBinary)
            .expect("coverage model should serialize");
        assert_eq!(json, r#""static_binary""#);

        let json = serde_json::to_string(&CoverageModel::StaticEstimated)
            .expect("coverage model should serialize");
        assert_eq!(json, r#""static_estimated""#);

        let json = serde_json::to_string(&CoverageModel::Istanbul)
            .expect("coverage model should serialize");
        assert_eq!(json, r#""istanbul""#);
    }

    #[test]
    fn finding_severity_serializes_as_snake_case() {
        assert_eq!(
            serde_json::to_string(&FindingSeverity::Moderate)
                .expect("finding severity should serialize"),
            r#""moderate""#,
        );
        assert_eq!(
            serde_json::to_string(&FindingSeverity::High)
                .expect("finding severity should serialize"),
            r#""high""#,
        );
        assert_eq!(
            serde_json::to_string(&FindingSeverity::Critical)
                .expect("finding severity should serialize"),
            r#""critical""#,
        );
    }

    #[test]
    fn finding_severity_ordering() {
        assert!(FindingSeverity::Moderate < FindingSeverity::High);
        assert!(FindingSeverity::High < FindingSeverity::Critical);
    }

    #[test]
    fn compute_severity_moderate_when_below_high_thresholds() {
        let severity = compute_finding_severity(20, 25, None, 25, 40, 30, 50);
        assert_eq!(severity, FindingSeverity::Moderate);
    }

    #[test]
    fn compute_severity_high_from_cognitive() {
        let severity = compute_finding_severity(25, 20, None, 25, 40, 30, 50);
        assert_eq!(severity, FindingSeverity::High);
    }

    #[test]
    fn compute_severity_high_from_cyclomatic() {
        let severity = compute_finding_severity(20, 30, None, 25, 40, 30, 50);
        assert_eq!(severity, FindingSeverity::High);
    }

    #[test]
    fn compute_severity_critical_from_cognitive() {
        let severity = compute_finding_severity(40, 20, None, 25, 40, 30, 50);
        assert_eq!(severity, FindingSeverity::Critical);
    }

    #[test]
    fn compute_severity_critical_from_cyclomatic() {
        let severity = compute_finding_severity(20, 50, None, 25, 40, 30, 50);
        assert_eq!(severity, FindingSeverity::Critical);
    }

    #[test]
    fn compute_severity_uses_highest_across_dimensions() {
        let severity = compute_finding_severity(45, 20, None, 25, 40, 30, 50);
        assert_eq!(severity, FindingSeverity::Critical);
    }

    #[test]
    fn compute_severity_at_exact_boundaries() {
        let severity = compute_finding_severity(25, 30, None, 25, 40, 30, 50);
        assert_eq!(severity, FindingSeverity::High);

        let severity = compute_finding_severity(24, 29, None, 25, 40, 30, 50);
        assert_eq!(severity, FindingSeverity::Moderate);

        let severity = compute_finding_severity(40, 50, None, 25, 40, 30, 50);
        assert_eq!(severity, FindingSeverity::Critical);
    }

    #[test]
    fn compute_severity_crap_contributes_high() {
        let severity = compute_finding_severity(10, 10, Some(60.0), 25, 40, 30, 50);
        assert_eq!(severity, FindingSeverity::High);
    }

    #[test]
    fn compute_severity_crap_contributes_critical() {
        let severity = compute_finding_severity(10, 10, Some(120.0), 25, 40, 30, 50);
        assert_eq!(severity, FindingSeverity::Critical);
    }

    #[test]
    fn compute_severity_crap_moderate_under_high() {
        let severity = compute_finding_severity(10, 10, Some(30.0), 25, 40, 30, 50);
        assert_eq!(severity, FindingSeverity::Moderate);
    }

    #[test]
    fn exceeded_threshold_from_bools() {
        assert!(matches!(
            ExceededThreshold::from_bools(true, false, false),
            ExceededThreshold::Cyclomatic
        ));
        assert!(matches!(
            ExceededThreshold::from_bools(true, true, true),
            ExceededThreshold::All
        ));
        assert!(matches!(
            ExceededThreshold::from_bools(false, false, true),
            ExceededThreshold::Crap
        ));
        assert!(matches!(
            ExceededThreshold::from_bools(true, false, true),
            ExceededThreshold::CyclomaticCrap
        ));
    }

    #[test]
    fn exceeded_threshold_includes_helpers() {
        let all = ExceededThreshold::All;
        assert!(all.includes_cyclomatic());
        assert!(all.includes_cognitive());
        assert!(all.includes_crap());

        let crap_only = ExceededThreshold::Crap;
        assert!(!crap_only.includes_cyclomatic());
        assert!(!crap_only.includes_cognitive());
        assert!(crap_only.includes_crap());

        assert!(ExceededThreshold::CyclomaticCrap.includes_crap());
        assert!(ExceededThreshold::CognitiveCrap.includes_crap());
        assert!(!ExceededThreshold::Both.includes_crap());
        assert!(!ExceededThreshold::Cyclomatic.includes_crap());
        assert!(!ExceededThreshold::Cognitive.includes_crap());
    }

    #[test]
    fn coverage_source_consistency_omits_empty_sources() {
        let sources = Vec::new();
        assert_eq!(summarize_coverage_source_consistency(sources), None);
    }

    #[test]
    fn coverage_source_consistency_reports_uniform_sources() {
        assert_eq!(
            summarize_coverage_source_consistency([
                CoverageSource::Estimated,
                CoverageSource::Estimated,
            ]),
            Some(CoverageSourceConsistency::Uniform)
        );
    }

    #[test]
    fn coverage_source_consistency_reports_mixed_sources() {
        assert_eq!(
            summarize_coverage_source_consistency([
                CoverageSource::Istanbul,
                CoverageSource::Estimated,
            ]),
            Some(CoverageSourceConsistency::Mixed)
        );
    }
}
