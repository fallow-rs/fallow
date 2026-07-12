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
fn per_class_this_scoping_credits_both_colliding_fields_cross_module() {
    // Issue #1821 (Fix B): two classes in ONE consumer file both name their
    // receiver field `dep` (`constructor(private dep: DepPrivParam)` and
    // `readonly dep = new DepPubField()`), and a second pair collides on a
    // `#`-private `#dep`. Before per-class scoping the module-flat `this.dep` /
    // `this.#dep` binding key collided (last-write-wins), so only one class of
    // each pair credited its dep's members and the other's real member was
    // falsely reported as `unused-class-member` cross-module. Consumers live in
    // a separate file from the dep classes so the file-level self-access map
    // cannot mask the bug. Every dep class also carries a genuinely-dead control
    // member that must STAY flagged, proving the fix credits precisely per class
    // and never over-credits.
    let unused = unused_member_names(fixture_path("issue-1821-per-class-this-scoping"));

    for credited in [
        "DepPrivParam.privParam",
        "DepPubField.pubField",
        "DepHashA.hashA",
        "DepHashB.hashB",
    ] {
        assert!(
            !unused.contains(&credited.to_string()),
            "{credited} is reached through a same-named `dep` / `#dep` receiver on a \
             sibling class and must be credited per class (issue #1821 Fix B), found: {unused:?}"
        );
    }
    for control in [
        "DepPrivParam.deadOnPrivParam",
        "DepPubField.deadOnPubField",
        "DepHashA.deadOnHashA",
        "DepHashB.deadOnHashB",
    ] {
        assert!(
            unused.contains(&control.to_string()),
            "{control} has no call site and must stay flagged (per-class crediting must \
             not blanket-credit), found: {unused:?}"
        );
    }
}
