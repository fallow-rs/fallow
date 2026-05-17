//! Integration tests for JSX asset tracking and JSDoc `import()` type
//! extraction, both landed for issue #105 (till's comment).
//!
//! The fixture (`tests/fixtures/jsx-assets-and-jsdoc/`) models a Hono-style
//! layout: `src/layout.tsx` is the entry point and emits HTML via JSX with
//! root-relative `<link rel="stylesheet" href="/static/style.css" />`,
//! `<link rel="modulepreload" href="/static/vendor.js" />`, and
//! `<script src="/static/app.js" />` references. The `static/app.js` file in
//! turn references `src/lib/types.ts::Config` only via a JSDoc
//! `@param cfg {import('../src/lib/types.ts').Config}` annotation — no ES
//! import statement binds it. Together, these exercise:
//!
//! 1. JSX asset tracking routing through the web-root-relative resolver
//!    branch (which previously only fired for `.html` source files).
//! 2. JSDoc `import()` scanner recording the type reference so `Config` is
//!    not flagged as unused.
//! 3. End-to-end reachability propagation: JSX → `static/app.js` (SideEffect)
//!    → JSDoc → `src/lib/types.ts` (type-only).

use super::common::{create_config, fixture_path};

#[test]
fn jsx_layout_makes_static_assets_reachable() {
    let root = fixture_path("jsx-assets-and-jsdoc");
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
        !unused_file_names.contains(&"style.css".to_string()),
        "static/style.css should be reachable via JSX <link href>, unused: {unused_file_names:?}"
    );
    assert!(
        !unused_file_names.contains(&"vendor.js".to_string()),
        "static/vendor.js should be reachable via JSX <link rel=modulepreload>, unused: {unused_file_names:?}"
    );
    assert!(
        !unused_file_names.contains(&"app.js".to_string()),
        "static/app.js should be reachable via JSX <script src>, unused: {unused_file_names:?}"
    );
}

#[test]
fn jsdoc_import_type_makes_referenced_types_module_reachable() {
    let root = fixture_path("jsx-assets-and-jsdoc");
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
        !unused_file_names.contains(&"types.ts".to_string()),
        "src/lib/types.ts should be reachable via JSDoc import() in static/app.js, unused: {unused_file_names:?}"
    );
}

#[test]
fn jsdoc_referenced_type_not_flagged_unused() {
    let root = fixture_path("jsx-assets-and-jsdoc");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    // `Config` is only referenced via JSDoc import() in static/app.js. With
    // the scanner, it should NOT be flagged as an unused type export.
    let unused_type_names: Vec<&str> = results
        .unused_types
        .iter()
        .map(|e| e.export.export_name.as_str())
        .collect();

    assert!(
        !unused_type_names.contains(&"Config"),
        "Config type should be credited as used via JSDoc import(), unused types: {unused_type_names:?}"
    );
}

#[test]
fn jsdoc_scanner_does_not_credit_unrelated_types() {
    let root = fixture_path("jsx-assets-and-jsdoc");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    // `Unused` in types.ts is not referenced anywhere (including no JSDoc).
    // The JSDoc scanner must credit ONLY the named member, not every export
    // in the imported module.
    let unused_type_names: Vec<&str> = results
        .unused_types
        .iter()
        .map(|e| e.export.export_name.as_str())
        .collect();

    assert!(
        unused_type_names.contains(&"Unused"),
        "Unused type should still be flagged: JSDoc scanner must credit only the named member, unused types: {unused_type_names:?}"
    );
}
