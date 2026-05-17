use super::common::{create_config, fixture_path};

#[test]
fn static_factory_method_credits_instance_members_across_files() {
    // Issue #346: `MyClass.getInstance()` returns `new this()`, so members
    // accessed on the call result must be credited to `MyClass`. Both the
    // `new this()` and named-self (`return new Service()`) factory shapes
    // are covered.
    //
    // Regression guard: `MyClass.unusedHelper` is never consumed; it must
    // remain flagged. This proves the fix does not over-credit every member
    // of a class once one factory call has been observed.
    let root = fixture_path("issue-346-static-factory-method");
    let mut config = create_config(root);
    config.rules.unused_class_members = fallow_config::Severity::Error;
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused: Vec<String> = results
        .unused_class_members
        .iter()
        .map(|m| format!("{}.{}", m.member.parent_name, m.member.member_name))
        .collect();

    assert!(
        !unused.contains(&"MyClass.getData".to_string()),
        "MyClass.getData is consumed via MyClass.getInstance().getData() and \
         must not be flagged unused: {unused:?}"
    );
    assert!(
        !unused.contains(&"Service.start".to_string()),
        "Service.start is consumed via Service.create().start() and \
         must not be flagged unused: {unused:?}"
    );

    assert!(
        unused.contains(&"MyClass.unusedHelper".to_string()),
        "MyClass.unusedHelper has no callers and must remain flagged: {unused:?}"
    );
}
