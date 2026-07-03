use super::*;
use fallow_output::{CssAnalyticsReport, CssAnalyticsSummary};

/// A `@theme`-token population large enough that any incidental
/// `unused_theme_tokens` set by a test for an UNRELATED category contributes a
/// negligible token-death term, isolating that category. Tests that exercise the
/// token-death term itself call [`compute_styling_health`] with an explicit
/// population instead.
const MANY_TOKENS: u32 = 10_000;

/// Score a report with a token population large enough to neutralize the
/// `dead_surface` token-death term, for tests that are not about that term.
fn score(report: &CssAnalyticsReport) -> StylingHealth {
    compute_styling_health(report, MANY_TOKENS)
}

/// Assert two scores / penalties match within rounding tolerance. The scoring
/// pipeline rounds to one decimal, so an exact float compare is both lint-noisy
/// (`clippy::float_cmp`) and brittle; a 0.05 window is well under one rounding
/// step.
fn approx(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() < 0.05,
        "expected ~{expected}, got {actual}"
    );
}

/// A CSS report whose summary is all-zero (every signal clean) but which is a
/// real, non-empty report (one analyzed stylesheet).
fn clean_report() -> CssAnalyticsReport {
    CssAnalyticsReport {
        files: Vec::new(),
        summary: CssAnalyticsSummary {
            files_analyzed: 1,
            total_declarations: 50,
            ..CssAnalyticsSummary::default()
        },
        scoped_unused: Vec::new(),
        unreferenced_keyframes: Vec::new(),
        undefined_keyframes: Vec::new(),
        duplicate_declaration_blocks: Vec::new(),
        cva_duplicate_variant_blocks: Vec::new(),
        cva_variant_token_drifts: Vec::new(),
        tailwind_arbitrary_values: Vec::new(),
        raw_style_values: Vec::new(),
        unused_at_rules: Vec::new(),
        unresolved_class_references: Vec::new(),
        unreferenced_css_classes: Vec::new(),
        unused_font_faces: Vec::new(),
        unused_theme_tokens: Vec::new(),
        near_duplicate_theme_tokens: Vec::new(),
        token_consumers: Vec::new(),
        font_size_unit_mix: None,
    }
}

#[test]
fn clean_report_scores_100_grade_a() {
    let styling = score(&clean_report());
    assert_eq!(styling.formula_version, STYLING_HEALTH_FORMULA_VERSION);
    approx(styling.score, 100.0);
    assert_eq!(styling.grade, "A");
    let p = &styling.penalties;
    approx(p.duplication, 0.0);
    approx(p.dead_surface, 0.0);
    approx(p.broken_references, 0.0);
    approx(p.token_erosion, 0.0);
    approx(p.structural, 0.0);
}

#[test]
fn duplication_penalty_is_down_weighted_and_caps() {
    // v3 down-weight: exact CSS dup is the least-harmful pattern, so the scale
    // dropped from `* 200` to `* 80`. 5 removable out of 100 = 5% -> 5/100 * 80 =
    // 4pt (was 10pt under v2).
    let mut report = clean_report();
    report.summary.total_declarations = 100;
    report.summary.duplicate_declarations_total = 5;
    let styling = score(&report);
    approx(styling.penalties.duplication, 4.0);

    // 20 removable out of 100 = 20% -> 20/100 * 80 = 16pt, still UNDER the 20pt cap
    // (would have capped at 20pt under v2's `* 200`).
    report.summary.duplicate_declarations_total = 20;
    let styling = score(&report);
    approx(styling.penalties.duplication, 16.0);

    // Cap now needs ~25% removable: 30 out of 100 -> 24pt uncapped, capped at 20pt.
    report.summary.duplicate_declarations_total = 30;
    let styling = score(&report);
    approx(styling.penalties.duplication, DUPLICATION_CAP);
}

