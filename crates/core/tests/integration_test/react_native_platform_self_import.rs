//! A React Native platform variant that imports from its own base specifier.
//!
//! `StatusIcon.ios.tsx` re-exports a type with `export type { ... } from
//! "./StatusIcon"`, and `Toolbar.android.tsx` imports a type from
//! `"./Toolbar"`. Metro platform extensions would resolve both specifiers to
//! the importing file. TypeScript resolves them to the base files, so the
//! analysis must not report a self-loop or an unused re-exported type.

use super::common::{create_config, fixture_path};

#[test]
fn platform_variant_import_of_its_base_is_not_a_self_loop() {
    let root = fixture_path("react-native-platform-self-import");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let cycles: Vec<_> = results
        .re_export_cycles
        .iter()
        .map(|finding| &finding.cycle.files)
        .collect();
    assert!(
        cycles.is_empty(),
        "a platform variant must not re-export from itself: {cycles:?}"
    );

    let circular: Vec<_> = results
        .circular_dependencies
        .iter()
        .map(|finding| &finding.cycle.files)
        .collect();
    assert!(
        circular.is_empty(),
        "a platform variant and its base file do not form a cycle: {circular:?}"
    );

    let unused_types: Vec<&str> = results
        .unused_types
        .iter()
        .map(|finding| finding.export.export_name.as_str())
        .collect();
    assert!(
        unused_types.is_empty(),
        "the re-exported type reaches its declaration in the base file: {unused_types:?}"
    );

    let unused_files: Vec<_> = results
        .unused_files
        .iter()
        .map(|finding| &finding.file.path)
        .collect();
    assert!(
        unused_files.is_empty(),
        "every family member stays reachable: {unused_files:?}"
    );
}
