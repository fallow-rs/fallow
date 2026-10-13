//! Integration tests for the osls plugin.

use super::common::{create_config, fixture_path};

#[test]
fn osls_reads_serverless_yml_handlers_and_plugins() {
    // osls is a fork of the Serverless Framework and reads the same
    // `serverless.yml` config.
    let root = fixture_path("osls-plugin");
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
    assert!(
        !unused_paths.contains(&"src/handlers/harvest.ts".to_string()),
        "the handler should be reachable through serverless.yml, unused files: {unused_paths:?}"
    );
    assert!(
        unused_paths.contains(&"src/orphan.ts".to_string()),
        "ordinary unused files should still report, unused files: {unused_paths:?}"
    );

    let unused_dev_dependencies: Vec<&str> = results
        .unused_dev_dependencies
        .iter()
        .map(|dep| dep.dep.package_name.as_str())
        .collect();
    for dep in ["osls", "serverless-offline"] {
        assert!(
            !unused_dev_dependencies.contains(&dep),
            "{dep} should be credited by the osls plugin, unused dev deps: {unused_dev_dependencies:?}"
        );
    }
    assert!(
        unused_dev_dependencies.contains(&"unused-control"),
        "unreferenced control dependency should still be reported, unused dev deps: {unused_dev_dependencies:?}"
    );
}
