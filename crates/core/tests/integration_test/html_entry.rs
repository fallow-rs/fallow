use super::common::{create_config, fixture_path};

#[test]
fn html_entry_makes_referenced_script_reachable() {
    let root = fixture_path("html-entry");
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
        !unused_file_names.contains(&"entry.ts".to_string()),
        "entry.ts should be reachable via HTML <script src>, unused files: {unused_file_names:?}"
    );

    assert!(
        !unused_file_names.contains(&"helper.ts".to_string()),
        "helper.ts should be transitively reachable via HTML entry, unused files: {unused_file_names:?}"
    );
}

#[test]
fn html_entry_makes_referenced_stylesheet_reachable() {
    let root = fixture_path("html-entry");
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
        !unused_file_names.contains(&"global.css".to_string()),
        "global.css should be reachable via HTML <link href>, unused files: {unused_file_names:?}"
    );
}

#[test]
fn html_entry_does_not_suppress_unused_exports() {
    let root = fixture_path("html-entry");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_export_names: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export.export_name.as_str())
        .collect();
    assert!(
        unused_export_names.contains(&"unused"),
        "unused export should still be detected, got: {unused_export_names:?}"
    );
}

#[test]
fn html_files_not_reported_as_unused() {
    let root = fixture_path("html-entry");
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
        !unused_file_names.iter().any(|f| std::path::Path::new(f)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("html"))),
        "HTML files should be excluded from unused-file detection, got: {unused_file_names:?}"
    );
}

#[test]
fn html_entry_no_unresolved_imports() {
    let root = fixture_path("html-entry");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let html_unresolved: Vec<&str> = results
        .unresolved_imports
        .iter()
        .filter(|u| u.import.path.to_string_lossy().ends_with(".html"))
        .map(|u| u.import.specifier.as_str())
        .collect();
    assert!(
        html_unresolved.is_empty(),
        "HTML asset references should resolve, got unresolved: {html_unresolved:?}"
    );
}

#[test]
fn html_root_relative_script_is_reachable() {
    let root = fixture_path("html-root-relative");
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
        !unused_file_names.contains(&"entry.ts".to_string()),
        "entry.ts should be reachable via root-relative HTML script src, unused files: {unused_file_names:?}"
    );

    assert!(
        !unused_file_names.contains(&"helper.ts".to_string()),
        "helper.ts should be transitively reachable, unused files: {unused_file_names:?}"
    );
}

#[test]
fn html_root_relative_stylesheet_is_reachable() {
    let root = fixture_path("html-root-relative");
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
        !unused_file_names.contains(&"global.css".to_string()),
        "global.css should be reachable via root-relative HTML link href, unused files: {unused_file_names:?}"
    );
}

#[test]
fn html_root_relative_no_unresolved_imports() {
    let root = fixture_path("html-root-relative");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let html_unresolved: Vec<&str> = results
        .unresolved_imports
        .iter()
        .filter(|u| u.import.path.to_string_lossy().ends_with(".html"))
        .map(|u| u.import.specifier.as_str())
        .collect();
    assert!(
        html_unresolved.is_empty(),
        "root-relative HTML asset references should resolve, got unresolved: {html_unresolved:?}"
    );
}

#[test]
fn html_workspace_root_relative_script_is_reachable() {
    let root = fixture_path("html-workspace-root-relative");
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
        !unused_file_names.contains(&"main.ts".to_string()),
        "main.ts should be reachable via workspace root-relative HTML script src, unused files: {unused_file_names:?}"
    );

    assert!(
        !unused_file_names.contains(&"utils.ts".to_string()),
        "utils.ts should be transitively reachable, unused files: {unused_file_names:?}"
    );
}

#[test]
fn html_workspace_root_relative_stylesheet_is_reachable() {
    let root = fixture_path("html-workspace-root-relative");
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
        !unused_file_names.contains(&"global.css".to_string()),
        "global.css should be reachable via workspace root-relative HTML link href, unused files: {unused_file_names:?}"
    );
}

#[test]
fn html_workspace_root_relative_no_unresolved_imports() {
    let root = fixture_path("html-workspace-root-relative");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let html_unresolved: Vec<&str> = results
        .unresolved_imports
        .iter()
        .filter(|u| u.import.path.to_string_lossy().ends_with(".html"))
        .map(|u| u.import.specifier.as_str())
        .collect();
    assert!(
        html_unresolved.is_empty(),
        "workspace root-relative HTML asset references should resolve, got unresolved: {html_unresolved:?}"
    );
}

#[test]
fn html_public_root_relative_assets_are_reachable() {
    let root = fixture_path("issue-915-public-root-html-assets");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_paths: Vec<String> = results
        .unused_files
        .iter()
        .map(|finding| finding.file.path.to_string_lossy().replace('\\', "/"))
        .collect();

    for expected in [
        "public/js/key.pressed.js",
        "public/style/animations.css",
        "public/style/index.css",
        "public/style/screens.css",
    ] {
        assert!(
            !unused_paths.iter().any(|path| path.ends_with(expected)),
            "{expected} should be reachable via root-relative HTML asset reference, unused files: {unused_paths:?}"
        );
    }

    let html_unresolved: Vec<&str> = results
        .unresolved_imports
        .iter()
        .filter(|finding| {
            finding
                .import
                .path
                .to_string_lossy()
                .replace('\\', "/")
                .ends_with("index.html")
        })
        .map(|finding| finding.import.specifier.as_str())
        .collect();

    for resolved in [
        "/js/key.pressed.js",
        "/style/animations.css",
        "/style/index.css",
        "/style/screens.css",
    ] {
        assert!(
            !html_unresolved.contains(&resolved),
            "{resolved} should resolve from public, got unresolved: {html_unresolved:?}"
        );
    }

    assert!(
        html_unresolved.contains(&"/missing.js"),
        "missing public assets should still report unresolved, got: {html_unresolved:?}"
    );
}
