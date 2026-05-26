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
fn issue_616_browser_extension_manifest_entries_are_reachable_without_dependency() {
    let root = fixture_path("issue-616-browser-extension-manifest");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_paths = unused_file_paths(&root, &results);
    for path in [
        "extension/background.js",
        "extension/content.js",
        "extension/shared.js",
        "extension/styles/content.css",
        "extension/popup/popup.js",
        "extension/options.js",
        "extension/web-accessible.js",
    ] {
        assert!(
            !unused_paths.contains(&path.to_string()),
            "{path} should be reachable from extension manifest, unused files: {unused_paths:?}"
        );
    }
    assert!(
        unused_paths.contains(&"extension/orphan.js".to_string()),
        "unreferenced control file should still report, unused files: {unused_paths:?}"
    );
}

#[test]
fn issue_616_plain_web_manifest_does_not_activate_browser_extension_plugin() {
    let root = fixture_path("issue-616-web-manifest");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_paths = unused_file_paths(&root, &results);
    assert!(
        unused_paths.contains(&"src/app.js".to_string()),
        "ordinary web app manifest should not make adjacent JS reachable, unused files: {unused_paths:?}"
    );
}
