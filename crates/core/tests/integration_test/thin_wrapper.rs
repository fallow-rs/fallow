//! `thin-wrapper`: a React/Preact component whose ENTIRE body forwards its own
//! props to a single child via a bare spread (`return <Child {...props}/>`),
//! with no host wrapper, no named attrs, no hooks, no branching. A candidate for
//! inlining. The rule defaults to `off` (dormant), so every positive test
//! enables it. Asserts the genuine wrapper is flagged with the right located
//! record, and that each abstain case (forwardRef, memo, exported public-API,
//! provider, own markup, hook/logic, named-attr, spread-of-other-object) yields
//! NO finding (zero-FP doctrine).

use super::common::{create_config, fixture_path};

/// A genuine thin wrapper (`Wrapper` forwards `{...props}` to `Button`) is
/// flagged as ONE located finding with the wrapper and child names.
#[test]
fn detects_genuine_thin_wrapper() {
    let root = fixture_path("thin-wrapper");
    let mut config = create_config(root);
    config.rules.thin_wrapper = fallow_config::Severity::Warn;
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let wrappers = &results.thin_wrappers;
    assert_eq!(
        wrappers.len(),
        1,
        "exactly one thin wrapper expected: {:?}",
        wrappers
            .iter()
            .map(|w| (
                w.wrapper.component.as_str(),
                w.wrapper.child_component.as_str()
            ))
            .collect::<Vec<_>>()
    );

    let w = &wrappers[0].wrapper;
    assert_eq!(w.component, "Wrapper", "the flagged wrapper is `Wrapper`");
    assert_eq!(w.child_component, "Button", "it forwards to `Button`");
    assert!(w.line >= 1, "the finding has a 1-based line: {w:?}");
    let stem = w
        .file
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default();
    assert_eq!(stem, "App", "the wrapper lives in App.tsx");
}

/// Every abstain case yields NO finding: forwardRef, memo, an exported
/// public-API wrapper, a context-provider wrapper, a wrapper that adds its own
/// host markup, a wrapper with a hook + branching, a wrapper with a named attr
/// alongside the spread, and a wrapper that spreads a different object.
#[test]
fn abstains_on_every_ladder_case() {
    let root = fixture_path("thin-wrapper-abstain");
    let mut config = create_config(root);
    config.rules.thin_wrapper = fallow_config::Severity::Warn;
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(
        results.thin_wrappers.is_empty(),
        "every abstain case must yield zero thin wrappers: {:?}",
        results
            .thin_wrappers
            .iter()
            .map(|w| (
                w.wrapper.component.clone(),
                w.wrapper.child_component.clone()
            ))
            .collect::<Vec<_>>()
    );
}

/// Dormant by default: with the rule at its `off` default, the positive fixture
/// emits NO findings even though a genuine thin wrapper exists.
#[test]
fn dormant_when_rule_off() {
    let root = fixture_path("thin-wrapper");
    let config = create_config(root); // rule defaults to off
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    assert!(
        results.thin_wrappers.is_empty(),
        "the thin-wrapper rule is off by default: {}",
        results.thin_wrappers.len()
    );
}

/// Dep gate: a non-React project never emits thin-wrapper findings even with the
/// rule enabled.
#[test]
fn dep_gated_to_react() {
    // The Vue fixture declares only `vue` (no react/react-dom/next/preact).
    let root = fixture_path("unused-component-prop");
    let mut config = create_config(root);
    config.rules.thin_wrapper = fallow_config::Severity::Warn;
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    assert!(
        results.thin_wrappers.is_empty(),
        "thin-wrapper must not fire on a non-React project"
    );
}
