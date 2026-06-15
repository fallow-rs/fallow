//! `unused-component-prop`: a Vue `defineProps` prop used nowhere in its SFC.
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
