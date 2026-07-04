use super::common::{create_config, fixture_path};

#[test]
fn dynamic_import_makes_module_reachable() {
    let root = fixture_path("dynamic-imports");
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
        !unused_file_names.contains(&"lazy.ts".to_string()),
        "lazy.ts should be reachable via dynamic import, unused files: {unused_file_names:?}"
    );

    assert!(
        unused_file_names.contains(&"orphan.ts".to_string()),
        "orphan.ts should be unused, found: {unused_file_names:?}"
    );
}

#[test]
fn dynamic_import_literal_edges_match_static_imports() {
    let root = fixture_path("dynamic-import-literals");
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
        !unused_file_names.contains(&"notes.ts".to_string()),
        "parent-relative literal dynamic import should keep notes.ts reachable: {unused_file_names:?}"
    );
    assert!(
        unused_file_names.contains(&"orphan.ts".to_string()),
        "unreferenced files should still be reported: {unused_file_names:?}"
    );

    let unresolved_specs: Vec<&str> = results
        .unresolved_imports
        .iter()
        .map(|import| import.import.specifier.as_str())
        .collect();
    assert!(
        unresolved_specs.contains(&"./missing"),
        "missing literal dynamic import should be reported unresolved: {unresolved_specs:?}"
    );

    let unlisted_names: Vec<&str> = results
        .unlisted_dependencies
        .iter()
        .map(|dep| dep.dep.package_name.as_str())
        .collect();
    assert!(
        !unlisted_names.contains(&"@some/package"),
        "listed package used through literal dynamic import should not be unlisted: {unlisted_names:?}"
    );

    let unused_dep_names: Vec<&str> = results
        .unused_dependencies
        .iter()
        .map(|dep| dep.dep.package_name.as_str())
        .collect();
    assert!(
        !unused_dep_names.contains(&"@some/package"),
        "literal dynamic package import should credit dependency usage: {unused_dep_names:?}"
    );
}

