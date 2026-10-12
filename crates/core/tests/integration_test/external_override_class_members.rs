use crate::common::{create_config, fixture_path};

/// Code outside the project calls an `override` member of a class whose base
/// comes from a package or is a global constructor, and the runtime reads the
/// `Error` protocol members of a native error subclass. Other members keep
/// reporting. An `override` of a project base, of a mixin call over a project
/// class, of an unresolved import or of a local class with a global name gets
/// no credit.
#[test]
fn external_override_members_are_credited() {
    let root = fixture_path("external-override-class-members");
    let results = fallow_core::analyze(&create_config(root)).expect("analysis should succeed");

    let unused_members: Vec<String> = results
        .unused_class_members
        .iter()
        .map(|m| format!("{}.{}", m.member.parent_name, m.member.member_name))
        .collect();

    for used in [
        "PortContainer.defaultPort",
        "PortContainer.onStop",
        "ChildContainer.onStop",
        "NotFound.message",
        "Failure.toString",
        "PlainFailure.message",
        "LooseFailure.toString",
        "LooseFailure.cause",
        "RenamedContainer.onStop",
        "default.onStart",
    ] {
        assert!(
            !unused_members.contains(&used.to_string()),
            "{used} overrides an external contract: {unused_members:?}"
        );
    }
    for unused in [
        "PortContainer.ownHelper",
        "ChildContainer.childHelper",
        "NotFound.unusedHelper",
        "PlainFailure.unusedHelper",
        "LocalChild.run",
        "Mixed.run",
        "MixedMember.run",
        "MixedArray.run",
        // The base of `ProjectLeaf` is a project class that does not declare
        // `onStop`. Only a direct external base gives the credit.
        "ProjectLeaf.onStop",
        "ProjectMid.midHelper",
        "FromMissing.run",
        "LocalEvent.fire",
        "RenamedContainer.innerHelper",
        "default.defaultHelper",
    ] {
        assert!(
            unused_members.contains(&unused.to_string()),
            "{unused} is outside an external contract: {unused_members:?}"
        );
    }
}
