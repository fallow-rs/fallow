use super::common::{create_config, fixture_path};

// Regression test for issue #752: class methods/properties called as
// `<local>.<method>(...)` inside a Svelte component, where `<local>` is a typed
// `$props()` binding (`let { resultState }: Props = $props()`), must be credited
// as used and not reported as `unused-class-member`. The instance arrives as a
// prop typed by an interface, so there is no `new ResultState()` to seed the
// binding; the fix resolves the destructured-binding type through the interface.
// See https://github.com/fallow-rs/fallow/issues/752.
#[test]
fn svelte_typed_prop_member_access_credits_class_members() {
    let root = fixture_path("issue-752-svelte-template-typed-props");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused: Vec<(&str, &str)> = results
        .unused_class_members
        .iter()
        .map(|m| (m.member.parent_name.as_str(), m.member.member_name.as_str()))
        .collect();

    // Script-level call `resultState.onOpen()` inside `handleOpen`.
    assert!(
        !unused.contains(&("ResultState", "onOpen")),
        "ResultState.onOpen is called from the component script via the typed prop, found: {unused:?}"
    );
    // Markup event-handler member calls.
    assert!(
        !unused.contains(&("ResultState", "pin")),
        "ResultState.pin is called from markup `onclick={{() => resultState.pin(...)}}`, found: {unused:?}"
    );
    assert!(
        !unused.contains(&("ResultState", "addSkipRule")),
        "ResultState.addSkipRule is called from markup inside `{{#if}}`, found: {unused:?}"
    );
    assert!(
        !unused.contains(&("ResultState", "updateLabel")),
        "ResultState.updateLabel is called from markup inside `{{#if}}`, found: {unused:?}"
    );
    // Property accessed via `bind:value`.
    assert!(
        !unused.contains(&("ResultState", "labelInput")),
        "ResultState.labelInput is bound via `bind:value={{resultState.labelInput}}`, found: {unused:?}"
    );
    // Property read in an `{#if}` condition.
    assert!(
        !unused.contains(&("ResultState", "labelMessage")),
        "ResultState.labelMessage is read in `{{#if resultState.labelMessage}}`, found: {unused:?}"
    );

    // Control: a member never referenced from script or template must still be
    // reported. This proves the fix does not blanket-credit every member.
    assert!(
        unused.contains(&("ResultState", "neverCalled")),
        "ResultState.neverCalled is never referenced and should be flagged, found: {unused:?}"
    );
}
