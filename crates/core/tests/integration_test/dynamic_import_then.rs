use super::common::{create_config, fixture_path};

#[test]
fn then_callback_makes_modules_reachable() {
    let root = fixture_path("dynamic-import-then");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_file_names: Vec<String> = results
        .unused_files
        .iter()
        .map(|f| {
            f.file
                .path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .to_string()
        })
        .collect();

    assert!(
        !unused_file_names.contains(&"lib.ts".to_string()),
        "lib.ts should be reachable via .then() imports, unused files: {unused_file_names:?}"
    );

    assert!(
        !unused_file_names.contains(&"dashboard.component.ts".to_string()),
        "dashboard.component.ts should be reachable via .then() import, unused files: {unused_file_names:?}"
    );

    assert!(
        !unused_file_names.contains(&"settings.component.ts".to_string()),
        "settings.component.ts should be reachable via .then() import, unused files: {unused_file_names:?}"
    );

    assert!(
        unused_file_names.contains(&"orphan.ts".to_string()),
        "orphan.ts should be unused, found: {unused_file_names:?}"
    );
}

#[test]
fn then_callback_credits_accessed_exports() {
    let root = fixture_path("dynamic-import-then");
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
            .any(|(name, file)| *name == "foo" && file == "lib.ts"),
        "foo should be credited via .then(m => m.foo), unused exports: {unused_export_names:?}"
    );

    assert!(
        !unused_export_names
            .iter()
            .any(|(name, file)| *name == "bar" && file == "lib.ts"),
        "bar should be credited via destructured .then() param, unused exports: {unused_export_names:?}"
    );

    assert!(
        !unused_export_names
            .iter()
            .any(|(name, file)| *name == "baz" && file == "lib.ts"),
        "baz should be credited via destructured .then() param, unused exports: {unused_export_names:?}"
    );

    assert!(
        !unused_export_names
            .iter()
            .any(|(name, file)| *name == "DashboardComponent" && file == "dashboard.component.ts"),
        "DashboardComponent should be credited via .then() member access, unused exports: {unused_export_names:?}"
    );

    assert!(
        !unused_export_names
            .iter()
            .any(|(name, file)| *name == "SettingsComponent" && file == "settings.component.ts"),
        "SettingsComponent should be credited via .then() member access, unused exports: {unused_export_names:?}"
    );

    assert!(
        unused_export_names
            .iter()
            .any(|(name, file)| *name == "unusedExport" && file == "lib.ts"),
        "unusedExport should be unused (not accessed via .then()), unused exports: {unused_export_names:?}"
    );

    assert!(
        unused_export_names
            .iter()
            .any(|(name, file)| *name == "UnusedComponent" && file == "dashboard.component.ts"),
        "UnusedComponent should be unused (only DashboardComponent is accessed), unused exports: {unused_export_names:?}"
    );

    assert!(
        !unused_export_names
            .iter()
            .any(|(name, file)| *name == "startInspector" && file == "inspector.ts"),
        "startInspector should be credited via .then((m) => m.startInspector()), unused exports: {unused_export_names:?}"
    );

    assert!(
        unused_export_names
            .iter()
            .any(|(name, file)| *name == "unusedInspectorExport" && file == "inspector.ts"),
        "unusedInspectorExport should be unused (the callback reads only startInspector), unused exports: {unused_export_names:?}"
    );

    assert!(
        !unused_export_names
            .iter()
            .any(|(_, file)| file == "escaped.ts"),
        "escaped.ts exports should be credited when the callback param escapes, unused exports: {unused_export_names:?}"
    );

    let unused_type_names: Vec<&str> = results
        .unused_types
        .iter()
        .map(|t| t.export.export_name.as_str())
        .collect();
    assert!(
        !unused_type_names.contains(&"InspectorOptions"),
        "InspectorOptions is used by startInspector, unused types: {unused_type_names:?}"
    );
}
