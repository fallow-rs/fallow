//! Cross-graph `unprovided-inject` detection for Vue `inject` / Svelte
//! `getContext` whose symbol key is provided nowhere in the project.
//!
//! Covers the dead-half direction (an inject with no matching provide), the
//! matched-pair credit (including barrel-asymmetric and app-level provide), and
//! the abstain ladder (external package keys, string keys, the dep gate, the
//! public-API headless-library case, and the dynamic-provide project-wide
//! abstain).

use std::path::Path;

use super::common::{create_config, fixture_path};
use fallow_types::results::AnalysisResults;

fn injects(results: &AnalysisResults, root: &Path) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = results
        .unprovided_injects
        .iter()
        .map(|f| {
            let path = f
                .inject
                .path
                .strip_prefix(root)
                .unwrap_or(&f.inject.path)
                .to_string_lossy()
                .replace('\\', "/");
            (path, f.inject.key_name.clone())
        })
        .collect();
    out.sort();
    out
}

#[test]
fn flags_dead_vue_inject_and_credits_matched_pairs() {
    let root = fixture_path("unprovided-inject-vue");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let found = injects(&results, &root);
    let names: Vec<&str> = found.iter().map(|(_, k)| k.as_str()).collect();

    // THEME_KEY is injected but provided nowhere: the one real dead half.
    assert!(
        names.contains(&"THEME_KEY"),
        "THEME_KEY should be flagged: {found:?}"
    );
    // SHARED_KEY (direct provide), GLOBAL_KEY (app.provide), and BARREL_KEY
    // (provider imports it directly, consumer through a barrel) are all
    // provided somewhere and must NOT be flagged.
    for credited in ["SHARED_KEY", "GLOBAL_KEY", "BARREL_KEY"] {
        assert!(
            !names.contains(&credited),
            "{credited} is provided and must be credited, not flagged: {found:?}"
        );
    }
    assert_eq!(
        names.len(),
        1,
        "only the dead inject should be flagged: {found:?}"
    );
}

#[test]
fn flags_dead_svelte_get_context() {
    let root = fixture_path("unprovided-inject-svelte");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let found = injects(&results, &root);
    let names: Vec<&str> = found.iter().map(|(_, k)| k.as_str()).collect();

    // DEAD_CTX has no setContext; CTX_KEY does. Exercises the SFC `<script>`
    // merge path for both setContext and getContext.
    assert!(
        names.contains(&"DEAD_CTX"),
        "DEAD_CTX should be flagged: {found:?}"
    );
    assert!(
        !names.contains(&"CTX_KEY"),
        "CTX_KEY is setContext'd and must be credited: {found:?}"
    );
}

#[test]
fn abstains_on_package_imported_key() {
    let root = fixture_path("unprovided-inject-external-abstain");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let found = injects(&results, &root);

    // The key is imported from an npm package; the provide may live inside that
    // package's own code, so abstain.
    assert!(
        found.is_empty(),
        "a package-imported inject key must abstain: {found:?}"
    );
}

#[test]
fn abstains_on_string_literal_and_string_const_keys() {
    let root = fixture_path("unprovided-inject-string-abstain");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let found = injects(&results, &root);

    // Both a bare string-literal key (`inject('stringKey')`) and a key bound to
    // a string-literal const (`const K = 'jsonforms'; inject(K)`) have STRING
    // identity, not symbol identity: a provider supplying the literal (often
    // inside a package) matches them, so both must abstain.
    assert!(
        found.is_empty(),
        "string-literal and string-const inject keys must never be flagged: {found:?}"
    );
}

#[test]
fn dep_gate_suppresses_without_vue_or_svelte() {
    let root = fixture_path("unprovided-inject-no-dep");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let found = injects(&results, &root);

    // The project declares neither vue nor svelte, so detection is off.
    assert!(
        found.is_empty(),
        "unprovided-inject detection must be gated on a declared vue/svelte dependency: {found:?}"
    );
}

#[test]
fn abstains_on_public_api_inject_composable() {
    let root = fixture_path("unprovided-inject-public-api");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let found = injects(&results, &root);

    // A headless library exports the key AND an inject composable for the
    // consumer to provide. STORE_KEY is re-exported from the package entry, so
    // the in-repo inject with no local provide is intentional public API.
    assert!(
        found.is_empty(),
        "a public-API inject key (re-exported from a package entry) must abstain: {found:?}"
    );
}

#[test]
fn dynamic_provide_forces_project_wide_abstain() {
    let root = fixture_path("unprovided-inject-dynamic-provide");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let found = injects(&results, &root);

    // A loop `provide(k, ...)` keyed by a transient local could provide any
    // key, so a surviving inject finding could be a false positive: abstain
    // wholesale even though A_KEY has no statically-visible matching provide.
    assert!(
        found.is_empty(),
        "a dynamic-keyed provide must force a project-wide inject abstain: {found:?}"
    );
}