#[test]
fn dead_surface_token_death_term_is_per_population() {
    // v2 token term = min(unused_theme_tokens / max(theme_tokens_defined, 1) * 15, 15).
    // The denominator is the @theme-token POPULATION, not total_declarations, so a
    // few unused tokens in a declaration-sparse Tailwind project no longer explode.

    // HIGH death-ratio with FEW tokens defined -> high token term.
    // 8 unused out of 10 defined = 0.8 ratio -> 0.8 * 15 = 12pt (capped at 15).
    let mut report = clean_report();
    report.summary.total_declarations = 24; // a sparse Tailwind project
    report.summary.unused_theme_tokens = 8;
    let styling = compute_styling_health(&report, 10);
    approx(styling.penalties.dead_surface, 12.0);

    // LOW death-ratio with MANY tokens defined -> low token term, EVEN with a
    // declaration-sparse project. 8 unused out of 400 defined = 0.02 -> 0.3pt.
    // (Under the old `total_declarations` denominator this same input would have
    // been 8/24*150 = 50pt, capped at 20: the explosion this fix removes.)
    let styling = compute_styling_health(&report, 400);
    approx(styling.penalties.dead_surface, 0.3);

    // The fallow-tools regression: 4 unused tokens over 24 declarations. Under v1
    // (declaration-share) this capped dead_surface at 20; with the population
    // denominator it is a 4/N ratio. With 32 defined tokens: 4/32*15 = 1.875 -> 1.9pt.
    report.summary.unused_theme_tokens = 4;
    let styling = compute_styling_health(&report, 32);
    approx(styling.penalties.dead_surface, 1.9);

    // A saturated token term caps at 15pt. This deliberately VIOLATES the
    // population invariant (50 unused > 20 defined) to prove the
    // `.min(TOKEN_DEATH_TERM_CAP)` bounds the term even when a caller passes a
    // population smaller than the unused count and the ratio exceeds 1.0;
    // production cannot reach that state (both counts derive from the same
    // `theme_token_definers` map), but `compute_styling_health` is `pub`.
    report.summary.unused_theme_tokens = 50;
    let styling = compute_styling_health(&report, 20);
    approx(styling.penalties.dead_surface, TOKEN_DEATH_TERM_CAP);
}

#[test]
fn dead_surface_other_term_scales_by_declaration_share_and_is_size_stable() {
    // The non-token dead entities (unreferenced classes, unused at-rules, dead
    // @font-face) still divide by total_declarations: a size-stable declaration
    // share, NOT a per-file count. token population is huge here so the token term
    // is ~0 and we isolate the other-dead term.
    // 2 dead over 50 declarations = 2/50*150 = 6pt.
    let mut report = clean_report();
    report.summary.total_declarations = 50;
    report.summary.unreferenced_css_classes = 2;
    let styling = compute_styling_health(&report, MANY_TOKENS);
    approx(styling.penalties.dead_surface, 6.0);

    // Stylesheet count is irrelevant: 32 files, same ratio -> same penalty.
    report.summary.files_analyzed = 32;
    let styling = compute_styling_health(&report, MANY_TOKENS);
    approx(styling.penalties.dead_surface, 6.0);

    // The other-dead term is capped at 8pt on its own: 10 dead over 50 = 30pt
    // uncapped -> capped at 8.
    report.summary.files_analyzed = 1;
    report.summary.unreferenced_css_classes = 4;
    report.summary.unused_property_registrations = 3;
    report.summary.unused_layers = 2;
    report.summary.unused_font_faces = 1;
    let styling = compute_styling_health(&report, MANY_TOKENS);
    approx(styling.penalties.dead_surface, OTHER_DEAD_TERM_CAP);
}

#[test]
fn dead_surface_combines_terms_and_caps_at_category_ceiling() {
    // Both terms maxed: token term 15 (100% dead population) + other term 8
    // (saturated) = 23 uncapped, capped at the 20pt category ceiling.
    let mut report = clean_report();
    report.summary.total_declarations = 50;
    report.summary.unused_theme_tokens = 30;
    report.summary.unreferenced_css_classes = 20; // 20/50*150 = 60 -> capped 8
    let styling = compute_styling_health(&report, 30);
    approx(styling.penalties.dead_surface, DEAD_SURFACE_CAP);

    // Declaration count independence for the token term: doubling declarations
    // (which would shrink a declaration-share denominator) leaves the token term
    // unchanged because it is normalized by the token population, not declarations.
    let mut a = clean_report();
    a.summary.total_declarations = 24;
    a.summary.unused_theme_tokens = 6;
    let mut b = clean_report();
    b.summary.total_declarations = 480; // 20x the declarations
    b.summary.unused_theme_tokens = 6;
    let styling_a = compute_styling_health(&a, 40);
    let styling_b = compute_styling_health(&b, 40);
    // 6/40*15 = 2.25 -> 2.3pt in BOTH, independent of declaration count.
    approx(styling_a.penalties.dead_surface, 2.3);
    approx(styling_b.penalties.dead_surface, 2.3);
}

