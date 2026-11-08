use super::common::{create_config, fixture_path};

#[test]
fn nested_function_this_gives_no_class_member_credit() {
    // Issue #4351: `this` in a nested `function` or in an object-literal
    // method inside a class method is not the class instance. There,
    // `this.find().setVip()` must not credit `Table.setVip` through the
    // return type of the inherited `Floor.find`. An arrow function keeps the
    // `this` of the method, so `this.pick().lock()` still credits `Table.lock`.
    // A bare `this.<member>` read in such a function keeps the file-level
    // credit that every function with its own `this` gets, because code often
    // binds the instance (`function () { this.tick(); }.bind(this)`).
    let root = fixture_path("issue-4351-nested-function-this-member");
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

    for flagged in ["Table.setVip", "Floor.create"] {
        assert!(
            unused.contains(&flagged.to_string()),
            "{flagged} has no access on a class instance and must stay flagged, found: {unused:?}"
        );
    }
    for used in ["Floor.pick", "Table.lock", "Viewer.tick"] {
        assert!(
            !unused.contains(&used.to_string()),
            "{used} is read through `this` of the method, found: {unused:?}"
        );
    }
}
