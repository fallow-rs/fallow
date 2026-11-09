use super::common::{create_config, fixture_path};

/// The root `package.json` declares `mocha` and `vite`, and the package
/// manager hoists them. A workspace script that calls `mocha` or `vite`
/// activates the plugin for that workspace. A workspace with no such script
/// keeps its test files reportable.
#[test]
fn hoisted_root_dependency_activates_the_plugin_of_a_workspace_script() {
    let root = fixture_path("hoisted-root-plugin-activation");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_files: Vec<String> = results
        .unused_files
        .iter()
        .map(|file| {
            file.file
                .path
                .strip_prefix(&root)
                .unwrap_or(&file.file.path)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();

    for reachable in [
        "packages/runner/test/index.js",
        "packages/runner/test/helper.js",
        "packages/web/linked.css",
    ] {
        assert!(
            !unused_files.iter().any(|unused| unused == reachable),
            "{reachable} is reachable through a hoisted root tool, unused files: {unused_files:?}"
        );
    }

    assert!(
        unused_files
            .iter()
            .any(|unused| unused == "packages/plain/test/orphan.js"),
        "a workspace that calls no root tool keeps its test files reportable, unused files: {unused_files:?}"
    );
}
