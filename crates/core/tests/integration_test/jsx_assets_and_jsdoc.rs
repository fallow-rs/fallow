//! Integration tests for issue #640 and the JSDoc `import()` type extraction
//! that landed with issue #105.
//!
//! The fixture (`tests/fixtures/jsx-assets-and-jsdoc/`) models a TSX layout
//! that emits HTML metadata via JSX. Generic JSX resource attributes should not
//! become module graph edges or unresolved imports. The same fixture keeps the
//! JSDoc type-reference coverage on a normal JavaScript side-effect import so
//! it no longer depends on JSX asset reachability.

use fallow_types::results::AnalysisResults;

use super::common::{create_config, fixture_path};

fn analyze_fixture() -> AnalysisResults {
    let root = fixture_path("jsx-assets-and-jsdoc");
    let config = create_config(root);
    fallow_core::analyze(&config).expect("analysis should succeed")
}

#[test]
fn jsx_resource_attributes_do_not_emit_unresolved_imports() {
    let results = analyze_fixture();
    let unresolved_specifiers: Vec<&str> = results
        .unresolved_imports
        .iter()
        .map(|issue| issue.import.specifier.as_str())
        .collect();

    for specifier in ["/static/style.css", "/static/vendor.js", "/static/app.js"] {
        assert!(
            !unresolved_specifiers.contains(&specifier),
            "{specifier} should be treated as a JSX runtime attribute, unresolved: {unresolved_specifiers:?}"
        );
    }
}

#[test]
fn jsdoc_import_type_makes_referenced_types_module_reachable() {
    let results = analyze_fixture();
    let unused_file_names: Vec<String> = results
        .unused_files
        .iter()
        .map(|file| {
            file.file
                .path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .to_string()
        })
        .collect();

    assert!(
        !unused_file_names.contains(&"types.ts".to_string()),
        "src/lib/types.ts should be reachable via JSDoc import() in src/jsdoc-consumer.js, unused: {unused_file_names:?}"
    );
}

#[test]
fn jsdoc_referenced_type_not_flagged_unused() {
    let results = analyze_fixture();

    let unused_type_names: Vec<&str> = results
        .unused_types
        .iter()
        .map(|export| export.export.export_name.as_str())
        .collect();

    assert!(
        !unused_type_names.contains(&"Config"),
        "Config type should be credited as used via JSDoc import(), unused types: {unused_type_names:?}"
    );
}

#[test]
fn jsdoc_scanner_does_not_credit_unrelated_types() {
    let results = analyze_fixture();

    let unused_type_names: Vec<&str> = results
        .unused_types
        .iter()
        .map(|export| export.export.export_name.as_str())
        .collect();

    assert!(
        unused_type_names.contains(&"Unused"),
        "Unused type should still be flagged because the JSDoc scanner credits only the named member, unused types: {unused_type_names:?}"
    );
}
