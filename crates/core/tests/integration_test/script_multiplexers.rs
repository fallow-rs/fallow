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

    assert!(
        !unused_dev_dep_names.contains(&"concurrently"),
        "concurrently should be detected as used via scripts, unused dev deps: {unused_dev_dep_names:?}"
    );

    assert!(
        !unused_dev_dep_names.contains(&"npm-run-all"),
        "npm-run-all should be detected as used via run-s script, unused dev deps: {unused_dev_dep_names:?}"
    );

    assert!(
        !unused_dev_dep_names.contains(&"tsx"),
        "tsx should be detected as used via scripts, unused dev deps: {unused_dev_dep_names:?}"
    );
}

#[test]
fn concurrently_inline_commands_credit_binaries_and_files() {
    let root = fixture_path("script-multiplexers");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_files: Vec<String> = results
        .unused_files
        .iter()
        .map(|f| f.file.path.to_string_lossy().replace('\\', "/"))
        .collect();
    assert!(
        !unused_files.iter().any(|path| path.ends_with("src/api.ts")),
        "a file argument of a quoted concurrently command is an entry point, unused files: {unused_files:?}"
    );

    let unused_dev_dep_names: Vec<&str> = results
        .unused_dev_dependencies
        .iter()
        .map(|d| d.dep.package_name.as_str())
        .collect();
    assert!(
        !unused_dev_dep_names.contains(&"http-server"),
        "the binary of a quoted concurrently command is used, unused dev deps: {unused_dev_dep_names:?}"
    );
}
