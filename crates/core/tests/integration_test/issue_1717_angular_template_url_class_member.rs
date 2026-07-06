use super::common::{create_config, fixture_path};

/// Issue #1717: Angular external `templateUrl` loops must credit class members
/// accessed through `@for` / `*ngFor` loop items over a typed component field.
#[test]
fn angular_external_template_for_loop_variable_credits_class_member_accesses() {
    let root = fixture_path("issue-1717-angular-template-url-class-member");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_members: Vec<String> = results
        .unused_class_members
        .iter()
        .map(|member| {
            format!(
                "{}.{}",
                member.member.parent_name, member.member.member_name
            )
        })
        .collect();

    for member in ["property", "getter", "getName"] {
        assert!(
            !unused_members.contains(&format!("Util.{member}")),
            "Util.{member} is accessed via an external-template loop item and must be credited, found: {unused_members:?}"
        );
    }

    assert!(
        unused_members.contains(&"Util.unusedMethod".to_string()),
        "Util.unusedMethod is never accessed and must still be reported, found: {unused_members:?}"
    );
}
