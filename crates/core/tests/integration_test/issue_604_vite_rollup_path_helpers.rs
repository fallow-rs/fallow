//! Issue #604: Vite `build.rollupOptions.input` entries declared via path-helper
//! calls (`resolve(__dirname, "src/app.ts")`, `path.resolve(...)`, `join(...)`)
//! must be evaluated and seeded as entry points, including CSS inputs. Files
//! referenced only this way were previously reported as `unused-files` until the
//! user duplicated the entry list into `.fallowrc`.

use super::common::{create_config, fixture_path};

fn unused_file_names(results: &fallow_types::results::AnalysisResults) -> Vec<String> {
    results
        .unused_files
        .iter()
        .map(|f| {
            f.file
                .path
                .to_string_lossy()
                .replace('\\', "/")
                .rsplit('/')
                .next()
                .unwrap_or("")
                .to_string()
        })
        .collect()
}

#[test]
fn vite_rollup_input_path_helpers_seed_entry_points() {
    let root = fixture_path("issue-604-vite-rollup-path-helpers");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let names = unused_file_names(&results);

    for entry in ["app.ts", "modal.ts", "tabs.ts"] {
        assert!(
            !names.contains(&entry.to_string()),
            "{entry} is a rollupOptions.input path-helper entry and should be reachable. Got: {names:?}"
        );
    }

    // CSS entry inputs must be preserved like any other entry, not dropped.
    assert!(
        !names.contains(&"index.css".to_string()),
        "index.css is a rollupOptions.input path-helper entry and should be reachable. Got: {names:?}"
    );

    // Control: a file referenced by nothing and declared in no entry stays
    // flagged, proving the path-helper seeding is scoped to real config entries
    // rather than blanket-crediting the project.
    assert!(
        names.contains(&"orphan.ts".to_string()),
        "orphan.ts is referenced by nothing and must remain unused. Got: {names:?}"
    );
}
