use super::common::{create_config, fixture_path};

#[test]
fn css_modules_exports_tracked() {
    let root = fixture_path("css-modules-project");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_file_names: Vec<String> = results
        .unused_files
        .iter()
        .filter_map(|f| f.file.path.file_name())
        .filter_map(|n| n.to_str())
        .map(ToString::to_string)
        .collect();
    assert!(
        unused_file_names.contains(&"unused.module.css".to_string()),
        "unused.module.css should be unused: {unused_file_names:?}"
    );
    assert!(
        !unused_file_names.contains(&"Layout.module.css".to_string()),
        "Layout.module.css should be used via named imports: {unused_file_names:?}"
    );
    assert!(
        !unused_file_names.contains(&"Button.module.css".to_string()),
        "Button.module.css should be used via default import: {unused_file_names:?}"
    );

    let unused_export_names: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export.export_name.as_str())
        .collect();
    assert!(
        unused_export_names.contains(&"sidebar"),
        "sidebar should be an unused export: {unused_export_names:?}"
    );
    assert!(
        unused_export_names.contains(&"secondary"),
        "secondary should be an unused export: {unused_export_names:?}"
    );
    assert!(
        !unused_export_names.contains(&"header"),
        "header should be used via named import: {unused_export_names:?}"
    );
    assert!(
        !unused_export_names.contains(&"footer"),
        "footer should be used via named import: {unused_export_names:?}"
    );
    assert!(
        !unused_export_names.contains(&"primary"),
        "primary should be used via member access: {unused_export_names:?}"
    );
    assert!(
        unused_file_names.contains(&"regular.css".to_string()),
        "regular.css should be unused: {unused_file_names:?}"
    );
}
