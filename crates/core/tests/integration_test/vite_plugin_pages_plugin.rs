//! Integration tests for the vite-plugin-pages plugin.

use super::common::{create_config, fixture_path};

#[test]
fn page_directories_of_the_pages_plugin_call_are_entry_points() {
    let root = fixture_path("vite-plugin-pages-plugin");
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
        "packages/common/src/pages/index.vue",
        "packages/common/src/pages/profile/index.vue",
        "packages/common/src/components/Header.vue",
        "packages/web/src/pages/login.vue",
        "packages/docs/src/pages/index.vue",
    ] {
        assert!(
            !unused_paths.contains(&path.to_string()),
            "vite-plugin-pages loads {path} as a route, unused files: {unused_paths:?}"
        );
    }
    for path in [
        "packages/common/src/components/Unused.vue",
        "packages/docs/src/views/Orphan.vue",
    ] {
        assert!(
            unused_paths.contains(&path.to_string()),
            "{path} is not in a pages directory and must stay unused: {unused_paths:?}"
        );
    }
}
