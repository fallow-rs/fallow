//! A `node --test` script without file arguments makes Node discover test
//! files through its default patterns. Those files are entry points of the
//! package that owns the script. A script with explicit file arguments runs
//! only those files.

use super::common::{create_config, fixture_path};

fn relative_paths(root: &std::path::Path, paths: &[&std::path::Path]) -> Vec<String> {
    paths
        .iter()
        .map(|path| {
            path.strip_prefix(root)
                .unwrap_or(path)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect()
}

#[test]
fn bare_node_test_script_uses_node_default_test_patterns() {
    let root = fixture_path("node-test-bare-default-patterns");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_files: Vec<&std::path::Path> = results
        .unused_files
        .iter()
        .map(|finding| finding.file.path.as_path())
        .collect();
    let unused_files = relative_paths(&root, &unused_files);
    let unused_exports: Vec<String> = results
        .unused_exports
        .iter()
        .map(|finding| finding.export.export_name.clone())
        .collect();

    for reachable in [
        "packages/a/test/add.test.ts",
        "packages/a/test/helpers.ts",
        "packages/b/src/double_test.ts",
        "packages/b/test-util.mjs",
        "packages/c/test/only.test.ts",
    ] {
        assert!(
            !unused_files.contains(&reachable.to_string()),
            "{reachable} matches a Node test pattern and must be an entry: {unused_files:?}"
        );
    }
    assert!(
        unused_files.contains(&"packages/c/test/other.test.ts".to_string()),
        "an explicit file argument must stop the default patterns: {unused_files:?}"
    );
    assert!(
        !unused_exports.contains(&"fixture".to_string()),
        "a helper that a default-pattern test imports must keep its export: {unused_exports:?}"
    );
}
