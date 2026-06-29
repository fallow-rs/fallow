use crate::common::{create_config, fixture_path};

/// Issue #1638 (GAP 2): a class `toString` invoked only through implicit string
/// coercion (template-literal interpolation, `String(...)`, or `+` with a string
/// operand) has no explicit `.toString()` call site, so it must be credited as
/// used when a `new Class()` flows directly into a coercion position. Genuine
/// dead members on the same classes, and a `toString` on a class that is
/// constructed but never coerced, must keep reporting.
#[test]
fn coerced_to_string_is_not_flagged() {
    let root = fixture_path("issue-1638-tostring-coercion");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_members: Vec<String> = results
        .unused_class_members
        .iter()
        .map(|m| format!("{}.{}", m.member.parent_name, m.member.member_name))
        .collect();

    assert!(
        !unused_members.contains(&"Money.toString".to_string()),
        "template-literal-coerced `Money.toString` must be credited: {unused_members:?}"
    );
    assert!(
        !unused_members.contains(&"Label.toString".to_string()),
        "`String(...)`-coerced `Label.toString` must be credited: {unused_members:?}"
    );
    assert!(
        !unused_members.contains(&"Tag.toString".to_string()),
        "string-concat-coerced `Tag.toString` must be credited: {unused_members:?}"
    );

    // Non-vacuous controls: genuine dead members on the credited classes must
    // still report.
    assert!(
        unused_members.contains(&"Money.deadMoney".to_string()),
        "a genuinely-unused member on a coerced class must still report: {unused_members:?}"
    );
    assert!(
        unused_members.contains(&"Label.deadLabel".to_string()),
        "a genuinely-unused member on a coerced class must still report: {unused_members:?}"
    );
    assert!(
        unused_members.contains(&"Tag.deadTag".to_string()),
        "a genuinely-unused member on a coerced class must still report: {unused_members:?}"
    );

    // Scope gate: a class constructed but never coerced keeps reporting its
    // `toString`.
    assert!(
        unused_members.contains(&"NotCoerced.toString".to_string()),
        "a `toString` on a class never used in a coercion position must still report: \
         {unused_members:?}"
    );
}
