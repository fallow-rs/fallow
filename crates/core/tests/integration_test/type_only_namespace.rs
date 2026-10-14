//! A non-ambient namespace that holds only types emits no JavaScript. When it
//! is unused, it reports as an unused type, not as an unused export. A
//! namespace with a value member stays an unused export, and a function that
//! merges with a type-only namespace stays a used value.

use super::common::{create_config, fixture_path};

#[test]
fn unused_type_only_namespace_reports_as_unused_type() {
    let root = fixture_path("type-only-namespace");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_export_names: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export.export_name.as_str())
        .collect();
    let unused_type_names: Vec<&str> = results
        .unused_types
        .iter()
        .map(|t| t.export.export_name.as_str())
        .collect();

    assert!(
        unused_type_names.contains(&"UnusedTypes"),
        "a namespace with only types is an unused type: {unused_type_names:?}"
    );
    assert!(
        !unused_export_names.contains(&"UnusedTypes"),
        "a namespace with only types is not an unused value export: {unused_export_names:?}"
    );
    assert!(
        unused_export_names.contains(&"UnusedRuntime"),
        "a namespace with a value member stays an unused export: {unused_export_names:?}"
    );
    for used in ["JSX", "merged"] {
        assert!(
            !unused_export_names.contains(&used) && !unused_type_names.contains(&used),
            "`{used}` is referenced and must not be reported: \
             exports {unused_export_names:?}, types {unused_type_names:?}"
        );
    }
}
