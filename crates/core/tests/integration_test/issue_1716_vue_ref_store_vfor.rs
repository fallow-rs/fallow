use super::common::{create_config, fixture_path};

/// Issue #1716: Vue `v-for` sources such as `items.value` and `store.entries`
/// must carry the source array element type to the loop item.
#[test]
fn vue_ref_value_and_store_member_vfor_credit_class_member_accesses() {
    let root = fixture_path("issue-1716-vue-ref-store-vfor");
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
            "Util.{member} is accessed via a v-for item and must be credited, found: {unused_members:?}"
        );
    }

    assert!(
        unused_members.contains(&"Util.unusedMethod".to_string()),
        "Util.unusedMethod is never accessed and must still be reported, found: {unused_members:?}"
    );
}
