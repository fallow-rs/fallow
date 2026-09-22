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

fn write_empty_survivor_inputs(dir: &tempfile::TempDir) -> (String, String) {
    let candidates = dir.path().join("candidates.json");
    let verdicts = dir.path().join("verdicts.json");
    std::fs::write(&candidates, r#"{"security_findings":[]}"#).expect("write candidates");
    std::fs::write(&verdicts, "[]").expect("write verdicts");
    (
        candidates.to_string_lossy().to_string(),
        verdicts.to_string_lossy().to_string(),
    )
}

#[test]
fn security_same_line_sinks_keep_distinct_ids_across_formats_and_verdicts() {
    let output = run_fallow(
        "security",
        "security-same-line-sinks",
        &["--format", "json"],
    );
    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    let json = parse_json(&output);
    let findings = json["security_findings"]
        .as_array()
        .expect("findings array");
    assert_eq!(findings.len(), 2);
    assert_eq!(findings[0]["line"], findings[1]["line"]);
    assert_ne!(findings[0]["col"], findings[1]["col"]);
    assert_ne!(findings[0]["finding_id"], findings[1]["finding_id"]);

    let scoped = run_fallow(
        "security",
        "security-same-line-sinks",
        &["--format", "json", "--file", "index.ts"],
    );
    assert_eq!(scoped.code, 0, "stderr: {}", scoped.stderr);
    assert_eq!(
        parse_json(&scoped)["security_findings"],
        json["security_findings"]
    );

    let sarif_output = run_fallow(
        "security",
        "security-same-line-sinks",
        &["--format", "sarif"],
    );
    assert_eq!(sarif_output.code, 0, "stderr: {}", sarif_output.stderr);
    let sarif = parse_json(&sarif_output);
    let results = sarif["runs"][0]["results"]
        .as_array()
        .expect("SARIF results");
    assert_eq!(results.len(), findings.len());
    for finding in findings {
        let column = finding["col"].as_u64().expect("finding column") + 1;
        let result = results
            .iter()
            .find(|result| {
                result["locations"][0]["physicalLocation"]["region"]["startColumn"] == column
            })
            .expect("SARIF result for column");
        assert_eq!(
            result["partialFingerprints"]["fallowSecurity/v2"],
            finding["finding_id"]
        );
        assert!(
            result["partialFingerprints"]
                .get("fallowSecurity/v1")
                .is_none()
        );
    }

    let dir = tempfile::tempdir().expect("temp dir");
    let candidates = dir.path().join("candidates.json");
    let verdicts = dir.path().join("verdicts.json");
    std::fs::write(&candidates, &output.stdout).expect("write candidates");
    std::fs::write(
        &verdicts,
        serde_json::json!([
            {"schema_version": "fallow-security-verdict/v1", "finding_id": findings[0]["finding_id"], "verdict": "survivor"},
            {"schema_version": "fallow-security-verdict/v1", "finding_id": findings[1]["finding_id"], "verdict": "dismissed"}
        ]).to_string(),
    ).expect("write independent verdicts");
    let survivors = run_fallow_raw(&[
        "security",
        "survivors",
        "--candidates",
        &candidates.to_string_lossy(),
        "--verdicts",
        &verdicts.to_string_lossy(),
        "--require-verdict-for-each-candidate",
        "--format",
        "json",
    ]);
    assert_eq!(survivors.code, 0, "stderr: {}", survivors.stderr);
    let survivors = parse_json(&survivors);
    assert_eq!(survivors["summary"]["unverdicted"], 0);
    let retained = survivors["survivors"].as_object().expect("survivor map");
    assert_eq!(retained.len(), 1);
    assert!(retained.contains_key(findings[0]["finding_id"].as_str().expect("finding id")));
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
    assert_eq!(json["summary"]["unverdicted"], 0);
    assert!(json["survivors"]["sec-a"].is_object());
    assert!(json["survivors"]["sec-b"].is_null());
    assert_eq!(json["survivors"]["sec-a"]["fix_direction"], "restrict-url");
}

#[test]
fn security_survivors_reports_unreviewed_candidates() {
    let dir = tempfile::tempdir().expect("temp dir");
    let candidates = dir.path().join("candidates.json");
    let verdicts = dir.path().join("verdicts.json");
    std::fs::write(
        &candidates,
        serde_json::json!({
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
        r#"[{"schema_version":"fallow-security-verdict/v1","finding_id":"sec-a","verdict":"survivor"}]"#,
    )
    .expect("write verdicts");

    let candidates = candidates.to_string_lossy().to_string();
    let verdicts = verdicts.to_string_lossy().to_string();
    let json_output = run_fallow_raw(&[
        "security",
        "survivors",
        "--candidates",
        &candidates,
        "--verdicts",
        &verdicts,
        "--format",
        "json",
    ]);

    assert_eq!(json_output.code, 0, "stderr: {}", json_output.stderr);
    let json = parse_json(&json_output);
    assert_eq!(json["summary"]["unverdicted"], 1);

    let human_output = run_fallow_raw(&[
        "security",
        "survivors",
        "--candidates",
        &candidates,
        "--verdicts",
        &verdicts,
    ]);

    assert_eq!(human_output.code, 0, "stderr: {}", human_output.stderr);
    assert!(
        human_output
            .stdout
            .contains("Unreviewed candidates: 1 candidate.")
    );
}

#[test]
fn security_survivors_strict_gate_rejects_unreviewed_candidates() {
    let dir = tempfile::tempdir().expect("temp dir");
    let candidates = dir.path().join("candidates.json");
    let verdicts = dir.path().join("verdicts.json");
    std::fs::write(
        &candidates,
        serde_json::json!({
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
        r#"[{"schema_version":"fallow-security-verdict/v1","finding_id":"sec-a","verdict":"dismissed"}]"#,
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
        "--require-verdict-for-each-candidate",
        "--format",
        "json",
    ]);

    assert_eq!(output.code, 2);
    assert!(output.stderr.is_empty());
    let json = parse_json(&output);
    assert_eq!(json["error"], true);
    assert!(
        json["message"]
            .as_str()
            .is_some_and(|message| message.contains("missing verdicts for 1 candidate"))
    );
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
fn security_blind_spots_accepts_subcommand_file_scope() {
    let output = run_fallow(
        "security",
        "security-tls-validation-disabled-895",
        &[
            "blind-spots",
            "--file",
            "src/a.ts",
            "--format",
            "json",
            "--quiet",
            "--no-cache",
        ],
    );

    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    let json = parse_json(&output);
    assert_eq!(json["kind"], "security-blind-spots");
}

#[test]
fn security_blind_spots_merges_parent_and_subcommand_file_scope() {
    let output = run_fallow(
        "security",
        "security-tls-validation-disabled-895",
        &[
            "--file",
            "src/a.ts",
            "blind-spots",
            "--file",
            "src/b.ts",
            "--format",
            "json",
            "--quiet",
            "--no-cache",
        ],
    );

    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    let json = parse_json(&output);
    assert_eq!(json["kind"], "security-blind-spots");
}

#[test]
fn security_subcommand_help_reaches_subcommands() {
    let survivors = run_fallow_raw(&["security", "survivors", "--help"]);
    assert_eq!(survivors.code, 0);
    assert!(survivors.stdout.contains("--candidates"));
    assert!(survivors.stdout.contains("--verdicts"));
    assert!(
        survivors
            .stdout
            .contains("--require-verdict-for-each-candidate")
    );
    assert!(survivors.stdout.contains("fallow-security-verdict/v1"));
    assert!(
        survivors
            .stdout
            .contains("Repo-local docs: docs/security-agent-verification.md")
    );
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
    assert!(blind_spots.stdout.contains("--file <PATH>"));
}

#[test]
fn security_survivors_rejects_candidate_generation_flags() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (candidates, verdicts) = write_empty_survivor_inputs(&dir);
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
fn security_survivors_rejects_hidden_parent_flags() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (candidates, verdicts) = write_empty_survivor_inputs(&dir);
    let sarif = dir.path().join("out.sarif").to_string_lossy().to_string();
    let output = run_fallow_raw(&[
        "security",
        "survivors",
        "--candidates",
        &candidates,
        "--verdicts",
        &verdicts,
        "--sarif-file",
        &sarif,
        "--format",
        "json",
    ]);

    assert_eq!(output.code, 2);
    let json = parse_json(&output);
    assert_eq!(
        json["message"],
        "--sarif-file is not valid with `fallow security survivors`."
    );

    let output = run_fallow_raw(&[
        "security",
        "survivors",
        "--candidates",
        &candidates,
        "--verdicts",
        &verdicts,
        "--fail-on-issues",
    ]);

    assert_eq!(output.code, 2);
    assert!(
        output
            .stderr
            .contains("--fail-on-issues is not valid with `fallow security survivors`.")
    );
}

#[test]
fn security_survivors_rejects_unsupported_parent_flags() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (candidates, verdicts) = write_empty_survivor_inputs(&dir);
    let sarif = dir.path().join("out.sarif").to_string_lossy().to_string();
    let cases: Vec<(Vec<&str>, &str)> = vec![
        (vec!["--ci"], "--ci"),
        (vec!["--fail-on-issues"], "--fail-on-issues"),
        (vec!["--sarif-file", &sarif], "--sarif-file"),
        (vec!["--summary"], "--summary"),
        (vec!["--explain"], "--explain"),
        (
            vec!["--runtime-coverage", "/tmp/fallow-missing-runtime.json"],
            "--runtime-coverage",
        ),
        (vec!["--min-invocations-hot", "1"], "--min-invocations-hot"),
        (vec!["--file", "src/a.ts"], "--file"),
        (vec!["--gate", "new"], "--gate"),
        (vec!["--surface"], "--surface"),
    ];

    for (flag_args, expected_flag) in cases {
        let mut args = vec!["security"];
        args.extend(flag_args);
        args.extend([
            "survivors",
            "--candidates",
            &candidates,
            "--verdicts",
            &verdicts,
            "--format",
            "json",
        ]);
        let output = run_fallow_raw(&args);

        assert_eq!(output.code, 2, "flag {expected_flag}");
        let json = parse_json(&output);
        assert_eq!(
            json["message"],
            format!("{expected_flag} is not valid with `fallow security survivors`.")
        );
    }
}

#[test]
fn security_survivors_rejects_non_array_verdict_wrapper() {
    let dir = tempfile::tempdir().expect("temp dir");
    let candidates = dir.path().join("candidates.json");
    let verdicts = dir.path().join("verdicts.json");
    std::fs::write(&candidates, r#"{"security_findings":[]}"#).expect("write candidates");
    std::fs::write(
        &verdicts,
        r#"{"schema_version":"fallow-security-verdicts/v1","verdicts":{}}"#,
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

    assert_eq!(output.code, 2);
    let json = parse_json(&output);
    assert!(
        json["message"]
            .as_str()
            .is_some_and(|message| message.contains("must contain a verdicts array"))
    );
}

#[test]
fn security_survivors_parse_errors_honor_json_format() {
    let output = run_fallow_raw(&[
        "security",
        "survivors",
        "--candidates",
        "/tmp/fallow-missing-candidates.json",
        "--format",
        "json",
    ]);

    assert_eq!(output.code, 2);
    assert!(output.stderr.is_empty());
    let json = parse_json(&output);
    assert_eq!(json["error"], true);
    assert_eq!(json["exit_code"], 2);
    assert!(
        json["message"]
            .as_str()
            .is_some_and(|message| message.contains("--verdicts <PATH>"))
    );
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

#[test]
fn security_blind_spots_rejects_hidden_parent_flags() {
    let output = run_fallow(
        "security",
        "security-tls-validation-disabled-895",
        &[
            "--runtime-coverage",
            "/tmp/fallow-missing-runtime.json",
            "blind-spots",
            "--format",
            "json",
        ],
    );

    assert_eq!(output.code, 2);
    let json = parse_json(&output);
    assert_eq!(
        json["message"],
        "--runtime-coverage is not valid with `fallow security blind-spots`."
    );

    let output = run_fallow(
        "security",
        "security-tls-validation-disabled-895",
        &[
            "--sarif-file",
            "/tmp/fallow-blind-spots.sarif",
            "blind-spots",
            "--format",
            "json",
        ],
    );

    assert_eq!(output.code, 2);
    let json = parse_json(&output);
    assert_eq!(
        json["message"],
        "--sarif-file is not valid with `fallow security blind-spots`."
    );
}

#[test]
fn security_blind_spots_rejects_unsupported_parent_flags() {
    let cases: Vec<(Vec<&str>, &str)> = vec![
        (vec!["--ci"], "--ci"),
        (vec!["--fail-on-issues"], "--fail-on-issues"),
        (
            vec!["--sarif-file", "/tmp/fallow-blind-spots.sarif"],
            "--sarif-file",
        ),
        (vec!["--summary"], "--summary"),
        (vec!["--explain"], "--explain"),
        (
            vec!["--runtime-coverage", "/tmp/fallow-missing-runtime.json"],
            "--runtime-coverage",
        ),
        (vec!["--min-invocations-hot", "1"], "--min-invocations-hot"),
        (vec!["--gate", "new"], "--gate"),
        (vec!["--surface"], "--surface"),
    ];

    for (flag_args, expected_flag) in cases {
        let mut args = flag_args;
        args.extend(["blind-spots", "--format", "json"]);
        let output = run_fallow("security", "security-tls-validation-disabled-895", &args);

        assert_eq!(output.code, 2, "flag {expected_flag}");
        let json = parse_json(&output);
        assert_eq!(
            json["message"],
            format!("{expected_flag} is not valid with `fallow security blind-spots`.")
        );
    }
}
