//! Issue #4724: a module reads `holder[name].prototype.member` on an object
//! holder that another module exports. The member pass follows one hop from
//! the holder export to the class under each key that the read can select.

use super::common::{create_config, fixture_path};

fn unused_member_names() -> Vec<String> {
    let root = fixture_path("issue-4724-cross-file-holder-prototype-reads");
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
fn computed_key_read_credits_each_class_of_an_imported_default_holder() {
    let members = unused_member_names();
    for used in [
        "WebSocketTransport.handlesUpgrades",
        "PollingTransport.handlesUpgrades",
    ] {
        assert!(
            !members.iter().any(|m| m == used),
            "{used} is read through transports[name].prototype. Got: {members:?}"
        );
    }
    for unused in [
        "WebSocketTransport.unusedWebSocketHelper",
        "PollingTransport.unusedPollingHelper",
    ] {
        assert!(
            members.iter().any(|m| m == unused),
            "{unused} is not read, so it stays unused. Got: {members:?}"
        );
    }
}

#[test]
fn static_and_string_keys_credit_one_class_behind_a_re_export() {
    let members = unused_member_names();
    for used in [
        "MsgpackCodec.binary",
        "MsgpackCodec.mimeType",
        "JsonCodec.binary",
    ] {
        assert!(
            !members.iter().any(|m| m == used),
            "{used} is read through its key of the re-exported holder. Got: {members:?}"
        );
    }
    for unused in ["MsgpackCodec.name", "JsonCodec.name"] {
        assert!(
            members.iter().any(|m| m == unused),
            "{unused} is not read, so it stays unused. Got: {members:?}"
        );
    }
    assert!(
        members.iter().any(|m| m == "JsonCodec.mimeType"),
        "the static key msgpack selects only MsgpackCodec. Got: {members:?}"
    );
}

#[test]
fn computed_key_read_of_an_exported_holder_in_the_same_file() {
    let members = unused_member_names();
    for used in ["StrictParser.strict", "LooseParser.strict"] {
        assert!(
            !members.iter().any(|m| m == used),
            "{used} is read through parsers[name].prototype. Got: {members:?}"
        );
    }
    assert!(
        members
            .iter()
            .any(|m| m == "StrictParser.unusedStrictHelper"),
        "unusedStrictHelper is not read, so it stays unused. Got: {members:?}"
    );
}

#[test]
fn holder_value_through_a_namespace_import_is_credited() {
    let members = unused_member_names();
    assert!(
        !members.iter().any(|m| m == "CsvFormat.delimiter"),
        "the holder value all.CsvFormat is read through formats.csv.prototype. Got: {members:?}"
    );
    assert!(
        members.iter().any(|m| m == "CsvFormat.unusedCsvHelper"),
        "unusedCsvHelper is not read, so it stays unused. Got: {members:?}"
    );
}
