use super::common::{create_config, fixture_path};

#[test]
fn local_of_method_call_result_credits_return_type_member() {
    // Issue #4552: `const rotated = local.rotate(angle)` with
    // `rotate(angle: number): Point` must credit the members that a later
    // read on `rotated` reaches. The receiver can be a parameter typed
    // `Point` or a `new Point()` local, and the init can be the call or a
    // conditional with the call in one branch. An optional call
    // (`local?.rotate(angle)`) counts as the call. `type-only.ts` imports `Point`
    // with `import type`. A member with no access stays flagged.
    let root = fixture_path("issue-4552-method-call-result-local");
    let mut config = create_config(root);
    config.rules.unused_class_members = fallow_config::Severity::Error;
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused: Vec<String> = results
        .unused_class_members
        .iter()
        .map(|member| {
            format!(
                "{}.{}",
                member.member.parent_name, member.member.member_name
            )
        })
        .collect();

    for used in [
        "Point.translate",
        "Point.scale",
        "Point.mirror",
        "Point.flip",
        "Point.shift",
        "Point.skew",
        "Point.nudge",
        "Point.tilt",
    ] {
        assert!(
            !unused.contains(&used.to_string()),
            "{used} is read on a local of a method call result, found: {unused:?}"
        );
    }
    assert!(
        unused.contains(&"Point.unused".to_string()),
        "Point.unused has no access and must stay flagged, found: {unused:?}"
    );
}
