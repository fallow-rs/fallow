//! Integration tests for the railway plugin.

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

fn unused_dependency_names(results: &fallow_types::results::AnalysisResults) -> Vec<&str> {
    results
        .unused_dependencies
        .iter()
        .map(|dep| dep.dep.package_name.as_str())
        .collect()
}

#[test]
fn railway_service_commands_credit_packages_and_files() {
    // `.railway/railway.ts` declares services with `service()` from
    // `railway/iac`. The commands of a service run in its root directory.
    let root = fixture_path("railway-plugin");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_paths = unused_file_paths(&root, &results);
    for path in [
        ".railway/railway.ts",
        "services/storefront/scripts/build.ts",
        "services/storefront/src/serve.ts",
        "services/inventory/scripts/deploy.ts",
    ] {
        assert!(
            !unused_paths.contains(&path.to_string()),
            "{path} should be reachable through the Railway config, unused files: {unused_paths:?}"
        );
    }
    // The analytics service deploys another repository and the dashboard
    // service deploys an image, so their commands run no file of this project.
    for path in ["scripts/build.ts", "dashboard/start.ts"] {
        assert!(
            unused_paths.contains(&path.to_string()),
            "{path} should still be reported, unused files: {unused_paths:?}"
        );
    }

    let unused_dependencies = unused_dependency_names(&results);
    for dep in ["railway", "storefront-tool"] {
        assert!(
            !unused_dependencies.contains(&dep),
            "{dep} should be credited by the Railway config, unused deps: {unused_dependencies:?}"
        );
    }
    assert!(
        unused_dependencies.contains(&"external-tool"),
        "a tool of an image service should still be reported, unused deps: {unused_dependencies:?}"
    );
}
