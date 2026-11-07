use super::common::{create_config, fixture_path};

#[test]
fn function_array_return_element_credits_member() {
    // Issue #4313: with `function all(): Table[]`, a member read on an
    // element of the returned array must credit the `Table` member: in
    // `for (const t of all())`, `all().forEach((t) => ...)`, `all()[0]`, and
    // through `const tables = all()`. The function comes after the reads, so
    // the order of declaration does not matter. A member with no access
    // stays flagged.
    let root = fixture_path("issue-4313-function-array-return-element-member");
    let mut config = create_config(root);
    config.rules.unused_class_members = fallow_config::Severity::Error;
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused: Vec<String> = results
        .unused_class_members
        .iter()
        .map(|member| {
            format!(
                "{}.{}",
                member.member.parent_name, member.member.member_name
            )
        })
        .collect();

    for used in ["Table.setVip", "Table.lock", "Table.seat", "Table.clean"] {
        assert!(
            !unused.contains(&used.to_string()),
            "{used} is read on an element of the returned array, found: {unused:?}"
        );
    }
    assert!(
        unused.contains(&"Table.reset".to_string()),
        "Table.reset has no access and must stay flagged, found: {unused:?}"
    );
}
