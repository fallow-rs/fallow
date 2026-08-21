//! Issue #2349: an interface declared inside a `declare module './library'`
//! augmentation block augments the named module. It is not an export of the
//! declaring file and must not surface as `unused-type`, while genuinely
//! unused exports outside the augmentation body in the same file must keep
//! reporting.

use super::common::{create_config, fixture_path};

#[test]
fn module_augmentation_interface_is_not_reported_unused() {
    let root = fixture_path("issue-2349-module-augmentation");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_type_names: Vec<&str> = results
        .unused_types
        .iter()
        .map(|t| t.export.export_name.as_str())
        .collect();
    let unused_export_names: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export.export_name.as_str())
        .collect();

    assert!(
        !unused_type_names.contains(&"Theme") && !unused_export_names.contains(&"Theme"),
        "the augmentation-scoped `Theme` interface must not be reported as an \
         unused export of theme-augmentation.ts; \
         types: {unused_type_names:?}, exports: {unused_export_names:?}"
    );
    assert!(
        unused_type_names.contains(&"UnusedLocalAlias")
            || unused_export_names.contains(&"UnusedLocalAlias"),
        "a genuinely unused export outside the `declare module` body must keep \
         reporting; types: {unused_type_names:?}, exports: {unused_export_names:?}"
    );
}
