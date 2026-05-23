use super::common::{create_config, fixture_path};

/// Create a symlink, removing any existing entry (file, directory, or stale symlink) first.
/// This makes symlink setup idempotent across repeated test runs.
fn force_symlink(target: &std::path::Path, link: &std::path::Path) {
    // Remove existing entry at the link path (regular dir, file, or broken symlink)
    if link.symlink_metadata().is_ok() {
        if link.is_dir() && !link.is_symlink() {
            let _ = std::fs::remove_dir_all(link);
        } else {
            let _ = std::fs::remove_file(link);
        }
    }
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, link).expect("symlink creation should succeed");
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(target, link).expect("symlink creation should succeed");
}

#[test]
fn workspace_patterns_from_package_json() {
    let pkg: fallow_config::PackageJson =
        serde_json::from_str(r#"{"workspaces": ["packages/*", "apps/*"]}"#).unwrap();

    let patterns = pkg.workspace_patterns();
    assert_eq!(patterns, vec!["packages/*", "apps/*"]);
}

#[test]
fn workspace_patterns_yarn_format() {
    let pkg: fallow_config::PackageJson =
        serde_json::from_str(r#"{"workspaces": {"packages": ["packages/*"]}}"#).unwrap();

    let patterns = pkg.workspace_patterns();
    assert_eq!(patterns, vec!["packages/*"]);
}

// ── Workspace integration ──────────────────────────────────────

#[test]
fn workspace_project_discovers_workspace_packages() {
    let root = fixture_path("workspace-project");

    // Set up node_modules symlinks for cross-workspace resolution (like npm/pnpm install would).
    // Uses force_symlink to handle stale directories from prior runs.
    let nm = root.join("node_modules");
    let _ = std::fs::create_dir_all(nm.join("@workspace"));
    force_symlink(&root.join("packages/shared"), &nm.join("shared"));
    force_symlink(&root.join("packages/utils"), &nm.join("@workspace/utils"));

    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    // Workspace discovery should find files across workspace packages
    // orphan.ts should always be detected as unused since nothing imports it
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
        "orphan.ts should be detected as unused file, found: {unused_file_names:?}"
    );

    // Cross-workspace resolution via node_modules symlinks:
    // app imports `@workspace/utils/src/deep` which resolves through the symlink,
    // making deep.ts reachable. If symlinks are broken, deep.ts would be unreachable.
    assert!(
        !unused_file_names.contains(&"deep.ts".to_string()),
        "deep.ts should NOT be unused (reachable via cross-workspace import through symlink), \
         but found in unused files: {unused_file_names:?}"
    );

    // `unusedDeep` should be detected as unused export (deep.ts is reachable but
    // only `deepHelper` is imported, not `unusedDeep`)
    let unused_export_names: Vec<String> = results
        .unused_exports
        .iter()
        .map(|e| e.export.export_name.clone())
        .collect();
    assert!(
        unused_export_names.contains(&"unusedDeep".to_string()),
        "unusedDeep should be detected as unused export, found: {unused_export_names:?}"
    );

    // No unresolved imports — all cross-workspace imports should resolve
    assert!(
        results.unresolved_imports.is_empty(),
        "should have no unresolved imports, found: {:?}",
        results
            .unresolved_imports
            .iter()
            .map(|i| &i.import.specifier)
            .collect::<Vec<_>>()
    );

    // The analysis should have found issues across all workspace packages
    assert!(
        results.has_issues(),
        "workspace project should have issues detected"
    );
}

#[test]
fn public_packages_suppress_exported_class_and_enum_members() {
    let root = fixture_path("public-package-members");

    let mut config = create_config(root);
    config.public_packages = vec!["@workspace/public-lib".to_string()];
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_class_members: Vec<String> = results
        .unused_class_members
        .iter()
        .map(|m| format!("{}.{}", m.member.parent_name, m.member.member_name))
        .collect();
    assert!(
        !unused_class_members.contains(&"WorkspaceService.externalApiMethod".to_string()),
        "public package class members are public API and should not be flagged: {unused_class_members:?}"
    );

    let unused_enum_members: Vec<String> = results
        .unused_enum_members
        .iter()
        .map(|m| format!("{}.{}", m.member.parent_name, m.member.member_name))
        .collect();
    assert!(
        !unused_enum_members.contains(&"PublicStatus.External".to_string()),
        "public package enum members are public API and should not be flagged: {unused_enum_members:?}"
    );
}

#[test]
fn non_public_packages_still_report_unused_class_and_enum_members() {
    let root = fixture_path("public-package-members");

    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_class_members: Vec<String> = results
        .unused_class_members
        .iter()
        .map(|m| format!("{}.{}", m.member.parent_name, m.member.member_name))
        .collect();
    assert!(
        unused_class_members.contains(&"WorkspaceService.externalApiMethod".to_string()),
        "non-public packages should still report unused class members: {unused_class_members:?}"
    );

    let unused_enum_members: Vec<String> = results
        .unused_enum_members
        .iter()
        .map(|m| format!("{}.{}", m.member.parent_name, m.member.member_name))
        .collect();
    assert!(
        unused_enum_members.contains(&"PublicStatus.External".to_string()),
        "non-public packages should still report unused enum members: {unused_enum_members:?}"
    );
}

#[test]
fn project_state_stable_file_ids_by_path() {
    // FileIds should be deterministic: sorted by path, not size.
    // Running discovery twice on the same project must produce identical IDs.
    let root = fixture_path("workspace-project");
    let config = create_config(root);

    let files_a = fallow_core::discover::discover_files(&config);
    let files_b = fallow_core::discover::discover_files(&config);

    assert_eq!(files_a.len(), files_b.len());
    for (a, b) in files_a.iter().zip(files_b.iter()) {
        assert_eq!(a.id, b.id, "FileId mismatch for {:?}", a.path);
        assert_eq!(a.path, b.path);
    }

    // Files should be sorted by path (not by size)
    for window in files_a.windows(2) {
        assert!(
            window[0].path <= window[1].path,
            "Files not sorted by path: {:?} > {:?}",
            window[0].path,
            window[1].path
        );
    }
}

#[test]
fn project_state_workspace_queries() {
    use fallow_config::discover_workspaces;

    let root = fixture_path("workspace-project");
    let config = create_config(root.clone());
    let files = fallow_core::discover::discover_files(&config);
    let workspaces = discover_workspaces(&root);
    let project = fallow_core::project::ProjectState::new(files, workspaces);

    // Should find all three workspace packages
    assert!(project.workspace_by_name("app").is_some());
    assert!(project.workspace_by_name("shared").is_some());
    assert!(project.workspace_by_name("@workspace/utils").is_some());
    assert!(project.workspace_by_name("nonexistent").is_none());

    // Files should be assignable to workspaces
    let app_ws = project.workspace_by_name("app").unwrap();
    let app_files = project.files_in_workspace(app_ws);
    assert!(
        !app_files.is_empty(),
        "app workspace should have at least one file"
    );

    // All app files should be under the app workspace root
    for fid in &app_files {
        if let Some(file) = project.file_by_id(*fid) {
            assert!(
                file.path.starts_with(&app_ws.root),
                "File {:?} should be under app workspace root {:?}",
                file.path,
                app_ws.root
            );
        }
    }
}

// ── Workspace exports map resolution ───────────────────────────

#[test]
fn workspace_exports_map_resolves_subpath_imports() {
    let root = fixture_path("workspace-exports-map");

    // Set up node_modules symlinks for cross-workspace resolution.
    // Uses force_symlink to handle stale directories from prior runs.
    let nm = root.join("node_modules");
    let _ = std::fs::create_dir_all(nm.join("@workspace"));
    force_symlink(&root.join("packages/ui"), &nm.join("@workspace/ui"));

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

    // orphan.ts is not exported via exports map and not imported — should be unused
    assert!(
        unused_file_names.contains(&"orphan.ts".to_string()),
        "orphan.ts should be detected as unused file, found: {unused_file_names:?}"
    );

    // utils.ts is imported via `@workspace/ui/utils` through exports map → should NOT be unused
    assert!(
        !unused_file_names.contains(&"utils.ts".to_string()),
        "utils.ts should be reachable via exports map subpath import, unused: {unused_file_names:?}"
    );

    // helpers.ts (source) should be reachable via exports map pointing to dist/helpers.js
    // fallow should map dist/helpers.js back to src/helpers.ts
    assert!(
        !unused_file_names.contains(&"helpers.ts".to_string()),
        "helpers.ts should be reachable via dist→src fallback from exports map, unused: {unused_file_names:?}"
    );

    // internal.ts is imported by utils.ts, so it should be reachable
    assert!(
        !unused_file_names.contains(&"internal.ts".to_string()),
        "internal.ts should be reachable via import from utils.ts, unused: {unused_file_names:?}"
    );

    // Unused exports on non-entry-point files should still be detected.
    // internal.ts is NOT an entry point (not in exports map) but is imported
    // by utils.ts — so its unused exports should be flagged.
    let unused_export_names: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export.export_name.as_str())
        .collect();

    assert!(
        unused_export_names.contains(&"unusedInternal"),
        "unusedInternal should be unused (internal.ts is not an entry point), found: {unused_export_names:?}"
    );

    // Used exports should NOT be flagged
    assert!(
        !unused_export_names.contains(&"internalHelper"),
        "internalHelper should be used (imported by utils.ts)"
    );

    // No unresolved imports — exports map subpaths should all resolve
    assert!(
        results.unresolved_imports.is_empty(),
        "should have no unresolved imports, found: {:?}",
        results
            .unresolved_imports
            .iter()
            .map(|i| &i.import.specifier)
            .collect::<Vec<_>>()
    );
}

