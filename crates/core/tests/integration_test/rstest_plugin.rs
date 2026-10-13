//! Integration tests for the rstest plugin.

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
fn rstest_projects_setup_files_and_dependencies_are_credited() {
    // Rstest reads `rstest.config.ts`. With `projects`, each inline project
    // and each project directory sets its own test files, and the root
    // default `include` no longer applies.
    let root = fixture_path("rstest-plugin");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_paths = unused_file_paths(&root, &results);
    for path in [
        "rstest.config.ts",
        "src/math.check.ts",
        "src/setup/per-file.ts",
        "src/setup/global.ts",
        "src/__mocks__/lodash.ts",
        "dom/render.dom.ts",
        "dom/theme/colors.ts",
        "packages/cat/rstest.config.ts",
        "packages/cat/global-setup.ts",
        "packages/cat/purr.meow.ts",
    ] {
        assert!(
            !unused_paths.contains(&path.to_string()),
            "{path} should be reachable through the rstest config, unused files: {unused_paths:?}"
        );
    }
    for path in [
        "src/ignored.check.ts",
        "src/skipped.check.ts",
        "src/orphan.ts",
        "root.test.ts",
        "packages/excluded/rstest.config.mjs",
    ] {
        assert!(
            unused_paths.contains(&path.to_string()),
            "{path} should still report, unused files: {unused_paths:?}"
        );
    }

    let unused_dev_dependencies = unused_dev_dependency_names(&results);
    for dep in [
        "@rstest/core",
        "@rstest/coverage-v8",
        "@rstest/browser",
        "playwright",
        "jsdom",
    ] {
        assert!(
            !unused_dev_dependencies.contains(&dep),
            "{dep} should be credited by the rstest plugin, unused dev deps: {unused_dev_dependencies:?}"
        );
    }
    for dep in ["@rstest/coverage-istanbul", "unused-control"] {
        assert!(
            unused_dev_dependencies.contains(&dep),
            "{dep} should still be reported, unused dev deps: {unused_dev_dependencies:?}"
        );
    }
}
