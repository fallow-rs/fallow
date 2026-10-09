//! Integration tests for the per-file `/** @jsxImportSource <source> */`
//! pragma. With the automatic JSX runtime, the pragma makes the file import
//! `<source>/jsx-runtime`. The fixture
//! (`tests/fixtures/jsx-import-source-pragma/`) has a `.tsx` entry with a
//! relative pragma that points to a local runtime, and a `.tsx` file with a
//! package pragma.

use super::common::{create_config, fixture_path};

#[test]
fn relative_pragma_makes_local_jsx_runtime_reachable() {
    let root = fixture_path("jsx-import-source-pragma");
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
            .any(|path| path.ends_with("src/jsx/jsx-runtime.ts")
                || path.ends_with("src/jsx/context.ts")),
        "the pragma runtime and its imports must be reachable, unused: {unused_files:?}"
    );

    let unused_exports: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export.export_name.as_str())
        .collect();
    for name in ["jsx", "jsxs", "Fragment"] {
        assert!(
            !unused_exports.contains(&name),
            "the runtime export `{name}` must be used by the pragma, unused: {unused_exports:?}"
        );
    }

    assert!(
        results.unresolved_imports.is_empty(),
        "the pragma runtime must resolve, unresolved: {:?}",
        results
            .unresolved_imports
            .iter()
            .map(|i| i.import.specifier.as_str())
            .collect::<Vec<_>>()
    );
}

#[test]
fn package_pragma_uses_the_jsx_runtime_package() {
    let root = fixture_path("jsx-import-source-pragma");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_deps: Vec<&str> = results
        .unused_dependencies
        .iter()
        .map(|d| d.dep.package_name.as_str())
        .collect();
    assert!(
        !unused_deps.contains(&"preact"),
        "`@jsxImportSource preact` must use the preact dependency, unused: {unused_deps:?}"
    );
}
