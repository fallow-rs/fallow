use super::common::{create_config, fixture_path};

#[test]
fn script_multiplexer_dependencies_not_flagged_as_unused() {
    let root = fixture_path("script-multiplexers");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_dev_dep_names: Vec<&str> = results
        .unused_dev_dependencies
        .iter()
        .map(|d| d.dep.package_name.as_str())
        .collect();

    // concurrently is used in the "dev" script
    assert!(
        !unused_dev_dep_names.contains(&"concurrently"),
        "concurrently should be detected as used via scripts, unused dev deps: {unused_dev_dep_names:?}"
    );

    // npm-run-all is used via run-s in the "build" script
    assert!(
        !unused_dev_dep_names.contains(&"npm-run-all"),
        "npm-run-all should be detected as used via run-s script, unused dev deps: {unused_dev_dep_names:?}"
    );

    // tsx is used in the "server" and "worker" scripts
    assert!(
        !unused_dev_dep_names.contains(&"tsx"),
        "tsx should be detected as used via scripts, unused dev deps: {unused_dev_dep_names:?}"
    );
}
