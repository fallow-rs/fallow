use super::common::{create_config, fixture_path};

/// A `"name" in value` or `Reflect.has(value, "name")` probe credits members
/// with that name on classes in the probe module and in its direct import
/// neighbors. Classes in other modules, members with other names, classes
/// whose module imports the probe module only for a type (`typed.ts`), and
/// classes that only an unreachable probe module (`orphan.ts`) imports keep
/// their report.
#[test]
fn member_name_probe_credits_neighbor_class_members() {
    let root = fixture_path("in-operator-member-probe");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_members: Vec<String> = results
        .unused_class_members
        .iter()
        .map(|m| format!("{}.{}", m.member.parent_name, m.member.member_name))
        .collect();

    for credited in [
        "RequestError.displayDetail",
        "RequestError.retryHint",
        "TimeoutError.displayDetail",
    ] {
        assert!(
            !unused_members.contains(&credited.to_string()),
            "{credited} must be credited by the probe: {unused_members:?}"
        );
    }
    for reported in [
        "RequestError.unusedRequestDetail",
        "TimeoutError.unusedTimeoutDetail",
        "UnrelatedReport.displayDetail",
        "TypedReport.displayDetail",
    ] {
        assert!(
            unused_members.contains(&reported.to_string()),
            "{reported} must stay reported: {unused_members:?}"
        );
    }
}
