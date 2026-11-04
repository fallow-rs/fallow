use super::common::{create_config, fixture_path};

#[test]
fn inherited_this_method_result_credits_union_return_type_members() {
    // Issue #4183: `Viewer extends Floor` calls `this.find()`, and the base
    // class `Floor` declares `find(): Table | undefined` in another file.
    // `Table` is a type alias for `RectTable | RoundTable`. A read through a
    // local (`const table = this.find(); table?.setVip()`) and a direct read
    // (`this.find()?.lock()`) must credit the member on both classes of the
    // union. A member with no access stays flagged.
    let root = fixture_path("issue-4183-inherited-this-method-result-member");
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

    for used in [
        "RectTable.setVip",
        "RoundTable.setVip",
        "RectTable.lock",
        "RoundTable.lock",
    ] {
        assert!(
            !unused.contains(&used.to_string()),
            "{used} is reached through an inherited `this` method result, found: {unused:?}"
        );
    }
    assert!(
        unused.contains(&"RectTable.reset".to_string()),
        "RectTable.reset has no access and must stay flagged, found: {unused:?}"
    );
}
