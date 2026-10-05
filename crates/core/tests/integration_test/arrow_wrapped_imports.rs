use super::common::{create_config, fixture_path};

#[test]
fn arrow_wrapped_lazy_imports_make_modules_reachable() {
    let root = fixture_path("arrow-wrapped-dynamic-imports");
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
        !unused_file_names.contains(&"Foo.tsx".to_string()),
        "Foo.tsx should be reachable via React.lazy arrow-wrapped import, unused files: {unused_file_names:?}"
    );

    assert!(
        !unused_file_names.contains(&"Bar.tsx".to_string()),
        "Bar.tsx should be reachable via lazy() arrow-wrapped import, unused files: {unused_file_names:?}"
    );

    assert!(
        unused_file_names.contains(&"orphan.ts".to_string()),
        "orphan.ts should be unused, found: {unused_file_names:?}"
    );
}

#[test]
fn arrow_wrapped_lazy_imports_credit_default_exports() {
    let root = fixture_path("arrow-wrapped-dynamic-imports");
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
            .any(|(name, file)| *name == "default" && file == "Foo.tsx"),
        "Foo.tsx default export should be credited via arrow-wrapped import, unused exports: {unused_export_names:?}"
    );

    assert!(
        !unused_export_names
            .iter()
            .any(|(name, file)| *name == "default" && file == "Bar.tsx"),
        "Bar.tsx default export should be credited via arrow-wrapped import, unused exports: {unused_export_names:?}"
    );

    assert!(
        !unused_export_names
            .iter()
            .any(|(name, file)| *name == "default" && file == "feature.routes.ts"),
        "feature.routes.ts default export should be credited via route callback import, unused exports: {unused_export_names:?}"
    );

    assert!(
        unused_export_names
            .iter()
            .any(|(name, file)| *name == "unusedNamedExport" && file == "Foo.tsx"),
        "unusedNamedExport should be unused (only default is credited via lazy import), unused exports: {unused_export_names:?}"
    );

    assert!(
        unused_export_names
            .iter()
            .any(|(name, file)| *name == "unusedRouteHelper" && file == "feature.routes.ts"),
        "unusedRouteHelper should remain unused; only default is credited via route callback import, unused exports: {unused_export_names:?}"
    );
}

#[test]
fn local_loader_functions_credit_dynamic_import_exports() {
    let root = fixture_path("dynamic-import-local-loader");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let mut unused: Vec<(String, String)> = results
        .unused_exports
        .iter()
        .map(|e| {
            (
                e.export
                    .path
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .to_string(),
                e.export.export_name.clone(),
            )
        })
        .collect();
    unused.sort();

    let expected: Vec<(String, String)> = [
        ("drawer.ts", "drawerTitle"),
        ("helpers.ts", "unusedHelper"),
        ("tools.ts", "unusedTool"),
        ("view.tsx", "unusedView"),
    ]
    .iter()
    .map(|(file, name)| ((*file).to_string(), (*name).to_string()))
    .collect();

    assert_eq!(
        unused, expected,
        "a loader passed as an argument credits `default`, a loader call bound with await \
         credits the accessed or destructured names, a loader with any other use \
         credits the whole module, and a loader without a reference credits nothing"
    );
}