#[test]
fn workspace_missing_dist_exports_resolve_to_source() {
    let root = fixture_path("workspace-missing-dist-exports");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(
        results
            .unused_files
            .iter()
            .any(|f| f.file.path.ends_with("packages/toolkit/src/orphan.ts")),
        "unrelated workspace source file should still be unused"
    );
    for terminal in [
        "packages/toolkit/src/blocked.ts",
        "packages/toolkit/src/private.ts",
    ] {
        assert!(
            results
                .unused_files
                .iter()
                .any(|f| f.file.path.ends_with(terminal)),
            "{terminal} should stay unused because blocked or unexported package subpaths must not fall back to source"
        );
    }
    for reachable in [
        "packages/toolkit/src/index.ts",
        "packages/toolkit/src/query/index.ts",
        "packages/toolkit/src/query/react/index.ts",
    ] {
        assert!(
            !results
                .unused_files
                .iter()
                .any(|f| f.file.path.ends_with(reachable)),
            "{reachable} should be reachable through workspace exports fallback"
        );
    }

    let unresolved_specifiers: Vec<&str> = results
        .unresolved_imports
        .iter()
        .map(|u| u.import.specifier.as_str())
        .collect();
    assert!(
        unresolved_specifiers.contains(&"@reduxjs/toolkit/missing"),
        "workspace export with no source target should remain unresolved: {unresolved_specifiers:?}"
    );
    assert!(
        unresolved_specifiers.contains(&"@reduxjs/toolkit/blocked"),
        "workspace export blocked by package map should remain unresolved: {unresolved_specifiers:?}"
    );
    assert!(
        unresolved_specifiers.contains(&"@reduxjs/toolkit/private"),
        "workspace subpath omitted from exports should remain unresolved: {unresolved_specifiers:?}"
    );
    assert!(
        !unresolved_specifiers.contains(&"@reduxjs/toolkit/query/react"),
        "mapped workspace export should resolve: {unresolved_specifiers:?}"
    );

    let unused_dep_names: Vec<&str> = results
        .unused_dependencies
        .iter()
        .map(|d| d.dep.package_name.as_str())
        .collect();
    assert!(
        !unused_dep_names.contains(&"@reduxjs/toolkit"),
        "declared workspace dependency should receive usage credit: {unused_dep_names:?}"
    );

    let unlisted = results
        .unlisted_dependencies
        .iter()
        .find(|dep| dep.dep.package_name == "@reduxjs/toolkit")
        .expect("undeclared workspace import should report as unlisted");
    assert!(
        unlisted
            .dep
            .imported_from
            .iter()
            .any(|site| site.path.ends_with("examples/undeclared/src/index.ts")),
        "unlisted dependency should point at undeclared workspace import sites: {:?}",
        unlisted.dep.imported_from
    );
    assert!(
        !unlisted
            .dep
            .imported_from
            .iter()
            .any(|site| site.path.ends_with("examples/app/src/index.ts")),
        "declared app workspace should not contribute unlisted sites: {:?}",
        unlisted.dep.imported_from
    );
}

