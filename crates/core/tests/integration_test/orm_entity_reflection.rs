//! End-to-end test against the `tests/fixtures/orm-entity-reflection/`
//! fixture. An ORM reads every property of an entity that a repository lookup
//! receives, a GraphQL schema exposes every value of a registered enum, and
//! `nest-commander` calls `run` on a `CommandRunner` subclass. Members that no
//! library reads must still be reported.

use super::common::{create_config, fixture_path};

#[test]
fn reflected_members_are_credited_but_real_dead_members_survive() {
    let root = fixture_path("orm-entity-reflection");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let mut unused_members: Vec<String> = results
        .unused_class_members
        .iter()
        .map(|finding| {
            format!(
                "{}.{}",
                finding.member.parent_name, finding.member.member_name
            )
        })
        .collect();
    unused_members.sort();
    assert_eq!(
        unused_members,
        vec![
            "CompanyStore.unusedHelper".to_string(),
            "SeedCommand.unusedOption".to_string(),
        ],
        "only members that no library reads may surface as unused-class-member"
    );

    let unused_enum_members: Vec<String> = results
        .unused_enum_members
        .iter()
        .map(|finding| {
            format!(
                "{}.{}",
                finding.member.parent_name, finding.member.member_name
            )
        })
        .collect();
    assert_eq!(
        unused_enum_members,
        vec!["Priority.High".to_string()],
        "a registered GraphQL enum is read whole, an unregistered enum is not"
    );
}
