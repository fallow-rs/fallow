//! `unrendered-component`: a Vue/Svelte SFC that is reachable (a barrel
//! re-exports it) but rendered nowhere. The barrel-masked `Orphan` is flagged;
//! a template-rendered component and a script value-read component are credited.

use super::common::{create_config, fixture_path};

#[test]
fn flags_barrel_masked_component_but_credits_rendered_and_value_read() {
    let root = fixture_path("unrendered-component");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let flagged: Vec<&str> = results
        .unrendered_components
        .iter()
        .map(|c| c.component.component_name.as_str())
        .collect();

    // Orphan is re-exported by the barrel and rendered nowhere: flagged.
    assert!(
        flagged.contains(&"Orphan"),
        "Orphan should be flagged as unrendered: {flagged:?}"
    );
    // Used is rendered as `<Used />` in App.vue (through the barrel chain): credited.
    assert!(
        !flagged.contains(&"Used"),
        "a template-rendered component must not be flagged: {flagged:?}"
    );
    // Lazy is value-read in registry.ts (script-side use): credited.
    assert!(
        !flagged.contains(&"Lazy"),
        "a value-read component must not be flagged: {flagged:?}"
    );
    // App is an SFC but not barrel-re-exported (and is rendered via main.ts): not flagged.
    assert!(
        !flagged.contains(&"App"),
        "the app root must not be flagged: {flagged:?}"
    );
    // PublicWidget is re-exported through a MULTI-HOP chain from the package's
    // `./lib` public-API entry (entry -> `export *` -> widgets barrel -> .vue),
    // rendered nowhere in this package: abstained, not flagged (the full
    // public-API re-export-chain walk).
    assert!(
        !flagged.contains(&"PublicWidget"),
        "a multi-hop public-API component must be abstained: {flagged:?}"
    );
}
