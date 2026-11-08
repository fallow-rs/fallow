use super::common::{create_config, fixture_path};

#[test]
fn for_of_element_binding_applies_only_inside_the_loop() {
    // Issue #4352: the element class of a `for...of` loop variable applies
    // only inside the loop body. A parameter with the same name in another
    // function has another type, so `t.lock()` and `u.reset()` there must
    // not credit `Table.lock` or `Table.reset`. The loop over an array
    // (`tables`) and the loop over a call (`all()`) still credit the members
    // that the loop bodies read.
    let root = fixture_path("issue-4352-for-of-element-binding-scope");
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

    for used in ["Table.setVip", "Table.seat"] {
        assert!(
            !unused.contains(&used.to_string()),
            "{used} is read on the loop variable, found: {unused:?}"
        );
    }
    for flagged in ["Table.lock", "Table.reset"] {
        assert!(
            unused.contains(&flagged.to_string()),
            "{flagged} is read only on a value of another type and must stay flagged, found: {unused:?}"
        );
    }
}
