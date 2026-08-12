use super::common::{create_config, fixture_path};

#[test]
fn string_enum_computed_keys_credit_exact_protocol_members() {
    let root = fixture_path("computed-enum-key-members");
    let mut config = create_config(root);
    config.rules.unused_class_members = fallow_config::Severity::Error;
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused: Vec<String> = results
        .unused_class_members
        .iter()
        .map(|finding| {
            format!(
                "{}.{}",
                finding.member.parent_name, finding.member.member_name
            )
        })
        .collect();

    assert!(
        !unused.contains(&"FirstProtocol.__protocol".to_string())
            && !unused.contains(&"SecondProtocol.__protocol".to_string()),
        "the exact string enum key is a protocol-level use on either receiver: {unused:?}"
    );
    assert!(
        unused.contains(&"FirstProtocol.dead".to_string())
            && unused.contains(&"SecondProtocol.dead".to_string())
            && unused.contains(&"NumericControl.numericOnly".to_string()),
        "unrelated and numeric-key members must remain reportable: {unused:?}"
    );
}
