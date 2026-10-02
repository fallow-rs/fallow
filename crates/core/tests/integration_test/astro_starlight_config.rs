//! Astro config with the Starlight integration: files named in the
//! `components` overrides and the local `customCss` entries are used, and a
//! package named in `customCss` is a used dependency. A component that the
//! config does not name stays an unused file.

use super::common::{create_config, fixture_path};

#[test]
fn credits_starlight_component_overrides_and_custom_css() {
    let root = fixture_path("astro-starlight-config");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_files: Vec<String> = results
        .unused_files
        .iter()
        .map(|f| f.file.path.to_string_lossy().replace('\\', "/"))
        .collect();
    assert!(
        !unused_files
            .iter()
            .any(|p| p.ends_with("src/components/Header.astro")),
        "a component override named in the config is used: {unused_files:?}"
    );
    assert!(
        !unused_files
            .iter()
            .any(|p| p.ends_with("src/styles/custom.css")),
        "a local customCss file is used: {unused_files:?}"
    );
    assert!(
        unused_files
            .iter()
            .any(|p| p.ends_with("src/components/Orphan.astro")),
        "a component that the config does not name stays unused: {unused_files:?}"
    );

    let unused_deps: Vec<&str> = results
        .unused_dependencies
        .iter()
        .map(|d| d.dep.package_name.as_str())
        .collect();
    assert!(
        !unused_deps.contains(&"@fontsource/inter"),
        "a package named in customCss is used: {unused_deps:?}"
    );
}
