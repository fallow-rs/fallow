use super::common::{create_config, fixture_path};

#[test]
fn workspace_cross_import_resolves() {
    let root = fixture_path("workspace-cross-imports");

    // Set up node_modules symlinks for cross-workspace resolution
    let nm = root.join("node_modules").join("@myorg");
    let _ = std::fs::create_dir_all(&nm);
    #[cfg(unix)]
    {
        let _ = std::os::unix::fs::symlink(root.join("packages/core"), nm.join("core"));
    }
    #[cfg(windows)]
    {
        let _ = std::os::windows::fs::symlink_dir(root.join("packages/core"), nm.join("core"));
    }

    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    // No unresolved imports — cross-workspace @myorg/core should resolve
    assert!(
        results.unresolved_imports.is_empty(),
        "cross-workspace imports should resolve, found unresolved: {:?}",
        results
            .unresolved_imports
            .iter()
            .map(|i| &i.import.specifier)
            .collect::<Vec<_>>()
    );
}

#[test]
fn workspace_cross_import_detects_orphan() {
    let root = fixture_path("workspace-cross-imports");

    // Set up node_modules symlinks
    let nm = root.join("node_modules").join("@myorg");
    let _ = std::fs::create_dir_all(&nm);
    #[cfg(unix)]
    {
        let _ = std::os::unix::fs::symlink(root.join("packages/core"), nm.join("core"));
    }
    #[cfg(windows)]
    {
        let _ = std::os::windows::fs::symlink_dir(root.join("packages/core"), nm.join("core"));
    }

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
        unused_file_names.contains(&"orphan.ts".to_string()),
        "orphan.ts should be unused, found: {unused_file_names:?}"
    );
}

#[test]
fn workspace_cross_import_detects_unused_export() {
    let root = fixture_path("workspace-cross-imports");

    // Set up node_modules symlinks
    let nm = root.join("node_modules").join("@myorg");
    let _ = std::fs::create_dir_all(&nm);
    #[cfg(unix)]
    {
        let _ = std::os::unix::fs::symlink(root.join("packages/core"), nm.join("core"));
    }
    #[cfg(windows)]
    {
        let _ = std::os::windows::fs::symlink_dir(root.join("packages/core"), nm.join("core"));
    }

    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_export_names: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export.export_name.as_str())
        .collect();

    // unusedCoreExport is not imported by the web package
    assert!(
        unused_export_names.contains(&"unusedCoreExport"),
        "unusedCoreExport should be unused, found: {unused_export_names:?}"
    );

    // coreHelper IS imported by web, should NOT be flagged
    assert!(
        !unused_export_names.contains(&"coreHelper"),
        "coreHelper should NOT be unused (imported by web), found: {unused_export_names:?}"
    );
}

/// Regression test for issue #106.
///
/// A workspace library (`@repro/ui-kit`) declares secondary entry points in
/// `package.json` `exports` that map to compiled output under `dist/`.
/// Consumers inside the same monorepo (both self-references from the library's
/// own index barrel and cross-workspace imports from a sibling app) use bare
/// `@repro/ui-kit/<subpath>` specifiers. No `node_modules` symlinks exist.
///
/// Before the fix, every secondary entry point file was reported as unused
/// because the bare specifier fell through to `NpmPackage` classification
/// without creating a graph edge.
///
/// After the fix, the workspace package fallback resolves the subpath against
/// the library's source tree, so the re-export chain from `ui-kit/index.ts`
/// reaches every `button`/`modal`/`tabs`/`internal/base` source file and the
/// direct imports from `my-app` are wired up through the source tree as well.
#[test]
fn workspace_self_reference_resolves_secondary_entry_points() {
    let root = fixture_path("workspace-self-reference");

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

    // None of the ui-kit secondary entry point files should be reported.
    // Before the fix, button/modal/tabs/internal-base were all flagged.
    for secondary in ["button", "modal", "tabs", "internal/base"] {
        assert!(
            !results.unused_files.iter().any(|f| {
                f.file
                    .path
                    .to_string_lossy()
                    .replace('\\', "/")
                    .contains(&format!("ui-kit/{secondary}/index.ts"))
            }),
            "ui-kit/{secondary}/index.ts should not be flagged as unused, found: {unused_file_names:?}"
        );
    }

    // Cross-workspace self-referencing imports should not surface as unresolved
    // nor as unlisted dependencies.
    let unresolved_specifiers: Vec<&str> = results
        .unresolved_imports
        .iter()
        .map(|i| i.import.specifier.as_str())
        .collect();
    assert!(
        !unresolved_specifiers
            .iter()
            .any(|s| s.starts_with("@repro/ui-kit")),
        "self-referencing @repro/ui-kit subpaths should resolve, found unresolved: {unresolved_specifiers:?}"
    );

    let unlisted_package_names: Vec<&str> = results
        .unlisted_dependencies
        .iter()
        .map(|d| d.dep.package_name.as_str())
        .collect();
    assert!(
        !unlisted_package_names.contains(&"@repro/ui-kit"),
        "@repro/ui-kit should not be reported as unlisted, found: {unlisted_package_names:?}"
    );
}
