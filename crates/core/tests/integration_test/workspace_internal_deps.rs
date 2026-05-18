use super::common::{create_config, fixture_path};

#[test]
fn workspace_package_dependencies_are_checked_like_external_dependencies() {
    let root = fixture_path("workspace-internal-deps");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_dep_names: Vec<&str> = results
        .unused_dependencies
        .iter()
        .map(|dep| dep.dep.package_name.as_str())
        .collect();

    assert!(
        unused_dep_names.contains(&"@repo/unused-internal"),
        "unused declared workspace dependency should be reported, found: {unused_dep_names:?}"
    );
    assert!(
        !unused_dep_names.contains(&"@repo/ui"),
        "used declared workspace dependency should not be reported, found: {unused_dep_names:?}"
    );
    assert!(
        !unused_dep_names.contains(&"@repo/plugin-framework"),
        "workspace dependency that activates a custom plugin and is listed as tooling should not be reported, found: {unused_dep_names:?}"
    );
}

#[test]
fn missing_workspace_package_dependency_is_unlisted_for_importing_workspace() {
    let root = fixture_path("workspace-internal-deps");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unlisted_names: Vec<&str> = results
        .unlisted_dependencies
        .iter()
        .map(|dep| dep.dep.package_name.as_str())
        .collect();

    assert!(
        unlisted_names.contains(&"@repo/tool"),
        "workspace package imported without a dependency declaration should be unlisted, found: {unlisted_names:?}"
    );
    let tool = results
        .unlisted_dependencies
        .iter()
        .find(|dep| dep.dep.package_name == "@repo/tool")
        .expect("@repo/tool should be reported as unlisted");
    assert!(
        tool.dep
            .imported_from
            .iter()
            .any(|site| site.path.ends_with("packages/app/src/index.ts")),
        "@repo/tool should point at the importing app file, found: {:?}",
        tool.dep.imported_from
    );
}

#[test]
fn custom_plugin_enabled_by_workspace_dependency_affects_analysis() {
    let root = fixture_path("workspace-internal-deps");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_file_paths: Vec<String> = results
        .unused_files
        .iter()
        .map(|file| file.file.path.to_string_lossy().replace('\\', "/"))
        .collect();

    assert!(
        !unused_file_paths
            .iter()
            .any(|path| path.ends_with("packages/app/src/plugin-kept.ts")),
        "custom plugin enabled by workspace dependency should keep plugin-kept.ts alive, found: {unused_file_paths:?}"
    );
}
