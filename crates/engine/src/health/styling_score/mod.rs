//! Styling-health score: a SECOND health axis derived purely from the structural
//! CSS analytics ([`CssAnalyticsReport`]), orthogonal to the JS/TS code-health
//! score. Mirrors the code-score shape (`vital_signs::compute_health_score`):
//! start at 100, subtract capped per-category penalties, map the result to a
//! letter grade via the shared [`letter_grade`]. The code score is never touched.
//!
//! # Penalty rubric (v3, [`STYLING_HEALTH_FORMULA_VERSION`])
//!
//! The weights below were recalibrated from v1 (v1 -> v2 re-normalized
//! `dead_surface` and `token_erosion`; v2 -> v3 re-weighted the duplication
//! family toward value DRIFT, see below) after running across real projects
//! (government design systems plus small Tailwind apps); any further
//! recalibration bumps [`STYLING_HEALTH_FORMULA_VERSION`] (the same versioning
//! discipline the code score uses). Every category is capped and evaluated
//! against the analytics that are always present once `--css` ran, so there is
//! no "missing pipeline" partial-credit case to model.
//!
//! | Category | Cap | Signal | Scaling |
//! |---|---|---|---|
//! | `duplication` | 20pt | `summary.duplicate_declarations_total` (removable declarations from copy-paste blocks) | `total / max(non_atomic_declarations, 1) * 80`, capped (v3 down-weighted from `* 200`: exact CSS duplication is the least-harmful pattern, so ~25% of declarations removable = full 20pt, a soft hint not a dominant term; the non-atomic denominator is the 3c atomic-exclusion behavior) |
//! | `dead_surface` | 20pt | (a) unused `@theme` tokens, as a share of all defined `@theme` tokens; (b) unreferenced classes + unused at-rules + dead `@font-face`, as a share of `total_declarations` | token term `min(unused_theme_tokens / max(theme_tokens_defined, 1) * 15, 15)` + other term `min(other_dead / max(total_declarations, 1) * 150, 8)`, summed then capped at 20 (the token term is a *per-population* death ratio so a handful of dead tokens in a declaration-sparse Tailwind project no longer explodes the penalty) |
//! | `broken_references` | 15pt | `unresolved_class_references` + `keyframes_undefined` | `count * 3`, capped (so 5 broken refs = full 15pt) |
//! | `token_erosion` | 10pt | mixed `font-size` units (above 2) + distinct Tailwind arbitrary-value tokens + distinct HARDCODED `box-shadow`/`border-radius`/`line-height` values (v3 sprawl/drift sub-term) | `min((extra_units * 2), 4)` unit term + `min(arbitrary / 18, 8)` arbitrary term + `min(sum_per_axis_excess / 6, 5)` sprawl term (per-axis excess above baselines 10/8/6), summed then capped at 10 |
//! | `structural` | 10pt | `!important` density + deep nesting | `(important_pct - 5).clamp * 1` + `(max_nesting - 4).clamp * 1`, capped |
//!
//! All point values are rounded to one decimal, the final score is clamped to
//! `[0, 100]`, matching the code score's `round1` / clamp behaviour.
//!
//! ## What v1 -> v2 changed, and why
//!
//! - `dead_surface` was normalized by `files_analyzed` in v1, so a 2-stylesheet
//!   Tailwind project with a few dead entities scored far higher than a 32-file
//!   design system with one dead entity. v2 splits the category into two
//!   independently-normalized terms. Unused `@theme` tokens are divided by the
//!   *total number of `@theme` tokens defined* (a per-population death ratio,
//!   threaded in as `theme_tokens_defined`): this is the principled denominator
//!   because Tailwind projects author almost no CSS declarations, so dividing a
//!   few unused tokens by `total_declarations` exploded the penalty (a project
//!   with 4 unused tokens over 24 declarations capped the whole category at 20).
//!   The remaining dead entities (unreferenced classes, unused at-rules, dead
//!   `@font-face`) still divide by `total_declarations`, the size-stable
//!   denominator `duplication` uses, so neither term swings with stylesheet count.
//! - `token_erosion` in v1 added `tailwind_arbitrary_values` raw, so just ~10
//!   distinct arbitrary values maxed the category, punishing ordinary Tailwind
//!   apps. v2 saturates the arbitrary term via a divisor and caps the unit term
//!   so neither sub-signal alone reaches the ceiling.
//!
//! ## What v2 -> v3 changed, and why
//!
//! Research is clear that exact CSS duplication is the LEAST-harmful CSS pattern
//! (repeated declarations gzip away; graphical properties are loosely coupled; CSS
//! has no native abstraction so some repetition is unavoidable), while the real
//! maintenance harm is value DRIFT / inconsistency (the same design intent
//! expressed with divergence: a radius `6px` here and `5px` there; a shadow
//! `rgba(0,0,0,.1)` vs `.12`), which design tokens address. So v3 re-weights the
//! duplication FAMILY toward drift and away from byte-identical repetition:
//!
//! - `duplication` exact-block scale dropped from `* 200` to `* 80` (see
//!   `EXACT_DUP_SCALE`). The notation-canonical exact-block detector is kept
//!   (lightningcss already collapses `0px`/`#fff`/`rgb()`), just down-weighted to
//!   a soft hint. The 20pt cap is unchanged.
//! - `token_erosion` gains a HARDCODED-value-sprawl drift sub-term (see
//!   `value_sprawl_term`) sourced from the previously-descriptive-only
//!   `summary.unique_box_shadows` / `unique_border_radii` / `unique_line_heights`
//!   distinct-value counts. It counts only HARDCODED literals: a system that
//!   tokenizes its scales via `var(--*)` scores 0 (lightningcss parses
//!   `box-shadow: var(--x)` as `Property::Unparsed`, so var-referenced and
//!   `@theme`-defined values never reach the typed-property collectors). Per-axis
//!   baselines (10/8/6) clear every observed real design system; the term
//!   saturates and is sub-capped at 5pt inside the unchanged 10pt category.
//!
//! The original v3 plan (re-canonicalize the duplicate-block fingerprint to detect
//! drift clones) was DROPPED after a Step-0 audit found it a no-op: the css-metrics
//! fingerprint serializes via lightningcss `to_css_string`, which already
//! canonicalizes `0px`/`#fff`/`rgb()`, so it is notation-semantic, not byte-literal.
//! Genuine value drift is surfaced via the un-tokenized-literal sprawl count above.
//!
//! ## Deliberately excluded signals
//!
//! `custom_properties_unreferenced` and `custom_properties_undefined` are
//! intentionally NOT folded into this score. They are false-positive-prone for
//! exactly the projects this axis most needs to grade fairly: a design system
//! exports custom properties that are referenced by *consumers*, not within its
//! own package, so "unreferenced-in-package" is the intended state rather than a
//! smell; and in a cross-package monorepo a property defined in one package and
//! consumed in another reads as "undefined" to a single-package analysis. Both
//! signals stay available in the raw `CssAnalyticsReport` for descriptive
//! surfaces, but they do not move the grade.
//!
//! ## Confidence (descriptive metadata, NOT part of the formula)
//!
//! The grade carries a [`StylingHealthConfidence`] marking it `Low` when the
//! authored-declaration count is below `MIN_CONFIDENT_DECLARATIONS` (with a
//! stated reason), so a grade computed from a thin CSS surface is not presented
//! with the same authority as one from a full design system. Confidence NEVER
//! feeds the score: `score` / `grade` / `penalties` are byte-identical whether it
//! is high or low. The framing is "thin authored-CSS surface", not "unreliable
//! analysis": a utility-first Tailwind project legitimately authors little CSS,
//! so a low mark means the declaration-normalized rubric had little to measure,
//! not that fallow's analysis failed. The EMPTY case (no import-reachable
//! stylesheet, so `css_analytics` and `styling_health` are both `None`) is the
//! strongest form of withholding and is handled one layer up by the human "No
//! stylesheets analyzed" note; this function only ever runs on a non-empty report.
//!
//! ## Calibration (v3, corpus-locked)
//!
//! The v2 weights were validated against a 10-project corpus (3 design systems /
//! SCSS, 4 Tailwind apps, 3 empty / CSS-in-JS); v3 re-ran that corpus plus the 3c
//! atomic CSS-in-JS smoke (Braid / vanilla-extract / StyleX / Panda / emotion) and
//! drift anchors. [`STYLING_HEALTH_FORMULA_VERSION`] bumps 2 -> 3 because rubric
//! constants moved (`EXACT_DUP_SCALE` and the new sprawl sub-term). v3 result, NO
//! band misclassification: every real design system stays A with a 0 sprawl term
//! (utrecht 5 distinct hardcoded shadows < baseline 10; rijkshuisstijl, jsonforms
//! 7 radii < baseline 8, all 0pt sprawl); atomic CSS-in-JS gains no false drift
//! penalty (<= 3 distinct values per axis); a clean token-driven system keeps a 0
//! sprawl term (var-referenced values are uncounted). The sprawl term fires only on
//! genuine hardcoded drift (a 16-distinct-per-axis anchor scores ~4pt). The
//! duplication down-weight moves moderate-dup projects by ~1-2pt (no band flip).
//!
//! Consumer note: a v2 -> v3 bump moves `styling_health.score`/`grade` for some
//! projects, so `--format json` snapshot diffs (including fallow's own golden
//! snapshots) and styling-grade trend dashboards see a one-time step-change at the
//! version boundary; gate on `formula_version`. No exit-code, badge, gate, regression
//! baseline, or trend-snapshot consumes the styling score, so nothing fails; the
//! code `health_score` is byte-unchanged.

