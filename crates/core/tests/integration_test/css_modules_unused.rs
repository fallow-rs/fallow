use super::common::{create_config, fixture_path};

#[test]
fn css_module_unused_classes_detected() {
    let root = fixture_path("css-modules-unused");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_export_names: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export.export_name.as_str())
        .collect();

    // container and title are imported via named imports, should NOT be unused
    assert!(
        !unused_export_names.contains(&"container"),
        "container should NOT be unused (imported via named import)"
    );
    assert!(
        !unused_export_names.contains(&"title"),
        "title should NOT be unused (imported via named import)"
    );

    // subtitle and hidden are not imported, should be unused
    assert!(
        unused_export_names.contains(&"subtitle"),
        "subtitle should be unused (not imported), found: {unused_export_names:?}"
    );
    assert!(
        unused_export_names.contains(&"hidden"),
        "hidden should be unused (not imported), found: {unused_export_names:?}"
    );
}

#[test]
fn orphan_css_module_detected_as_unused_file() {
    let root = fixture_path("css-modules-unused");
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
        unused_file_names.contains(&"orphan.module.css".to_string()),
        "orphan.module.css should be unused (not imported), found: {unused_file_names:?}"
    );

    // App.module.css IS imported, should NOT be unused
    assert!(
        !unused_file_names.contains(&"App.module.css".to_string()),
        "App.module.css should NOT be unused (imported by index.ts)"
    );
}
