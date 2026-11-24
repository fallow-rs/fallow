use crate::common::{create_config, fixture_path};

/// Issue #5099: `JSON.stringify` calls `toJSON`, so a `toJSON` member is
/// always used. A plain `dispose` member is used when the same class declares
/// `[Symbol.dispose]` or `[Symbol.asyncDispose]`. A `dispose` member of a
/// class without those members still reports, and so do other unused members.
#[test]
fn tojson_and_dispose_protocol_members_are_used() {
    let root = fixture_path("issue-5099-tojson-dispose");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_members: Vec<String> = results
        .unused_class_members
        .iter()
        .map(|m| format!("{}.{}", m.member.parent_name, m.member.member_name))
        .collect();

    for used in ["Money.toJSON", "Lock.dispose", "AsyncLock.dispose"] {
        assert!(
            !unused_members.contains(&used.to_string()),
            "{used} should be credited: {unused_members:?}"
        );
    }
    for unused in ["Money.unusedHelper", "Handle.dispose"] {
        assert!(
            unused_members.contains(&unused.to_string()),
            "{unused} must still report: {unused_members:?}"
        );
    }
}
