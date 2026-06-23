//! React component intelligence: a DESCRIPTIVE per-component summary of render
//! sites, props, and hooks, surfaced as ambient editor context (LSP code lens +
//! per-prop hover). NOT rule-gated (it runs whenever React is declared), so
//! these tests assert on `AnalysisResults.react_component_intel` directly with
//! the default (no rule enabled) config.

use super::common::{create_config, fixture_path};

/// Find one component's intel by name.
fn intel_for<'a>(
    results: &'a fallow_core::results::AnalysisResults,
    name: &str,
) -> Option<&'a fallow_core::results::ReactComponentIntel> {
    results
        .react_component_intel
        .iter()
        .find(|c| c.component_name == name)
}

/// The headline case: `<Card>` is rendered 3 times from one parent (Home), has
/// two props (`title` used in body, `subtitle` unused), and two hooks (one
/// useState, one useEffect). The 5 render sites in the test file are EXCLUDED
/// from render_sites / distinct_parents / per-prop pass counts.
#[test]
fn summarizes_render_props_and_hooks() {
    let root = fixture_path("react-component-intel");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let card = intel_for(&results, "Card").expect("Card is in the intel set");

    // Render aggregation: 3 production sites, 1 distinct parent. The 5 test-file
    // sites are excluded.
    assert_eq!(
        card.render_sites, 3,
        "Card rendered in 3 production sites (Home x3); the 5 test-file sites are excluded"
    );
    assert_eq!(
        card.distinct_parents, 1,
        "Card rendered by exactly one distinct parent (Home)"
    );

    // Props: two declared.
    assert_eq!(
        card.prop_count, 2,
        "Card declares 2 props (title, subtitle)"
    );
    assert_eq!(card.props.len(), 2, "both props are in the per-prop intel");

    // Hooks: one useState, one useEffect.
    assert_eq!(card.hooks.state, 1, "one useState");
    assert_eq!(card.hooks.effect, 1, "one useEffect");
    assert_eq!(card.hooks.memo, 0);
    assert_eq!(card.hooks.callback, 0);
    assert_eq!(card.hooks.custom, 0);

    // `title` is read in the body and passed at all 3 production sites.
    let title = card
        .props
        .iter()
        .find(|p| p.name == "title")
        .expect("title prop present");
    assert!(title.used_in_body, "title is read in the component body");
    assert_eq!(
        title.passed_from_sites, 3,
        "title is passed at all 3 production render sites (test sites excluded)"
    );

    // `subtitle` is declared but never read; passed at only 1 production site.
    let subtitle = card
        .props
        .iter()
        .find(|p| p.name == "subtitle")
        .expect("subtitle prop present");
    assert!(
        !subtitle.used_in_body,
        "subtitle is declared but never read in the body"
    );
    assert_eq!(
        subtitle.passed_from_sites, 1,
        "subtitle is passed at exactly 1 production site (the 4 test sites are excluded)"
    );

    // The anchor lands on a real source line (1-based), not a fallback.
    assert!(card.anchor_line >= 1, "component anchor line is 1-based");
    assert!(
        title.anchor_line >= 1,
        "prop anchor line is 1-based and resolved"
    );
}

/// A non-React project (no react/react-dom/next/preact dep) computes no intel.
#[test]
fn no_intel_on_non_react_project() {
    // `complexity-project` is a plain TS project with no React dep.
    let root = fixture_path("complexity-project");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    assert!(
        results.react_component_intel.is_empty(),
        "no React intel on a non-React project"
    );
}
