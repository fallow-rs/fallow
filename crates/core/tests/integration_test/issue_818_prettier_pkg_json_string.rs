//! Issue #818: package.json#prettier string configs reference external config packages.

use super::common::{create_config, fixture_path};

fn unused_dev_deps(fixture: &str) -> Vec<String> {
    let root = fixture_path(fixture);
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    results
        .unused_dev_dependencies
        .iter()
        .map(|dep| dep.dep.package_name.clone())
        .collect()
}

#[test]
fn package_json_string_config_credits_external_package() {
    // Prettier treats package.json#prettier string values as an external config
    // package or file. Fallow must credit the package instead of reporting the
    // shared config as an unused dev dependency.
    let unused = unused_dev_deps("issue-818-prettier-pkg-json-string");

    assert!(
        !unused.contains(&"@scope/prettier-config".to_string()),
        "@scope/prettier-config is loaded from package.json#prettier and must be credited, got {unused:?}"
    );
    assert!(
        unused.contains(&"unused-control".to_string()),
        "unused-control should still be reported, got {unused:?}"
    );
}
