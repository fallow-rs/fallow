use crate::common::{create_config, fixture_path};

/// A static member read through a namespace (`Ns.Class.member`) is a use of
/// that member. The namespace can be a namespace import, a namespace import of
/// a re-export barrel, or a named import of `export * as Ns from`.
/// A parameter that shadows the namespace name does not credit the member.
#[test]
fn namespace_qualified_static_member_read_is_credited() {
    let root = fixture_path("namespace-class-static-member");
    let results = fallow_core::analyze(&create_config(root)).expect("analysis should succeed");

    let unused_members: Vec<String> = results
        .unused_class_members
        .iter()
        .map(|m| format!("{}.{}", m.member.parent_name, m.member.member_name))
        .chain(
            results
                .unused_enum_members
                .iter()
                .map(|m| format!("{}.{}", m.member.parent_name, m.member.member_name)),
        )
        .collect();

    for used in [
        "Registry.layer",
        "Service.live",
        "Client.layer",
        "Mode.Read",
    ] {
        assert!(
            !unused_members.contains(&used.to_string()),
            "{used} is read through a namespace: {unused_members:?}"
        );
    }
    for unused in [
        "Registry.other",
        "Service.unusedLayer",
        "Client.testLayer",
        "Mode.Write",
    ] {
        assert!(
            unused_members.contains(&unused.to_string()),
            "{unused} has no namespace-qualified read: {unused_members:?}"
        );
    }
}
