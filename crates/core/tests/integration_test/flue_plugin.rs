//! Integration tests for the flue plugin.

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
fn flue_default_modules_and_agents_are_entry_points() {
    // Without config keys, Flue loads app, db and the agents from `src/`.
    let root = fixture_path("flue-plugin/default");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_paths = unused_file_paths(&root, &results);
    for path in [
        "flue.config.ts",
        "src/app.ts",
        "src/db.ts",
        "src/agents/triage.ts",
        "src/agents/support/agent.ts",
    ] {
        assert!(
            !unused_paths.contains(&path.to_string()),
            "{path} should be a Flue entry point, unused files: {unused_paths:?}"
        );
    }
    assert!(
        unused_paths.contains(&"src/agents/shared/format.ts".to_string()),
        "a nested agent helper is not an agent, unused files: {unused_paths:?}"
    );

    let unused_dev_dependencies: Vec<&str> = results
        .unused_dev_dependencies
        .iter()
        .map(|dep| dep.dep.package_name.as_str())
        .collect();
    assert!(
        unused_dev_dependencies.contains(&"unused-control"),
        "unreferenced control dependency should still be reported, unused dev deps: {unused_dev_dependencies:?}"
    );
    assert!(
        !results
            .unused_dependencies
            .iter()
            .any(|dep| dep.dep.package_name == "@flue/runtime"),
        "@flue/runtime is imported by the config"
    );
}

#[test]
fn flue_config_keys_replace_default_entries() {
    // The config names the app and Cloudflare modules and the agent pattern.
    // The agent pattern is relative to the `.flue` source root.
    let root = fixture_path("flue-plugin/configured");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_paths = unused_file_paths(&root, &results);
    for path in [
        "flue.config.ts",
        "server/routes.ts",
        "server/workers.ts",
        ".flue/assistants/concierge.ts",
    ] {
        assert!(
            !unused_paths.contains(&path.to_string()),
            "{path} should be a configured Flue entry point, unused files: {unused_paths:?}"
        );
    }
    assert!(
        unused_paths.contains(&".flue/app.ts".to_string()),
        "the config moves the app module, so the default file is unused: {unused_paths:?}"
    );
}