#[test]
fn vitest_vi_mock_makes_auto_mock_reachable() {
    let root = fixture_path("vitest-auto-mocks");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_files: Vec<String> = results
        .unused_files
        .iter()
        .map(|f| {
            f.file
                .path
                .strip_prefix(&root)
                .unwrap_or(&f.file.path)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();

    assert!(
        !unused_files.contains(&"src/services/__mocks__/api.ts".to_string()),
        "auto mock should be reachable via vi.mock(), unused files: {unused_files:?}"
    );
    assert!(
        unused_files.contains(&"src/services/__mocks__/unused.ts".to_string()),
        "unreferenced mock siblings should still be unused, found: {unused_files:?}"
    );
    assert!(
        unused_files.contains(&"src/services/orphan.ts".to_string()),
        "ordinary orphan files should still be unused, found: {unused_files:?}"
    );

    let unused_exports: Vec<String> = results
        .unused_exports
        .iter()
        .filter_map(|export| {
            let path = export
                .export
                .path
                .strip_prefix(&root)
                .unwrap_or(&export.export.path)
                .to_string_lossy()
                .replace('\\', "/");
            (path == "src/services/__mocks__/api.ts").then(|| export.export.export_name.clone())
        })
        .collect();
    assert!(
        unused_exports.is_empty(),
        "auto mock exports should be credited as namespace-used, found: {unused_exports:?}"
    );
}

#[test]
fn vitest_vi_mock_factory_credits_target_and_skips_auto_mock_synthesis() {
    let root = fixture_path("issue-311-vi-mock-factory-target");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_files: Vec<String> = results
        .unused_files
        .iter()
        .map(|f| {
            f.file
                .path
                .strip_prefix(&root)
                .unwrap_or(&f.file.path)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();
    assert!(
        !unused_files.contains(&"src/bar/foo.ts".to_string()),
        "vi.mock target must be credited as referenced even when paired with a factory; \
         found unused_files: {unused_files:?}"
    );

    let unresolved_specifiers: Vec<&str> = results
        .unresolved_imports
        .iter()
        .map(|imp| imp.import.specifier.as_str())
        .collect();
    assert!(
        !unresolved_specifiers
            .iter()
            .any(|s| s.contains("__mocks__")),
        "factory-form vi.mock must NOT synthesize a `__mocks__/<file>` import; \
         found unresolved_imports: {unresolved_specifiers:?}"
    );

    let unused_exports: Vec<String> = results
        .unused_exports
        .iter()
        .filter_map(|export| {
            let path = export
                .export
                .path
                .strip_prefix(&root)
                .unwrap_or(&export.export.path)
                .to_string_lossy()
                .replace('\\', "/");
            (path == "src/bar/foo.ts").then(|| export.export.export_name.clone())
        })
        .collect();
    assert_eq!(
        unused_exports,
        vec![
            "useRegenerateSlotTextMutation".to_string(),
            "stillUnused".to_string(),
        ],
        "factory-form vi.mock should keep the target file reachable without blanket-crediting its exports"
    );
}

#[test]
fn vitest_vi_mock_without_sibling_does_not_surface_unresolved_import() {
    let root = fixture_path("issue-378-vi-mock-no-sibling");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unresolved: Vec<&str> = results
        .unresolved_imports
        .iter()
        .map(|imp| imp.import.specifier.as_str())
        .collect();
    assert!(
        unresolved.is_empty(),
        "vi.mock auto-mock synthesis with no on-disk sibling must not surface as `unresolved-import`, got: {unresolved:?}"
    );

    let unused_files: Vec<String> = results
        .unused_files
        .iter()
        .map(|f| {
            f.file
                .path
                .strip_prefix(&root)
                .unwrap_or(&f.file.path)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();
    assert!(
        !unused_files.contains(&"src/utils/exportElementAsPng.ts".to_string()),
        "alias-resolved vi.mock target must still be credited as referenced, got unused_files: {unused_files:?}"
    );
    assert!(
        !unused_files.contains(&"src/utils/sibling.ts".to_string()),
        "relative vi.mock target must still be credited as referenced, got unused_files: {unused_files:?}"
    );
}

#[test]
fn dynamic_import_pattern_makes_files_reachable() {
    let root = fixture_path("dynamic-import-patterns");
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
        !unused_file_names.contains(&"en.ts".to_string()),
        "en.ts should be reachable via template literal import pattern, unused: {unused_file_names:?}"
    );
    assert!(
        !unused_file_names.contains(&"fr.ts".to_string()),
        "fr.ts should be reachable via template literal import pattern, unused: {unused_file_names:?}"
    );

    assert!(
        !unused_file_names.contains(&"home.ts".to_string()),
        "home.ts should be reachable via concat import pattern, unused: {unused_file_names:?}"
    );
    assert!(
        !unused_file_names.contains(&"about.ts".to_string()),
        "about.ts should be reachable via concat import pattern, unused: {unused_file_names:?}"
    );

    assert!(
        !unused_file_names.contains(&"utils.ts".to_string()),
        "utils.ts should be reachable via static dynamic import"
    );

    assert!(
        unused_file_names.contains(&"orphan.ts".to_string()),
        "orphan.ts should be detected as unused file, found: {unused_file_names:?}"
    );
}

#[test]
fn conditional_dynamic_import_reaches_both_branches() {
    let root = fixture_path("dynamic-import-conditional");
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

    for reachable in ["m.mjs", "x.mjs", "y.mjs"] {
        assert!(
            !unused_file_names.contains(&reachable.to_string()),
            "{reachable} should be reachable through the dynamic import, unused: {unused_file_names:?}"
        );
    }
    assert!(
        unused_file_names.contains(&"orphan.mjs".to_string()),
        "a genuinely-unreferenced file should still be reported, found: {unused_file_names:?}"
    );

    // The destructured `a` and the conditional-branch `run` (via `backend.run()`)
    // are consumed, so only the orphan's export remains dead.
    let unused_exports: Vec<(String, String)> = results
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
    assert!(
        !unused_exports
            .iter()
            .any(|(file, _)| matches!(file.as_str(), "m.mjs" | "x.mjs" | "y.mjs")),
        "exports of dynamically-imported modules should be credited, unused: {unused_exports:?}"
    );
}

#[test]
fn fully_dynamic_import_does_not_fabricate_reachability() {
    let root = fixture_path("dynamic-import-runtime-var");
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
        unused_file_names.contains(&"real.mjs".to_string()),
        "a runtime-variable import (import(someVar)) must not reach arbitrary files, unused: {unused_file_names:?}"
    );
}

#[test]
fn vite_glob_makes_files_reachable() {
    let root = fixture_path("vite-glob");
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
        !unused_file_names.contains(&"Button.ts".to_string()),
        "Button.ts should be reachable via import.meta.glob, unused: {unused_file_names:?}"
    );
    assert!(
        !unused_file_names.contains(&"Modal.ts".to_string()),
        "Modal.ts should be reachable via import.meta.glob, unused: {unused_file_names:?}"
    );

    assert!(
        unused_file_names.contains(&"orphan.ts".to_string()),
        "orphan.ts should be unused (not matched by glob), found: {unused_file_names:?}"
    );
}

#[test]
fn webpack_context_makes_files_reachable() {
    let root = fixture_path("webpack-context");
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
        !unused_file_names.contains(&"arrow.ts".to_string()),
        "arrow.ts should be reachable via require.context, unused: {unused_file_names:?}"
    );
    assert!(
        !unused_file_names.contains(&"star.ts".to_string()),
        "star.ts should be reachable via require.context, unused: {unused_file_names:?}"
    );

    assert!(
        unused_file_names.contains(&"orphan.ts".to_string()),
        "orphan.ts should be unused (not in icons/), found: {unused_file_names:?}"
    );
}
