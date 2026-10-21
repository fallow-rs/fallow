//! Integration tests for the serverless-framework plugin.

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
fn serverless_handlers_plugins_and_esbuild_references_are_credited() {
    // The framework loads each function handler, each plugin and each esbuild
    // inject file from `serverless.yml`. Some of them come from files that a
    // `${file(...)}` variable names.
    let root = fixture_path("serverless-framework-plugin");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_paths = unused_file_paths(&root, &results);
    for path in [
        "function/handler.js",
        "function-2/function2.ts",
        "src/hello.js",
        "src/users.ts",
        "src/globals.ts",
        "plugins/local-plugin.js",
    ] {
        assert!(
            !unused_paths.contains(&path.to_string()),
            "{path} should be reachable through serverless.yml, unused files: {unused_paths:?}"
        );
    }
    assert!(
        unused_paths.contains(&"src/orphan.js".to_string()),
        "ordinary unused files should still report, unused files: {unused_paths:?}"
    );

    let unused_dev_dependencies = unused_dev_dependency_names(&results);
    for dep in [
        "serverless",
        "serverless-esbuild",
        "serverless-offline",
        "esbuild",
    ] {
        assert!(
            !unused_dev_dependencies.contains(&dep),
            "{dep} should be credited by the serverless-framework plugin, unused dev deps: {unused_dev_dependencies:?}"
        );
    }
    assert!(
        unused_dev_dependencies.contains(&"unused-control"),
        "unreferenced control dependency should still be reported, unused dev deps: {unused_dev_dependencies:?}"
    );
}

#[test]
fn nested_script_config_is_a_service_config_only_in_a_service_directory() {
    // The framework reads `serverless.*` from the service directory only. A
    // nested `serverless.yml` and a `serverless.ts` next to a `package.json`
    // are service configs. `src/lib/serverless.ts` is an ordinary module.
    let root = fixture_path("serverless-nested-config");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_paths = unused_file_paths(&root, &results);
    assert!(
        unused_paths.contains(&"src/lib/orphan.ts".to_string()),
        "a handler of a helper module is not an entry point, unused files: {unused_paths:?}"
    );
    for path in ["services/api/handler.ts", "services/web/handler.ts"] {
        assert!(
            !unused_paths.contains(&path.to_string()),
            "{path} should be reachable through its service config, unused files: {unused_paths:?}"
        );
    }

    let unused_dev_dependencies = unused_dev_dependency_names(&results);
    assert!(
        unused_dev_dependencies.contains(&"serverless-offline"),
        "a plugin of a helper module is not credited, unused dev deps: {unused_dev_dependencies:?}"
    );
    assert!(
        !unused_dev_dependencies.contains(&"serverless-esbuild"),
        "a plugin of a nested serverless.yml is credited, unused dev deps: {unused_dev_dependencies:?}"
    );
}
