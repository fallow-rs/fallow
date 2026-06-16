//! Vue Options API coverage for `unused-component-prop` / `unused-component-emit`:
//! the same detectors that handle `<script setup>` now harvest `props:` / `emits:`
//! from `export default { ... }` and `defineComponent({ ... })` in a non-setup
//! `<script>` block. Covers the dead-prop / dead-emit true positives, the
//! `this.<prop>` and template usage credits, the `this.$emit('name')` credit, and
//! the `mixins:` / `extends:` whole-component abstains.

use super::common::{create_config, fixture_path};

#[test]
fn flags_options_api_dead_prop_and_emit_with_usage_credit_and_abstains() {
    let root = fixture_path("vue-options-api-prop-emit");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let flagged_props: Vec<&str> = results
        .unused_component_props
        .iter()
        .map(|p| p.prop.prop_name.as_str())
        .collect();
    let flagged_emits: Vec<&str> = results
        .unused_component_emits
        .iter()
        .map(|e| e.emit.emit_name.as_str())
        .collect();

    // A declared Options-API prop read nowhere is flagged (object form).
    assert!(
        flagged_props.contains(&"deadProp"),
        "an Options-API object-form dead prop should be flagged: {flagged_props:?}"
    );
    // A prop read via `this.<prop>` (array form) is credited.
    assert!(
        !flagged_props.contains(&"usedViaThis"),
        "a prop read via this.<prop> must not be flagged: {flagged_props:?}"
    );
    // A prop read only in the template is credited.
    assert!(
        !flagged_props.contains(&"shown"),
        "a template-rendered Options-API prop must not be flagged: {flagged_props:?}"
    );
    // `defineComponent({ props })` form: dead prop flagged, this-read prop credited.
    assert!(
        flagged_props.contains(&"deadDcProp"),
        "a defineComponent dead prop should be flagged: {flagged_props:?}"
    );
    assert!(
        !flagged_props.contains(&"usedDcProp"),
        "a defineComponent this-read prop must not be flagged: {flagged_props:?}"
    );

    // A declared Options-API emit fired nowhere is flagged (array form).
    assert!(
        flagged_emits.contains(&"deadEmit"),
        "an Options-API dead emit should be flagged: {flagged_emits:?}"
    );
    // An emit fired via `this.$emit('name')` is credited; its dead sibling flagged.
    assert!(
        !flagged_emits.contains(&"saved"),
        "an emit fired via this.$emit must not be flagged: {flagged_emits:?}"
    );
    assert!(
        flagged_emits.contains(&"unfired"),
        "the unused sibling emit should be flagged: {flagged_emits:?}"
    );

    // `mixins:` and `extends:` abstain the whole component: their props and emits
    // are not flagged even though neither is used in-file.
    assert!(
        !flagged_props.contains(&"hiddenProp") && !flagged_emits.contains(&"hidden"),
        "a mixins: component must abstain props and emits: props={flagged_props:?} emits={flagged_emits:?}"
    );
    assert!(
        !flagged_props.contains(&"baseProp") && !flagged_emits.contains(&"based"),
        "an extends: component must abstain props and emits: props={flagged_props:?} emits={flagged_emits:?}"
    );

    // A `setup(props, { emit })` method fires `emit('done')` through the context
    // binding, invisible to the this.$emit walk, so the component's emits abstain.
    assert!(
        !flagged_emits.contains(&"done"),
        "an emit fired via the setup() context binding must not be flagged: {flagged_emits:?}"
    );
}
