use super::common::{create_config, fixture_path};

#[test]
fn composes_from_another_file_makes_that_file_reachable() {
    let root = fixture_path("css-modules-composes-from");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_files: Vec<String> = results
        .unused_files
        .iter()
        .filter_map(|f| f.file.path.file_name())
        .map(|name| name.to_string_lossy().to_string())
        .collect();
    assert!(
        !unused_files.contains(&"shared.module.css".to_string()),
        "shared.module.css is reachable through `composes ... from`, found: {unused_files:?}"
    );

    let unused_exports: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export.export_name.as_str())
        .collect();
    assert!(
        !unused_exports.contains(&"base") && !unused_exports.contains(&"rounded"),
        "classes named in `composes ... from` count as used, found: {unused_exports:?}"
    );
    assert!(
        unused_exports.contains(&"unusedShared"),
        "a class that nothing composes stays unused, found: {unused_exports:?}"
    );
}
