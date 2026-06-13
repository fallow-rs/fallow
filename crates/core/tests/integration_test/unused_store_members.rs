//! Cross-graph `unused-store-member` detection for Pinia stores.
//!
//! Covers the dead-member direction (a `state` / `getters` / `actions` key, or
//! a setup-store returned key, never accessed by any consumer) plus the
//! abstain rules (whole-object use, dynamic dispatch, `mapState`) and the
//! `pinia` dependency activation gate.

use std::path::Path;

use super::common::{create_config, fixture_path};
use fallow_types::results::AnalysisResults;

fn store_members(results: &AnalysisResults, root: &Path) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = results
        .unused_store_members
        .iter()
        .map(|f| {
            let path = f
                .member
                .path
                .strip_prefix(root)
                .unwrap_or(&f.member.path)
                .to_string_lossy()
                .replace('\\', "/");
            (path, f.member.member_name.clone())
        })
        .collect();
    out.sort();
    out
}

#[test]
fn flags_unused_option_and_setup_store_members() {
    let root = fixture_path("pinia-store-members");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let members = store_members(&results, &root);
    let names: Vec<&str> = members.iter().map(|(_, m)| m.as_str()).collect();

    // Option store: declared-but-unaccessed state / getter / action are flagged.
    assert!(
        names.contains(&"deadState"),
        "deadState should be flagged: {members:?}"
    );
    assert!(
        names.contains(&"deadGetter"),
        "deadGetter should be flagged: {members:?}"
    );
    assert!(
        names.contains(&"deadAction"),
        "deadAction should be flagged: {members:?}"
    );
    // Setup store: declared-but-unaccessed returned keys are flagged.
    assert!(
        names.contains(&"deadRef"),
        "deadRef should be flagged: {members:?}"
    );
    assert!(
        names.contains(&"deadFn"),
        "deadFn should be flagged: {members:?}"
    );
}

#[test]
fn credits_consumed_store_members() {
    let root = fixture_path("pinia-store-members");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let names: Vec<String> = store_members(&results, &root)
        .into_iter()
        .map(|(_, m)| m)
        .collect();

    // `count` (store.count), `increment` (store.increment()), `double`
    // (storeToRefs destructure + return), `name` (u.name), `login` (u.login())
    // are all consumed and must NOT be flagged.
    for credited in ["count", "increment", "double", "name", "login"] {
        assert!(
            !names.contains(&credited.to_string()),
            "{credited} is consumed and should be credited, not flagged: {names:?}"
        );
    }
}

#[test]
fn abstains_on_whole_object_dynamic_and_map_helpers() {
    let root = fixture_path("pinia-store-members-abstain");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let members = store_members(&results, &root);

    // Spread (`{...store}`), `Object.keys(store)`, dynamic `store[key]()`, and
    // `mapState(store, [...])` each mark the whole store as used, so no member
    // is falsely flagged.
    assert!(
        members.is_empty(),
        "whole-object / dynamic / mapState consumers must abstain on all members: {members:?}"
    );
}

#[test]
fn flags_dead_members_in_a_workspace_store_package() {
    // A shared `packages/stores` module is a workspace-package entry boundary,
    // yet its members are app-internal: a member consumed by no sibling package
    // is dead, while cross-package consumers credit the used members.
    let root = fixture_path("pinia-store-members-monorepo");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let names: Vec<String> = store_members(&results, &root)
        .into_iter()
        .map(|(_, m)| m)
        .collect();

    assert!(
        names.contains(&"deadShared".to_string()),
        "a workspace-store member consumed by no package is dead: {names:?}"
    );
    assert!(
        names.contains(&"deadSharedAction".to_string()),
        "a workspace-store action consumed by no package is dead: {names:?}"
    );
    for credited in ["count", "double", "inc"] {
        assert!(
            !names.contains(&credited.to_string()),
            "{credited} is consumed cross-package and must be credited: {names:?}"
        );
    }
}

#[test]
fn dep_gate_suppresses_without_pinia() {
    let root = fixture_path("pinia-store-members-no-dep");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let members = store_members(&results, &root);

    // A local `defineStore`-named helper in a project that does NOT declare
    // pinia / @pinia/nuxt must never produce store-member findings.
    assert!(
        members.is_empty(),
        "store-member detection must be gated on a declared pinia dependency: {members:?}"
    );
}
