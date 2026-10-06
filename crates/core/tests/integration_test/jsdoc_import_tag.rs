//! Integration tests for JSDoc `@import` tags.
//!
//! The fixture (`tests/fixtures/jsdoc-import-tag/`) is a checked JavaScript
//! project. Its entry file loads types only through `@import` tags. Two of
//! the targets are module declaration files outside the jsconfig `include`,
//! so only the `@import` edge keeps them used.

use fallow_types::results::AnalysisResults;

use super::common::{create_config, fixture_path};

fn analyze_fixture() -> AnalysisResults {
    let root = fixture_path("jsdoc-import-tag");
    let config = create_config(root);
    fallow_core::analyze(&config).expect("analysis should succeed")
}

fn unused_file_paths(results: &AnalysisResults) -> Vec<String> {
    results
        .unused_files
        .iter()
        .map(|file| file.file.path.to_string_lossy().replace('\\', "/"))
        .collect()
}

fn unused_type_names(results: &AnalysisResults) -> Vec<&str> {
    results
        .unused_types
        .iter()
        .map(|export| export.export.export_name.as_str())
        .collect()
}

#[test]
fn jsdoc_import_tag_keeps_target_modules_used() {
    let results = analyze_fixture();
    let unused = unused_file_paths(&results);

    for target in [
        "types/config.d.ts",
        "types/shapes.d.ts",
        "src/model.ts",
        "src/settings.ts",
        "src/units.ts",
    ] {
        assert!(
            !unused.iter().any(|path| path.ends_with(target)),
            "{target} should be reachable through a JSDoc @import tag, unused: {unused:?}"
        );
    }
}

#[test]
fn jsdoc_import_tag_credits_the_named_types() {
    let results = analyze_fixture();
    let unused = unused_type_names(&results);

    for name in ["Config", "Circle", "Meter", "Item", "Store", "default"] {
        assert!(
            !unused.contains(&name),
            "{name} should be credited through a JSDoc @import tag, unused types: {unused:?}"
        );
    }
}

#[test]
fn jsdoc_import_tag_does_not_credit_other_types() {
    let results = analyze_fixture();
    let unused = unused_type_names(&results);

    for name in ["UnusedConfig", "Orphan", "Square"] {
        assert!(
            unused.contains(&name),
            "{name} is not referenced through a JSDoc @import tag and should stay unused, unused types: {unused:?}"
        );
    }
}

#[test]
fn jsdoc_namespace_import_tag_credits_only_the_members_in_use() {
    let results = analyze_fixture();
    let unused_values: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|export| export.export.export_name.as_str())
        .collect();

    assert!(
        unused_values.contains(&"unusedScale"),
        "unusedScale is not read through the namespace binding and should stay unused, unused exports: {unused_values:?}"
    );
}
