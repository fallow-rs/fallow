use super::common::{create_config, fixture_path};
use fallow_config::{ScopedUsedClassMemberRule, UsedClassMemberRule};

#[test]
fn scoped_used_class_members_respect_class_heritage() {
    let root = fixture_path("scoped-used-class-members");
    let mut config = create_config(root);
    config.used_class_members = vec![
        UsedClassMemberRule::from("agInit"),
        UsedClassMemberRule::Scoped(ScopedUsedClassMemberRule {
            extends: None,
            implements: Some("ICellRendererAngularComp".to_string()),
            members: vec!["refresh".to_string()],
        }),
        UsedClassMemberRule::Scoped(ScopedUsedClassMemberRule {
            extends: Some("BaseCommand".to_string()),
            implements: None,
            members: vec!["execute".to_string()],
        }),
        UsedClassMemberRule::Scoped(ScopedUsedClassMemberRule {
            extends: None,
            implements: Some("Authorizable".to_string()),
            members: vec!["authorize".to_string()],
        }),
        UsedClassMemberRule::Scoped(ScopedUsedClassMemberRule {
            extends: Some("BaseCommand".to_string()),
            implements: Some("Authorizable".to_string()),
            members: vec!["hydrate".to_string()],
        }),
    ];

    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused_members: Vec<String> = results
        .unused_class_members
        .iter()
        .map(|m| format!("{}.{}", m.member.parent_name, m.member.member_name))
        .collect();

    assert!(
        !unused_members.contains(&"PriceCellRenderer.agInit".to_string()),
        "agInit should stay globally allowlisted: {unused_members:?}"
    );
    assert!(
        !unused_members.contains(&"PriceCellRenderer.refresh".to_string()),
        "refresh should be scoped to ICellRendererAngularComp: {unused_members:?}"
    );
    assert!(
        !unused_members.contains(&"DeployCommand.execute".to_string()),
        "execute should be scoped to BaseCommand subclasses: {unused_members:?}"
    );
    assert!(
        !unused_members.contains(&"SecureCommand.authorize".to_string()),
        "authorize should be scoped to Authorizable implementors: {unused_members:?}"
    );
    assert!(
        !unused_members.contains(&"SecureCommand.hydrate".to_string()),
        "hydrate should require both BaseCommand and CanActivate: {unused_members:?}"
    );

    assert!(
        unused_members.contains(&"DashboardComponent.refresh".to_string()),
        "refresh should still be flagged on unrelated classes: {unused_members:?}"
    );
    assert!(
        unused_members.contains(&"DashboardComponent.execute".to_string()),
        "execute should still be flagged on unrelated classes: {unused_members:?}"
    );
    assert!(
        unused_members.contains(&"DashboardComponent.authorize".to_string()),
        "authorize should still be flagged on unrelated classes: {unused_members:?}"
    );
    assert!(
        unused_members.contains(&"PriceCellRenderer.unusedHelper".to_string()),
        "scoped allowlists must not hide unrelated members: {unused_members:?}"
    );
    assert!(
        unused_members.contains(&"DeployCommand.cleanup".to_string()),
        "extends-scoped allowlist must not hide other members: {unused_members:?}"
    );
    assert!(
        unused_members.contains(&"SecureCommand.cleanup".to_string()),
        "combined scoped allowlist must not hide other members: {unused_members:?}"
    );
}

#[test]
fn scoped_used_class_members_support_glob_patterns() {
    let root = fixture_path("scoped-used-class-members");
    let mut config = create_config(root);
    config.used_class_members = vec![UsedClassMemberRule::Scoped(ScopedUsedClassMemberRule {
        extends: Some("BaseCommand".to_string()),
        implements: None,
        members: vec![
            "enter*".to_string(),
            "exit*".to_string(),
            "*Handler".to_string(),
        ],
    })];

    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused_members: Vec<String> = results
        .unused_class_members
        .iter()
        .map(|m| format!("{}.{}", m.member.parent_name, m.member.member_name))
        .collect();

    assert!(
        !unused_members.contains(&"DeployCommand.enterDeploy".to_string()),
        "enter* should suppress matching BaseCommand subclass members: {unused_members:?}"
    );
    assert!(
        !unused_members.contains(&"DeployCommand.exitDeploy".to_string()),
        "exit* should suppress matching BaseCommand subclass members: {unused_members:?}"
    );
    assert!(
        !unused_members.contains(&"DeployCommand.deployHandler".to_string()),
        "*Handler should suppress matching BaseCommand subclass members: {unused_members:?}"
    );

    assert!(
        unused_members.contains(&"DashboardComponent.enterDeploy".to_string()),
        "scoped globs must not suppress unrelated classes: {unused_members:?}"
    );
    assert!(
        unused_members.contains(&"DashboardComponent.deployHandler".to_string()),
        "scoped globs must not suppress unrelated classes: {unused_members:?}"
    );
    assert!(
        unused_members.contains(&"DeployCommand.cleanup".to_string()),
        "scoped globs must not suppress unmatched members: {unused_members:?}"
    );
}
