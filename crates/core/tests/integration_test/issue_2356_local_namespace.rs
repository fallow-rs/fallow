//! Issue #2356: a top-level namespace declared without the `export` keyword is
//! a local binding. Its inner `export` declarations are members of that local
//! namespace, never exports of the containing file, so they must not surface
//! as `unused-export` findings. Genuinely unused exports in the same file and
//! unused exported namespaces keep reporting, and imports referenced inside a
//! local namespace body keep their credit.

use super::common::{create_config, fixture_path};

#[test]
fn local_namespace_inner_exports_are_not_reported_unused() {
    let root = fixture_path("issue-2356-local-namespace");
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

    for local_member in ["inner", "viaSpecifier", "value"] {
        assert!(
            !unused_export_names.contains(&local_member)
                && !unused_type_names.contains(&local_member),
            "`{local_member}` is a member of a local namespace and must not be reported \
             as an unused export; exports: {unused_export_names:?}, types: {unused_type_names:?}"
        );
    }

    assert!(
        unused_export_names.contains(&"unusedSibling"),
        "a genuinely unused export next to a local namespace must keep reporting: \
         {unused_export_names:?}"
    );
    assert!(
        unused_export_names.contains(&"Exported"),
        "an unused exported namespace keeps reporting as before: {unused_export_names:?}"
    );

    // `helper` is only referenced inside the local `Wrapped` namespace body.
    assert!(
        !unused_export_names.contains(&"helper"),
        "an import referenced inside a local namespace body keeps its credit: \
         {unused_export_names:?}"
    );
    let unused_files: Vec<String> = results
        .unused_files
        .iter()
        .map(|f| f.file.path.to_string_lossy().replace('\\', "/"))
        .collect();
    assert!(
        !unused_files.iter().any(|p| p.ends_with("helper.ts")),
        "helper.ts is reachable through the local namespace body: {unused_files:?}"
    );
}
