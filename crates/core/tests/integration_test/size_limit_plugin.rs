use super::common::{create_config, fixture_path};

fn unused_file_paths(
    root: &std::path::Path,
    results: &fallow_types::results::AnalysisResults,
) -> Vec<String> {
    results
        .unused_files
        .iter()
        .map(|finding| {
            finding
                .file
                .path
                .strip_prefix(root)
                .unwrap_or(&finding.file.path)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect()
}

fn unused_dev_dependency_names(results: &fallow_types::results::AnalysisResults) -> Vec<&str> {
    results
        .unused_dev_dependencies
        .iter()
        .map(|dep| dep.dep.package_name.as_str())
        .collect()
}

#[test]
fn size_limit_config_file_and_preset_are_credited() {
    // A JS config is the meaningful always-used check: a `.size-limit.json`
    // is never a source file, so only the `.size-limit.js` form can show up
    // as an unused file when the plugin fails to keep it reachable.
    let root = fixture_path("size-limit-plugin");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_paths = unused_file_paths(&root, &results);
    assert!(
        !unused_paths.contains(&".size-limit.js".to_string()),
        "size-limit config should be reachable, unused files: {unused_paths:?}"
    );
    assert!(
        unused_paths.contains(&"src/orphan.js".to_string()),
        "ordinary unused files should still report, unused files: {unused_paths:?}"
    );

    // size-limit's loader accepts both the `@size-limit/` scope and the
    // community `size-limit-*` prefix, so an unscoped plugin must be credited too.
    let unused_dev_dependencies = unused_dev_dependency_names(&results);
    for dep in [
        "@size-limit/preset-small-lib",
        "size-limit-node-esbuild",
        "size-limit",
    ] {
        assert!(
            !unused_dev_dependencies.contains(&dep),
            "{dep} should be credited by size-limit plugin support, unused dev deps: {unused_dev_dependencies:?}"
        );
    }
    assert!(
        unused_dev_dependencies.contains(&"unused-control"),
        "unreferenced control dependency should still be reported, unused dev deps: {unused_dev_dependencies:?}"
    );
}

#[test]
fn size_limit_package_json_config_activates_plugin() {
    // No config file and no script: the `size-limit` devDependency alone
    // enables the plugin, and the manifest `"size-limit"` array is the config
    // size-limit reads first, so the `@size-limit/*` plugin it loads by
    // convention must still be credited.
    let root = fixture_path("size-limit-plugin-package-json");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_dev_dependencies = unused_dev_dependency_names(&results);
    for dep in ["@size-limit/file", "size-limit"] {
        assert!(
            !unused_dev_dependencies.contains(&dep),
            "{dep} should be credited by size-limit plugin support, unused dev deps: {unused_dev_dependencies:?}"
        );
    }
    assert!(
        unused_dev_dependencies.contains(&"unused-control"),
        "unreferenced control dependency should still be reported, unused dev deps: {unused_dev_dependencies:?}"
    );
}

#[test]
fn size_limit_workspace_config_is_credited_when_tool_is_hoisted() {
    // size-limit walks up from the workspace package to the root that holds
    // the tool and its presets, then loads the config from the workspace cwd.
    // The plugin only activates at the root, and always-used globs anchor
    // there, so the workspace config depends on the config-file predicate.
    let root = fixture_path("size-limit-plugin-workspace");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_paths = unused_file_paths(&root, &results);
    assert!(
        !unused_paths.contains(&"packages/lib/.size-limit.js".to_string()),
        "workspace size-limit config should be reachable, unused files: {unused_paths:?}"
    );
    assert!(
        unused_paths.contains(&"packages/lib/src/orphan.js".to_string()),
        "ordinary unused workspace files should still report, unused files: {unused_paths:?}"
    );

    let unused_dev_dependencies = unused_dev_dependency_names(&results);
    for dep in ["@size-limit/preset-small-lib", "size-limit"] {
        assert!(
            !unused_dev_dependencies.contains(&dep),
            "{dep} should be credited at the hoisting root, unused dev deps: {unused_dev_dependencies:?}"
        );
    }
    assert!(
        unused_dev_dependencies.contains(&"unused-control"),
        "unreferenced workspace dependency should still be reported, unused dev deps: {unused_dev_dependencies:?}"
    );
}
