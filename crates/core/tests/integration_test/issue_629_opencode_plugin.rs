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

#[test]
fn issue_629_opencode_plugin_files_and_declared_plugin_deps_are_reachable() {
    let root = fixture_path("issue-629-opencode-plugin");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_paths = unused_file_paths(&root, &results);
    assert!(
        !unused_paths.contains(&".opencode/plugins/local.ts".to_string()),
        "OpenCode local plugin file should be reachable, got {unused_paths:?}"
    );
    assert!(
        unused_paths.contains(&"src/orphan.ts".to_string()),
        "ordinary unused file should still be reported, got {unused_paths:?}"
    );

    let unused_dev_dependencies: Vec<&str> = results
        .unused_dev_dependencies
        .iter()
        .map(|dep| dep.dep.package_name.as_str())
        .collect();
    for dep in [
        "@acme/opencode-theme",
        "@opencode-ai/plugin",
        "opencode-wakatime",
    ] {
        assert!(
            !unused_dev_dependencies.contains(&dep),
            "{dep} should be credited by OpenCode plugin support, got {unused_dev_dependencies:?}"
        );
    }
    assert!(
        unused_dev_dependencies.contains(&"unused-control"),
        "unreferenced control dependency should still be reported, got {unused_dev_dependencies:?}"
    );
}
