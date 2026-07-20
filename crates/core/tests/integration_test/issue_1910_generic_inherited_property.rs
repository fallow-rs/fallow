//! Regression tests for issue #1910: a false positive `unused-class-members`
//! for a method reached through a class field DECLARED ON A BASE CLASS and
//! accessed from a subclass via `this.<field>.<member>()`.
//!
//! Two stacked gaps are covered:
//!  1. non-generic inheritance: `PlainBaseService { client: PlainClient }` +
//!     `PlainDerivedService extends PlainBaseService` calling
//!     `this.client.plainUsed()`;
//!  2. generic-parameter substitution on top: `BaseService<TClient>.client` +
//!     `DerivedService extends BaseService<DerivedClient>` calling
//!     `this.client.getSyntheticRecords()`, where `getSyntheticRecords` lives on
//!     the concrete type argument `DerivedClient`, not the constraint `BaseClient`.
//!
//! Every client also carries a genuinely-dead control member that MUST stay
//! flagged, proving the fix credits precisely through the inheritance chain and
//! never blanket-credits.

use super::common::{create_config, fixture_path};

fn unused_member_names(root: std::path::PathBuf) -> Vec<String> {
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
fn credits_members_reached_through_inherited_base_class_property() {
    let unused = unused_member_names(fixture_path("issue-1910-generic-inherited-property"));

    // The fix: methods reached through an inherited `this.<field>` must be
    // credited, both the generic-substituted and the non-generic case.
    for credited in ["DerivedClient.getSyntheticRecords", "PlainClient.plainUsed"] {
        assert!(
            !unused.contains(&credited.to_string()),
            "{credited} is reached through an inherited base-class property and must be \
             credited (issue #1910), found: {unused:?}"
        );
    }

    // Controls: genuinely-dead members must STAY flagged (no over-credit).
    for control in [
        "BaseClient.inheritedMethod",
        "DerivedClient.deadDerivedMethod",
        "PlainClient.plainDead",
    ] {
        assert!(
            unused.contains(&control.to_string()),
            "{control} has no call site and must stay flagged (the inheritance credit \
             must not blanket-credit), found: {unused:?}"
        );
    }
}
