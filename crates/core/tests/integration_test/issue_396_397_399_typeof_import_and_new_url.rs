//! Issues #396, #397, #399, #840: extraction false positives in Vite / Vue projects.
//!
//! - #396 `auto-imports.d.ts`: `declare global { const X: typeof import('./x').X }`
//!   embedded inside an ambient declaration must trace the referenced file so
//!   it does not surface as `unused-files`.
//! - #397 `components.d.ts`: `declare module 'vue' { interface GlobalComponents
//!   { X: typeof import('./x.vue')['default'] } }` must do the same for module
//!   augmentation bodies.
//! - #399 `new URL('./', import.meta.url)`: the canonical __dirname idiom must
//!   not produce an `unresolved-imports` finding. The string is a directory URL
//!   argument, not a module specifier.
//! - #840 `new URL('./services', import.meta.url)` without trailing slash: an
//!   extensionless specifier pointing at a directory with no index module is
//!   speculative and must not produce an `unresolved-import` finding, while a
//!   file target with an extension and a genuinely missing file (`./missing.js`)
//!   must still report.
//!
//! These tests use the shared `create_config(root)` helper which builds a
//! `FallowConfig` with `entry: vec![]` and DOES NOT read the fixture's
//! `.fallowrc.json`. The fixtures keep a `.fallowrc.json` for documentation
//! and for anyone running the binary against the fixture directly, but the
//! tests exercise the graph-level `.d.ts -> entry-point` auto-promotion path
//! (see `ModuleGraph::build_with_reachability_roots`) which makes the fixes
//! work without any user-supplied entry config.

use super::common::{create_config, fixture_path};

fn unused_file_names(results: &fallow_types::results::AnalysisResults) -> Vec<String> {
    results
        .unused_files
        .iter()
        .map(|f| {
            f.file
                .path
                .to_string_lossy()
                .replace('\\', "/")
                .rsplit('/')
                .next()
                .unwrap_or("")
                .to_string()
        })
        .collect()
}

fn unresolved_specifiers(results: &fallow_types::results::AnalysisResults) -> Vec<String> {
    results
        .unresolved_imports
        .iter()
        .map(|u| u.import.specifier.clone())
        .collect()
}

#[test]
fn auto_imports_dts_typeof_import_traces_target_file() {
    let root = fixture_path("issue-396-auto-imports");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let names = unused_file_names(&results);
    assert!(
        !names.contains(&"useCounter.ts".to_string()),
        "useCounter.ts is referenced from auto-imports.d.ts via typeof import(), \
         should not be unused. Got: {names:?}"
    );
}

#[test]
fn components_dts_typeof_import_traces_target_file() {
    let root = fixture_path("issue-397-vue-components");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let names = unused_file_names(&results);
    assert!(
        !names.contains(&"MyButton.vue".to_string()),
        "MyButton.vue is referenced from components.d.ts via typeof import(), \
         should not be unused. Got: {names:?}"
    );
}

#[test]
fn new_url_dot_slash_does_not_produce_unresolved_import() {
    let root = fixture_path("issue-399-new-url");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let specifiers = unresolved_specifiers(&results);
    assert!(
        !specifiers.iter().any(|s| s == "./"),
        "`new URL('./', import.meta.url)` must not flag `./` as unresolved. Got: {specifiers:?}"
    );
}

/// Issue #840: `new URL("./services", import.meta.url)` where `services/` is an
/// existing directory with NO resolvable index module must not produce an
/// `unresolved-import` finding. The specifier has no file extension, so it is
/// marked speculative; the resolver cannot find a module at that path, so the
/// finding is silently dropped.
///
/// This is a real resolve-layer regression: the `services/` fixture directory
/// deliberately contains only a non-module asset (`data.css`), so `./services`
/// resolves to `Unresolvable`. With the pre-#840 behavior (`is_speculative =
/// false` for every `new URL` specifier) this would surface `./services` as an
/// `unresolved-import`. The assertion below fails before the fix and passes
/// after it.
///
/// The test also pins both halves of the selectivity guarantee:
/// - `new URL("./worker.js", import.meta.url)` with `worker.js` present must
///   resolve (no finding), and
/// - `new URL("./missing.js", import.meta.url)` with an extension but no file
///   keeps `is_speculative = false`, so a genuinely missing file is STILL
///   reported. This proves the speculative gate is keyed on the extension, not
///   a blanket suppression of all `new URL` specifiers.
#[cfg_attr(miri, ignore)]
#[test]
fn new_url_directory_target_does_not_produce_unresolved_import() {
    let root = fixture_path("issue-840-new-url-directory");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let specifiers = unresolved_specifiers(&results);
    assert!(
        !specifiers.iter().any(|s| s == "./services"),
        "`new URL('./services', import.meta.url)` pointing at a directory with \
         no index module must not be flagged as unresolved-import (speculative \
         drop). Got: {specifiers:?}"
    );
    assert!(
        !specifiers.iter().any(|s| s == "./worker.js"),
        "`new URL('./worker.js', import.meta.url)` with a present file must \
         resolve without an unresolved-import finding. Got: {specifiers:?}"
    );
    assert!(
        specifiers.iter().any(|s| s == "./missing.js"),
        "`new URL('./missing.js', import.meta.url)` with a file extension but no \
         file on disk must STILL be reported as unresolved-import; the \
         speculative gate keys on the extension, not on the `new URL` form. \
         Got: {specifiers:?}"
    );
}
