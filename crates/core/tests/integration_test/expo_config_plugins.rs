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

fn assert_config_plugins_credited(fixture: &str, local_plugin: &str) {
    let root = fixture_path(fixture);
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    // Expo loads each `plugins` entry by name at prebuild time, so a package
    // listed there has no import anywhere in the source.
    let unused_dev_dependencies = unused_dev_dependency_names(&results);
    for dep in ["@sample/expo-plugin-pdf", "sample-expo-plugin-camera"] {
        assert!(
            !unused_dev_dependencies.contains(&dep),
            "{dep} is an Expo config plugin and must be credited, unused dev deps: {unused_dev_dependencies:?}"
        );
    }
    assert!(
        unused_dev_dependencies.contains(&"unused-control"),
        "an unlisted dependency must still be reported, unused dev deps: {unused_dev_dependencies:?}"
    );

    // A relative `plugins` entry names a local config plugin file.
    let unused_paths = unused_file_paths(&root, &results);
    assert!(
        !unused_paths.contains(&local_plugin.to_string()),
        "{local_plugin} is a local config plugin and must be reachable, unused files: {unused_paths:?}"
    );
}

#[test]
fn expo_app_json_config_plugins_are_credited() {
    assert_config_plugins_credited(
        "expo-app-json-config-plugins",
        "plugins/with-sample-setting.js",
    );
}

#[test]
fn expo_router_app_config_plugins_are_credited() {
    assert_config_plugins_credited(
        "expo-router-app-config-plugins",
        "plugins/with-sample-setting.ts",
    );
}
