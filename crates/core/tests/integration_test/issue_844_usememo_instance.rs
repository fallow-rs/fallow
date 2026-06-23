use crate::common::{create_config, fixture_path};

/// Issue #844: a method called on a `useMemo`-bound instance
/// (`const svc = useMemo(() => new ClipsService(), [])`) is a real use of
/// `ClipsService.analyze` and must not be reported as an unused class member,
/// while genuinely-unused members on the same class keep reporting.
#[test]
fn usememo_bound_instance_method_credits_class_member() {
    let root = fixture_path("issue-844-usememo-instance");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_members: Vec<String> = results
        .unused_class_members
        .iter()
        .map(|m| format!("{}.{}", m.member.parent_name, m.member.member_name))
        .collect();

    assert!(
        !unused_members.contains(&"ClipsService.analyze".to_string()),
        "method called on a useMemo-bound instance must be credited: {unused_members:?}"
    );
    assert!(
        unused_members.contains(&"ClipsService.unusedHelper".to_string()),
        "a genuinely-unused member on the same class must still report: {unused_members:?}"
    );
}

/// Issue #844 (monorepo scale): the same `useMemo`-bound typed-instance
/// crediting must hold when the instance's class is imported across packages
/// through a tsconfig `@services/*` path alias, not just within one package.
#[test]
fn usememo_bound_instance_method_credits_across_monorepo_path_alias() {
    let root = fixture_path("issue-844-typed-instance-monorepo-alias");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused: Vec<String> = results
        .unused_class_members
        .iter()
        .map(|m| format!("{}.{}", m.member.parent_name, m.member.member_name))
        .collect();

    assert!(
        !unused.contains(&"DataService.fetchData".to_string()),
        "DataService.fetchData is called via a useMemo-wrapped instance through a path alias and must not be reported unused; found: {unused:?}"
    );
    assert!(
        unused.contains(&"DataService.unusedMethod".to_string()),
        "DataService.unusedMethod is genuinely unused and must still be reported; found: {unused:?}"
    );
}
