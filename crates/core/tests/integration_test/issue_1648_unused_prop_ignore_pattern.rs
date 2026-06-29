//! `unusedComponentProps.ignorePattern` (issue #1648): an opt-in regex matched
//! against a component prop's LOCAL destructure binding name exempts it from
//! `unused-component-props`. The fixture is the issue's exact Svelte 5 shape:
//! `let { stage: _stage, variant }: Props = $props();` where neither prop is
//! read. With the pattern unset both are flagged; with `^_` the underscored
//! local (`_stage`) is exempted while `variant` still flags.

use super::common::{create_config, create_config_with_unused_props_ignore, fixture_path};

/// Neuter / baseline: with NO pattern, BOTH intentionally-unused props flag.
/// This proves the fixture reproduces the issue and that the exemption below is
/// the only thing suppressing `_stage` (regression-strength).
#[test]
fn without_pattern_both_underscore_and_plain_props_flag() {
    let root = fixture_path("issue-1648-underscore-props");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let flagged: Vec<&str> = results
        .unused_component_props
        .iter()
        .map(|p| p.prop.prop_name.as_str())
        .collect();

    assert!(
        flagged.contains(&"stage"),
        "without ignorePattern, the underscore-aliased prop must flag: {flagged:?}"
    );
    assert!(
        flagged.contains(&"variant"),
        "without ignorePattern, the plain prop must flag: {flagged:?}"
    );
    assert_eq!(
        results.unused_component_props_exempted, 0,
        "no props are exempted when ignorePattern is unset"
    );
}

/// With `^_`, the underscored LOCAL alias (`_stage`) is exempted; `variant`
/// (local == name, no leading underscore) still flags. The finding reports the
/// PUBLIC key (`stage`), but the match is on the local alias, so the public
/// `stage` finding disappears.
#[test]
fn ignore_pattern_exempts_underscore_local_keeps_plain_prop() {
    let root = fixture_path("issue-1648-underscore-props");
    let config = create_config_with_unused_props_ignore(root, "^_");
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let flagged: Vec<&str> = results
        .unused_component_props
        .iter()
        .map(|p| p.prop.prop_name.as_str())
        .collect();

    assert!(
        !flagged.contains(&"stage"),
        "a prop whose local alias matches ^_ must be exempted: {flagged:?}"
    );
    assert!(
        flagged.contains(&"variant"),
        "a non-matching unused prop must still flag: {flagged:?}"
    );
    assert_eq!(
        results.unused_component_props_exempted, 1,
        "exactly one prop (the _stage alias) is exempted by ^_"
    );
}
