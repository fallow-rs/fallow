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

#[test]
fn issue_625_k6_script_file_and_runtime_imports_are_reachable_without_dependency() {
    let root = fixture_path("issue-625-k6-plugin");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_paths = unused_file_paths(&root, &results);
    assert!(
        !unused_paths.contains(&"load/smoke.k6.js".to_string()),
        "k6 load script should be a static entry point, unused files: {unused_paths:?}"
    );
    assert!(
        unused_paths.contains(&"src/orphan.js".to_string()),
        "ordinary unused file should still report, unused files: {unused_paths:?}"
    );

    let unresolved_specifiers: Vec<&str> = results
        .unresolved_imports
        .iter()
        .map(|import| import.import.specifier.as_str())
        .collect();
    for specifier in ["k6", "k6/http"] {
        assert!(
            !unresolved_specifiers.contains(&specifier),
            "{specifier} should be treated as provided by the k6 runtime, unresolved imports: {unresolved_specifiers:?}"
        );
    }

    let unlisted_names: Vec<&str> = results
        .unlisted_dependencies
        .iter()
        .map(|dep| dep.dep.package_name.as_str())
        .collect();
    assert!(
        !unlisted_names.contains(&"k6"),
        "k6 runtime modules should not require package.json declaration, unlisted dependencies: {unlisted_names:?}"
    );
    assert!(
        unlisted_names.contains(&"k6-tools"),
        "similar package names should not be hidden by k6 runtime handling, unlisted dependencies: {unlisted_names:?}"
    );
}

#[test]
fn issue_625_k6_package_script_credits_cli_dependency() {
    let root = fixture_path("issue-625-k6-script");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_paths = unused_file_paths(&root, &results);
    assert!(
        !unused_paths.contains(&"load/scripted.k6.js".to_string()),
        "k6 package-script target should be reachable, unused files: {unused_paths:?}"
    );

    let unused_dev_dependencies: Vec<&str> = results
        .unused_dev_dependencies
        .iter()
        .map(|dep| dep.dep.package_name.as_str())
        .collect();
    assert!(
        !unused_dev_dependencies.contains(&"k6"),
        "k6 CLI dependency should be credited by package script usage, unused dev deps: {unused_dev_dependencies:?}"
    );
    assert!(
        unused_dev_dependencies.contains(&"unused-control"),
        "unreferenced control dependency should still be reported, unused dev deps: {unused_dev_dependencies:?}"
    );
}
