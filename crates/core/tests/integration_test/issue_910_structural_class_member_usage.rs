use super::common::{create_config, fixture_path};

#[test]
fn structural_class_member_usage_credits_only_called_members() {
    let root = fixture_path("issue-910-structural-class-member-usage");
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

    assert!(
        !unused_members.contains(&"DurationMS.toMs".to_string()),
        "DurationMS.toMs should be credited through the typed local call, found: {unused_members:?}"
    );
    assert!(
        !unused_members.contains(&"DurationMS.toSec".to_string()),
        "DurationMS.toSec should be credited through the typed local call, found: {unused_members:?}"
    );
    assert!(
        unused_members.contains(&"DurationMS.unused".to_string()),
        "unrelated members should still be reported, found: {unused_members:?}"
    );
}
