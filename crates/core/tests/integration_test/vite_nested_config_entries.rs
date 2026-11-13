//! A workspace package can keep its own `vite.config.*` while only the root
//! manifest lists `vite` or `vite-plus`. The nested config is an entry point,
//! so the local files that it imports are reachable.

use super::common::{create_config, fixture_path};

fn unused_files(
    results: &fallow_core::results::AnalysisResults,
    root: &std::path::Path,
) -> Vec<String> {
    let mut files: Vec<String> = results
        .unused_files
        .iter()
        .map(|file| {
            file.file
                .path
                .strip_prefix(root)
                .unwrap_or(&file.file.path)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();
    files.sort();
    files
}

fn assert_nested_config_helper_is_used(fixture: &str, helper: &str) {
    let root = fixture_path(fixture);
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let files = unused_files(&results, &root);
    assert!(
        !files.iter().any(|file| file == helper),
        "{helper} is imported by packages/a/vite.config.ts and must be used, got {files:?}"
    );
    assert!(
        !files.iter().any(|file| file == "packages/a/vite.config.ts"),
        "the nested Vite config must be used, got {files:?}"
    );
    assert!(
        files.iter().any(|file| file == "orphan.ts"),
        "orphan.ts is imported by nothing and must stay unused, got {files:?}"
    );
}

#[test]
fn nested_vite_plus_config_with_root_dependency_reaches_its_helper() {
    assert_nested_config_helper_is_used("vite-plus-nested-config", "vite.package.ts");
}

#[test]
fn nested_vite_config_with_root_dependency_reaches_its_helper() {
    assert_nested_config_helper_is_used("vite-nested-config", "vite.shared.ts");
}
