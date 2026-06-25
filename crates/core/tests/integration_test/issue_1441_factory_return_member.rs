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
