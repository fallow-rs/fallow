//! Integration tests for the gulp plugin.

use super::common::{create_config, fixture_path};

#[test]
fn gulpfiles_and_their_imports_are_entry_points() {
    let root = fixture_path("gulp-plugin");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_paths: Vec<String> = results
        .unused_files
        .iter()
        .map(|finding| {
            finding
                .file
                .path
                .strip_prefix(&root)
                .unwrap_or(&finding.file.path)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();

    for path in [
        "gulpfile.mjs",
        "tools/gulp/tasks.js",
        "legacy/gulpfile.js/index.js",
        "legacy/gulpfile.js/clean.js",
    ] {
        assert!(
            !unused_paths.contains(&path.to_string()),
            "gulp loads {path}, unused files: {unused_paths:?}"
        );
    }
    assert!(
        unused_paths.contains(&"src/unused.js".to_string()),
        "a file that no gulpfile imports must stay unused: {unused_paths:?}"
    );
}
