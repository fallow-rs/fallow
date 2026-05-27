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

fn unused_members(results: &fallow_types::results::AnalysisResults) -> Vec<String> {
    results
        .unused_class_members
        .iter()
        .map(|finding| {
            format!(
                "{}.{}",
                finding.member.parent_name, finding.member.member_name
            )
        })
        .collect()
}

#[test]
fn issue_617_obsidian_entry_assets_and_lifecycle_members_are_credited() {
    let root = fixture_path("issue-617-obsidian-plugin");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_paths = unused_file_paths(&root, &results);
    for path in ["src/main.ts", "manifest.json", "styles.css", "cdp.js"] {
        assert!(
            !unused_paths.contains(&path.to_string()),
            "{path} should be credited by the Obsidian plugin; unused files: {unused_paths:?}"
        );
    }
    assert!(
        unused_paths.contains(&"src/cdp.js".to_string()),
        "only root-level cdp.js should be credited; unused files: {unused_paths:?}"
    );

    let unused_members = unused_members(&results);
    for lifecycle in [
        "WorkTerminalPlugin.onload",
        "WorkTerminalPlugin.onunload",
        "TerminalModal.onOpen",
        "TerminalModal.onClose",
        "TerminalItemView.getViewType",
        "TerminalItemView.getDisplayText",
        "TerminalItemView.getIcon",
        "TerminalItemView.onOpen",
        "TerminalItemView.onClose",
        "TerminalItemView.onPaneMenu",
        "TerminalView.getViewType",
        "TerminalView.getDisplayText",
        "TerminalView.onOpen",
        "TerminalView.onClose",
    ] {
        assert!(
            !unused_members.contains(&lifecycle.to_string()),
            "{lifecycle} is called by Obsidian and must not report; unused members: {unused_members:?}"
        );
    }

    for dead in [
        "WorkTerminalPlugin.helperNeverCalled",
        "TerminalModal.modalHelper",
        "TerminalItemView.viewHelper",
        "TerminalView.viewHelper",
        "PlainObject.onload",
        "PlainObject.onOpen",
        "AliasPlugin.onload",
        "DerivedProjectPlugin.onunload",
    ] {
        assert!(
            unused_members.contains(&dead.to_string()),
            "{dead} should remain reportable; unused members: {unused_members:?}"
        );
    }
}

#[test]
fn issue_617_non_obsidian_project_keeps_similarly_named_methods_reportable() {
    let root = fixture_path("issue-617-non-obsidian-control");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused_members = unused_members(&results);

    for member in [
        "PlainPlugin.onload",
        "PlainPlugin.onunload",
        "PlainModal.onOpen",
    ] {
        assert!(
            unused_members.contains(&member.to_string()),
            "{member} should report without Obsidian activation; unused members: {unused_members:?}"
        );
    }
}
