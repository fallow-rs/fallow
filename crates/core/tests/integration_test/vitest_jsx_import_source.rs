//! Integration tests for the JSX import source that a Vitest config sets.
//! With the automatic JSX runtime, the config makes each test file with JSX
//! and no pragma import `<source>/jsx-dev-runtime`. The fixture
//! (`tests/fixtures/vitest-jsx-import-source/`) has a project with a relative
//! source that resolves from the config directory, and a project with a
//! source that does not resolve.

use super::common::{create_config, fixture_path};

#[test]
fn config_jsx_import_source_makes_local_dev_runtime_reachable() {
    let root = fixture_path("vitest-jsx-import-source");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_files: Vec<String> = results
        .unused_files
        .iter()
        .map(|f| f.file.path.to_string_lossy().replace('\\', "/"))
        .collect();
    assert!(
        !unused_files
            .iter()
            .any(|path| path.ends_with("src/jsx/jsx-dev-runtime.ts")
                || path.ends_with("src/jsx/context.ts")),
        "the config runtime and its imports must be reachable, unused: {unused_files:?}"
    );

    let unused_exports: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export.export_name.as_str())
        .collect();
    for name in ["jsxDEV", "Fragment"] {
        assert!(
            !unused_exports.contains(&name),
            "the runtime export `{name}` must be used by the config, unused: {unused_exports:?}"
        );
    }
}

#[test]
fn unresolvable_config_jsx_import_source_adds_no_unresolved_import() {
    let root = fixture_path("vitest-jsx-import-source");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(
        results.unresolved_imports.is_empty(),
        "a config JSX source must never cause an unresolved import: {:?}",
        results
            .unresolved_imports
            .iter()
            .map(|i| i.import.specifier.as_str())
            .collect::<Vec<_>>()
    );
}
