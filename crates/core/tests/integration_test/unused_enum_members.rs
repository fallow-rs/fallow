use super::common::{create_config, fixture_path};

#[test]
fn unused_enum_members_detected_by_access_pattern() {
    let root = fixture_path("unused-enum-members");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_enum_member_names: Vec<&str> = results
        .unused_enum_members
        .iter()
        .map(|m| m.member.member_name.as_str())
        .collect();

    // Color: only Red is accessed, Green and Blue should be unused
    assert!(
        !unused_enum_member_names.contains(&"Red"),
        "Red should NOT be unused (accessed via Color.Red)"
    );
    assert!(
        unused_enum_member_names.contains(&"Green"),
        "Green should be unused (not accessed), found: {unused_enum_member_names:?}"
    );
    assert!(
        unused_enum_member_names.contains(&"Blue"),
        "Blue should be unused (not accessed), found: {unused_enum_member_names:?}"
    );
}

#[test]
fn partially_used_enum_members() {
    let root = fixture_path("unused-enum-members");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_enum_member_names: Vec<&str> = results
        .unused_enum_members
        .iter()
        .map(|m| m.member.member_name.as_str())
        .collect();

    // HttpStatus: Ok and NotFound are accessed, InternalError and BadGateway are unused
    assert!(
        !unused_enum_member_names.contains(&"Ok"),
        "Ok should NOT be unused (accessed via HttpStatus.Ok)"
    );
    assert!(
        !unused_enum_member_names.contains(&"NotFound"),
        "NotFound should NOT be unused (accessed via HttpStatus.NotFound)"
    );
    assert!(
        unused_enum_member_names.contains(&"InternalError"),
        "InternalError should be unused, found: {unused_enum_member_names:?}"
    );
    assert!(
        unused_enum_member_names.contains(&"BadGateway"),
        "BadGateway should be unused, found: {unused_enum_member_names:?}"
    );
}

#[test]
fn whole_object_use_suppresses_enum_members() {
    let root = fixture_path("unused-enum-members");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_enum_member_names: Vec<&str> = results
        .unused_enum_members
        .iter()
        .map(|m| m.member.member_name.as_str())
        .collect();

    // LogLevel is used via Object.values — all members should be considered used
    assert!(
        !unused_enum_member_names.contains(&"Debug"),
        "Debug should NOT be unused (Object.values), found: {unused_enum_member_names:?}"
    );
    assert!(
        !unused_enum_member_names.contains(&"Info"),
        "Info should NOT be unused (Object.values), found: {unused_enum_member_names:?}"
    );
    assert!(
        !unused_enum_member_names.contains(&"Warn"),
        "Warn should NOT be unused (Object.values), found: {unused_enum_member_names:?}"
    );
    assert!(
        !unused_enum_member_names.contains(&"Error"),
        "Error should NOT be unused (Object.values), found: {unused_enum_member_names:?}"
    );
}
