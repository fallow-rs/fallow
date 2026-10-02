//! A CI step that runs a file with Bun (`bun <file>`, `bun run <file>`,
//! `bun --watch <file>`) makes that file an entry point.

use super::common::{create_config, fixture_path};

fn unused_file_paths(results: &fallow_types::results::AnalysisResults) -> Vec<String> {
    results
        .unused_files
        .iter()
        .map(|f| f.file.path.to_string_lossy().replace('\\', "/"))
        .collect()
}

#[test]
fn ci_bun_file_runner_forms_seed_entries() {
    let root = fixture_path("ci-bun-file-runner");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let paths = unused_file_paths(&results);

    for file in [
        "scripts/bare-run.ts",
        "scripts/explicit-run.ts",
        "scripts/watch-run.ts",
    ] {
        assert!(
            !paths.iter().any(|p| p.ends_with(file)),
            "{file} runs from a CI step and must be an entry. Got: {paths:?}"
        );
    }
    assert!(
        paths.iter().any(|p| p.ends_with("scripts/orphan.ts")),
        "scripts/orphan.ts has no reference and must stay unused. Got: {paths:?}"
    );
}
