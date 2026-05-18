//! Issues #396, #397, #399: extraction false positives in Vite / Vue projects.
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

// ── Issue #396 ───────────────────────────────────────────────────────

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

// ── Issue #397 ───────────────────────────────────────────────────────

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

// ── Issue #399 ───────────────────────────────────────────────────────

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
