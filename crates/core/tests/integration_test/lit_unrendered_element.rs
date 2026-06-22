//! `unrendered-component` Lit arm: a custom element registered via
//! `@customElement('x-foo')` but rendered as a tag in no `html` template.
//! `dead-el` is flagged (framework `lit`, the finding names the TAG); `used-el`
//! (rendered `<used-el>` in `my-app`'s template) and `my-app` (mounted via
//! `document.createElement('my-app')`) are credited.

use super::common::{create_config, fixture_path};

#[test]
fn flags_unrendered_lit_element_but_credits_rendered_and_imperative() {
    let root = fixture_path("lit-unrendered-element");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let flagged: Vec<(&str, &str)> = results
        .unrendered_components
        .iter()
        .map(|c| {
            (
                c.component.component_name.as_str(),
                c.component.framework.as_str(),
            )
        })
        .collect();

    // dead-el is registered but rendered nowhere: flagged, framework `lit`, and
    // the finding names the TAG (not the file stem).
    assert!(
        flagged.contains(&("dead-el", "lit")),
        "dead-el should be flagged unrendered (framework lit): {flagged:?}"
    );
    // used-el is rendered as `<used-el>` in my-app's html`` template: credited.
    assert!(
        !flagged.iter().any(|(name, _)| *name == "used-el"),
        "an html``-rendered element must not be flagged: {flagged:?}"
    );
    // my-app is mounted via `document.createElement('my-app')`: credited.
    assert!(
        !flagged.iter().any(|(name, _)| *name == "my-app"),
        "an imperatively-rendered element must not be flagged: {flagged:?}"
    );
    // html-el is rendered only as `<html-el>` in the standalone index.html app
    // shell: credited via the .html custom-element scan.
    assert!(
        !flagged.iter().any(|(name, _)| *name == "html-el"),
        "an element rendered in a standalone .html must not be flagged: {flagged:?}"
    );
    // opt-el is mounted via `opts.document.createElement('opt-el')` (a non-`document`
    // receiver): credited via the receiver-agnostic createElement capture.
    assert!(
        !flagged.iter().any(|(name, _)| *name == "opt-el"),
        "a non-`document`-receiver createElement render must not be flagged: {flagged:?}"
    );
    // docs-el is defined under `src/docs/` and rendered nowhere, but the
    // tooling-rendered abstain suppresses it (docs-site render is invisible).
    assert!(
        !flagged.iter().any(|(name, _)| *name == "docs-el"),
        "an element under a docs/ directory must abstain: {flagged:?}"
    );
    // win-el is registered via `window.customElements.define('win-el', ...)` and
    // rendered nowhere: flagged, proving the `window.`-qualified registry call is
    // captured as a registration (else it would silently produce no finding).
    assert!(
        flagged.contains(&("win-el", "lit")),
        "a window.customElements.define-registered dead element should flag: {flagged:?}"
    );
}