// ── Workspace nested exports map ──────────────────────────────

#[test]
fn workspace_nested_exports_resolves_dist_to_source() {
    let root = fixture_path("workspace-nested-exports");

    // Set up node_modules symlinks for cross-workspace resolution.
    // Uses force_symlink to handle stale directories from prior runs.
    let nm = root.join("node_modules");
    let _ = std::fs::create_dir_all(nm.join("@workspace"));
    force_symlink(&root.join("packages/ui"), &nm.join("@workspace/ui"));

    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_file_names: Vec<String> = results
        .unused_files
        .iter()
        .map(|f| {
            f.file
                .path
                .to_string_lossy()
                .replace('\\', "/")
                .rsplit('/')
                .next()
                .unwrap_or_default()
                .to_string()
        })
        .collect();

    // Source files reachable via exports map dist→src fallback should NOT be unused
    assert!(
        !unused_file_names.contains(&"index.ts".to_string()),
        "index.ts should be reachable via exports map root entry, unused: {unused_file_names:?}"
    );
    assert!(
        !unused_file_names.contains(&"utils.ts".to_string()),
        "utils.ts should be reachable via dist/esm/utils.mjs→src/utils.ts fallback, \
         unused: {unused_file_names:?}"
    );
    assert!(
        !unused_file_names.contains(&"Button.ts".to_string()),
        "Button.ts should be reachable via dist/esm/components/Button.mjs→src/components/Button.ts \
         fallback, unused: {unused_file_names:?}"
    );

    // Unused exports should still be detected on reachable files
    let unused_export_names: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export.export_name.as_str())
        .collect();

    // unusedComponent is on index.ts which is the root entry point ("." in exports map),
    // so its exports are treated as public API and not flagged as unused
    assert!(
        !unused_export_names.contains(&"unusedComponent"),
        "unusedComponent should NOT be flagged (index.ts is an entry point)"
    );

    // Non-entry-point files resolved via dist→src fallback should still have unused exports flagged
    assert!(
        unused_export_names.contains(&"unusedUtil"),
        "unusedUtil should be unused (utils.ts export not imported by app), \
         found: {unused_export_names:?}"
    );
    assert!(
        unused_export_names.contains(&"unusedButtonHelper"),
        "unusedButtonHelper should be unused (Button.ts export not imported by app), \
         found: {unused_export_names:?}"
    );

    // Used exports should NOT be flagged
    assert!(
        !unused_export_names.contains(&"Card"),
        "Card should be used (imported by app)"
    );
    assert!(
        !unused_export_names.contains(&"formatColor"),
        "formatColor should be used (imported by app)"
    );
    assert!(
        !unused_export_names.contains(&"Button"),
        "Button should be used (imported by app)"
    );

    // No unresolved imports — nested exports map subpaths should all resolve
    assert!(
        results.unresolved_imports.is_empty(),
        "should have no unresolved imports, found: {:?}",
        results
            .unresolved_imports
            .iter()
            .map(|i| &i.import.specifier)
            .collect::<Vec<_>>()
    );
}

