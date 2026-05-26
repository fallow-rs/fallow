use super::common::{create_config, fixture_path};

fn rel(path: &std::path::Path, root: &std::path::Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

#[test]
fn storybook_static_dirs_and_manager_runtime_imports_are_framework_provided() {
    let root = fixture_path("issue-546-storybook-runtime-resources");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unresolved: Vec<_> = results
        .unresolved_imports
        .iter()
        .map(|finding| {
            (
                rel(&finding.import.path, &config.root),
                finding.import.specifier.as_str(),
            )
        })
        .collect();
    assert!(
        !unresolved
            .iter()
            .any(|(path, _)| path.contains(".storybook/preview")),
        "Storybook preview HTML assets should resolve through staticDirs, found {unresolved:?}"
    );

    let unused_files: Vec<_> = results
        .unused_files
        .iter()
        .map(|finding| rel(&finding.file.path, &config.root))
        .collect();
    assert!(
        !unused_files.contains(&"packages/ui/src/lib/tokens/tokens.css".to_string()),
        "bare preview-head href should keep staticDirs token CSS alive, found {unused_files:?}"
    );
    assert!(
        !unused_files.contains(&"packages/ui/src/lib/tokens/body.css".to_string()),
        "preview-body href should keep staticDirs token CSS alive, found {unused_files:?}"
    );
    assert!(
        !unused_files.contains(&"packages/ui/src/lib/icons/style.css".to_string()),
        "mounted absolute preview href should keep staticDirs icon CSS alive, found {unused_files:?}"
    );
    assert!(
        unused_files.contains(&"icons/style.css".to_string()),
        "Storybook staticDirs mapping should win over the project-root absolute URL fallback"
    );

    let unlisted: Vec<_> = results
        .unlisted_dependencies
        .iter()
        .map(|dep| {
            (
                dep.dep.package_name.as_str(),
                dep.dep
                    .imported_from
                    .iter()
                    .map(|site| rel(&site.path, &config.root))
                    .collect::<Vec<_>>(),
            )
        })
        .collect();
    assert!(
        !unlisted.iter().any(|(_, sites)| sites
            .iter()
            .any(|path| path.ends_with(".storybook/manager.tsx"))),
        "Storybook manager runtime imports should be file-scoped framework deps, found {unlisted:?}"
    );
    assert!(
        !unlisted.iter().any(|(name, sites)| {
            *name == "vendor-icons"
                && sites
                    .iter()
                    .any(|path| path.ends_with(".storybook/preview-head.html"))
        }),
        "Storybook staticDirs assets from node_modules should not become unlisted deps, found {unlisted:?}"
    );
    assert!(
        unlisted.iter().any(|(name, sites)| {
            *name == "react" && sites == &vec!["packages/ui/src/app.tsx".to_string()]
        }),
        "ordinary source imports must still report unlisted manager-runtime packages, found {unlisted:?}"
    );
}