use fallow_output::{
    CssAnalyticsReport, STYLING_HEALTH_FORMULA_VERSION, StylingHealth, StylingHealthConfidence,
    StylingHealthPenalties, letter_grade,
};

const DUPLICATION_CAP: f64 = 20.0;
const DEAD_SURFACE_CAP: f64 = 20.0;

/// Per-removable-declaration scale for the EXACT-block duplication penalty (v3).
/// Down-weighted from the v2 `* 200` because exact (notation-canonical) CSS
/// duplication is the LEAST-harmful CSS pattern: repeated declarations gzip away,
/// graphical properties are loosely coupled, and CSS has no native abstraction so
/// some repetition is unavoidable. Exact duplication therefore stays a soft hint,
/// not a dominant term; the maintenance harm CSS tooling should weight is value
/// DRIFT (the `token_erosion` sprawl sub-term below), not byte-identical repeats.
/// At `* 80` the category caps at 25% removable declarations (was 10% at `* 200`).
const EXACT_DUP_SCALE: f64 = 80.0;
const BROKEN_REFERENCES_CAP: f64 = 15.0;
const TOKEN_EROSION_CAP: f64 = 10.0;
const STRUCTURAL_CAP: f64 = 10.0;

/// Authored-CSS declaration floor below which the grade is marked low-confidence.
/// Below this, the declaration-normalized penalty ratios are hypersensitive: a
/// single minimal duplicate block (4 declarations appearing twice = 4 removable)
/// contributes `4 / total * 200` to duplication, which is the full 20pt cap at 40
/// declarations and 16pt at 50, and a handful of `!important` declarations
/// likewise pushes the structural penalty to its cap. So below ~50 authored
/// declarations a single finding can move an entire penalty category to its
/// ceiling and the grade reflects sampling noise rather than systematic quality;
/// above it, individual findings contribute proportionally. Empirically separates
/// the calibration corpus: fallow-tools (24 declarations) and leenders-coaching
/// (38) read as thin authored surfaces, while every design system and the
/// `>= 145`-declaration Tailwind apps stay confident.
const MIN_CONFIDENT_DECLARATIONS: u32 = 50;