#[test]
fn workspace_package_export_star_barrel_chain_marks_leaf_export_used() {
    let root = fixture_path("workspace-nested-barrel-exports");

    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_exports: Vec<String> = results
        .unused_exports
        .iter()
        .map(|e| {
            format!(
                "{}:{}",
                e.export.path.to_string_lossy().replace('\\', "/"),
                e.export.export_name
            )
        })
        .collect();

    assert!(
        !unused_exports
            .iter()
            .any(|entry| entry.ends_with("foo/bar/baz/qux.tsx:PaletteColorSwatch")),
        "PaletteColorSwatch should be used through the workspace package export barrel chain, found: {unused_exports:?}"
    );
    assert!(
        results.unresolved_imports.is_empty(),
        "workspace package export should resolve without node_modules, found: {:?}",
        results
            .unresolved_imports
            .iter()
            .map(|i| &i.import.specifier)
            .collect::<Vec<_>>()
    );
}

// ── TypeScript project references ──────────────────────────────

#[test]
fn tsconfig_references_discovers_workspaces() {
    use fallow_config::discover_workspaces;

    let root = fixture_path("tsconfig-references");
    let workspaces = discover_workspaces(&root);

    // Should discover both referenced projects from tsconfig.json references
    assert!(
        workspaces.len() >= 2,
        "Expected at least 2 workspaces from tsconfig references, got: {workspaces:?}"
    );
    assert!(
        workspaces.iter().any(|ws| ws.name == "@project/core"),
        "Should discover @project/core from package.json name: {workspaces:?}"
    );
    assert!(
        workspaces.iter().any(|ws| ws.name == "ui"),
        "Should discover ui from directory name (no package.json): {workspaces:?}"
    );
}

