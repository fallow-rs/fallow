use super::common::{create_config, fixture_path};

#[test]
fn shadowed_this_method_result_local_gives_no_credit() {
    // Issue #4198: `const table = this.find()` credits the members that the
    // method reads through `table`. A callback parameter or a loop head with
    // the name `table` holds another value, so `table.reset()` there gives no
    // credit to `RectTable.reset` or `RoundTable.reset`. The same rule applies
    // to a local that holds the result of a method of the class itself.
    let root = fixture_path("issue-4198-shadowed-this-method-result-local");
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

    for used in ["RectTable.setVip", "RoundTable.setVip", "Seat.claim"] {
        assert!(
            !unused.contains(&used.to_string()),
            "{used} is read through the outer local, found: {unused:?}"
        );
    }
    for shadowed in ["RectTable.reset", "RoundTable.reset", "Seat.release"] {
        assert!(
            unused.contains(&shadowed.to_string()),
            "{shadowed} is read only through a shadowing binding, found: {unused:?}"
        );
    }
}
