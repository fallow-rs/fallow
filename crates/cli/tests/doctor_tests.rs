#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests use unwrap and expect to keep fixture setup concise"
)]

#[path = "common/mod.rs"]
mod common;

use common::{parse_json, run_fallow_raw, run_fallow_raw_with_env};

#[test]
fn zero_config_json_is_stable_and_path_free() {
    let root = tempfile::tempdir().expect("temp root");
    let root_text = root.path().to_string_lossy();
    let output = run_fallow_raw(&[
        "doctor",
        "--root",
        root_text.as_ref(),
        "--format",
        "json",
        "--quiet",
    ]);

    assert_eq!(output.code, 0, "doctor failed: {}", output.stderr);
    assert!(output.stderr.is_empty());
    assert!(!output.stdout.contains(root_text.as_ref()));
    let json = parse_json(&output);
    assert_eq!(json["kind"], "doctor");
    assert_eq!(json["schema_version"], 1);
    assert_eq!(json["root"], ".");
    assert_eq!(json["status"], "pass");
    assert_eq!(
        json["checks"]
            .as_array()
            .expect("checks array")
            .iter()
            .map(|check| check["id"].as_str().expect("check id"))
            .collect::<Vec<_>>(),
        ["root", "config", "workspaces", "plugins", "type-aware"]
    );
}

#[test]
fn invalid_config_returns_complete_failed_json_report() {
    let root = tempfile::tempdir().expect("temp root");
    std::fs::write(root.path().join(".fallowrc.json"), "{").expect("write invalid config");
    let root_text = root.path().to_string_lossy();
    let output = run_fallow_raw(&[
        "doctor",
        "--root",
        root_text.as_ref(),
        "--format",
        "json",
        "--quiet",
    ]);

    assert_eq!(output.code, 2);
    assert!(output.stderr.is_empty());
    assert!(!output.stdout.contains(root_text.as_ref()));
    let json = parse_json(&output);
    assert_eq!(json["kind"], "doctor");
    assert_eq!(json["status"], "fail");
    assert_eq!(json["checks"].as_array().map(Vec::len), Some(5));
    assert_eq!(json["checks"][1]["id"], "config");
    assert_eq!(json["checks"][1]["status"], "fail");
}

#[test]
fn human_report_is_readable_and_path_free() {
    let root = tempfile::tempdir().expect("temp root");
    let root_text = root.path().to_string_lossy();
    let output = run_fallow_raw(&["doctor", "--root", root_text.as_ref(), "--quiet"]);

    assert_eq!(output.code, 0, "doctor failed: {}", output.stderr);
    assert!(output.stdout.starts_with("Fallow doctor (.)\n"));
    assert!(output.stdout.contains("[OK] root:"));
    assert!(output.stdout.contains("Status: ready"));
    assert!(!output.stdout.contains(root_text.as_ref()));
}

#[test]
fn analysis_flags_are_rejected_instead_of_ignored() {
    let output = run_fallow_raw(&["doctor", "--changed-since", "main", "--format", "json"]);

    assert_eq!(output.code, 2);
    let json = parse_json(&output);
    assert_eq!(json["error"], true);
    assert!(
        json["message"]
            .as_str()
            .is_some_and(|message| message.contains("--changed-since"))
    );
}

#[test]
fn type_aware_environment_requirement_controls_readiness() {
    let root = tempfile::tempdir().expect("temp root");
    let root_text = root.path().to_string_lossy();
    let output = run_fallow_raw_with_env(
        &[
            "doctor",
            "--root",
            root_text.as_ref(),
            "--format",
            "json",
            "--quiet",
        ],
        &[
            ("FALLOW_TYPE_AWARE", "true"),
            ("FALLOW_TYPE_AWARE_REQUIRE", "complete"),
            (
                "FALLOW_TYPE_AWARE_BIN",
                "/definitely/missing/fallow-type-aware",
            ),
        ],
    );

    assert_eq!(output.code, 2);
    let json = parse_json(&output);
    assert_eq!(json["status"], "fail");
    assert_eq!(json["checks"][4]["id"], "type-aware");
    assert_eq!(json["checks"][4]["status"], "fail");
    assert_eq!(json["checks"][4]["required"], true);
    assert_eq!(json["checks"][4]["remediation"]["cwd"], ".");
    assert_eq!(json["checks"][4]["remediation"]["mutating"], true);
}
