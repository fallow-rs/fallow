//! Issue #5299: a class that implements an interface keeps each required
//! member of that interface and of each interface that it extends, also when
//! the parent interface is in another module.

use super::common::{create_config, fixture_path};

fn unused_member_names() -> Vec<String> {
    let root = fixture_path("issue-5299-inherited-interface-contract");
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
fn inherited_required_interface_members_stay_used() {
    let members = unused_member_names();
    for used in [
        "IssueDetail.deleteSubIssue",
        "IssueDetail.fetchLinks",
        "IssueDetail.localAction",
        "IssueDetail.parentAction",
        "IssueDetail.grandparentAction",
        "IssueDetail.fetchIssue",
    ] {
        assert!(
            !members.iter().any(|m| m == used),
            "{used} is required through the implements contract. Got: {members:?}"
        );
    }
}

#[test]
fn optional_and_class_only_members_stay_unused() {
    let members = unused_member_names();
    for unused in [
        "IssueDetail.optionalSubIssue",
        "IssueDetail.storeOnlyMethod",
    ] {
        assert!(
            members.iter().any(|m| m == unused),
            "{unused} is not required by the contract. Got: {members:?}"
        );
    }
}
