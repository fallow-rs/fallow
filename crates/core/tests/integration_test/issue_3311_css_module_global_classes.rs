use super::common::{create_config, fixture_path};

/// Issue #3311: CSS Modules do not export a class in a global scope, so such a
/// class must never be an unused export. Local classes keep their findings.
#[test]
fn css_module_global_classes_are_not_exports() {
    let root = fixture_path("issue-3311-css-module-global-classes");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let mut unused: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export.export_name.as_str())
        .collect();
    unused.sort_unstable();

    assert_eq!(
        unused,
        vec!["lessUnused", "scssUnused", "unusedLocal"],
        "only the unused local classes are findings"
    );
}
