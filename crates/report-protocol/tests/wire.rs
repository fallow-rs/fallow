//! Pins the exact JSON of each contract type.
//!
//! A consumer of the report contract reads these bytes. A change to a member
//! name, to the member order or to an omission rule must make this test fail.

#![expect(
    clippy::expect_used,
    reason = "tests use expect to keep serialization assertions concise"
)]

use fallow_report_protocol::{
    ErrorOutput, GateFile, GateFileReason, GateOutcome, GateStatus, REPORT_PROTOCOL_VERSION,
    exit_code,
};

fn to_json<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string(value).expect("value serializes")
}

#[test]
fn a_gate_that_compares_no_number_writes_status_and_enforced_only() {
    assert_eq!(
        to_json(&GateOutcome::new(GateStatus::Pass, true)),
        r#"{"status":"pass","enforced":true}"#
    );
}

#[test]
fn a_measured_gate_writes_observed_and_threshold() {
    assert_eq!(
        to_json(&GateOutcome::measured(GateStatus::Fail, true, 85.0, 90.0)),
        r#"{"status":"fail","enforced":true,"observed":85.0,"threshold":90.0}"#
    );
}

#[test]
fn a_counted_gate_writes_observed_and_threshold_label() {
    assert_eq!(
        to_json(&GateOutcome::counted(GateStatus::Warn, false, 3.0, "error")),
        r#"{"status":"warn","enforced":false,"observed":3.0,"threshold_label":"error"}"#
    );
}

#[test]
fn a_file_judging_gate_writes_each_file_and_counts_them() {
    let outcome = GateOutcome::with_files(
        GateStatus::Fail,
        true,
        vec![
            GateFile {
                path: "src/a.ts".to_owned(),
                error_count: 2,
                panicked: false,
                reason: None,
            },
            GateFile {
                path: "src/b.ts".to_owned(),
                error_count: 1,
                panicked: true,
                reason: Some(GateFileReason::NestingTooDeep),
            },
        ],
    );
    assert_eq!(
        to_json(&outcome),
        concat!(
            r#"{"status":"fail","enforced":true,"observed":2.0,"files":["#,
            r#"{"path":"src/a.ts","error_count":2,"panicked":false},"#,
            r#"{"path":"src/b.ts","error_count":1,"panicked":true,"reason":"nesting-too-deep"}"#,
            r#"]}"#
        )
    );
}

#[test]
fn a_skipped_gate_writes_the_kebab_case_status() {
    assert_eq!(
        to_json(&GateOutcome::new(GateStatus::Skipped, false)),
        r#"{"status":"skipped","enforced":false}"#
    );
}

#[test]
fn an_error_document_without_code_or_help_writes_three_members() {
    assert_eq!(
        to_json(&ErrorOutput::new("boom", 2)),
        r#"{"error":true,"message":"boom","exit_code":2}"#
    );
}

#[test]
fn an_error_document_with_code_and_help_writes_them_last() {
    let error = ErrorOutput::new("boom", 2)
        .with_code(Some("FALLOW_BOOM".to_owned()))
        .with_help(Some("try again".to_owned()));
    assert_eq!(
        to_json(&error),
        r#"{"error":true,"message":"boom","exit_code":2,"code":"FALLOW_BOOM","help":"try again"}"#
    );
}

#[test]
fn the_contract_constants_keep_their_values() {
    assert_eq!(REPORT_PROTOCOL_VERSION, 1);
    assert_eq!(exit_code::EXIT_OK, 0);
    assert_eq!(exit_code::EXIT_GATE_FAILED, 1);
    assert_eq!(exit_code::EXIT_INVALID_INPUT, 2);
}
