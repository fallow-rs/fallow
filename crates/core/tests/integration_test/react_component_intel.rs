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
    // react_component_intel is computed only on the editor/LSP `collect_usages`
    // path (gated off bare `fallow` / `audit`), so exercise it via the same
    // analyze entry the LSP uses.
    let results = fallow_core::analyze_with_usages(&config).expect("analysis should succeed");

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

/// Per-component hook attribution in a MULTI-component file. `ComponentA` and
/// `ComponentB` live in one file; A calls useState + useEffect, B calls useMemo.
/// The hook summary must attribute each hook to its enclosing component exactly,
/// not leave both empty (the old single-component-per-file heuristic) and not
/// cross-attribute.
#[test]
fn attributes_hooks_per_component_in_multi_component_file() {
    let root = fixture_path("react-multi-component-hooks");
    let config = create_config(root);
    // react_component_intel is computed only on the editor/LSP `collect_usages`
    // path (gated off bare `fallow` / `audit`), so exercise it via the same
    // analyze entry the LSP uses.
    let results = fallow_core::analyze_with_usages(&config).expect("analysis should succeed");

    let a = intel_for(&results, "ComponentA").expect("ComponentA is in the intel set");
    assert_eq!(a.hooks.state, 1, "ComponentA owns the one useState");
    assert_eq!(a.hooks.effect, 1, "ComponentA owns the one useEffect");
    assert_eq!(a.hooks.memo, 0, "ComponentA does not call useMemo");
    assert_eq!(a.hooks.callback, 0);
    assert_eq!(a.hooks.custom, 0);

    let b = intel_for(&results, "ComponentB").expect("ComponentB is in the intel set");
    assert_eq!(b.hooks.memo, 1, "ComponentB owns the one useMemo");
    assert_eq!(b.hooks.state, 0, "ComponentB does not call useState");
    assert_eq!(b.hooks.effect, 0, "ComponentB does not call useEffect");
    assert_eq!(b.hooks.callback, 0);
    assert_eq!(b.hooks.custom, 0);
}

/// Descriptive prop-drilling trace: in `Page > Layout > Sidebar > Profile`, the
/// `user` prop is forwarded unchanged from `Page` through two pass-throughs to
/// `Profile` which consumes it. The `user` ReactPropIntel at the chain SOURCE
/// (`Page`) must carry a `drill` trace listing the hops in order. This is
/// computed UNCONDITIONALLY (the `prop-drilling` rule is off in the default
/// config used here), proving the descriptive trace is independent of the rule.
#[test]
fn carries_prop_drilling_trace_on_chain_source() {
    let root = fixture_path("prop-drilling");
    let config = create_config(root);
    // react_component_intel is computed only on the editor/LSP `collect_usages`
    // path (gated off bare `fallow` / `audit`), so exercise it via the same
    // analyze entry the LSP uses.
    let results = fallow_core::analyze_with_usages(&config).expect("analysis should succeed");

    // The rule is off by default, so no prop-drilling FINDINGS are emitted: the
    // descriptive trace must still be present.
    assert!(
        results.prop_drilling_chains.is_empty(),
        "prop-drilling rule is off, so no findings are emitted"
    );

    let page = intel_for(&results, "Page").expect("Page is in the intel set");
    let user = page
        .props
        .iter()
        .find(|p| p.name == "user")
        .expect("Page declares the user prop");
    let drill = user
        .drill
        .as_ref()
        .expect("the source-of-chain prop carries a drill trace");
    assert!(
        drill.depth >= 3,
        "the chain forwards through at least 3 components, got {}",
        drill.depth
    );
    assert_eq!(
        drill.hops,
        vec![
            "Page".to_string(),
            "Layout".to_string(),
            "Sidebar".to_string(),
            "Profile".to_string(),
        ],
        "the trace lists the hops source-to-consumer"
    );

    // The consumer end (Profile) is NOT a chain source, so its `user` prop
    // carries no drill trace.
    let profile = intel_for(&results, "Profile").expect("Profile is in the intel set");
    let profile_user = profile
        .props
        .iter()
        .find(|p| p.name == "user")
        .expect("Profile declares the user prop");
    assert!(
        profile_user.drill.is_none(),
        "the chain consumer is not a source, so it carries no drill trace"
    );
}

/// A non-React project (no react/react-dom/next/preact dep) computes no intel.
#[test]
fn no_intel_on_non_react_project() {
    // `complexity-project` is a plain TS project with no React dep.
    let root = fixture_path("complexity-project");
    let config = create_config(root);
    // react_component_intel is computed only on the editor/LSP `collect_usages`
    // path (gated off bare `fallow` / `audit`), so exercise it via the same
    // analyze entry the LSP uses.
    let results = fallow_core::analyze_with_usages(&config).expect("analysis should succeed");
    assert!(
        results.react_component_intel.is_empty(),
        "no React intel on a non-React project"
    );
}
