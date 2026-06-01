//! Issue #820: Vercel programmatic config files are convention entrypoints.

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
fn vercel_ts_config_is_not_reported_as_unused() {
    let root = fixture_path("issue-820-vercel-ts-config");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_paths = unused_file_paths(&root, &results);
    assert!(
        !unused_paths.contains(&"vercel.ts".to_string()),
        "vercel.ts is loaded by Vercel convention and must be credited, got {unused_paths:?}"
    );
    assert!(
        unused_paths.contains(&"src/orphan.ts".to_string()),
        "ordinary unused file should still be reported, got {unused_paths:?}"
    );
}
