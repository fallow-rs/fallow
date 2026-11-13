use super::common::{create_config, fixture_path};

/// Biome loads each package that `extends` in `biome.json` or `biome.jsonc`
/// names. Fallow must credit these packages as used.
#[test]
fn biome_extends_credits_shared_config_packages() {
    let root = fixture_path("biome-extends-packages");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_dev_dependencies: Vec<&str> = results
        .unused_dev_dependencies
        .iter()
        .map(|dep| dep.dep.package_name.as_str())
        .collect();

    for package in ["biome-config-shared", "@acme/biome-config"] {
        assert!(
            !unused_dev_dependencies.contains(&package),
            "{package} should be credited through biome.jsonc extends, got {unused_dev_dependencies:?}"
        );
    }
    assert!(
        unused_dev_dependencies.contains(&"biome-config-unused-control"),
        "unreferenced control dependency should still be reported, got {unused_dev_dependencies:?}"
    );
}