/// Share of analyzed declarations originating from flat-by-construction atomic
/// object CSS-in-JS (StyleX/Panda) at or above which the grade is marked
/// low-confidence: the structural axis (nesting, `!important` density) is inert
/// for compile-time-atomic CSS, so a predominantly-atomic project's grade
/// reflects token hygiene only. Distinct from [`MIN_CONFIDENT_DECLARATIONS`]
/// (a thin-surface caveat); when both fire the atomic caveat is the more
/// informative one and wins the single `confidence_reason` slot.
const ATOMIC_CONFIDENCE_SHARE: f64 = 0.7;

/// `!important` density (as a percentage of declarations) below which no
/// structural penalty accrues. A small amount of `!important` is normal.
const IMPORTANT_DENSITY_FLOOR: f64 = 5.0;

/// Style-rule nesting depth at or below which no structural penalty accrues.
/// Shallow nesting is healthy; the penalty grows past this floor.
const NESTING_DEPTH_FLOOR: f64 = 4.0;

/// The number of distinct `font-size` units considered a healthy baseline (e.g.
/// `rem` for type plus `px` for fixed chrome). Each unit beyond this erodes the
/// token system.
const FONT_SIZE_UNIT_BASELINE: u32 = 2;

/// `dead_surface` unused-`@theme`-token term scale (v2): the unused-token count
/// is normalized by the TOTAL number of `@theme` tokens defined (a per-population
/// death ratio that is `>= 0`; the [`TOKEN_DEATH_TERM_CAP`] `.min` bounds the
/// term even if a caller violates the population invariant and the ratio exceeds
/// 1.0), then multiplied by this factor. So 100% of defined tokens dead
/// contributes the full [`TOKEN_DEATH_TERM_CAP`], 20% dead ~= 3pt. This is the
/// principled, size-independent denominator: Tailwind projects author almost no
/// CSS declarations, so the v1 `total_declarations` denominator let a few unused
/// tokens explode the penalty; dividing by the token population fixes that
/// without rewarding or punishing project size.
const TOKEN_DEATH_SCALE: f64 = 15.0;

