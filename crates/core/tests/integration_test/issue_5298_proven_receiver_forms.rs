//! Issue #5298: four receiver forms whose class the syntax proves. A factory
//! that returns the instance inside a branch, a factory that returns a
//! subclass without own members, a static factory that returns a builder
//! chain, and a destructured property of an object factory result.

use super::common::{create_config, fixture_path};

fn unused_member_names() -> Vec<String> {
    let root = fixture_path("issue-5298-proven-receiver-forms");
    let mut config = create_config(root);
    config.rules.unused_class_members = fallow_config::Severity::Error;
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    results
        .unused_class_members
        .iter()
        .map(|m| format!("{}.{}", m.member.parent_name, m.member.member_name))
        .collect()
}

fn assert_used(members: &[String], used: &str, form: &str) {
    assert!(
        !members.iter().any(|m| m == used),
        "{used} is read through {form}. Got: {members:?}"
    );
}

fn assert_unused(members: &[String], unused: &str) {
    assert!(
        members.iter().any(|m| m == unused),
        "{unused} has no reader, so it stays unused. Got: {members:?}"
    );
}

#[test]
fn factory_that_returns_the_instance_inside_a_branch() {
    let members = unused_member_names();
    assert_used(
        &members,
        "PageCoreService.branchUsed",
        "a branch-return factory",
    );
    assert_unused(&members, "PageCoreService.branchUnused");
}

#[test]
fn factory_that_returns_a_subclass_without_own_members() {
    let members = unused_member_names();
    assert_used(&members, "CoreService.inheritedUsed", "a subclass factory");
    assert_unused(&members, "CoreService.inheritedUnused");
}

#[test]
fn static_factory_that_returns_a_builder_chain() {
    let members = unused_member_names();
    assert_used(&members, "PartnerFactory.build", "a static builder factory");
    assert_used(
        &members,
        "PartnerFactory.chained",
        "a static builder factory",
    );
    assert_unused(&members, "PartnerFactory.fluentUnused");
}

#[test]
fn async_or_early_return_static_is_not_a_builder_factory() {
    let members = unused_member_names();
    assert_unused(&members, "PartnerFactory.asyncOnly");
    assert_unused(&members, "PartnerFactory.flagOnly");
}

#[test]
fn destructured_property_of_an_object_factory_result() {
    let members = unused_member_names();
    assert_used(&members, "TestContext.pathUsed", "an object factory path");
    assert_used(
        &members,
        "TestContext.destructuredUsed",
        "a destructured object factory property",
    );
    assert_used(
        &members,
        "TestContext.nestedUsed",
        "a nested path below a destructured property",
    );
    assert_unused(&members, "TestContext.contextUnused");
}

#[test]
fn destructured_local_with_a_second_binding_or_a_default_gives_no_credit() {
    let members = unused_member_names();
    assert_unused(&members, "TestContext.collisionOnly");
    assert_unused(&members, "TestContext.defaultOnly");
}
