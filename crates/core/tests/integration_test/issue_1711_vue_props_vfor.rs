use super::common::{create_config, fixture_path};

/// Issue #1711: a Vue `v-for="(util, i) of props.items"` where `props` is the
/// `defineProps` return binding and `items` is typed `Util[]` must credit member
/// accesses on the loop item, so the class members are not falsely reported as
/// unused. The fixture's `App.vue` iterates `props.items` (a `Util[]`) and reads
/// `util.id` / `util.name` / `util.getValue()` in the template; `Util.unusedMethod`
/// is never accessed and is the non-vacuous control proving the detector still
/// fires.
#[test]
fn vue_props_items_vfor_credits_class_member_accesses() {
    let root = fixture_path("issue-1711-vue-props-vfor");
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

    // `name` and `getValue` are read ONLY via the `props.items` v-for item, so
    // they are the load-bearing credit assertions (they flag without the fix).
    // `id` is also read intra-class via `this.id` inside `getValue`, so it is not
    // a meaningful credit test on its own; it is still credited here for parity.
    for member in ["id", "name", "getValue"] {
        assert!(
            !unused_members.contains(&format!("Util.{member}")),
            "Util.{member} is accessed via the props.items v-for item `util` and must be credited, found: {unused_members:?}"
        );
    }

    assert!(
        unused_members.contains(&"Util.unusedMethod".to_string()),
        "Util.unusedMethod is never accessed and must still be reported (detector must not blanket-credit the class), found: {unused_members:?}"
    );
}
