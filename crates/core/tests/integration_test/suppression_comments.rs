use super::common::{create_config, fixture_path};

#[test]
fn next_line_suppression_hides_unused_export() {
    let root = fixture_path("suppression-comments");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_export_names: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export.export_name.as_str())
        .collect();

    // suppressedExport has a fallow-ignore-next-line comment, should NOT appear
    assert!(
        !unused_export_names.contains(&"suppressedExport"),
        "suppressedExport should be suppressed via next-line comment, found: {unused_export_names:?}"
    );

    // unsuppressedExport has no suppression, should appear
    assert!(
        unused_export_names.contains(&"unsuppressedExport"),
        "unsuppressedExport should still be reported, found: {unused_export_names:?}"
    );
}

#[test]
fn file_level_suppression_hides_all_exports() {
    let root = fixture_path("suppression-comments");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_export_names: Vec<(&str, String)> = results
        .unused_exports
        .iter()
        .map(|e| {
            (
                e.export.export_name.as_str(),
                e.export
                    .path
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .to_string(),
            )
        })
        .collect();

    // Neither export from file-suppressed.ts should appear
    assert!(
        !unused_export_names
            .iter()
            .any(|(name, file)| *name == "ignoredA" && file == "file-suppressed.ts"),
        "ignoredA should be suppressed via file-level comment, found: {unused_export_names:?}"
    );
    assert!(
        !unused_export_names
            .iter()
            .any(|(name, file)| *name == "ignoredB" && file == "file-suppressed.ts"),
        "ignoredB should be suppressed via file-level comment, found: {unused_export_names:?}"
    );
}

#[test]
fn enum_member_suppression() {
    let root = fixture_path("suppression-comments");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_enum_member_names: Vec<&str> = results
        .unused_enum_members
        .iter()
        .map(|m| m.member.member_name.as_str())
        .collect();

    // Inactive has fallow-ignore-next-line, should NOT appear
    assert!(
        !unused_enum_member_names.contains(&"Inactive"),
        "Inactive should be suppressed via next-line comment, found: {unused_enum_member_names:?}"
    );

    // Pending has no suppression, should appear
    assert!(
        unused_enum_member_names.contains(&"Pending"),
        "Pending should still be reported as unused, found: {unused_enum_member_names:?}"
    );
}
