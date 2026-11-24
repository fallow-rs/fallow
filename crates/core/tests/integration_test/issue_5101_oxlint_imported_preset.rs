use super::common::{create_config, fixture_path};

#[test]
fn oxlint_imported_preset_credits_its_js_plugins() {
    let root = fixture_path("issue-5101-oxlint-imported-preset");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_dev_dependencies: Vec<&str> = results
        .unused_dev_dependencies
        .iter()
        .map(|dep| dep.dep.package_name.as_str())
        .collect();

    for plugin in ["oxlint-plugin-react-doctor", "eslint-plugin-base-rule"] {
        assert!(
            !unused_dev_dependencies.contains(&plugin),
            "{plugin} should be credited through the imported preset, got {unused_dev_dependencies:?}"
        );
    }
    assert!(
        unused_dev_dependencies.contains(&"eslint-plugin-unused-control"),
        "unreferenced control dependency should still be reported, got {unused_dev_dependencies:?}"
    );
}
