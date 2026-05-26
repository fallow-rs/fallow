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
fn issue_612_wxt_config_modules_and_entrypoints_are_reachable() {
    let root = fixture_path("issue-612-wxt-plugin");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_paths = unused_file_paths(&root, &results);
    for reachable in [
        "wxt.config.ts",
        "entrypoints/background.ts",
        "entrypoints/popup/index.html",
        "entrypoints/popup/main.ts",
    ] {
        assert!(
            !unused_paths.contains(&reachable.to_string()),
            "{reachable} should be reachable through WXT conventions, unused files: {unused_paths:?}"
        );
    }
    assert!(
        unused_paths.contains(&"entrypoints/popup/unused-helper.ts".to_string()),
        "unimported entrypoint helper siblings should remain reportable, unused files: {unused_paths:?}"
    );
    assert!(
        unused_paths.contains(&"src/orphan.ts".to_string()),
        "ordinary orphan files should still report, unused files: {unused_paths:?}"
    );

    let unused_dev_dependencies: Vec<&str> = results
        .unused_dev_dependencies
        .iter()
        .map(|dep| dep.dep.package_name.as_str())
        .collect();
    for dep in ["@wxt-dev/module-svelte", "@wxt-dev/i18n", "wxt"] {
        assert!(
            !unused_dev_dependencies.contains(&dep),
            "{dep} should be credited by WXT plugin support, unused dev deps: {unused_dev_dependencies:?}"
        );
    }
    assert!(
        unused_dev_dependencies.contains(&"unused-control"),
        "unreferenced control dependency should still be reported, unused dev deps: {unused_dev_dependencies:?}"
    );
}