/// `dead_surface` unused-`@theme`-token term cap (v2). Bounds the token-death
/// contribution so even a fully-dead token population leaves room for the other
/// dead-entity term within the 20pt category cap.
const TOKEN_DEATH_TERM_CAP: f64 = 15.0;

/// `dead_surface` non-token-dead-entity term scale (v2): unreferenced classes,
/// unused `@property`/`@layer` at-rules, and dead `@font-face` families are
/// normalized by `total_declarations` (the same size-stable denominator the
/// duplication penalty uses) and multiplied by this factor before the term cap.
const OTHER_DEAD_SCALE: f64 = 150.0;

/// `dead_surface` non-token-dead-entity term cap (v2). Bounds the
/// declaration-share dead-entity contribution so it cannot dominate the category
/// on its own; the token-death term carries the rest of the 20pt budget.
const OTHER_DEAD_TERM_CAP: f64 = 8.0;

/// `token_erosion` per-extra-unit weight (v2). Each distinct `font-size` unit
/// past [`FONT_SIZE_UNIT_BASELINE`] adds this many points to the unit term.
const FONT_SIZE_UNIT_WEIGHT: f64 = 2.0;

/// `token_erosion` font-size-unit term cap (v2). Bounds the unit contribution so
/// `font-size` units alone can never dominate the category; the arbitrary-value
/// term carries the rest of the budget.
const FONT_SIZE_UNIT_TERM_CAP: f64 = 4.0;

/// `token_erosion` arbitrary-value divisor (v2). Distinct Tailwind arbitrary
/// values are divided by this before the arbitrary term cap, so the term
/// saturates gently around ~50-100+ distinct values instead of instant-capping
/// at 10. Normal Tailwind usage yields a moderate penalty, not the ceiling.
const ARBITRARY_VALUE_DIVISOR: f64 = 18.0;

/// `token_erosion` arbitrary-value term cap (v2). Bounds the arbitrary-value
/// contribution so the unit term still has room within the 10pt category cap.
const ARBITRARY_VALUE_TERM_CAP: f64 = 8.0;

/// `token_erosion` hardcoded-value-sprawl baselines (v3), per axis: the count of
/// distinct HARDCODED literal values for a should-be-tokenized property below
/// which no sprawl penalty accrues. A clean token-driven system reuses a small
/// palette via `var(--*)` (which lightningcss parses as `Property::Unparsed`, so
/// var-referenced and `@theme`-defined values are NOT counted), so its hardcoded
/// distinct count is ~0; a drifted system hardcodes many ad-hoc values. Baselines
/// are set to each property's natural FULL-SCALE cardinality and conservatively
/// (so a legitimately rich scale is not penalized), corpus-validated to clear
/// every real design system (utrecht 5 shadows, jsonforms 7 radii) at 0:
/// - shadow: rich elevation systems (Material authors up to ~24 levels), the
///   widest legit range, so the highest baseline.
/// - radius: a complete radius scale (none/xs..2xl/full) is ~8.
/// - line-height: a complete line-height scale (tight..loose + reset) is ~6.
const SHADOW_SPRAWL_BASELINE: u32 = 10;
const RADIUS_SPRAWL_BASELINE: u32 = 8;
const LINE_HEIGHT_SPRAWL_BASELINE: u32 = 6;

