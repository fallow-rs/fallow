//! `unrendered-component` + Vue namespace re-exports (issue #1351): a design
//! system exposes compound components as namespaces (`export * as List from
//! "./components/List"`) and consumers render members via dotted tags
//! (`<List.ListRoot>`). The render-usage chain walk must follow the namespace
//! re-export edge back to the underlying `.vue` files so they are credited as
//! rendered; a namespace barrel that is NEVER consumed must still flag.

use super::common::{create_config, fixture_path};

#[test]
fn credits_components_rendered_through_namespace_reexport() {
    let root = fixture_path("unrendered-component-namespace");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let flagged: Vec<&str> = results
        .unrendered_components
        .iter()
        .map(|c| c.component.component_name.as_str())
        .collect();

    // Rendered via `<List.ListRoot>` / `<List.ListItem>` through the
    // `export * as List` namespace barrel: credited, not flagged.
    assert!(
        !flagged.contains(&"ListRoot"),
        "a namespace-rendered component must not be flagged: {flagged:?}"
    );
    assert!(
        !flagged.contains(&"ListItem"),
        "a namespace-rendered component must not be flagged: {flagged:?}"
    );
    // Rendered via `<Popover.PopoverRoot>` / `<Popover.PopoverContent>` through a
    // second namespace barrel: credited, not flagged.
    assert!(
        !flagged.contains(&"PopoverRoot"),
        "a namespace-rendered component must not be flagged: {flagged:?}"
    );
    assert!(
        !flagged.contains(&"PopoverContent"),
        "a namespace-rendered component must not be flagged: {flagged:?}"
    );
    // Re-exported through `export * as Dead` but never consumed by any
    // component: reachable (the barrel is reachable) yet rendered nowhere, so it
    // MUST still be flagged. This non-vacuous control proves the namespace
    // crediting does not over-credit project-wide.
    assert!(
        flagged.contains(&"DeadOrphan"),
        "an unconsumed namespace component must still be flagged: {flagged:?}"
    );
}
