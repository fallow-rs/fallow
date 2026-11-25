use crate::common::{create_config, fixture_path};

#[test]
fn react_class_component_statics_are_used_by_react() {
    let root = fixture_path("react-class-statics");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_members: Vec<String> = results
        .unused_class_members
        .iter()
        .map(|m| format!("{}.{}", m.member.parent_name, m.member.member_name))
        .collect();

    for member in [
        "Greeting.defaultProps",
        "Greeting.displayName",
        "Greeting.propTypes",
        "Greeting.contextTypes",
        "Greeting.childContextTypes",
        "Panel.defaultProps",
        "Panel.displayName",
        "Badge.defaultProps",
        "Badge.displayName",
    ] {
        assert!(
            !unused_members.iter().any(|m| m == member),
            "React reads {member}, so it is not an unused member: {unused_members:?}"
        );
    }
    assert!(
        unused_members.iter().any(|m| m == "Greeting.formatName"),
        "a member that React does not read stays reported: {unused_members:?}"
    );
    assert!(
        unused_members.iter().any(|m| m == "UserModel.displayName"),
        "an instance field with a React static name stays reported: {unused_members:?}"
    );
}
