use super::common::{create_config, fixture_path};

#[test]
fn active_suppressions_capture_present_markers_all_kinds() {
    let root = fixture_path("suppression-comments");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let captured: Vec<(String, Option<String>, bool)> = results
        .active_suppressions
        .iter()
        .map(|s| {
            (
                s.path
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/"),
                s.kind.clone(),
                s.is_file_level,
            )
        })
        .collect();

    assert!(
        captured.contains(&(
            "exports.ts".to_owned(),
            Some("unused-export".to_owned()),
            false
        )),
        "expected the unused-export marker on exports.ts, got: {captured:?}"
    );
    assert!(
        captured.contains(&(
            "file-suppressed.ts".to_owned(),
            Some("unused-export".to_owned()),
            true
        )),
        "expected the file-level unused-export marker, got: {captured:?}"
    );
    assert!(
        captured.contains(&(
            "enums.ts".to_owned(),
            Some("unused-enum-member".to_owned()),
            false
        )),
        "expected the unused-enum-member marker on enums.ts, got: {captured:?}"
    );
}

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

    assert!(
        !unused_export_names.contains(&"suppressedExport"),
        "suppressedExport should be suppressed via next-line comment, found: {unused_export_names:?}"
    );

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
fn jsdoc_suppression_markers_hide_unused_exports() {
    let root = fixture_path("suppression-comments");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_export_names: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export.export_name.as_str())
        .collect();

    for suppressed in [
        "jsdocSingle",
        "jsdocMulti",
        "jsdocMultiProse",
        "jsdocFileA",
        "jsdocFileB",
    ] {
        assert!(
            !unused_export_names.contains(&suppressed),
            "{suppressed} should be suppressed via a JSDoc marker, found: {unused_export_names:?}"
        );
    }
    assert!(
        unused_export_names.contains(&"jsdocUnsuppressed"),
        "a JSDoc comment without a marker must not suppress, found: {unused_export_names:?}"
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

    assert!(
        !unused_enum_member_names.contains(&"Inactive"),
        "Inactive should be suppressed via next-line comment, found: {unused_enum_member_names:?}"
    );

    assert!(
        unused_enum_member_names.contains(&"Pending"),
        "Pending should still be reported as unused, found: {unused_enum_member_names:?}"
    );
}
