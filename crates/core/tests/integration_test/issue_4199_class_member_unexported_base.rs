use super::common::{create_config, fixture_path};
use fallow_config::{ScopedUsedClassMemberRule, UsedClassMemberRule};

#[test]
fn inherited_this_method_result_credits_through_unexported_bases_and_arguments() {
    // Issue #4199: a subclass reads the result of `this.find()` or
    // `this.findBy(0)`, and a base class declares the return type.
    // - `LocalViewer` extends `LocalFloor`, a class that the file does not
    //   export.
    // - `ChainViewer` extends `Mid`, a class that the file does not export,
    //   and `Mid` extends the exported `Floor` of another file.
    // - `DeepViewer` extends `DeepMid` extends `DeepFloor`. The file exports
    //   neither base class.
    // - `ArgViewer` calls `this.findBy(0)`, a method with an argument.
    // A read through a local and a direct read must credit the member on the
    // return type. A member with no access stays flagged.
    let root = fixture_path("issue-4199-class-member-unexported-base");
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

    for table in ["LocalTable", "ChainTable", "DeepTable", "ArgTable"] {
        for member in ["setVip", "lock"] {
            let used = format!("{table}.{member}");
            assert!(
                !unused.contains(&used),
                "{used} is reached through an inherited `this` method result, found: {unused:?}"
            );
        }
        let idle = format!("{table}.idle");
        assert!(
            unused.contains(&idle),
            "{idle} has no access and must stay flagged, found: {unused:?}"
        );
    }
}

#[test]
fn scoped_rule_matches_declared_unexported_base() {
    // `Deploy` extends `BaseCommand`, a class that the file does not export.
    // A scoped `usedClassMembers` rule for `extends: "BaseCommand"` matches
    // the declared base name, so it credits `Deploy.onTick`. A member that no
    // rule names stays flagged.
    let root = fixture_path("issue-4199-class-member-unexported-base");
    let mut config = create_config(root);
    config.rules.unused_class_members = fallow_config::Severity::Error;
    config.used_class_members = vec![UsedClassMemberRule::Scoped(ScopedUsedClassMemberRule {
        extends: Some("BaseCommand".to_string()),
        implements: None,
        members: vec!["onTick".to_string()],
    })];
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

    assert!(
        !unused.contains(&"Deploy.onTick".to_string()),
        "the scoped rule must credit Deploy.onTick, found: {unused:?}"
    );
    assert!(
        unused.contains(&"Deploy.other".to_string()),
        "Deploy.other has no access and must stay flagged, found: {unused:?}"
    );
}