/// `token_erosion` sprawl divisor (v3). The summed per-axis excess (distinct
/// hardcoded values above each baseline) is divided by this before the sprawl
/// sub-cap, so the term saturates gently (mirroring [`ARBITRARY_VALUE_DIVISOR`]):
/// a large multi-brand monorepo accumulates a bounded, slowly-growing penalty
/// rather than a per-value cliff. Deliberately UN-NORMALIZED by declaration count:
/// normalizing would rebuild the v1 size-denominator bug in reverse (20 drifted
/// radii over 100k declarations would round to nothing, masking real drift),
/// whereas a literals-only project-wide count is bounded by design INTENT, not
/// file count (utrecht is 792 declarations yet only 5 distinct shadows).
const SPRAWL_DIVISOR: f64 = 6.0;

/// `token_erosion` sprawl sub-term cap (v3). Bounds the hardcoded-value-sprawl
/// contribution within the unchanged [`TOKEN_EROSION_CAP`] (10pt), summed with
/// the unit + arbitrary terms. The category cap is NOT raised, so the existing
/// font-size-unit / Tailwind-arbitrary cohort is unaffected; when those terms
/// already approach the 10pt ceiling the sprawl term is mildly diluted (it
/// under-counts, never false-positives, the correct direction for a descriptive
/// grade).
const SPRAWL_TERM_CAP: f64 = 5.0;

/// Internal scoring inputs threaded into the styling-health grade, NOT serialized
/// (mirrors the prior `theme_tokens_defined` parameter, so the wire contract /
/// schema is unchanged). The declaration counts are split into an atomic and a
/// non-atomic share so flat-by-construction atomic object CSS-in-JS (StyleX,
/// Panda) does not dilute the declaration-normalized penalties or invert the
/// confidence knee: every penalty that divides by a declaration count divides by
/// the NON-ATOMIC count, and the confidence trigger reads the non-atomic count
/// plus the atomic share. For authored `.css` / SFC / non-atomic CSS-in-JS the
/// non-atomic counts equal the summary aggregates and `atomic_declarations` is 0,
/// so the grade is byte-identical to the pre-object-CSS-in-JS behavior.
#[derive(Debug, Clone, Copy, Default)]
pub struct StylingScoringInputs {
    /// Total Tailwind `@theme` tokens DEFINED across the project (the population
    /// from which `summary.unused_theme_tokens` is a subset). Denominator for the
    /// per-population token-death ratio in `dead_surface`.
    pub theme_tokens_defined: u32,
    /// Declarations from CSS whose structure is meaningful: authored `.css`, SFC
    /// `<style>`, and non-atomic CSS-in-JS (vanilla-extract / emotion, object +
    /// template). The denominator for every declaration-normalized penalty and
    /// the confidence trigger.
    pub non_atomic_declarations: u32,
    /// `!important` declarations from the non-atomic surface (structural numerator).
    pub non_atomic_important_declarations: u32,
    /// Deepest nesting from the non-atomic surface (structural nesting term).
    pub non_atomic_max_nesting_depth: u8,
    /// Declarations from flat atomic object CSS-in-JS (StyleX/Panda). Excluded
    /// from the penalties; drives the predominantly-atomic confidence caveat.
    pub atomic_declarations: u32,
}

impl StylingScoringInputs {
    /// Build inputs for a report with no atomic object CSS-in-JS: the non-atomic
    /// surface IS the whole summary, so the grade matches the pre-3c behavior.
    /// Used by the back-compat [`compute_styling_health`] entry and unit tests.
    #[allow(
        dead_code,
        reason = "back-compat test helper; production uses explicit StylingScoringInputs"
    )]
    #[must_use]
    pub fn from_report(report: &CssAnalyticsReport, theme_tokens_defined: u32) -> Self {
        let s = &report.summary;
        Self {
            theme_tokens_defined,
            non_atomic_declarations: s.total_declarations,
            non_atomic_important_declarations: s.important_declarations,
            non_atomic_max_nesting_depth: s.max_nesting_depth,
            atomic_declarations: 0,
        }
    }
}

