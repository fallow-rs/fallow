use super::common::{create_config, fixture_path};

#[test]
fn chained_call_with_arguments_credits_return_type_member() {
    // Issue #4184: `local.rotate(1).translate(1, 1)` with `rotate(): Point`
    // must credit `Point.translate`. A chain with arguments at more than one
    // call (`rotate(1).translate(2, 2).scale(3)`) and a method that returns
    // another class (`label("origin").render()`) also credit the member. A
    // member with no access stays flagged.
    let root = fixture_path("issue-4184-method-call-arguments-chain-member");
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

    for used in ["Point.translate", "Point.scale", "Label.render"] {
        assert!(
            !unused.contains(&used.to_string()),
            "{used} is reached through a chained call with arguments, found: {unused:?}"
        );
    }
    for flagged in ["Point.mirror", "Label.hide"] {
        assert!(
            unused.contains(&flagged.to_string()),
            "{flagged} has no access and must stay flagged, found: {unused:?}"
        );
    }
}
