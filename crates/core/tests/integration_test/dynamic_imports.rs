use super::common::{create_config, fixture_path};

// ── Dynamic imports ────────────────────────────────────────────

#[test]
fn dynamic_import_makes_module_reachable() {
    let root = fixture_path("dynamic-imports");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_file_names: Vec<String> = results
        .unused_files
        .iter()
        .map(|f| f.path.file_name().unwrap().to_string_lossy().to_string())
        .collect();

    // lazy.ts is dynamically imported, so it should be reachable
    assert!(
        !unused_file_names.contains(&"lazy.ts".to_string()),
        "lazy.ts should be reachable via dynamic import, unused files: {unused_file_names:?}"
    );

    // orphan.ts should still be unused
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
        .map(|f| f.path.file_name().unwrap().to_string_lossy().to_string())
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
        .map(|import| import.specifier.as_str())
        .collect();
    assert!(
        unresolved_specs.contains(&"./missing"),
        "missing literal dynamic import should be reported unresolved: {unresolved_specs:?}"
    );

    let unlisted_names: Vec<&str> = results
        .unlisted_dependencies
        .iter()
        .map(|dep| dep.package_name.as_str())
        .collect();
    assert!(
        !unlisted_names.contains(&"@some/package"),
        "listed package used through literal dynamic import should not be unlisted: {unlisted_names:?}"
    );

    let unused_dep_names: Vec<&str> = results
        .unused_dependencies
        .iter()
        .map(|dep| dep.package_name.as_str())
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
            f.path
                .strip_prefix(&root)
                .unwrap_or(&f.path)
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
                .path
                .strip_prefix(&root)
                .unwrap_or(&export.path)
                .to_string_lossy()
                .replace('\\', "/");
            (path == "src/services/__mocks__/api.ts").then(|| export.export_name.clone())
        })
        .collect();
    assert!(
        unused_exports.is_empty(),
        "auto mock exports should be credited as namespace-used, found: {unused_exports:?}"
    );
}

// ── Dynamic import pattern resolution ──────────────────────────

#[test]
fn dynamic_import_pattern_makes_files_reachable() {
    let root = fixture_path("dynamic-import-patterns");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_file_names: Vec<String> = results
        .unused_files
        .iter()
        .map(|f| f.path.file_name().unwrap().to_string_lossy().to_string())
        .collect();

    // Locale files should be reachable via template literal pattern
    assert!(
        !unused_file_names.contains(&"en.ts".to_string()),
        "en.ts should be reachable via template literal import pattern, unused: {unused_file_names:?}"
    );
    assert!(
        !unused_file_names.contains(&"fr.ts".to_string()),
        "fr.ts should be reachable via template literal import pattern, unused: {unused_file_names:?}"
    );

    // Page files should be reachable via string concatenation pattern
    assert!(
        !unused_file_names.contains(&"home.ts".to_string()),
        "home.ts should be reachable via concat import pattern, unused: {unused_file_names:?}"
    );
    assert!(
        !unused_file_names.contains(&"about.ts".to_string()),
        "about.ts should be reachable via concat import pattern, unused: {unused_file_names:?}"
    );

    // utils.ts should be reachable via static dynamic import
    assert!(
        !unused_file_names.contains(&"utils.ts".to_string()),
        "utils.ts should be reachable via static dynamic import"
    );

    // orphan.ts should still be unused
    assert!(
        unused_file_names.contains(&"orphan.ts".to_string()),
        "orphan.ts should be detected as unused file, found: {unused_file_names:?}"
    );
}

// ── Vite import.meta.glob ──────────────────────────────────────

#[test]
fn vite_glob_makes_files_reachable() {
    let root = fixture_path("vite-glob");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_file_names: Vec<String> = results
        .unused_files
        .iter()
        .map(|f| f.path.file_name().unwrap().to_string_lossy().to_string())
        .collect();

    // Components matched by import.meta.glob('./components/*.ts') should be reachable
    assert!(
        !unused_file_names.contains(&"Button.ts".to_string()),
        "Button.ts should be reachable via import.meta.glob, unused: {unused_file_names:?}"
    );
    assert!(
        !unused_file_names.contains(&"Modal.ts".to_string()),
        "Modal.ts should be reachable via import.meta.glob, unused: {unused_file_names:?}"
    );

    // orphan.ts is outside components/, should be unused
    assert!(
        unused_file_names.contains(&"orphan.ts".to_string()),
        "orphan.ts should be unused (not matched by glob), found: {unused_file_names:?}"
    );
}

// ── Webpack require.context ────────────────────────────────────

#[test]
fn webpack_context_makes_files_reachable() {
    let root = fixture_path("webpack-context");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_file_names: Vec<String> = results
        .unused_files
        .iter()
        .map(|f| f.path.file_name().unwrap().to_string_lossy().to_string())
        .collect();

    // Icons matched by require.context('./icons', false) should be reachable
    assert!(
        !unused_file_names.contains(&"arrow.ts".to_string()),
        "arrow.ts should be reachable via require.context, unused: {unused_file_names:?}"
    );
    assert!(
        !unused_file_names.contains(&"star.ts".to_string()),
        "star.ts should be reachable via require.context, unused: {unused_file_names:?}"
    );

    // orphan.ts is outside icons/, should be unused
    assert!(
        unused_file_names.contains(&"orphan.ts".to_string()),
        "orphan.ts should be unused (not in icons/), found: {unused_file_names:?}"
    );
}
