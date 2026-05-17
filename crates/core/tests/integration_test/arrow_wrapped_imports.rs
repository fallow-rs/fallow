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

    // Foo.tsx is imported via React.lazy(() => import('./Foo'))
    assert!(
        !unused_file_names.contains(&"Foo.tsx".to_string()),
        "Foo.tsx should be reachable via React.lazy arrow-wrapped import, unused files: {unused_file_names:?}"
    );

    // Bar.tsx is imported via lazy(() => import('./Bar'))
    assert!(
        !unused_file_names.contains(&"Bar.tsx".to_string()),
        "Bar.tsx should be reachable via lazy() arrow-wrapped import, unused files: {unused_file_names:?}"
    );

    // orphan.ts is not imported anywhere
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

    // Foo.tsx default export should NOT be unused (credited via arrow-wrapped import)
    assert!(
        !unused_export_names
            .iter()
            .any(|(name, file)| *name == "default" && file == "Foo.tsx"),
        "Foo.tsx default export should be credited via arrow-wrapped import, unused exports: {unused_export_names:?}"
    );

    // Bar.tsx default export should NOT be unused
    assert!(
        !unused_export_names
            .iter()
            .any(|(name, file)| *name == "default" && file == "Bar.tsx"),
        "Bar.tsx default export should be credited via arrow-wrapped import, unused exports: {unused_export_names:?}"
    );

    // Angular-style route callbacks also consume the module default implicitly.
    assert!(
        !unused_export_names
            .iter()
            .any(|(name, file)| *name == "default" && file == "feature.routes.ts"),
        "feature.routes.ts default export should be credited via route callback import, unused exports: {unused_export_names:?}"
    );

    // Foo.tsx unusedNamedExport SHOULD be in unused exports (only default is credited)
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
