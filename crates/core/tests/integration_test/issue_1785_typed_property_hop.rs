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
fn interface_typed_property_hop_credits_class_member() {
    // Issue #1785: `this.opts.c.optM()` where `opts` is typed by a LOCAL
    // `interface Opts { c: OptDep }` and `OptDep` is imported must credit
    // `OptDep.optM` (Part A extract-time expansion), for both the interface
    // and the type-literal-alias form, and for the same-file variant (the
    // same gap: named-type hops were never resolved anywhere). A genuinely
    // unused method on the same classes stays flagged.
    let unused = unused_member_names(fixture_path("issue-1785-typed-property-hop"));

    for credited in [
        "OptDep.optM",
        "AliasDep.viaAlias",
        "SameFileDep.viaSameFile",
    ] {
        assert!(
            !unused.contains(&credited.to_string()),
            "{credited} is reached through a named-type property hop and must be credited \
             (issue #1785), found: {unused:?}"
        );
    }
    for control in ["OptDep.deadOnOptDep", "SameFileDep.deadOnSameFile"] {
        assert!(
            unused.contains(&control.to_string()),
            "{control} has no call site and must stay flagged (no blanket over-credit), \
             found: {unused:?}"
        );
    }
}

#[test]
fn imported_interface_typed_property_hop_credits_class_member() {
    // Issue #1785 Part B: the options interface lives in a THIRD file
    // (`export interface SharedOpts {{ c: SharedDep }}`), consumed directly
    // and through a type-only barrel re-export. The consumer-side
    // `TypedPropertyMemberAccess` fact must join through the declaring
    // module's `type_member_types` and credit `SharedDep.viaShared`, while a
    // dead method on the same class stays flagged.
    let unused = unused_member_names(fixture_path("issue-1785-imported-interface-hop"));

    assert!(
        !unused.contains(&"SharedDep.viaShared".to_string()),
        "SharedDep.viaShared is reached through an IMPORTED interface property hop and must \
         be credited (issue #1785), found: {unused:?}"
    );
    assert!(
        unused.contains(&"SharedDep.deadOnSharedDep".to_string()),
        "SharedDep.deadOnSharedDep has no call site and must stay flagged, found: {unused:?}"
    );
}
