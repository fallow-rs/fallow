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
fn release_it_config_file_and_plugins_are_credited() {
    // release-it loads `.release-it.mjs` by convention, and it loads each key
    // of the `plugins` object as a package or as a local module.
    let root = fixture_path("release-it-plugin");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_paths = unused_file_paths(&root, &results);
    for path in [".release-it.mjs", "scripts/release-plugin.js"] {
        assert!(
            !unused_paths.contains(&path.to_string()),
            "{path} should be reachable through the release-it config, unused files: {unused_paths:?}"
        );
    }
    assert!(
        unused_paths.contains(&"src/orphan.js".to_string()),
        "ordinary unused files should still report, unused files: {unused_paths:?}"
    );

    let unused_dev_dependencies = unused_dev_dependency_names(&results);
    for dep in [
        "release-it",
        "@release-it/conventional-changelog",
        "release-it-sample-plugin",
    ] {
        assert!(
            !unused_dev_dependencies.contains(&dep),
            "{dep} should be credited by the release-it plugin, unused dev deps: {unused_dev_dependencies:?}"
        );
    }
    assert!(
        unused_dev_dependencies.contains(&"unused-control"),
        "unreferenced control dependency should still be reported, unused dev deps: {unused_dev_dependencies:?}"
    );
}