#[test]
fn broken_references_penalty_scales_and_caps() {
    // 2 broken refs (1 class + 1 undefined keyframe) -> 6pt.
    let mut report = clean_report();
    report.summary.unresolved_class_references = 1;
    report.summary.keyframes_undefined = 1;
    let styling = score(&report);
    approx(styling.penalties.broken_references, 6.0);

    // 10 broken refs -> 30pt uncapped, capped at 15pt.
    report.summary.unresolved_class_references = 10;
    report.summary.keyframes_undefined = 0;
    let styling = score(&report);
    approx(styling.penalties.broken_references, BROKEN_REFERENCES_CAP);
}

#[test]
fn token_erosion_penalty_saturates_gently() {
    // v2: unit term = min(extra_units * 2, 4); arbitrary term = min(arb / 18, 8).
    // 3 font-size units (1 over the baseline of 2) -> unit_term 2.0; +1 arbitrary
    // value -> 1/18 ~= 0.06 -> total ~2.1pt.
    let mut report = clean_report();
    report.summary.font_size_units_used = 3;
    report.summary.tailwind_arbitrary_values = 1;
    let styling = score(&report);
    approx(styling.penalties.token_erosion, 2.1);

    // Below/at baseline units alone contribute nothing.
    report.summary.font_size_units_used = 2;
    report.summary.tailwind_arbitrary_values = 0;
    let styling = score(&report);
    approx(styling.penalties.token_erosion, 0.0);

    // A moderate number of arbitrary values yields a MODERATE penalty, not the
    // ceiling: 50 / 18 ~= 2.8pt (v1 instant-capped here at 10pt).
    report.summary.tailwind_arbitrary_values = 50;
    let styling = score(&report);
    approx(styling.penalties.token_erosion, 2.8);

    // The font-size-unit term alone is capped (so units cannot dominate): 10
    // units = 8 over the baseline -> 8 * 2 = 16 uncapped, capped at 4pt.
    report.summary.font_size_units_used = 10;
    report.summary.tailwind_arbitrary_values = 0;
    let styling = score(&report);
    approx(styling.penalties.token_erosion, 4.0);

    // Only a very high arbitrary-value count plus mixed units reaches the 10pt
    // category cap: unit_term 4 (10 units) + arbitrary_term 8 (>=144 values) = 12,
    // capped at 10pt.
    report.summary.tailwind_arbitrary_values = 200;
    let styling = score(&report);
    approx(styling.penalties.token_erosion, TOKEN_EROSION_CAP);
}

#[test]
fn value_sprawl_below_per_axis_baselines_is_zero() {
    // A real design system's hardcoded-value counts (utrecht: 5 shadows / 4 radii
    // / 2 line-heights, all below the 10 / 8 / 6 baselines) contribute NO sprawl
    // penalty, so token_erosion stays 0 (the FP-safety property the corpus pins).
    let mut report = clean_report();
    report.summary.unique_box_shadows = 5;
    report.summary.unique_border_radii = 4;
    report.summary.unique_line_heights = 2;
    let styling = score(&report);
    approx(styling.penalties.token_erosion, 0.0);

    // jsonforms' 7 radii sit exactly under the radius baseline of 8 -> still 0.
    report.summary.unique_border_radii = 7;
    report.summary.unique_line_heights = 4;
    report.summary.unique_box_shadows = 1;
    let styling = score(&report);
    approx(styling.penalties.token_erosion, 0.0);
}

#[test]
fn value_sprawl_fires_above_baselines_saturates_and_caps() {
    // Per-axis excess above the 10 / 8 / 6 baselines, summed and divided by 6.
    // Heavy drift 16/16/16 -> excess 6 + 8 + 10 = 24 -> 24 / 6 = 4pt.
    let mut report = clean_report();
    report.summary.unique_box_shadows = 16;
    report.summary.unique_border_radii = 16;
    report.summary.unique_line_heights = 16;
    let styling = score(&report);
    approx(styling.penalties.token_erosion, 4.0);

    // A single sprawled axis still fires: 20 shadows -> excess 10 -> 10/6 = 1.67pt.
    let mut single = clean_report();
    single.summary.unique_box_shadows = 20;
    let styling = score(&single);
    approx(styling.penalties.token_erosion, 1.7);

    // Extreme sprawl saturates at the 5pt sub-cap (well inside the 10pt category):
    // 40/40/40 -> excess 30 + 32 + 34 = 96 -> 16 uncapped, capped at 5pt.
    let mut extreme = clean_report();
    extreme.summary.unique_box_shadows = 40;
    extreme.summary.unique_border_radii = 40;
    extreme.summary.unique_line_heights = 40;
    let styling = score(&extreme);
    approx(styling.penalties.token_erosion, 5.0);
}

