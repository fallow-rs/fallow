use super::common::{create_config, fixture_path};

// ── Dynamic import .then() callback patterns (issue #115) ──────

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

    // lib.ts is imported via .then() patterns
    assert!(
        !unused_file_names.contains(&"lib.ts".to_string()),
        "lib.ts should be reachable via .then() imports, unused files: {unused_file_names:?}"
    );

    // dashboard.component.ts is imported via Angular .then(m => m.DashboardComponent)
    assert!(
        !unused_file_names.contains(&"dashboard.component.ts".to_string()),
        "dashboard.component.ts should be reachable via .then() import, unused files: {unused_file_names:?}"
    );

    // settings.component.ts is imported via Angular .then(m => m.SettingsComponent)
    assert!(
        !unused_file_names.contains(&"settings.component.ts".to_string()),
        "settings.component.ts should be reachable via .then() import, unused files: {unused_file_names:?}"
    );

    // orphan.ts is not imported anywhere
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

    // foo is accessed via .then(m => m.foo) — should NOT be unused
    assert!(
        !unused_export_names
            .iter()
            .any(|(name, file)| *name == "foo" && file == "lib.ts"),
        "foo should be credited via .then(m => m.foo), unused exports: {unused_export_names:?}"
    );

    // bar is accessed via destructured .then(({ bar, baz }) => ...) — should NOT be unused
    assert!(
        !unused_export_names
            .iter()
            .any(|(name, file)| *name == "bar" && file == "lib.ts"),
        "bar should be credited via destructured .then() param, unused exports: {unused_export_names:?}"
    );

    // baz is accessed via destructured .then(({ bar, baz }) => ...) — should NOT be unused
    assert!(
        !unused_export_names
            .iter()
            .any(|(name, file)| *name == "baz" && file == "lib.ts"),
        "baz should be credited via destructured .then() param, unused exports: {unused_export_names:?}"
    );

    // DashboardComponent accessed via .then(m => m.DashboardComponent) — should NOT be unused
    assert!(
        !unused_export_names
            .iter()
            .any(|(name, file)| *name == "DashboardComponent" && file == "dashboard.component.ts"),
        "DashboardComponent should be credited via .then() member access, unused exports: {unused_export_names:?}"
    );

    // SettingsComponent accessed via .then(m => m.SettingsComponent) — should NOT be unused
    assert!(
        !unused_export_names
            .iter()
            .any(|(name, file)| *name == "SettingsComponent" && file == "settings.component.ts"),
        "SettingsComponent should be credited via .then() member access, unused exports: {unused_export_names:?}"
    );

    // unusedExport in lib.ts SHOULD be unused (no .then() accesses it)
    assert!(
        unused_export_names
            .iter()
            .any(|(name, file)| *name == "unusedExport" && file == "lib.ts"),
        "unusedExport should be unused (not accessed via .then()), unused exports: {unused_export_names:?}"
    );

    // UnusedComponent SHOULD be unused (only DashboardComponent is accessed)
    assert!(
        unused_export_names
            .iter()
            .any(|(name, file)| *name == "UnusedComponent" && file == "dashboard.component.ts"),
        "UnusedComponent should be unused (only DashboardComponent is accessed), unused exports: {unused_export_names:?}"
    );
}
