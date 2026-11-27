//! Issue #5385: Vite 8 names the bundler options `build.rolldownOptions`.
//! An entry that a config declares under that name must be an entry point,
//! or each file that only such an entry reaches is a false `unused-file`.
//! The fixture uses the function form of `defineConfig` with a block body, a
//! spread inside `build`, and the `vite-plus` import.

use super::common::{create_config, fixture_path};

#[test]
fn vite_rolldown_options_input_seeds_entry_points() {
    let root = fixture_path("issue-5385-vite-rolldown-options-input");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let names: Vec<String> = results
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
        .collect();

    for entry in ["second.html", "secondMain.tsx", "workerMain.ts"] {
        assert!(
            !names.contains(&entry.to_string()),
            "{entry} is reached from a rolldownOptions.input entry. Got: {names:?}"
        );
    }

    assert!(
        names.contains(&"orphan.ts".to_string()),
        "orphan.ts is referenced by nothing and must remain unused. Got: {names:?}"
    );
}
