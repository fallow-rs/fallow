#![expect(
    clippy::expect_used,
    reason = "tests use expect to keep fixture setup concise"
)]

#[path = "common/mod.rs"]
mod common;

use common::{parse_json, run_fallow, run_fallow_raw};

fn security_finding_json(
    finding_id: &str,
    path: &str,
    line: u32,
    kind: &str,
    category: Option<&str>,
) -> serde_json::Value {
    serde_json::json!({
        "finding_id": finding_id,
        "kind": kind,
        "category": category,
        "path": path,
        "line": line,
        "col": 0,
        "evidence": "test evidence",
        "severity": "high",
        "trace": [],
        "actions": [],
        "candidate": {
            "sink": {
                "path": path,
                "line": line,
                "col": 0,
                "category": category
            },
            "boundary": {
                "client_server": kind == "client-server-leak",
                "cross_module": false
            }
        }
    })
}

#[test]
fn security_survivors_renders_verifier_filtered_candidates() {
    let dir = tempfile::tempdir().expect("temp dir");
    let candidates = dir.path().join("candidates.json");
    let verdicts = dir.path().join("verdicts.json");
    std::fs::write(
        &candidates,
        serde_json::json!({
            "kind": "security",
            "security_findings": [
                security_finding_json("sec-a", "src/a.ts", 1, "tainted-sink", Some("ssrf")),
                security_finding_json("sec-b", "src/b.ts", 2, "tainted-sink", Some("redos-regex"))
            ]
        })
        .to_string(),
    )
    .expect("write candidates");
    std::fs::write(
        &verdicts,
        r#"[
  {
    "schema_version": "fallow-security-verdict/v1",
    "finding_id": "sec-a",
    "verdict": "survivor",
    "reason": "attacker input reaches the sink",
    "fix_direction": "restrict-url"
  },
  {
    "schema_version": "fallow-security-verdict/v1",
    "finding_id": "sec-b",
    "verdict": "dismissed"
  }
]"#,
    )
    .expect("write verdicts");

    let candidates = candidates.to_string_lossy().to_string();
    let verdicts = verdicts.to_string_lossy().to_string();
    let output = run_fallow_raw(&[
        "security",
        "survivors",
        "--candidates",
        &candidates,
        "--verdicts",
        &verdicts,
        "--format",
        "json",
    ]);

    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    let json = parse_json(&output);
    assert_eq!(json["kind"], "security-survivors");
    assert!(json["survivors"]["sec-a"].is_object());
    assert!(json["survivors"]["sec-b"].is_null());
    assert_eq!(json["survivors"]["sec-a"]["fix_direction"], "restrict-url");
}

#[test]
fn security_survivors_human_leads_with_path() {
    let dir = tempfile::tempdir().expect("temp dir");
    let candidates = dir.path().join("candidates.json");
    let verdicts = dir.path().join("verdicts.json");
    std::fs::write(
        &candidates,
        serde_json::json!({
            "security_findings": [
                security_finding_json("sec-a", "src/a.ts", 1, "tainted-sink", Some("ssrf"))
            ]
        })
        .to_string(),
    )
    .expect("write candidates");
    std::fs::write(
        &verdicts,
        r#"[{"schema_version":"fallow-security-verdict/v1","finding_id":"sec-a","verdict":"survivor"}]"#,
    )
    .expect("write verdicts");

    let candidates = candidates.to_string_lossy().to_string();
    let verdicts = verdicts.to_string_lossy().to_string();
    let output = run_fallow_raw(&[
        "security",
        "survivors",
        "--candidates",
        &candidates,
        "--verdicts",
        &verdicts,
    ]);

    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    assert!(output.stdout.contains("- src/a.ts:1 (ssrf) [sec-a]"));
}

#[test]
fn security_blind_spots_renders_grouped_json() {
    let output = run_fallow(
        "security",
        "security-tls-validation-disabled-895",
        &["blind-spots", "--format", "json", "--quiet", "--no-cache"],
    );

    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    let json = parse_json(&output);
    assert_eq!(json["kind"], "security-blind-spots");
    assert!(json["summary"].is_object());
    assert!(json["groups"].is_array());
}

#[test]
fn security_subcommand_help_reaches_subcommands() {
    let survivors = run_fallow_raw(&["security", "survivors", "--help"]);
    assert_eq!(survivors.code, 0);
    assert!(survivors.stdout.contains("--candidates"));
    assert!(survivors.stdout.contains("--verdicts"));
    assert!(!survivors.stdout.contains("sarif"));
    assert!(!survivors.stdout.contains("markdown"));
    assert!(!survivors.stdout.contains("--baseline"));

    let blind_spots = run_fallow_raw(&["security", "blind-spots", "--help"]);
    assert_eq!(blind_spots.code, 0);
    assert!(blind_spots.stdout.contains("blind-spots"));
    assert!(!blind_spots.stdout.contains("survivors"));
    assert!(!blind_spots.stdout.contains("sarif"));
    assert!(!blind_spots.stdout.contains("markdown"));
    assert!(!blind_spots.stdout.contains("--baseline"));
    assert!(!blind_spots.stdout.contains("--gate"));
}

#[test]
fn security_survivors_rejects_candidate_generation_flags() {
    let dir = tempfile::tempdir().expect("temp dir");
    let candidates = dir.path().join("candidates.json");
    let verdicts = dir.path().join("verdicts.json");
    std::fs::write(&candidates, r#"{"security_findings":[]}"#).expect("write candidates");
    std::fs::write(&verdicts, "[]").expect("write verdicts");

    let candidates = candidates.to_string_lossy().to_string();
    let verdicts = verdicts.to_string_lossy().to_string();
    let output = run_fallow_raw(&[
        "security",
        "--surface",
        "survivors",
        "--candidates",
        &candidates,
        "--verdicts",
        &verdicts,
    ]);

    assert_eq!(output.code, 2);
    assert!(output.stderr.contains("--surface is not valid"));
}

#[test]
fn security_blind_spots_rejects_gate_flags() {
    let output = run_fallow(
        "security",
        "security-tls-validation-disabled-895",
        &["--gate", "new", "blind-spots", "--format", "json"],
    );

    assert_eq!(output.code, 2);
    let json = parse_json(&output);
    assert_eq!(
        json["message"],
        "--gate is not valid with `fallow security blind-spots`."
    );
}
