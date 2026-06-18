//! `unused-component-prop`: Vue `defineProps` and Svelte `$props()` props used
//! nowhere in their own SFC.
//! Covers the FP-safety regressions: a renamed-destructure prop used via its
//! local alias is NOT flagged, and a custom-named `defineProps` return spread
//! via `v-bind` abstains the whole component.

use super::common::{create_config, fixture_path};

#[test]
fn flags_unused_props_but_credits_alias_and_abstains_on_forward() {
    let root = fixture_path("unused-component-prop");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let flagged: Vec<&str> = results
        .unused_component_props
        .iter()
        .map(|p| p.prop.prop_name.as_str())
        .collect();

    // A declared prop used nowhere is flagged.
    assert!(
        flagged.contains(&"deadProp"),
        "an unused prop should be flagged: {flagged:?}"
    );
    // A renamed-destructure prop used via its local alias is NOT flagged (FP1).
    assert!(
        !flagged.contains(&"used"),
        "a prop read through its renamed local must not be flagged: {flagged:?}"
    );
    // The unused half of the same renamed destructure IS flagged.
    assert!(
        flagged.contains(&"deadRenamed"),
        "the unused renamed prop should be flagged: {flagged:?}"
    );
    // A custom-named defineProps return spread via v-bind abstains the file (FP2),
    // so its prop is not flagged.
    assert!(
        !flagged.contains(&"forwarded"),
        "a v-bind-forwarded props object must abstain the component: {flagged:?}"
    );
    // A prop rendered in the template is credited.
    assert!(
        !flagged.contains(&"shown"),
        "a template-rendered prop must not be flagged: {flagged:?}"
    );
}

#[test]
fn flags_unused_svelte_props_but_credits_usage_and_abstains_on_opaque_shapes() {
    let root = fixture_path("unused-svelte-component-prop");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let flagged: Vec<(&str, &str)> = results
        .unused_component_props
        .iter()
        .map(|p| (p.prop.component_name.as_str(), p.prop.prop_name.as_str()))
        .collect();

    assert!(
        flagged.contains(&("DeadProp", "deadProp")),
        "an unused Svelte $props prop should be flagged: {flagged:?}"
    );
    assert!(
        !flagged.contains(&("ScriptUsed", "usedScript")),
        "a Svelte prop read in script must not be flagged: {flagged:?}"
    );
    assert!(
        !flagged.contains(&("TemplateUsed", "shown")),
        "a Svelte prop read in template must not be flagged: {flagged:?}"
    );
    assert!(
        !flagged.contains(&("RestAbstain", "forwarded")),
        "a Svelte props rest binding must abstain the component: {flagged:?}"
    );
    assert!(
        !flagged.contains(&("ComputedAbstain", "computed")),
        "a Svelte computed props key must abstain the component: {flagged:?}"
    );
}

#[test]
fn svelte_props_are_gated_on_svelte_dependency() {
    let root = fixture_path("unused-svelte-component-prop-no-dep");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(
        results.unused_component_props.is_empty(),
        "Svelte $props findings require a Svelte dependency: {:?}",
        results.unused_component_props
    );
}
