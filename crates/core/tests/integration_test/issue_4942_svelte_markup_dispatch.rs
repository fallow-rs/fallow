//! `unused-svelte-event` for a `dispatch()` call in the markup of a Svelte
//! component. The markup handler `on:click={() => dispatch("dead")}` dispatches
//! the same event as a `dispatch("dead")` call in a script function, so the
//! detector gives the same finding. See issue #4942.

use super::common::{create_config, fixture_path};

#[test]
fn markup_dispatch_of_unlistened_event_is_flagged() {
    let root = fixture_path("issue-4942-svelte-markup-dispatch");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let events = &results.unused_svelte_events;

    assert_eq!(
        events.len(),
        1,
        "expected one finding for `dead`: {events:?}"
    );
    let finding = &events[0].event;
    assert_eq!(finding.event_name, "dead");
    assert_eq!(finding.component_name, "Child");
    assert_eq!(
        finding.line, 6,
        "the finding points at the line of the markup dispatch: {finding:?}"
    );
}
