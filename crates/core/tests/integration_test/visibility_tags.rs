use fallow_config::Severity;

use super::common::{create_config, create_config_with_rules, fixture_path};

#[test]
fn public_tag_prevents_unused_export_detection() {
    let root = fixture_path("visibility-tags");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_export_names: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export.export_name.as_str())
        .collect();

    assert!(!unused_export_names.contains(&"publicExport"));
    assert!(!unused_export_names.contains(&"internalExport"));
    assert!(!unused_export_names.contains(&"betaExport"));
    assert!(!unused_export_names.contains(&"alphaExport"));
}

#[test]
fn untagged_unused_export_still_detected() {
    let root = fixture_path("visibility-tags");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_export_names: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export.export_name.as_str())
        .collect();

    assert!(unused_export_names.contains(&"trulyUnused"));
    assert!(!unused_export_names.contains(&"usedExport"));
}

/// `/** @public */ export { a } from './x'` marks the re-export as public
/// API, so neither the re-export nor its source export is unused. The same
/// holds for an imported binding that a tagged local `export { f }` forwards.
/// A tag never reaches the next statement, also in a file without semicolons.
#[test]
fn visibility_tag_on_re_export_marks_re_export_and_source_used() {
    let root = fixture_path("visibility-tags-re-export");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let mut unused: Vec<(String, String, bool)> = results
        .unused_exports
        .iter()
        .map(|e| {
            let file = e
                .export
                .path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            (file, e.export.export_name.clone(), e.export.is_re_export)
        })
        .collect();
    unused.sort();

    assert_eq!(
        unused,
        vec![
            ("control.mjs".to_string(), "h".to_string(), true),
            ("impl.mjs".to_string(), "h".to_string(), false),
        ],
        "only the untagged re-export `h` and its source export are unused"
    );
}

/// `@expected-unused -- reason` on a re-export keeps its reason, so
/// `requireSuppressionReason` reports only the tag without a reason. The tag
/// suppresses the unused re-export, but it does not mark the source export
/// as used.
#[test]
fn expected_unused_tag_on_re_export_keeps_reason() {
    let root = fixture_path("visibility-tags-re-export-reason");
    let config = create_config_with_rules(root, |rules| {
        rules.require_suppression_reason = Severity::Error;
    });
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let mut missing_reason: Vec<(String, u32)> = results
        .stale_suppressions
        .iter()
        .filter(|s| s.missing_reason)
        .map(|s| {
            let file = s
                .path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            (file, s.line)
        })
        .collect();
    missing_reason.sort();
    assert_eq!(
        missing_reason,
        vec![("api.mjs".to_string(), 4)],
        "only the re-export tag without a reason reports a missing reason"
    );

    let mut unused: Vec<(String, String, bool)> = results
        .unused_exports
        .iter()
        .map(|e| {
            let file = e
                .export
                .path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            (file, e.export.export_name.clone(), e.export.is_re_export)
        })
        .collect();
    unused.sort();
    assert_eq!(
        unused,
        vec![
            ("impl.mjs".to_string(), "k".to_string(), false),
            ("impl.mjs".to_string(), "m".to_string(), false),
        ],
        "the tag suppresses the re-exports but does not credit the source exports"
    );
}
