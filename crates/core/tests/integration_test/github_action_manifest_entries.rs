use super::common::{create_config, fixture_path};
use super::framework_convention_coverage_common::collect_unused_files;

#[test]
fn node_action_manifest_pre_main_and_post_files_are_entry_points() {
    let root = fixture_path("github-action-node-entries");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_files = collect_unused_files(&root, &results);
    for path in [
        "src/setup.ts",
        "src/main.ts",
        "src/cleanup.ts",
        "src/util.ts",
    ] {
        assert!(
            !unused_files.iter().any(|unused| unused == path),
            "{path} is reachable from action.yml, unused files: {unused_files:?}"
        );
    }
    assert!(
        unused_files.iter().any(|unused| unused == "src/orphan.ts"),
        "the unreferenced control file must stay unused, unused files: {unused_files:?}"
    );
}

#[test]
fn composite_action_manifest_adds_no_entry_points() {
    let root = fixture_path("github-action-composite");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_files = collect_unused_files(&root, &results);
    assert!(
        unused_files.iter().any(|unused| unused == "src/run.ts"),
        "a composite action has no JavaScript entry, unused files: {unused_files:?}"
    );
}
