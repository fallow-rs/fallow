use super::common::{create_config, fixture_path};

#[test]
fn typed_parameter_chain_with_arguments_credits_return_type_member() {
    // Issue #4311: `local.rotate(1).translate(1, 1)`, where `local` is a
    // parameter typed `Point` and `rotate(angle: number): Point`, must credit
    // `Point.translate`. A longer chain (`.translate(2, 2).scale(3)`) also
    // credits each member. A member with no access stays flagged.
    let root = fixture_path("issue-4311-typed-parameter-chain-member");
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

    for used in ["Point.translate", "Point.scale"] {
        assert!(
            !unused.contains(&used.to_string()),
            "{used} is reached through a chain on a typed parameter, found: {unused:?}"
        );
    }
    assert!(
        unused.contains(&"Point.mirror".to_string()),
        "Point.mirror has no access and must stay flagged, found: {unused:?}"
    );
}
