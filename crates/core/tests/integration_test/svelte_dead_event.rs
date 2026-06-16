//! `unused-svelte-event`: a Svelte component dispatching a `createEventDispatcher`
//! custom event listened to nowhere project-wide. Covers the FP-safety
//! invariants: a dispatched event listened by a parent (`save`) is NOT flagged,
//! event forwarding (`<Child on:save>` with no handler) counts as a listen, a
//! dynamic `dispatch(<nonLiteral>)` abstains the whole component, and a DOM
//! `<button on:click>` is not a custom-event listener.

use super::common::{create_config, fixture_path};

#[test]
fn flags_unlistened_dispatch_but_credits_listened_forwarded_and_abstains_on_dynamic() {
    let root = fixture_path("svelte-dead-event");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let flagged: Vec<&str> = results
        .unused_svelte_events
        .iter()
        .map(|e| e.event.event_name.as_str())
        .collect();

    // A dispatched event listened to nowhere is flagged.
    assert!(
        flagged.contains(&"dead"),
        "an unlistened dispatched event should be flagged: {flagged:?}"
    );
    // The `save` event is listened by a parent (<Child on:save={...}>) AND
    // forwarded by Middle (<Child on:save>), so it must NOT be flagged.
    assert!(
        !flagged.contains(&"save"),
        "a listened/forwarded event must not be flagged: {flagged:?}"
    );
    // A dynamic dispatch(name) abstains the whole component, so `gone` (the
    // Dynamic component's default name) is never flagged. A DOM on:click is not
    // a custom-event listener, but `click` is never dispatched either, so it
    // simply does not appear.
    assert!(
        !flagged.contains(&"gone"),
        "a dynamic-dispatch component must abstain entirely: {flagged:?}"
    );

    // Exactly one finding (`dead`), anchored at the Child component.
    assert_eq!(
        results.unused_svelte_events.len(),
        1,
        "expected exactly one finding (dead): {:?}",
        results.unused_svelte_events
    );
    let finding = &results.unused_svelte_events[0];
    assert_eq!(finding.event.event_name, "dead");
    assert_eq!(finding.event.component_name, "Child");
    assert!(
        finding
            .event
            .path
            .to_string_lossy()
            .replace('\\', "/")
            .ends_with("Child.svelte"),
        "finding should anchor at Child.svelte: {:?}",
        finding.event.path
    );
}
