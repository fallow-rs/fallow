//! Issue #5297: `makeObservable(this, {...})` and `makeAutoObservable(this,
//! {...})` from `mobx` name class members in their annotation object. MobX
//! reads each named member when the constructor runs.

use super::common::{create_config, fixture_path};

fn unused_member_names() -> Vec<String> {
    let root = fixture_path("issue-5297-mobx-annotation-members");
    let mut config = create_config(root);
    config.rules.unused_class_members = fallow_config::Severity::Error;
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    results
        .unused_class_members
        .iter()
        .map(|m| format!("{}.{}", m.member.parent_name, m.member.member_name))
        .collect()
}

#[test]
fn mobx_annotation_keys_credit_class_members() {
    let members = unused_member_names();
    for used in [
        "FilterStore.issueFilters",
        "FilterStore.cleanup",
        "FilterStore.quotedAction",
        "AutoStore.overriddenMethod",
    ] {
        assert!(
            !members.iter().any(|m| m == used),
            "{used} is named in a MobX annotation object. Got: {members:?}"
        );
    }
}

#[test]
fn members_outside_the_annotation_object_stay_unused() {
    let members = unused_member_names();
    for unused in [
        "FilterStore.unannotatedMethod",
        "AutoStore.autoUnusedMethod",
        "AutoStore.excludedMethod",
        "AutoStore.staticContextMethod",
        "LocalHelperStore.localHelperMethod",
    ] {
        assert!(
            members.iter().any(|m| m == unused),
            "{unused} is not read through a MobX annotation object. Got: {members:?}"
        );
    }
}