#[test]
fn value_sprawl_sums_with_existing_token_erosion_terms_under_category_cap() {
    // The sprawl term sums with the unit + arbitrary terms, all bounded by the
    // unchanged 10pt category cap. 3 font-size units (2pt) + 50 arbitrary (~2.8pt)
    // + 16/16/16 sprawl (4pt) = ~8.8pt, under the 10pt cap.
    let mut report = clean_report();
    report.summary.font_size_units_used = 3;
    report.summary.tailwind_arbitrary_values = 50;
    report.summary.unique_box_shadows = 16;
    report.summary.unique_border_radii = 16;
    report.summary.unique_line_heights = 16;
    let styling = score(&report);
    approx(styling.penalties.token_erosion, 8.8);

    // When the unit + arbitrary terms already max the category, the sprawl term is
    // diluted by the 10pt cap (under-counts, never false-positives): 10 units (4pt)
    // + 200 arbitrary (8pt) + any sprawl still caps at 10pt.
    report.summary.font_size_units_used = 10;
    report.summary.tailwind_arbitrary_values = 200;
    let styling = score(&report);
    approx(styling.penalties.token_erosion, TOKEN_EROSION_CAP);
}

#[test]
fn structural_penalty_uses_important_density_and_nesting() {
    // 15% !important (10pt over the 5% floor) but nesting at the floor.
    let mut report = clean_report();
    report.summary.total_declarations = 100;
    report.summary.important_declarations = 15;
    report.summary.max_nesting_depth = 4;
    let styling = score(&report);
    approx(styling.penalties.structural, 10.0);

    // Deep nesting alone: depth 7 is 3 over the floor of 4 -> 3pt.
    report.summary.important_declarations = 5; // exactly at the floor -> 0
    report.summary.max_nesting_depth = 7;
    let styling = score(&report);
    approx(styling.penalties.structural, 3.0);
}

#[test]
fn penalties_compound_into_the_score() {
    // A report that trips every category at a known amount; the score is
    // 100 minus the sum of the (capped) per-category penalties.
    let mut report = clean_report();
    report.summary.total_declarations = 100;
    report.summary.duplicate_declarations_total = 5; // 4pt duplication (v3: 5/100*80)
    report.summary.files_analyzed = 1;
    report.summary.unreferenced_css_classes = 1; // 1.5pt dead_surface (1/100*150)
    report.summary.unresolved_class_references = 1; // 3pt broken_references
    report.summary.font_size_units_used = 3; // 2pt token_erosion (unit term, capped at 4)
    report.summary.important_declarations = 15; // 10pt structural
    report.summary.max_nesting_depth = 4;
    let styling = score(&report);
    let p = &styling.penalties;
    approx(p.duplication, 4.0);
    // dead_surface = token term 0 (no unused theme tokens) + other term
    // 1/100*150 = 1.5pt.
    approx(p.dead_surface, 1.5);
    approx(p.broken_references, 3.0);
    approx(p.token_erosion, 2.0);
    approx(p.structural, 10.0);
    // 100 - (4 + 1.5 + 3 + 2 + 10) = 79.5.
    approx(styling.score, 79.5);
    assert_eq!(styling.grade, "B");
}

#[test]
fn score_floors_at_rubric_minimum_for_pathological_report() {
    let mut report = clean_report();
    report.summary.total_declarations = 100;
    report.summary.duplicate_declarations_total = 100; // capped 20
    report.summary.files_analyzed = 1;
    report.summary.unused_theme_tokens = 100; // token term: 100/100*15 -> capped 15
    report.summary.unreferenced_css_classes = 100; // other term: 100/100*150 -> capped 8; sum capped 20
    report.summary.unresolved_class_references = 100; // capped 15
    report.summary.font_size_units_used = 20; // unit term -> capped 4
    report.summary.tailwind_arbitrary_values = 200; // arbitrary term -> capped 8; sum capped 10
    report.summary.important_declarations = 100; // capped 10
    report.summary.max_nesting_depth = 20;
    // 100 defined tokens, all 100 unused -> token term saturated.
    let styling = compute_styling_health(&report, 100);
    // Rubric floor is 25.0 (100 minus the sum of all per-category caps); the
    // `.clamp(0.0, 100.0)` is a defensive guard for any future uncapped category
    // and cannot fire under the current rubric.
    approx(styling.score, 25.0);
    assert!(styling.score >= 0.0);
    assert_eq!(styling.grade, "F");
}

