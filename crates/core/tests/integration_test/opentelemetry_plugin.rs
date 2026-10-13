//! End-to-end test for the `OpenTelemetry` plugin against the
//! `tests/fixtures/opentelemetry-plugin/` fixture. The SDK calls the methods
//! of a registered sampler, processor, exporter, propagator or metric reader,
//! so the project code never calls them. A member that is not on the
//! interface or on the base class hooks must still be reported.

use super::common::{create_config, fixture_path};

#[test]
fn sdk_interface_methods_are_credited_but_other_members_survive() {
    let root = fixture_path("opentelemetry-plugin");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let mut unused_members: Vec<String> = results
        .unused_class_members
        .iter()
        .map(|finding| {
            format!(
                "{}.{}",
                finding.member.parent_name, finding.member.member_name
            )
        })
        .collect();
    unused_members.sort();

    assert_eq!(
        unused_members,
        vec![
            "BufferExporter.drainBuffer".to_string(),
            "PollReader.pollInterval".to_string(),
        ],
        "only the members outside the OpenTelemetry contracts may surface as unused-class-member"
    );
}