/// Compute the styling-health score from the structural CSS analytics, treating
/// the whole report as the gradeable surface (no atomic object CSS-in-JS split).
///
/// Mirrors the code score: penalties subtract from a starting 100, the result is
/// rounded and clamped, and the grade reuses the shared [`letter_grade`]
/// thresholds. See the module docs for the per-category rubric (v2).
///
/// `theme_tokens_defined` is the total number of Tailwind `@theme` tokens DEFINED
/// across the project. It is an internal scoring input only, NOT a serialized
/// field, so the wire contract / schema is unchanged.
#[allow(
    dead_code,
    reason = "back-compat test helper; production uses compute_styling_health_with_inputs"
)]
#[must_use]
pub fn compute_styling_health(
    report: &CssAnalyticsReport,
    theme_tokens_defined: u32,
) -> StylingHealth {
    compute_styling_health_with_inputs(
        report,
        &StylingScoringInputs::from_report(report, theme_tokens_defined),
    )
}

/// Compute the styling-health score from the structural CSS analytics plus the
/// atomic / non-atomic declaration split (CSS program Phase 3c). The
/// declaration-normalized penalties and the confidence trigger read the
/// non-atomic counts in `inputs`, so flat atomic object CSS-in-JS does not dilute
/// the grade nor inflate confidence. The descriptive `report` is still the source
/// for the non-ratio signals (`token_erosion`, `broken_references`) and the
/// serialized aggregates.
#[must_use]
pub fn compute_styling_health_with_inputs(
    report: &CssAnalyticsReport,
    inputs: &StylingScoringInputs,
) -> StylingHealth {
    let penalties = compute_styling_penalties(report, inputs);
    let score = apply_styling_penalties(&penalties);
    let grade = letter_grade(score);
    let (confidence, confidence_reason) = styling_confidence(report, inputs);

    StylingHealth {
        formula_version: STYLING_HEALTH_FORMULA_VERSION,
        score,
        grade,
        penalties,
        confidence,
        confidence_reason,
    }
}

/// Classify the grade's confidence from the analyzed CSS surface. `Low` in two
/// cases, with the more informative reason winning the single slot:
///
/// 1. **Predominantly atomic** (precedence): at least [`ATOMIC_CONFIDENCE_SHARE`]
///    of analyzed declarations come from flat compile-time-atomic object
///    CSS-in-JS (StyleX/Panda), whose structure (nesting, `!important` density)
///    is inert, so the grade reflects token hygiene only, not structure.
/// 2. **Thin authored surface**: the non-atomic (gradeable) declaration count is
///    below [`MIN_CONFIDENT_DECLARATIONS`], so the declaration-normalized ratios
///    are hypersensitive.
///
/// `High` (no reason) otherwise. Descriptive metadata only: it never feeds the
/// score. See the module docs for why the framing is a property of the CSS
/// surface, not "unreliable analysis".
fn styling_confidence(
    report: &CssAnalyticsReport,
    inputs: &StylingScoringInputs,
) -> (StylingHealthConfidence, Option<String>) {
    let total = inputs
        .non_atomic_declarations
        .saturating_add(inputs.atomic_declarations);
    if inputs.atomic_declarations > 0
        && total > 0
        && f64::from(inputs.atomic_declarations) / f64::from(total) >= ATOMIC_CONFIDENCE_SHARE
    {
        // Atomic CSS is flat at the COMPILED layer; the source rules fallow lifts
        // are not representative of structure, so the structural axis is inert.
        let reason = "structure (nesting, !important density) is not assessable for \
                      compile-time-atomic CSS-in-JS (StyleX/Panda); this grade reflects \
                      token hygiene only"
            .to_string();
        return (StylingHealthConfidence::Low, Some(reason));
    }
    if inputs.non_atomic_declarations >= MIN_CONFIDENT_DECLARATIONS {
        return (StylingHealthConfidence::High, None);
    }
    let reason = format!(
        "graded from only {} declaration{} across {} stylesheet{}",
        inputs.non_atomic_declarations,
        if inputs.non_atomic_declarations == 1 {
            ""
        } else {
            "s"
        },
        report.summary.files_analyzed,
        if report.summary.files_analyzed == 1 {
            ""
        } else {
            "s"
        },
    );
    (StylingHealthConfidence::Low, Some(reason))
}

