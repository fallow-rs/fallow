use super::common::{create_config, fixture_path};

/// A custom `updater` module referenced from the package.json
/// `commit-and-tag-version` key is loaded by the tool at runtime and has no
/// static importer, so it must be credited as reachable. A sibling script that
/// nothing references must still be reported as unused. See issue #1640.
#[test]
fn commit_and_tag_version_updater_is_credited_control_still_flagged() {
    let root = fixture_path("issue-1640-commit-and-tag-version-updater");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_files: Vec<String> = results
        .unused_files
        .iter()
        .map(|file| file.file.path.to_string_lossy().replace('\\', "/"))
        .collect();

    assert!(
        !unused_files
            .iter()
            .any(|p| p.ends_with("scripts/gradle-updater.cjs")),
        "the commit-and-tag-version updater module must be reachable, got {unused_files:?}"
    );
    assert!(
        unused_files
            .iter()
            .any(|p| p.ends_with("scripts/genuinely-unused.cjs")),
        "an unreferenced control script must still be reported, got {unused_files:?}"
    );
}