#[test]
fn tsconfig_references_analysis_detects_unused() {
    let root = fixture_path("tsconfig-references");
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

    // unused.ts in core and orphan.ts in ui should be detected as unused
    assert!(
        unused_file_names.contains(&"unused.ts".to_string()),
        "unused.ts should be detected as unused file: {unused_file_names:?}"
    );
    assert!(
        unused_file_names.contains(&"orphan.ts".to_string()),
        "orphan.ts should be detected as unused file: {unused_file_names:?}"
    );

    // index.ts files should NOT be unused (core/index.ts is imported by ui/index.ts)
    assert!(
        !unused_file_names.contains(&"index.ts".to_string()),
        "index.ts should not be unused: {unused_file_names:?}"
    );
}

// ── Shallow nested package fallback ─────────────────────────────

#[test]
fn shallow_nested_package_scripts_become_entry_points_without_workspace_config() {
    let root = fixture_path("shallow-package-scripts");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_file_names: Vec<String> = results
        .unused_files
        .iter()
        .map(|f| {
            f.file
                .path
                .to_string_lossy()
                .replace('\\', "/")
                .rsplit('/')
                .next()
                .unwrap_or_default()
                .to_string()
        })
        .collect();

    assert!(
        !unused_file_names.contains(&"generate.mjs".to_string()),
        "generate.mjs should be treated as a package.json script entry point: {unused_file_names:?}"
    );
    assert!(
        !unused_file_names.contains(&"helper.mjs".to_string()),
        "helper.mjs should be reachable from generate.mjs: {unused_file_names:?}"
    );
    assert!(
        unused_file_names.contains(&"orphan.mjs".to_string()),
        "orphan.mjs should remain unused: {unused_file_names:?}"
    );
}
