use super::common::{create_config, fixture_path};

#[test]
fn factory_return_value_credits_class_member() {
    // `const api = useApi()` where the same-file `useApi` returns `new RESTApi()`
    // must credit `api.Plan()` onto `RESTApi.Plan`, while a genuinely unused
    // method on the same class stays flagged. Regression for issue #1441
    // (same-file factory; imported/composable wrappers are deferred).
    let root = fixture_path("issue-1441-factory-return-member");
    let mut config = create_config(root);
    config.rules.unused_class_members = fallow_config::Severity::Error;
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused: Vec<String> = results
        .unused_class_members
        .iter()
        .map(|m| format!("{}.{}", m.member.parent_name, m.member.member_name))
        .collect();

    assert!(
        !unused.contains(&"RESTApi.Plan".to_string()),
        "RESTApi.Plan is reached via `const api = useApi(); api.Plan()` and must be credited \
         (issue #1441), found: {unused:?}"
    );
    assert!(
        unused.contains(&"RESTApi.unusedMethod".to_string()),
        "RESTApi.unusedMethod has no call site and must stay flagged (no blanket over-credit), \
         found: {unused:?}"
    );
}

#[test]
fn cross_module_factory_return_credits_class_member() {
    // A consumer binds the result of an IMPORTED factory and reads a member:
    //   const a = useApi();    a.Plan()      -> typed module-local return
    //   const b = makeDirect(); b.Material() -> direct `new RESTApi()` (via barrel)
    //   const c = useAliased(); c.Settings() -> aliased export of a direct-new factory
    // Each must credit the class member across the module boundary (issue #1441,
    // Part A), while two over-credit negatives stay flagged:
    //   Ghost        -> reached only via notAFactory().Ghost(); notAFactory is not a
    //                   class factory, so the sentinel must resolve to nothing
    //   unusedMethod -> never reached at all
    let root = fixture_path("issue-1441-cross-module-factory-member");
    let mut config = create_config(root);
    config.rules.unused_class_members = fallow_config::Severity::Error;
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused: Vec<String> = results
        .unused_class_members
        .iter()
        .map(|m| format!("{}.{}", m.member.parent_name, m.member.member_name))
        .collect();

    for credited in ["RESTApi.Plan", "RESTApi.Material", "RESTApi.Settings"] {
        assert!(
            !unused.contains(&credited.to_string()),
            "{credited} is reached cross-module through an imported factory and must be credited \
             (issue #1441 Part A), found: {unused:?}"
        );
    }
    assert!(
        unused.contains(&"RESTApi.Ghost".to_string()),
        "RESTApi.Ghost is reached only through notAFactory() (not a class factory) and must stay \
         flagged , the cross-module credit must not fire for a non-factory callee, found: {unused:?}"
    );
    assert!(
        unused.contains(&"RESTApi.unusedMethod".to_string()),
        "RESTApi.unusedMethod has no call site and must stay flagged, found: {unused:?}"
    );
}
