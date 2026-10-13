//! Integration tests for the mise plugin.

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
fn mise_task_commands_credit_packages_and_files() {
    // mise runs the `run` commands of tasks from `mise.toml`, from
    // `.config/mise/config.toml`, and from `.mise/conf.d/*.toml`.
    let root = fixture_path("mise-plugin");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_paths = unused_file_paths(&root, &results);
    for path in [
        "scripts/check.ts",
        "scripts/build.ts",
        "scripts/grouped.ts",
        "scripts/release.ts",
    ] {
        assert!(
            !unused_paths.contains(&path.to_string()),
            "{path} should be reachable through a mise task, unused files: {unused_paths:?}"
        );
    }
    assert!(
        unused_paths.contains(&"src/orphan.ts".to_string()),
        "ordinary unused files should still report, unused files: {unused_paths:?}"
    );

    let unused_dev_dependencies = unused_dev_dependency_names(&results);
    for dep in ["lint-cli", "test-cli", "windows-cli", "local-cli"] {
        assert!(
            !unused_dev_dependencies.contains(&dep),
            "{dep} should be credited by a mise task, unused dev deps: {unused_dev_dependencies:?}"
        );
    }
    // A bare command without `node_modules/.bin` on the task PATH can be a
    // tool that mise installs, so it does not credit the package.
    for dep in ["global-cli", "unused-control"] {
        assert!(
            unused_dev_dependencies.contains(&dep),
            "{dep} should still be reported, unused dev deps: {unused_dev_dependencies:?}"
        );
    }
}
