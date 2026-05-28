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
fn issue_758_root_dangerfile_is_credited_without_danger_dependency() {
    // Danger is commonly run from CI without `danger` declared in package.json.
    // The dangerfile's presence must activate the plugin so it is not flagged
    // as an unused file.
    let root = fixture_path("issue-758-danger-no-dep");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_paths = unused_file_paths(&root, &results);
    assert!(
        !unused_paths.contains(&"dangerfile.js".to_string()),
        "dangerfile.js should be credited via filesystem activation, unused files: {unused_paths:?}"
    );
    assert!(
        unused_paths.contains(&"src/orphan.js".to_string()),
        "an ordinary unreferenced file should still report, unused files: {unused_paths:?}"
    );
}