#[test]
fn clean_report_is_high_confidence() {
    // clean_report() has 50 declarations, exactly at MIN_CONFIDENT_DECLARATIONS,
    // so it is High confidence with no reason.
    let styling = score(&clean_report());
    assert_eq!(styling.confidence, StylingHealthConfidence::High);
    assert!(styling.confidence_reason.is_none());
}

#[test]
fn sparse_report_is_low_confidence_with_reason() {
    // The fallow-tools shape: 24 declarations across 2 stylesheets is below the
    // floor, so the grade is marked low-confidence and the reason names both counts.
    let mut report = clean_report();
    report.summary.total_declarations = 24;
    report.summary.files_analyzed = 2;
    let styling = score(&report);
    assert_eq!(styling.confidence, StylingHealthConfidence::Low);
    let reason = styling
        .confidence_reason
        .expect("low confidence carries a reason");
    assert!(reason.contains("24 declarations"), "reason: {reason}");
    assert!(reason.contains("2 stylesheets"), "reason: {reason}");
}

#[test]
fn confidence_reason_is_singular_for_one_of_each() {
    let mut report = clean_report();
    report.summary.total_declarations = 1;
    report.summary.files_analyzed = 1;
    let reason = score(&report)
        .confidence_reason
        .expect("low confidence carries a reason");
    assert!(reason.contains("1 declaration across"), "reason: {reason}");
    assert!(reason.contains("1 stylesheet"), "reason: {reason}");
    assert!(!reason.contains("declarations"), "reason: {reason}");
    assert!(!reason.contains("stylesheets"), "reason: {reason}");
}

#[test]
fn confidence_boundary_is_at_min_declarations() {
    // 49 declarations -> Low; 50 -> High. The boundary is inclusive on High.
    let mut report = clean_report();
    report.summary.total_declarations = 49;
    assert_eq!(score(&report).confidence, StylingHealthConfidence::Low);
    report.summary.total_declarations = 50;
    assert_eq!(score(&report).confidence, StylingHealthConfidence::High);
}

#[test]
fn confidence_does_not_change_score_or_grade() {
    // Two reports identical except for declaration count straddling the floor:
    // their penalties are computed from the same ratios, and confidence is pure
    // metadata, so flipping confidence must not perturb the score or grade beyond
    // what the declaration-count denominator itself implies. Here both have zero
    // findings, so both score a clean 100 / A regardless of confidence.
    let mut sparse = clean_report();
    sparse.summary.total_declarations = 24;
    let mut solid = clean_report();
    solid.summary.total_declarations = 500;
    let a = score(&sparse);
    let b = score(&solid);
    assert_eq!(a.confidence, StylingHealthConfidence::Low);
    assert_eq!(b.confidence, StylingHealthConfidence::High);
    approx(a.score, 100.0);
    approx(b.score, 100.0);
    assert_eq!(a.grade, "A");
    assert_eq!(b.grade, "A");
}

#[test]
fn apply_styling_penalties_clamps_below_zero() {
    // Penalties whose sum exceeds 100 can only arise from an uncapped category;
    // construct that case directly to exercise the clamp branch in
    // `apply_styling_penalties`, which the capped rubric can never reach.
    let penalties = StylingHealthPenalties {
        duplication: 40.0,
        dead_surface: 40.0,
        broken_references: 30.0,
        token_erosion: 20.0,
        structural: 20.0,
    };
    // 100 - 150 = -50, clamped to 0.0.
    let score = apply_styling_penalties(&penalties);
    #[expect(
        clippy::float_cmp,
        reason = "the clamp lower bound is an exact 0.0 literal, so an exact compare is correct here"
    )]
    {
        assert_eq!(score, 0.0);
    }
}

// --- CSS program Phase 3c: atomic object CSS-in-JS exclusion ---

/// A report whose duplicate-block numerator is set, for the duplication-dilution
/// test. The summary `total_declarations` is the descriptive (full) count; the
/// grade reads the non-atomic count from `StylingScoringInputs` instead.
fn report_with_duplicates(duplicate_total: u32, summary_total: u32) -> CssAnalyticsReport {
    let mut report = clean_report();
    report.summary.total_declarations = summary_total;
    report.summary.duplicate_declarations_total = duplicate_total;
    report
}

