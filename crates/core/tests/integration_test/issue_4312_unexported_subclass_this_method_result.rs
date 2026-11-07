use super::common::{create_config, fixture_path};

#[test]
fn unexported_subclass_this_method_result_credits_return_type_member() {
    // Issue #4312: `this.find()?.setVip()` in `class Viewer extends Floor`,
    // where `Viewer` is not exported and `Floor` declares
    // `find(): Table | undefined` in another file, must credit `Table.setVip`.
    // A second unexported level (`Gallery extends Viewer`) also credits
    // `Table.lock`. A member with no access stays flagged.
    let root = fixture_path("issue-4312-unexported-subclass-this-method-result");
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

    for used in ["Table.setVip", "Table.lock"] {
        assert!(
            !unused.contains(&used.to_string()),
            "{used} is reached through an inherited `this` method result, found: {unused:?}"
        );
    }
    assert!(
        unused.contains(&"Table.reset".to_string()),
        "Table.reset has no access and must stay flagged, found: {unused:?}"
    );
}
