//! Integration tests for the pnpm hook files (`.pnpmfile.cjs` and friends).

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
fn pnpm_hook_files_are_used() {
    // pnpm loads `.pnpmfile.mjs` or `.pnpmfile.cjs` next to the lockfile, and
    // the files that the `pnpmfile` setting names, so no import reaches them.
    let root = fixture_path("pnpmfile-hooks");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_paths = unused_file_paths(&root, &results);
    for used in [
        ".pnpmfile.cjs",
        ".pnpmfile.mjs",
        "tools/workspace-hooks.mjs",
        "tools/npmrc-hooks.cjs",
    ] {
        assert!(
            !unused_paths.contains(&used.to_string()),
            "{used} should be used, unused files: {unused_paths:?}"
        );
    }
    assert!(
        unused_paths.contains(&"tools/orphan.cjs".to_string()),
        "ordinary unused files should still report, unused files: {unused_paths:?}"
    );
}
