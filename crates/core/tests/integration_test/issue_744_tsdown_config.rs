use super::common::{create_config, fixture_path};

#[test]
fn issue_744_tsdown_mts_and_cts_configs_are_entry_points() {
    let root = fixture_path("issue-744-tsdown-mts-cts-configs");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_files: Vec<String> = results
        .unused_files
        .iter()
        .map(|file| {
            file.file
                .path
                .strip_prefix(&root)
                .unwrap_or(&file.file.path)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();

    for used_path in [
        "tsdown.config.mts",
        "tsdown.config.cts",
        "src/index.ts",
        "src/cli.ts",
    ] {
        assert!(
            !unused_files.iter().any(|unused| unused == used_path),
            "{used_path} should be reachable through the tsdown plugin, unused files: {unused_files:?}"
        );
    }

    assert!(
        unused_files.iter().any(|unused| unused == "src/orphan.ts"),
        "unrelated source files should remain reportable, unused files: {unused_files:?}"
    );
}
