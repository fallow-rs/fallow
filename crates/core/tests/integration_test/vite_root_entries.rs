//! Vite reads its default entries from the directory that the `root` option
//! names. An `index.html` under that directory is an entry, and the scripts it
//! loads are reachable.

use super::common::{create_config, fixture_path};

fn unused_file_paths(results: &fallow_types::results::AnalysisResults) -> Vec<String> {
    let root = fixture_path("vite-root-entries");
    results
        .unused_files
        .iter()
        .map(|f| {
            f.file
                .path
                .strip_prefix(&root)
                .unwrap_or(&f.file.path)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect()
}

#[test]
fn vite_root_option_rebases_the_default_entries() {
    let root = fixture_path("vite-root-entries");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let paths = unused_file_paths(&results);

    for reachable in ["web/main.ts", "src/shared.ts"] {
        assert!(
            !paths.contains(&reachable.to_string()),
            "{reachable} is reachable from web/index.html under the Vite root. Got: {paths:?}"
        );
    }
    assert!(
        paths.contains(&"src/orphan.ts".to_string()),
        "src/orphan.ts is referenced by nothing and must stay unused. Got: {paths:?}"
    );
}
