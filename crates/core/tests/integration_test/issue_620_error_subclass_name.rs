use crate::common::{create_config, fixture_path};

/// Issue #620: a `name` member declared on a native `Error` subclass is
/// runtime-used (logs, serializers, `err.name === "..."` discrimination) and
/// must not be reported as an unused class member, while ordinary classes keep
/// reporting unused `name` members and error subclasses keep reporting other
/// unused members.
#[test]
fn error_subclass_name_override_is_not_flagged() {
    let root = fixture_path("issue-620-error-subclass-name");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_members: Vec<String> = results
        .unused_class_members
        .iter()
        .map(|m| format!("{}.{}", m.member.parent_name, m.member.member_name))
        .collect();

    // Direct `extends Error`.
    assert!(
        !unused_members.contains(&"DomainError.name".to_string()),
        "direct Error subclass `name` should be credited: {unused_members:?}"
    );
    // Transitive: `ApiError extends DomainError extends Error`.
    assert!(
        !unused_members.contains(&"ApiError.name".to_string()),
        "transitive Error subclass `name` should be credited: {unused_members:?}"
    );
    // Direct `extends TypeError` (native error family).
    assert!(
        !unused_members.contains(&"ValidationError.name".to_string()),
        "native error family subclass `name` should be credited: {unused_members:?}"
    );

    // Ordinary class: an unused `name` must still report.
    assert!(
        unused_members.contains(&"Person.name".to_string()),
        "ordinary class `name` must still report when unused: {unused_members:?}"
    );
    // Non-`name` member on an error subclass must still report.
    assert!(
        unused_members.contains(&"DomainError.unusedHelper".to_string()),
        "non-`name` members on error subclasses must still report: {unused_members:?}"
    );
}
