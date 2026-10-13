//! Integration tests for the jasmine plugin.

use super::common::{create_config, fixture_path};

fn unused_file_paths(
    root: &std::path::Path,
    results: &fallow_types::results::AnalysisResults,
) -> Vec<String> {
    results
        .unused_files
        .iter()
        .map(|finding| {
            finding
                .file
                .path
                .strip_prefix(root)
                .unwrap_or(&finding.file.path)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect()
}

fn unused_dev_dependency_names(results: &fallow_types::results::AnalysisResults) -> Vec<&str> {
    results
        .unused_dev_dependencies
        .iter()
        .map(|dep| dep.dep.package_name.as_str())
        .collect()
}

#[test]
fn jasmine_config_spec_files_helpers_and_requires_are_credited() {
    // Jasmine reads `spec/support/jasmine.mjs`, joins `spec_files` and
    // `helpers` to `spec_dir`, and loads `requires` and `loader` from the
    // project root.
    let root = fixture_path("jasmine-plugin");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_paths = unused_file_paths(&root, &results);
    for path in [
        "spec/support/jasmine.mjs",
        "tests/unit/cart.suite.js",
        "tests/setup/currency.js",
        "bootstrap/setup.js",
        "config/custom-loader.mjs",
    ] {
        assert!(
            !unused_paths.contains(&path.to_string()),
            "{path} should be reachable through the jasmine config, unused files: {unused_paths:?}"
        );
    }
    for path in ["tests/unit/legacy.suite.js", "src/orphan.js"] {
        assert!(
            unused_paths.contains(&path.to_string()),
            "{path} should still report, unused files: {unused_paths:?}"
        );
    }

    let unused_dev_dependencies = unused_dev_dependency_names(&results);
    for dep in ["jasmine", "source-map-support"] {
        assert!(
            !unused_dev_dependencies.contains(&dep),
            "{dep} should be credited by the jasmine plugin, unused dev deps: {unused_dev_dependencies:?}"
        );
    }
    assert!(
        unused_dev_dependencies.contains(&"unused-control"),
        "unreferenced control dependency should still be reported, unused dev deps: {unused_dev_dependencies:?}"
    );
}