fn compute_styling_penalties(
    report: &CssAnalyticsReport,
    inputs: &StylingScoringInputs,
) -> StylingHealthPenalties {
    StylingHealthPenalties {
        duplication: duplication_penalty(report, inputs),
        dead_surface: dead_surface_penalty(report, inputs),
        broken_references: broken_references_penalty(report),
        token_erosion: token_erosion_penalty(report),
        structural: structural_penalty(inputs),
    }
}

fn apply_styling_penalties(penalties: &StylingHealthPenalties) -> f64 {
    let mut score = 100.0_f64;
    score -= penalties.duplication;
    score -= penalties.dead_surface;
    score -= penalties.broken_references;
    score -= penalties.token_erosion;
    score -= penalties.structural;
    round1(score).clamp(0.0, 100.0)
}

/// Copy-paste declaration blocks: penalize by the share of all declarations that
/// could be removed by consolidating duplicate blocks. ~10% removable -> full cap.
///
/// Atomic object CSS-in-JS (StyleX/Panda) is excluded from duplicate-block
/// fingerprinting upstream, so `duplicate_declarations_total` is already a
/// non-atomic numerator; dividing by the non-atomic declaration count keeps the
/// ratio from being diluted by flat atomic declarations flooding the denominator.
fn duplication_penalty(report: &CssAnalyticsReport, inputs: &StylingScoringInputs) -> f64 {
    let removable = f64::from(report.summary.duplicate_declarations_total);
    let total = f64::from(inputs.non_atomic_declarations).max(1.0);
    round1((removable / total * EXACT_DUP_SCALE).min(DUPLICATION_CAP))
}

/// Dead styling surface, computed as two independently-normalized terms:
///
/// 1. **Token-death term** ([`TOKEN_DEATH_SCALE`], capped at
///    [`TOKEN_DEATH_TERM_CAP`]): unused `@theme` tokens as a share of ALL defined
///    `@theme` tokens (`theme_tokens_defined`). This per-population death ratio is
///    size-independent. v1 divided unused tokens by `total_declarations`, which
///    exploded for Tailwind projects (they author almost no CSS declarations, so
///    a few unused tokens capped the whole category); normalizing by the token
///    population the tokens are drawn from is the principled fix.
/// 2. **Other-dead term** ([`OTHER_DEAD_SCALE`], capped at
///    [`OTHER_DEAD_TERM_CAP`]): unreferenced classes, unused `@property`/`@layer`
///    at-rules, and dead `@font-face` families as a share of `total_declarations`
///    (the same size-stable denominator the duplication penalty uses). These
///    scale with authored-CSS size, so the declaration denominator is correct for
///    them; only the `@theme` tokens needed the per-population treatment.
///
/// The two terms are summed and capped at [`DEAD_SURFACE_CAP`]. Both `@theme`
/// tokens are EXCLUDED from the other-dead term (they live only in term 1). The
/// other-dead entities are authored-CSS constructs (no atomic object CSS-in-JS
/// contributes a class / at-rule / `@font-face` to them), so the term is
/// normalized by the non-atomic declaration count to avoid atomic dilution.
fn dead_surface_penalty(report: &CssAnalyticsReport, inputs: &StylingScoringInputs) -> f64 {
    let s = &report.summary;

    let token_population = f64::from(inputs.theme_tokens_defined).max(1.0);
    let token_death_ratio = f64::from(s.unused_theme_tokens) / token_population;
    let token_term = (token_death_ratio * TOKEN_DEATH_SCALE).min(TOKEN_DEATH_TERM_CAP);

    let other_dead = f64::from(
        s.unreferenced_css_classes
            .saturating_add(s.unused_property_registrations)
            .saturating_add(s.unused_layers)
            .saturating_add(s.unused_font_faces),
    );
    let total = f64::from(inputs.non_atomic_declarations).max(1.0);
    let other_term = (other_dead / total * OTHER_DEAD_SCALE).min(OTHER_DEAD_TERM_CAP);

    round1((token_term + other_term).min(DEAD_SURFACE_CAP))
}

