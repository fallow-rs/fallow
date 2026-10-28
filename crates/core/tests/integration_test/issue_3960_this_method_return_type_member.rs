use super::common::{create_config, fixture_path};

#[test]
fn member_access_on_this_method_return_type_credits_target() {
    // Issue #3960: `this.current()?.onDown?.()` inside the class reads
    // `onDown` on the declared return type of the private method
    // `current(): Tool | undefined`. The access must credit `Tool.onDown`, so
    // the implementer `SelectTool.onDown` is used. The same holds for a
    // `#`-private method, a private getter, and a class return type. Members
    // with no access stay flagged.
    let root = fixture_path("issue-3960-this-method-return-type-member");
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
        "SelectTool.onDown",
        "PenBrush.stroke",
        "SquareShape.fill",
        "Widget.paint",
    ] {
        assert!(
            !unused.contains(&used.to_string()),
            "{used} is reached through a `this` member return type, found: {unused:?}"
        );
    }
    for dead in ["SelectTool.onUp", "Widget.unusedOnWidget"] {
        assert!(
            unused.contains(&dead.to_string()),
            "{dead} has no access and must stay flagged, found: {unused:?}"
        );
    }
}
