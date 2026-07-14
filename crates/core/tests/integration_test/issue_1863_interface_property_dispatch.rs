use super::common::{create_config, fixture_path};

#[test]
fn interface_typed_property_dispatch_credits_implementer_member() {
    // Issue #1863: a member reached through a PROPERTY whose declared type is an
    // interface (`deps.greeter.greet()` where `deps: Deps`, `Deps.greeter:
    // GreeterPort`) must credit the member on every class that `implements`
    // GreeterPort. The #1785 typed-property hop resolves the terminal to the
    // interface, but the terminal credit only handled classes; routing an
    // interface terminal through the existing interface->implementer propagation
    // carries the member to `GreeterAdapter`, exactly as the direct-parameter
    // case already works. A genuinely-dead method on the same class stays
    // flagged (non-vacuous control).
    let root = fixture_path("issue-1863-interface-property-dispatch");
    let mut config = create_config(root);
    config.rules.unused_class_members = fallow_config::Severity::Error;
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused: Vec<String> = results
        .unused_class_members
        .iter()
        .map(|m| format!("{}.{}", m.member.parent_name, m.member.member_name))
        .collect();

    assert!(
        !unused.contains(&"GreeterAdapter.greet".to_string()),
        "GreeterAdapter.greet is reached via deps.greeter.greet() through the GreeterPort \
         interface property and must be credited (issue #1863), found: {unused:?}"
    );
    assert!(
        unused.contains(&"GreeterAdapter.deadOnAdapter".to_string()),
        "GreeterAdapter.deadOnAdapter has no call site and must stay flagged, found: {unused:?}"
    );
}

#[test]
fn interface_property_hop_credits_every_implementer_cross_file() {
    // Issue #1863 (fan-out + cross-file): the port interface (`GreeterPort`), the
    // interface holding the typed property (`Deps` in a separate file), and each
    // implementing adapter all live in different files, the realistic hexagonal
    // layout. `deps.greeter.greet()` must credit `greet` on EVERY class that
    // implements `GreeterPort` (the documented interface->implementer over-credit
    // direction), while a differently-named dead method on each adapter stays
    // flagged.
    let root = fixture_path("issue-1863-multi-implementer");
    let mut config = create_config(root);
    config.rules.unused_class_members = fallow_config::Severity::Error;
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused: Vec<String> = results
        .unused_class_members
        .iter()
        .map(|m| format!("{}.{}", m.member.parent_name, m.member.member_name))
        .collect();

    assert!(
        !unused.contains(&"AdapterA.greet".to_string())
            && !unused.contains(&"AdapterB.greet".to_string()),
        "greet must be credited on both implementers of GreeterPort via the interface \
         property hop (issue #1863), found: {unused:?}"
    );
    assert!(
        unused.contains(&"AdapterA.deadOnA".to_string())
            && unused.contains(&"AdapterB.deadOnB".to_string()),
        "the uncalled methods on each adapter must stay flagged (non-vacuous control), \
         found: {unused:?}"
    );
}