/// Broken references: markup classes one edit from a defined class, plus
/// animations referencing a `@keyframes` defined nowhere. Each is a likely typo
/// or stale rename; 5 broken refs reach the cap.
fn broken_references_penalty(report: &CssAnalyticsReport) -> f64 {
    let s = &report.summary;
    let broken = f64::from(
        s.unresolved_class_references
            .saturating_add(s.keyframes_undefined),
    );
    round1((broken * 3.0).min(BROKEN_REFERENCES_CAP))
}

/// Design-token erosion: mixing `font-size` units past a healthy baseline and
/// Tailwind arbitrary-value bypasses both work against a single source of truth
/// for the scale.
///
/// v2 splits the category into two saturating terms. The unit term is capped at
/// [`FONT_SIZE_UNIT_TERM_CAP`] so `font-size` units alone cannot dominate. The
/// arbitrary-value term divides the distinct-value count by
/// [`ARBITRARY_VALUE_DIVISOR`] and caps at [`ARBITRARY_VALUE_TERM_CAP`], so it
/// saturates gently around ~50-100+ distinct values instead of instant-capping
/// the whole category at 10 (v1 reached the ceiling at just 10 arbitrary
/// values, punishing normal Tailwind usage). The category cap stays 10pt.
fn token_erosion_penalty(report: &CssAnalyticsReport) -> f64 {
    let s = &report.summary;
    let extra_units = f64::from(
        s.font_size_units_used
            .saturating_sub(FONT_SIZE_UNIT_BASELINE),
    );
    let unit_term = (extra_units * FONT_SIZE_UNIT_WEIGHT).min(FONT_SIZE_UNIT_TERM_CAP);
    let arbitrary = f64::from(s.tailwind_arbitrary_values);
    let arbitrary_term = (arbitrary / ARBITRARY_VALUE_DIVISOR).min(ARBITRARY_VALUE_TERM_CAP);
    round1((unit_term + arbitrary_term + value_sprawl_term(s)).min(TOKEN_EROSION_CAP))
}

/// Hardcoded-value-sprawl sub-term (v3): the distinct count of hardcoded literal
/// `box-shadow` / `border-radius` / `line-height` values above each per-axis
/// healthy baseline, summed and saturated. This is the design-token DRIFT signal
/// the v3 reweight shifts the duplication-family weight toward: a system that
/// tokenizes its scales via `var(--*)` scores 0 (var-referenced values are
/// invisible to the `unique_*` counts), while one that hardcodes many ad-hoc
/// values accrues a bounded, gently-growing penalty. Counts come from
/// `summary.unique_*` (recomputed at health time, never cached); see the module
/// docs and the [`SHADOW_SPRAWL_BASELINE`] / [`SPRAWL_DIVISOR`] rationale.
fn value_sprawl_term(s: &fallow_output::CssAnalyticsSummary) -> f64 {
    let excess = s
        .unique_box_shadows
        .saturating_sub(SHADOW_SPRAWL_BASELINE)
        .saturating_add(s.unique_border_radii.saturating_sub(RADIUS_SPRAWL_BASELINE))
        .saturating_add(
            s.unique_line_heights
                .saturating_sub(LINE_HEIGHT_SPRAWL_BASELINE),
        );
    (f64::from(excess) / SPRAWL_DIVISOR).min(SPRAWL_TERM_CAP)
}

/// Structural smells: `!important` density above a healthy floor and deep
/// style-rule nesting past a shallow floor.
///
/// Computed over the NON-ATOMIC surface only: flat compile-time-atomic object
/// CSS-in-JS (StyleX/Panda) has zero `!important` and minimal nesting by
/// construction, so including it would dilute the `!important` density (lowering
/// the penalty) and never raise nesting, trivially inflating the grade.
fn structural_penalty(inputs: &StylingScoringInputs) -> f64 {
    let important_pct = if inputs.non_atomic_declarations > 0 {
        f64::from(inputs.non_atomic_important_declarations)
            / f64::from(inputs.non_atomic_declarations)
            * 100.0
    } else {
        0.0
    };
    let important = (important_pct - IMPORTANT_DENSITY_FLOOR).max(0.0);
    let nesting = (f64::from(inputs.non_atomic_max_nesting_depth) - NESTING_DEPTH_FLOOR).max(0.0);
    round1((important + nesting).min(STRUCTURAL_CAP))
}

fn round1(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

#[cfg(test)]
mod tests;
