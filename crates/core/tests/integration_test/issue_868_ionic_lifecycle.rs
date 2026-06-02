//! Regression test for Ionic Angular page lifecycle class members.

use super::common::{create_config, fixture_path};

#[test]
fn ionic_angular_lifecycle_members_are_credited_but_real_dead_members_survive() {
    let root = fixture_path("issue-868-ionic-lifecycle");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_members: Vec<String> = results
        .unused_class_members
        .iter()
        .map(|finding| {
            format!(
                "{}.{}",
                finding.member.parent_name, finding.member.member_name
            )
        })
        .collect();

    for lifecycle in [
        "IonicPage.ionViewWillEnter",
        "IonicPage.ionViewDidEnter",
        "IonicPage.ionViewWillLeave",
        "IonicPage.ionViewDidLeave",
    ] {
        assert!(
            !unused_members.contains(&lifecycle.to_string()),
            "{lifecycle} is invoked by Ionic Angular and must not surface as \
             unused-class-member; unused_class_members = {unused_members:?}"
        );
    }

    for dead in [
        "IonicPage.unusedHelper",
        "PlainClass.ionViewWillLoad",
        "PlainClass.unusedHelper",
    ] {
        assert!(
            unused_members.contains(&dead.to_string()),
            "{dead} is genuinely unused and must still be reported; \
             unused_class_members = {unused_members:?}"
        );
    }
}
