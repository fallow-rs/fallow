//! Issue #638: Node script entrypoints can be extensionless directory paths
//! and child-process runners can be passed through static path bindings.

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
fn issue_638_node_script_directory_index_and_fork_runner_are_reachable() {
    let root = fixture_path("issue-638-node-script-entrypoints");
    let mut config = create_config(root.clone());
    config.production = true;
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_paths = unused_file_paths(&root, &results);

    assert!(
        !unused_paths.contains(&"packages/svelte/scripts/process-messages/index.js".to_string()),
        "extensionless package script directory should resolve to index.js: {unused_paths:?}"
    );
    assert!(
        !unused_paths.contains(&"benchmarking/compare/runner.js".to_string()),
        "static child_process.fork runner should be reachable: {unused_paths:?}"
    );
    assert!(
        unused_paths.contains(&"benchmarking/compare/unrelated-worker.js".to_string()),
        "unrelated benchmark worker should remain unused: {unused_paths:?}"
    );
    assert!(
        unused_paths.contains(&"packages/svelte/scripts/process-messages/unrelated.js".to_string()),
        "unrelated process helper should remain unused: {unused_paths:?}"
    );
}
