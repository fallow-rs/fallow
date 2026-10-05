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

fn unused_default_export_paths(
    root: &std::path::Path,
    results: &fallow_types::results::AnalysisResults,
) -> Vec<String> {
    results
        .unused_exports
        .iter()
        .filter(|finding| finding.export.export_name == "default")
        .map(|finding| {
            finding
                .export
                .path
                .strip_prefix(root)
                .unwrap_or(&finding.export.path)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect()
}

#[test]
fn eve_agent_slot_files_and_evals_are_entry_points() {
    // eve loads each module under `agent/` by its path, and `eve eval` loads
    // each `evals/*.eval.ts` file. Modules under `lib/` are import-only.
    let root = fixture_path("eve-agent-plugin");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_paths = unused_file_paths(&root, &results);
    for path in [
        "agent/agent.ts",
        "agent/tools/get_weather.ts",
        "agent/lib/format.ts",
        "agent/subagents/researcher/agent.ts",
        "evals/weather.eval.ts",
        "evals/evals.config.ts",
    ] {
        assert!(
            !unused_paths.contains(&path.to_string()),
            "{path} should be reachable through the eve file conventions, unused files: {unused_paths:?}"
        );
    }
    for path in [
        "agent/lib/unused-helper.ts",
        "agent/subagents/researcher/lib/unused-research-helper.ts",
        "src/orphan.ts",
    ] {
        assert!(
            unused_paths.contains(&path.to_string()),
            "{path} has no importer and should still report, unused files: {unused_paths:?}"
        );
    }

    let unused_defaults = unused_default_export_paths(&root, &results);
    assert!(
        unused_defaults.is_empty(),
        "eve reads the default export of each slot file, unused default exports: {unused_defaults:?}"
    );
}
