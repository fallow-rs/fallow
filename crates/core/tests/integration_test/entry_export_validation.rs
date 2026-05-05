use super::common::{create_config, fixture_path};

#[test]
fn entry_exports_skipped_by_default() {
    let root = fixture_path("entry-export-validation");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_export_names: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export_name.as_str())
        .collect();

    // With default config, entry point exports are skipped
    assert!(
        !unused_export_names.contains(&"meatdata"),
        "meatdata should not be flagged (entry exports skipped by default), found: {unused_export_names:?}"
    );
    assert!(
        !unused_export_names.contains(&"config"),
        "config should not be flagged (entry exports skipped by default), found: {unused_export_names:?}"
    );
}

#[test]
fn entry_exports_detected_when_include_entry_exports_enabled() {
    let root = fixture_path("entry-export-validation");
    let mut config = create_config(root);
    config.include_entry_exports = true;
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_export_names: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export_name.as_str())
        .collect();

    // With include_entry_exports, unreferenced entry exports should be flagged
    assert!(
        unused_export_names.contains(&"meatdata"),
        "meatdata should be flagged with include_entry_exports, found: {unused_export_names:?}"
    );
    assert!(
        unused_export_names.contains(&"config"),
        "config should be flagged with include_entry_exports, found: {unused_export_names:?}"
    );

    // helper is imported by consumer.ts, so it should NOT be flagged
    assert!(
        !unused_export_names.contains(&"helper"),
        "helper should not be flagged (imported by consumer.ts), found: {unused_export_names:?}"
    );
}

#[test]
fn entry_exports_detected_via_config_file_include_entry_exports() {
    // Issue #249: fixture has `.fallowrc.json` with `includeEntryExports: true`.
    // Same expectations as the CLI-flag path: meatdata + config flagged, helper not.
    let root = fixture_path("entry-export-validation-config");
    let (loaded, _path) = fallow_config::FallowConfig::find_and_load(&root)
        .expect("config load")
        .expect("fixture has .fallowrc.json");
    assert!(loaded.include_entry_exports);
    let config = loaded.resolve(root, fallow_config::OutputFormat::Human, 1, true, true);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_export_names: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export_name.as_str())
        .collect();

    assert!(
        unused_export_names.contains(&"meatdata"),
        "meatdata should be flagged via config file, found: {unused_export_names:?}"
    );
    assert!(
        unused_export_names.contains(&"config"),
        "config should be flagged via config file, found: {unused_export_names:?}"
    );
    assert!(
        !unused_export_names.contains(&"helper"),
        "helper should not be flagged (imported by consumer.ts), found: {unused_export_names:?}"
    );
}

#[test]
fn vitest_config_default_export_is_framework_used_with_include_entry_exports() {
    let root = fixture_path("vitest-include-entry-exports-workspace");
    let mut config = create_config(root);
    config.include_entry_exports = true;
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_exports: Vec<(String, String)> = results
        .unused_exports
        .iter()
        .map(|export| {
            (
                export
                    .path
                    .strip_prefix(&config.root)
                    .unwrap_or(&export.path)
                    .to_string_lossy()
                    .replace('\\', "/"),
                export.export_name.clone(),
            )
        })
        .collect();

    assert!(
        !unused_exports.iter().any(|(path, export)| {
            path == "packages/web/charts/vitest.config.mts" && export == "default"
        }),
        "Vitest config default export should be framework-used, found: {unused_exports:?}"
    );
}

#[test]
fn vite_config_default_export_is_framework_used_with_include_entry_exports() {
    // Issue #282: vite.config.* default export is consumed by Vite itself; under
    // --include-entry-exports it must not be reported. Mirrors #271 for vitest.
    let root = fixture_path("vite-include-entry-exports-workspace");
    let mut config = create_config(root);
    config.include_entry_exports = true;
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_exports: Vec<(String, String)> = results
        .unused_exports
        .iter()
        .map(|export| {
            (
                export
                    .path
                    .strip_prefix(&config.root)
                    .unwrap_or(&export.path)
                    .to_string_lossy()
                    .replace('\\', "/"),
                export.export_name.clone(),
            )
        })
        .collect();

    assert!(
        !unused_exports.iter().any(|(path, export)| {
            path == "packages/web/charts/vite.config.mts" && export == "default"
        }),
        "Vite config default export should be framework-used, found: {unused_exports:?}"
    );
}

#[test]
fn storybook_exports_are_framework_used_with_include_entry_exports() {
    let root = fixture_path("storybook-include-entry-exports");
    let mut config = create_config(root);
    config.include_entry_exports = true;
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_exports: Vec<(String, String)> = results
        .unused_exports
        .iter()
        .map(|export| {
            (
                export
                    .path
                    .strip_prefix(&config.root)
                    .unwrap_or(&export.path)
                    .to_string_lossy()
                    .replace('\\', "/"),
                export.export_name.clone(),
            )
        })
        .collect();

    for (path, export) in [
        ("src/Button.stories.ts", "default"),
        ("src/Button.stories.ts", "Default"),
        ("src/Button.stories.ts", "Secondary"),
        (".storybook/preview.ts", "parameters"),
        (".storybook/preview.ts", "decorators"),
    ] {
        assert!(
            !unused_exports
                .iter()
                .any(|(unused_path, unused_export)| unused_path == path && unused_export == export),
            "{path}:{export} should be framework-used, found: {unused_exports:?}"
        );
    }
}
