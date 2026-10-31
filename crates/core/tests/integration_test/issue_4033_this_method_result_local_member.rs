use super::common::{create_config, fixture_path};

#[test]
fn local_holding_this_method_result_credits_return_type_member() {
    // Issue #4033: `const old = this.current(); old?.deactivate?.();` reads
    // `deactivate` on the declared return type of `current(): Tool | undefined`
    // through a local. The access must credit `Tool.deactivate`, so the
    // implementer `SelectTool.deactivate` is used. The same holds for an
    // optional call `this.current?.()` and a `#`-private method. The method
    // comes after the call site. A member with no access stays flagged.
    let root = fixture_path("issue-4033-this-method-result-local-member");
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

    for used in [
        "SelectTool.deactivate",
        "SelectTool.activate",
        "PenBrush.stroke",
    ] {
        assert!(
            !unused.contains(&used.to_string()),
            "{used} is reached through a local that holds a `this` method result, found: {unused:?}"
        );
    }
    assert!(
        unused.contains(&"SelectTool.reset".to_string()),
        "SelectTool.reset has no access and must stay flagged, found: {unused:?}"
    );
}