#[test]
fn duplication_uses_non_atomic_denominator_not_total() {
    // 8 removable declarations over a real 50-declaration authored surface. 400
    // atomic StyleX declarations in the SAME project must NOT dilute it. A
    // sub-cap expected value is deliberately chosen so the assertion distinguishes
    // the non-atomic denominator (8/50) from the diluted one (8/450): both would
    // pass if it expected the cap.
    let report = report_with_duplicates(8, 450);
    let inputs = StylingScoringInputs {
        theme_tokens_defined: MANY_TOKENS,
        non_atomic_declarations: 50,
        non_atomic_important_declarations: 0,
        non_atomic_max_nesting_depth: 0,
        atomic_declarations: 400,
    };
    let styling = compute_styling_health_with_inputs(&report, &inputs);
    // 8 / 50 * 80 = 12.8pt (non-atomic denominator); the diluted 8 / 450 * 80
    // would be ~1.4pt, so this value proves the non-atomic count is used.
    approx(styling.penalties.duplication, 12.8);
}

#[test]
fn structural_penalty_reads_non_atomic_inputs_only() {
    // The descriptive summary carries flat atomic numbers (0 important, 0
    // nesting). The structural penalty must come from the non-atomic inputs (a
    // real 12% !important density + depth 6), not the diluted summary.
    let report = clean_report();
    let inputs = StylingScoringInputs {
        theme_tokens_defined: MANY_TOKENS,
        non_atomic_declarations: 100,
        non_atomic_important_declarations: 12,
        non_atomic_max_nesting_depth: 6,
        atomic_declarations: 500,
    };
    let styling = compute_styling_health_with_inputs(&report, &inputs);
    // important: (12% - 5) = 7; nesting: (6 - 4) = 2; sum 9, under the 10pt cap.
    approx(styling.penalties.structural, 9.0);
}

#[test]
fn predominantly_atomic_is_low_confidence_with_atomic_reason() {
    // 80% atomic declarations, but a non-thin (>= 50) non-atomic surface: the
    // sample-size trigger does NOT fire, proving the atomic trigger is
    // independent and wins the reason slot.
    let report = clean_report();
    let inputs = StylingScoringInputs {
        theme_tokens_defined: MANY_TOKENS,
        non_atomic_declarations: 60,
        non_atomic_important_declarations: 0,
        non_atomic_max_nesting_depth: 0,
        atomic_declarations: 240,
    };
    let styling = compute_styling_health_with_inputs(&report, &inputs);
    assert_eq!(styling.confidence, StylingHealthConfidence::Low);
    let reason = styling.confidence_reason.expect("atomic caveat reason");
    assert!(
        reason.contains("compile-time-atomic") && reason.contains("token hygiene"),
        "atomic reason names non-assessability: {reason:?}"
    );
    assert!(
        !reason.contains("graded from only"),
        "atomic reason wins over the sample-size reason: {reason:?}"
    );
}

#[test]
fn mostly_non_atomic_with_a_little_atomic_stays_high_confidence() {
    // 100 authored + 20 atomic = 17% atomic, below the 0.7 share: the grade is
    // graded normally and stays High confidence.
    let report = clean_report();
    let inputs = StylingScoringInputs {
        theme_tokens_defined: MANY_TOKENS,
        non_atomic_declarations: 100,
        non_atomic_important_declarations: 0,
        non_atomic_max_nesting_depth: 0,
        atomic_declarations: 20,
    };
    let styling = compute_styling_health_with_inputs(&report, &inputs);
    assert_eq!(styling.confidence, StylingHealthConfidence::High);
    assert!(styling.confidence_reason.is_none());
}

#[test]
fn no_atomic_split_matches_back_compat_entry() {
    // With no atomic declarations, `compute_styling_health_with_inputs` built via
    // `from_report` must equal the back-compat `compute_styling_health`.
    let mut report = clean_report();
    report.summary.important_declarations = 15;
    report.summary.total_declarations = 100;
    report.summary.max_nesting_depth = 4;
    let via_wrapper = compute_styling_health(&report, MANY_TOKENS);
    let via_inputs = compute_styling_health_with_inputs(
        &report,
        &StylingScoringInputs::from_report(&report, MANY_TOKENS),
    );
    approx(via_wrapper.score, via_inputs.score);
    assert_eq!(via_wrapper.grade, via_inputs.grade);
    assert_eq!(via_wrapper.confidence, via_inputs.confidence);
}
