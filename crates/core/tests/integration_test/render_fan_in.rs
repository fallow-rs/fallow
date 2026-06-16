//! Component render fan-in: a DESCRIPTIVE blast-radius signal counting how many
//! JSX render SITES (and distinct parent components) render a given component
//! across the project. NOT rule-gated (it runs whenever React is declared), so
//! these tests assert on `AnalysisResults.render_fan_in` directly with the
//! default (no rule enabled) config. The component-graph analogue of module
//! fan-in.

use super::common::{create_config, fixture_path};

/// Look up one component's `(render_sites, distinct_parents)` by name from the
/// metric's per-component detail.
fn counts_for(metric: &fallow_core::results::RenderFanInMetric, name: &str) -> Option<(u32, u32)> {
    metric
        .per_component
        .iter()
        .find(|c| c.component == name)
        .map(|c| (c.render_sites, c.distinct_parents))
}

/// The headline case: `<Button>` is rendered in 6 SITES across 3 distinct parent
/// components (Home x3, Settings x2, Toolbar x1). The member-expression
/// `<Lib.Button/>` in Dynamic.tsx is NOT credited (safe undercount), so the count
/// stays exactly 6. The rarely-rendered baseline and the unrendered component are
/// represented with their real low/zero counts.
#[test]
fn counts_render_sites_and_distinct_parents() {
    let root = fixture_path("render-fan-in");
    let config = create_config(root); // not rule-gated; runs whenever React declared
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let metric = results
        .render_fan_in
        .as_ref()
        .expect("render fan-in computed on a React project");

    // Button: the high-fan-in blast-radius amplifier.
    let (button_sites, button_parents) =
        counts_for(metric, "Button").expect("Button is in the per-component map");
    assert_eq!(
        button_sites, 6,
        "Button rendered in 6 SITES (Home x3, Settings x2, Toolbar x1); the \
         member-expression <Lib.Button/> is undercounted, never a 7th site"
    );
    assert_eq!(
        button_parents, 3,
        "Button rendered by 3 distinct parent components (Home, Settings, Toolbar)"
    );

    // RareModal: the rarely-rendered baseline (one site, one parent).
    assert_eq!(
        counts_for(metric, "RareModal"),
        Some((1, 1)),
        "RareModal rendered in exactly one site by one parent"
    );

    // Unrendered: a real 0 (included in the population, not absent).
    assert_eq!(
        counts_for(metric, "Unrendered"),
        Some((0, 0)),
        "Unrendered is a real 0 in the population, not missing"
    );

    // The headline blast-radius number is the max DISTINCT-PARENTS (Button = 3
    // distinct parents: Home, Settings, Toolbar), NOT the inflated render-site
    // count (6).
    assert_eq!(
        metric.max_distinct_parents,
        Some(3),
        "max distinct-parents is the single highest count (Button = 3 parents)"
    );

    // The percentile aggregates are populated and finite.
    assert!(
        metric.p95_distinct_parents.is_some(),
        "p95 distinct-parents is populated"
    );
    let high_pct = metric
        .high_pct
        .expect("high_pct is populated on a React project");
    assert!(
        high_pct.is_finite() && (0.0..=100.0).contains(&high_pct),
        "high_pct is a finite percentage: {high_pct}"
    );
}

/// The undercount is the documented safe direction: editing Dynamic.tsx (the
/// member-expression render) does NOT inflate Button. Asserted by the exact 6
/// above; this test pins that Button is the lone amplifier and the others sit
/// below it, so a high-fan-in surface would never falsely flag RareModal.
#[test]
fn rarely_rendered_component_is_not_high_fan_in() {
    let root = fixture_path("render-fan-in");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let metric = results.render_fan_in.as_ref().expect("metric present");

    let button = counts_for(metric, "Button").expect("Button present").0;
    let rare = counts_for(metric, "RareModal")
        .expect("RareModal present")
        .0;
    assert!(
        button > rare,
        "the shared Button ({button}) is a far higher blast radius than RareModal ({rare})"
    );
}

/// Test-file exclusion: a component DEFINED in a test/spec file is NOT a fan-in
/// target (the test-local `Page` never appears in the per-component map), and a
/// render SITE whose parent is a test file does NOT count (the 5 `<Button>`
/// renders inside `__tests__/Button.test.tsx` do not inflate Button's headline).
/// This is the panel release-blocker: on real repos the headline was dominated
/// by test-local render loops (TanStack's `Page` at 146 sites / 2 parents).
#[test]
fn test_files_are_excluded_from_render_fan_in() {
    let root = fixture_path("render-fan-in");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let metric = results.render_fan_in.as_ref().expect("metric present");

    // The test-local `Page` component must not be a fan-in target at all.
    assert!(
        counts_for(metric, "Page").is_none(),
        "a component DEFINED in a test file must not appear in the per-component map"
    );

    // Button's counts are unchanged by the 5 test-file render sites: still 6
    // production sites from 3 distinct production parents.
    assert_eq!(
        counts_for(metric, "Button"),
        Some((6, 3)),
        "render SITES whose parent is a test file must NOT count toward Button"
    );

    // The honest headline (max distinct-parents) stays 3, NOT inflated by the
    // test-local loop.
    assert_eq!(
        metric.max_distinct_parents,
        Some(3),
        "the test-file render loop must not inflate the headline distinct-parents"
    );
}

/// Dep gate: a non-React project computes nothing (the dep gate fails AND
/// `render_edges` is empty), so the metric is `None`.
#[test]
fn dep_gated_to_react() {
    // The Vue fixture declares only `vue` (no react/react-dom/next/preact).
    let root = fixture_path("unused-component-prop");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    assert!(
        results.render_fan_in.is_none(),
        "render fan-in must not compute on a non-React project"
    );
}
