//! Integration tests for `` html`...` `` tagged template literal asset
//! tracking, shipped as a follow-up to the JSX asset fix for issue #105.
//!
//! The fixture (`tests/fixtures/hono-html-tagged-template/`) models till's
//! exact reproduction: a `.ts` layout component (no JSX) that uses an `html`
//! tagged template literal to emit HTML, with `<script src>` and
//! `<link rel="stylesheet|modulepreload" href>` attributes referencing
//! sibling files in `static/`. Without the tagged-template override those
//! files were flagged as unused because the asset references lived inside a
//! template string that the visitor ignored entirely.

use super::common::{create_config, fixture_path};

#[test]
fn html_tagged_template_makes_static_assets_reachable() {
    let root = fixture_path("hono-html-tagged-template");
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
        !unused_file_names.contains(&"otp-input.js".to_string()),
        "static/otp-input.js should be reachable via html`` <script src>, unused: {unused_file_names:?}"
    );
    assert!(
        !unused_file_names.contains(&"style.css".to_string()),
        "static/style.css should be reachable via html`` <link rel=stylesheet>, unused: {unused_file_names:?}"
    );
    assert!(
        !unused_file_names.contains(&"vendor.js".to_string()),
        "static/vendor.js should be reachable via html`` <link rel=modulepreload>, unused: {unused_file_names:?}"
    );
}
