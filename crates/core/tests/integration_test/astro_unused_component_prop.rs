//! `unused-component-prop` Astro arm: a `.astro` `interface Props` field read
//! nowhere via `Astro.props` (destructure / member access) nor in the template.
//! `Card.astro`'s `unused` is flagged; `title` (read in `{title}`) is credited;
//! `Forwarded.astro` abstains wholesale because it spreads `{...Astro.props}`.

use super::common::{create_config, fixture_path};

#[test]
fn flags_dead_astro_prop_but_credits_used_and_abstains_on_spread() {
    let root = fixture_path("astro-unused-prop");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let flagged: Vec<&str> = results
        .unused_component_props
        .iter()
        .map(|p| p.prop.prop_name.as_str())
        .collect();

    // Declared in `interface Props` but never destructured/read: flagged.
    assert!(
        flagged.contains(&"unused"),
        "an Astro prop read nowhere should be flagged: {flagged:?}"
    );
    // Read in `{title}` markup (the destructured local): credited, not flagged.
    assert!(
        !flagged.contains(&"title"),
        "a template-read Astro prop must not be flagged: {flagged:?}"
    );
    // `Forwarded.astro` spreads `{...Astro.props}`, forwarding every prop opaquely:
    // the whole component abstains, so `gone` is never flagged.
    assert!(
        !flagged.contains(&"gone"),
        "a {{...Astro.props}}-spread component must abstain: {flagged:?}"
    );

    // `DefineVars.astro`: a prop consumed only through `<style define:vars={{ x }}>`
    // is a real use and must not be flagged (the directive sits in the masked
    // `<style>` opening tag, so it is scanned separately).
    assert!(
        !flagged.contains(&"cssVar"),
        "a prop used via define:vars must be credited: {flagged:?}"
    );
    // A prop consumed via a NESTED destructure (`const { nested: { label } } =
    // Astro.props`) is used; the outer key must be credited.
    assert!(
        !flagged.contains(&"nested"),
        "a nested-destructured prop must be credited: {flagged:?}"
    );
    // The arm still flags a genuinely-dead prop on the same component, proving the
    // fixes credit precisely rather than abstaining the whole file.
    assert!(
        flagged.contains(&"reallyUnused"),
        "a truly dead prop on the define:vars component should still flag: {flagged:?}"
    );

    // `ArrayDestructure.astro` array-destructures `Astro.props` (`const [first] =
    // Astro.props`); the declared prop is unenumerable through that shape, so the
    // whole component abstains rather than false-flag `arrayProp`.
    assert!(
        !flagged.contains(&"arrayProp"),
        "an array-destructured Astro.props component must abstain: {flagged:?}"
    );
}
