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
    assert!(
        names.contains(&"deadInlinePermission"),
        "deadInlinePermission should be flagged: {members:?}"
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
    // (storeToRefs destructure + return), `name` (u.name), `login` (u.login()),
    // and inline refs-helper members are all consumed and must NOT be flagged.
    for credited in [
        "count",
        "increment",
        "double",
        "name",
        "login",
        "canCreateEvents",
        "canEditFloorPlans",
        "canSeeAnalytics",
    ] {
        assert!(
            !names.contains(&credited.to_string()),
            "{credited} is consumed and should be credited, not flagged: {names:?}"
        );
    }
}

#[test]
fn credits_inline_store_call_members_and_flags_dead_ones() {
    // Issue #1489 case 1: a member consumed only through an inline
    // `useCounterStore().member` (no bound local) must be credited, while a
    // genuinely unaccessed member on the same store still flags.
    let root = fixture_path("pinia-inline-store-member");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let names: Vec<String> = store_members(&results, &root)
        .into_iter()
        .map(|(_, m)| m)
        .collect();

    for credited in ["count", "increment"] {
        assert!(
            !names.contains(&credited.to_string()),
            "{credited} consumed via inline useCounterStore().{credited} must be credited: {names:?}"
        );
    }
    for dead in ["deadState", "deadAction"] {
        assert!(
            names.contains(&dead.to_string()),
            "{dead} is never accessed and should still flag: {names:?}"
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

#[test]
fn credits_typed_param_store_members_and_flags_dead_ones() {
    // Issue #1489 case 2: a store reaches the access through a param typed
    // `ReturnType<typeof useCounterStore>` (object-wrapped `props.store.member`
    // and `const { m } = props.store`). Both must be credited, while a genuinely
    // unused member on the same store still flags (non-vacuous control).
    let root = fixture_path("pinia-typed-param-store-member");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let names: Vec<String> = store_members(&results, &root)
        .into_iter()
        .map(|(_, m)| m)
        .collect();

    for credited in ["viaTypedParam", "viaParamDestructure"] {
        assert!(
            !names.contains(&credited.to_string()),
            "{credited} consumed through a typed store param must be credited: {names:?}"
        );
    }
    assert!(
        names.contains(&"deadMember".to_string()),
        "deadMember is never accessed and must still flag: {names:?}"
    );
}

#[test]
fn credits_setup_store_members_read_by_typed_helpers_and_setup_body() {
    // Issue #3348: `doubled` and `label` are read only through helpers whose
    // params are typed `ReturnType<typeof useCounterStore>` (inline and alias)
    // with a type-only import. `count` is read only by the `doubled` getter in
    // the setup body. `deadFlag` is returned and never read (control).
    let root = fixture_path("pinia-setup-store-helper-reads");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let members = store_members(&results, &root);

    assert_eq!(
        members,
        vec![("src/store.ts".to_string(), "deadFlag".to_string())],
        "only the never-read returned member must flag: {members:?}"
    );
}

#[test]
fn credits_store_members_read_through_imported_and_later_declared_aliases() {
    // Issue #3357: `doubled` is read through a `ReturnType` alias that another
    // file declares and exports. `label` is read through a `ReturnType` alias
    // that the same file declares after the helper. Issue #3550: `scopedTotal`
    // is read through an alias that a function body declares, and an inner
    // alias of the same name for another type must not credit `deadFlag`.
    // `deadFlag` is the control.
    let root = fixture_path("pinia-store-return-type-alias");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let members = store_members(&results, &root);

    assert_eq!(
        members,
        vec![("src/store.ts".to_string(), "deadFlag".to_string())],
        "only the never-read returned member must flag: {members:?}"
    );
}

#[test]
fn credits_state_keys_read_through_the_state_object() {
    // `system.$state.extra`, `system.$state['indexed']`, a destructure of
    // `system.$state`, an inline `useSystemStore().$state.token` and a typed
    // `props.store.$state.typedRead` each read one key. `prefs.$state` passed
    // to a helper and a spread of `useSessionStore().$state` use the whole
    // state. A whole `$state` write reads no key, so `deadState` and
    // `deadGetter` still flag.
    let root = fixture_path("pinia-state-object-reads");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let members = store_members(&results, &root);

    assert_eq!(
        members,
        vec![
            ("src/stores/system.ts".to_string(), "deadGetter".to_string()),
            ("src/stores/system.ts".to_string(), "deadState".to_string()),
        ],
        "only the never-read system members must flag: {members:?}"
    );
}

#[test]
fn credits_state_keys_read_through_this_state_in_an_option_store() {
    // The `counter` actions read `dot`, `bracket` and `pattern` through
    // `this.$state`. A whole `this.$state` write reads no key, so `written`
    // and `deadState` still flag. The `draft` action passes `this.$state` to a
    // helper, which can read every state key, so `title` and `body` do not
    // flag. `$state` holds no getter, so the unread `length` getter flags.
    let root = fixture_path("pinia-this-state-reads");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let members = store_members(&results, &root);

    assert_eq!(
        members,
        vec![
            ("src/stores/counter.ts".to_string(), "deadState".to_string()),
            ("src/stores/counter.ts".to_string(), "written".to_string()),
            ("src/stores/draft.ts".to_string(), "length".to_string()),
        ],
        "only the never-read members must flag: {members:?}"
    );
}
