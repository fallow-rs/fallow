//! Issue #4716: a Vite config in a workspace package names its
//! `rollupOptions.input` values relative to the package directory. When only
//! the root manifest lists `vite`, the root run reads that config, and the
//! input files must become entry points under the package directory.

use super::common::{create_config, fixture_path};

fn unused_file_paths(results: &fallow_types::results::AnalysisResults) -> Vec<String> {
    results
        .unused_files
        .iter()
        .map(|f| f.file.path.to_string_lossy().replace('\\', "/"))
        .collect()
}

#[test]
fn vite_workspace_rollup_inputs_are_entries_below_the_package() {
    let root = fixture_path("issue-4716-vite-workspace-rollup-input");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let paths = unused_file_paths(&results);
    for entry in ["packages/app/src/second.ts", "packages/app/src/third.ts"] {
        assert!(
            !paths.iter().any(|path| path.ends_with(entry)),
            "{entry} is a rollupOptions.input entry and should be reachable. Got: {paths:?}"
        );
    }
    assert!(
        paths
            .iter()
            .any(|path| path.ends_with("packages/app/src/orphan.ts")),
        "orphan.ts is referenced by nothing and must remain unused. Got: {paths:?}"
    );
}
