use crate::common::{create_config, fixture_path};

/// Issue #845: a method called on a value narrowed by `if (e instanceof
/// BaseException)` is a real use of `BaseException.getMessage` and must not be
/// reported as an unused class member, while genuinely-unused members on the
/// same class keep reporting.
#[test]
fn instanceof_narrowed_method_call_credits_class_member() {
    let root = fixture_path("issue-845-instanceof-narrowing");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_members: Vec<String> = results
        .unused_class_members
        .iter()
        .map(|m| format!("{}.{}", m.member.parent_name, m.member.member_name))
        .collect();

    assert!(
        !unused_members.contains(&"BaseException.getMessage".to_string()),
        "method reached via `instanceof` narrowing must be credited: {unused_members:?}"
    );
    assert!(
        unused_members.contains(&"BaseException.unusedHelper".to_string()),
        "a genuinely-unused member on the same class must still report: {unused_members:?}"
    );
}
