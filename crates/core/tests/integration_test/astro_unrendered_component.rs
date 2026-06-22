//! `unrendered-component` Astro arm: a `.astro` component reachable only because
//! a barrel re-exports it, rendered in no template. `Orphan.astro` is flagged
//! (framework `astro`); the template-rendered `Used.astro` is credited, and
//! neither is reported as `unused-file` (both reachable via the barrel).

use super::common::{create_config, fixture_path};

#[test]
fn flags_barrel_masked_astro_component_but_credits_rendered() {
    let root = fixture_path("astro-unrendered");
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

    // Orphan.astro is re-exported by the barrel and rendered nowhere: flagged,
    // and tagged with the new `astro` framework discriminator.
    assert!(
        flagged.contains(&("Orphan", "astro")),
        "Orphan.astro should be flagged unrendered (framework astro): {flagged:?}"
    );
    // Used.astro is rendered as `<Used />` in the page (through the barrel chain):
    // the frontmatter semantic pass + template-usage credit keeps it referenced.
    assert!(
        !flagged.iter().any(|(name, _)| *name == "Used"),
        "a template-rendered .astro component must not be flagged: {flagged:?}"
    );

    // Both components are reachable via the barrel, so neither is an unused file
    // (the eligibility gate, not the orphan signal).
    let unused_files: Vec<String> = results
        .unused_files
        .iter()
        .map(|f| f.file.path.to_string_lossy().replace('\\', "/"))
        .collect();
    assert!(
        !unused_files.iter().any(|p| p.ends_with("Orphan.astro")),
        "Orphan.astro is reachable via the barrel, not an unused file: {unused_files:?}"
    );
}
