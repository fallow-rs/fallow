use super::common::{create_config, fixture_path};

/// Issue #1718: function-local typed arrays must type `.map` callbacks and
/// `for...of` loop variables in the same function body.
#[test]
fn function_local_array_iteration_credits_class_member_accesses() {
    let root = fixture_path("issue-1718-function-local-iteration");
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

    for member in ["property", "getter", "hello"] {
        assert!(
            !unused_members.contains(&format!("Util.{member}")),
            "Util.{member} is accessed through function-local iteration and must be credited, found: {unused_members:?}"
        );
    }

    assert!(
        unused_members.contains(&"Util.unusedMethod".to_string()),
        "Util.unusedMethod is never accessed and must still be reported, found: {unused_members:?}"
    );
}
