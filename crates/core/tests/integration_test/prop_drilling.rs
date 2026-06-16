//! `prop-drilling` (Phase 3): a React/Preact prop forwarded UNCHANGED through 3+
//! intermediate pass-through components until a component that substantively
//! consumes it. The rule defaults to `off` (dormant), so every test enables it.
//! Asserts the genuine 3+-hop same-root chain is detected with the right located
//! hops, and that each abstain case (spread, cloneElement, element-as-prop,
//! Provider-present, renamed transform) yields NO chain (zero-FP doctrine).

use super::common::{create_config, fixture_path};

/// A real >=3-hop same-root drilling chain (`user` drilled Page -> Layout ->
/// Sidebar -> Profile) is detected as ONE located chain.
#[test]
fn detects_real_three_hop_chain() {
    let root = fixture_path("prop-drilling");
    let mut config = create_config(root);
    config.rules.prop_drilling = fallow_config::Severity::Warn;
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let chains = &results.prop_drilling_chains;
    assert_eq!(
        chains.len(),
        1,
        "exactly one prop-drilling chain expected: {:?}",
        chains
            .iter()
            .map(|c| (c.chain.prop.as_str(), c.chain.depth))
            .collect::<Vec<_>>()
    );

    let chain = &chains[0].chain;
    assert_eq!(chain.prop, "user", "the drilled prop is `user`");
    // Page (source) -> Layout (pass) -> Sidebar (pass) -> Profile (consumer).
    assert!(chain.depth >= 3, "depth must be >= 3: {}", chain.depth);
    assert_eq!(chain.depth as usize, chain.hops.len(), "depth == hop count");

    let components: Vec<&str> = chain.hops.iter().map(|h| h.component.as_str()).collect();
    assert_eq!(
        components,
        vec!["Page", "Layout", "Sidebar", "Profile"],
        "the located hop trail runs source -> pass -> pass -> consumer"
    );

    // Each hop carries a real located file + line so CI / an agent can act.
    for hop in &chain.hops {
        assert!(hop.line >= 1, "every hop has a 1-based line: {hop:?}");
        let stem = hop
            .file
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default();
        assert_eq!(
            stem, hop.component,
            "each hop file is the component's module: {hop:?}"
        );
    }
}

/// Every abstain case yields NO chain: a JSX spread, `cloneElement`, an
/// element-as-prop forward, a `*.Provider` in the subtree, and a renamed
/// transform each drop their whole 3+-hop chain.
#[test]
fn abstains_on_every_ladder_case() {
    let root = fixture_path("prop-drilling-abstain");
    let mut config = create_config(root);
    config.rules.prop_drilling = fallow_config::Severity::Warn;
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(
        results.prop_drilling_chains.is_empty(),
        "every abstain case must yield zero chains: {:?}",
        results
            .prop_drilling_chains
            .iter()
            .map(|c| {
                (
                    c.chain.prop.clone(),
                    c.chain
                        .hops
                        .iter()
                        .map(|h| h.component.clone())
                        .collect::<Vec<_>>(),
                )
            })
            .collect::<Vec<_>>()
    );
}

/// Dormant by default: with the rule at its `off` default, the positive fixture
/// emits NO chains even though a genuine 3-hop drill exists.
#[test]
fn dormant_when_rule_off() {
    let root = fixture_path("prop-drilling");
    let config = create_config(root); // rule defaults to off
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    assert!(
        results.prop_drilling_chains.is_empty(),
        "the prop-drilling rule is off by default: {:?}",
        results.prop_drilling_chains.len()
    );
}

/// Dep gate: a non-React project never emits prop-drilling chains even with the
/// rule enabled.
#[test]
fn dep_gated_to_react() {
    // The Vue fixture declares only `vue` (no react/react-dom/next/preact).
    let root = fixture_path("unused-component-prop");
    let mut config = create_config(root);
    config.rules.prop_drilling = fallow_config::Severity::Warn;
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    assert!(
        results.prop_drilling_chains.is_empty(),
        "prop-drilling must not fire on a non-React project"
    );
}
