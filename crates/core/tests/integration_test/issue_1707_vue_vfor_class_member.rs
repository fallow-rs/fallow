use super::common::{create_config, fixture_path};

/// Issue #1707: a Vue `v-for` loop variable iterating over a typed array /
/// reactive array of a class must credit member accesses on the item, so the
/// class members are not falsely reported as unused. The fixture's `App.vue`
/// iterates `computed(() => Util[])` and reads `util.getter` / `util.property`
/// / `util.hello()` in the template; `Util.deadMethod` is never accessed and is
/// the non-vacuous control proving the detector still fires.
#[test]
fn vue_vfor_loop_variable_credits_class_member_accesses() {
    let root = fixture_path("issue-1707-vue-vfor-class-member");
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
            "Util.{member} is accessed via the v-for item `util` and must be credited, found: {unused_members:?}"
        );
    }

    assert!(
        unused_members.contains(&"Util.deadMethod".to_string()),
        "Util.deadMethod is never accessed and must still be reported (detector must not blanket-credit the class), found: {unused_members:?}"
    );
}
