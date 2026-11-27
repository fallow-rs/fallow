//! Issue #5366: a local function that only returns the call of an object
//! factory, as in `const setup = () => newService(...)`. A destructure of
//! the wrapper result (`const { ctx } = setup()`) reads the factory result,
//! so the reads on `ctx` credit the class of the factory shape.

use super::common::{create_config, fixture_path};

fn unused_member_names() -> Vec<String> {
    let root = fixture_path("issue-5366-setup-wrapper-receiver");
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
fn wrapper_of_an_imported_object_factory() {
    let members = unused_member_names();
    assert_used(&members, "TestContext.blockWrapperUsed", "a block wrapper");
    assert_used(
        &members,
        "TestContext.conciseWrapperUsed",
        "a concise wrapper",
    );
    assert_used(
        &members,
        "TestContext.chainedWrapperUsed",
        "a wrapper of a wrapper",
    );
    assert_used(
        &members,
        "TestContext.pathWrapperUsed",
        "a wrapper result path",
    );
    assert_unused(&members, "TestContext.contextUnused");
}

#[test]
fn wrapper_of_a_same_file_object_factory() {
    let members = unused_member_names();
    assert_used(
        &members,
        "LocalContext.localWrapperUsed",
        "a same-file wrapper",
    );
    assert_unused(&members, "LocalContext.localUnused");
}

#[test]
fn callee_bound_by_a_parameter_is_not_the_factory() {
    let members = unused_member_names();
    assert_unused(&members, "TestContext.shadowedOnly");
}

#[test]
fn repeated_destructures_of_the_same_property_credit() {
    let members = unused_member_names();
    assert_used(
        &members,
        "TestContext.repeatedUsed",
        "a destructure in each test",
    );
    assert_used(
        &members,
        "TestContext.repeatedDirectUsed",
        "a renamed destructure in each block",
    );
}

#[test]
fn a_name_that_a_parameter_also_binds_gives_no_credit() {
    let members = unused_member_names();
    assert_unused(&members, "TestContext.mixedOnly");
}

#[test]
fn a_shadowed_or_reassigned_wrapper_name_is_not_the_factory() {
    let members = unused_member_names();
    assert_unused(&members, "TestContext.shadowWrapperOnly");
    assert_unused(&members, "TestContext.reassignedWrapperOnly");
}

#[test]
fn an_imported_name_stops_the_destructure_credit() {
    let members = unused_member_names();
    assert_unused(&members, "TestContext.importOnly");
}
