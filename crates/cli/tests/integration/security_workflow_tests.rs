#![expect(
    clippy::expect_used,
    reason = "tests use expect to keep fixture setup concise"
)]

use crate::common::{CommandOutput, parse_json, run_fallow, run_fallow_in_root, run_fallow_raw};

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

const LINE_MOVE_PACKAGE_JSON: &str =
    r#"{ "name": "fid-repro", "private": true, "type": "module", "main": "src/index.js" }"#;
const LINE_MOVE_SOURCE: &str = "import { exec } from 'node:child_process'\n\nexport function run(name) {\n\texec(`git log ${name}`)\n}\n";

fn write_line_move_project(root: &std::path::Path, source: &str) {
    std::fs::create_dir_all(root.join("src")).expect("create src");
    std::fs::write(root.join("package.json"), LINE_MOVE_PACKAGE_JSON).expect("write package.json");
    std::fs::write(root.join("src/index.js"), source).expect("write source");
}

fn security_findings_in(root: &std::path::Path) -> (String, Vec<serde_json::Value>) {
    let output = run_fallow_in_root(
        "security",
        root,
        &["--no-cache", "--format", "json", "--quiet"],
    );
    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    let findings = parse_json(&output)["security_findings"]
        .as_array()
        .expect("findings array")
        .clone();
    (output.stdout, findings)
}

fn stable_id(finding: &serde_json::Value) -> String {
    finding["stable_finding_id"]
        .as_str()
        .expect("stable_finding_id")
        .to_owned()
}

fn run_survivors(dir: &std::path::Path, candidates: &str, verdict_ids: &[&str]) -> CommandOutput {
    let candidates_path = dir.join("candidates.json");
    let verdicts_path = dir.join("verdicts.json");
    std::fs::write(&candidates_path, candidates).expect("write candidates");
    let verdicts: Vec<serde_json::Value> = verdict_ids
        .iter()
        .map(|id| {
            serde_json::json!({
                "schema_version": "fallow-security-verdict/v1",
                "finding_id": id,
                "verdict": "dismissed"
            })
        })
        .collect();
    std::fs::write(
        &verdicts_path,
        serde_json::Value::from(verdicts).to_string(),
    )
    .expect("write verdicts");
    run_fallow_raw(&[
        "security",
        "survivors",
        "--candidates",
        &candidates_path.to_string_lossy(),
        "--verdicts",
        &verdicts_path.to_string_lossy(),
        "--format",
        "json",
    ])
}

/// Issue #4429: a comment line above the function moves the sink one line
/// down. A verdict keyed by the stable id must still match the candidate.
#[test]
fn security_stable_finding_id_survives_an_unrelated_line_move() {
    let project = tempfile::tempdir().expect("project dir");
    write_line_move_project(project.path(), LINE_MOVE_SOURCE);
    let (_, before) = security_findings_in(project.path());
    assert_eq!(before.len(), 1);

    write_line_move_project(
        project.path(),
        &format!("// unrelated comment\n{LINE_MOVE_SOURCE}"),
    );
    let (after_json, after) = security_findings_in(project.path());
    assert_eq!(after.len(), 1);
    assert_ne!(before[0]["line"], after[0]["line"]);
    assert_ne!(before[0]["finding_id"], after[0]["finding_id"]);
    assert_eq!(stable_id(&before[0]), stable_id(&after[0]));

    let work = tempfile::tempdir().expect("work dir");
    let survivors = run_survivors(work.path(), &after_json, &[&stable_id(&before[0])]);
    assert_eq!(survivors.code, 0, "stderr: {}", survivors.stderr);
    let survivors = parse_json(&survivors);
    assert_eq!(survivors["summary"]["dismissed"], 1);
    assert_eq!(survivors["summary"]["unverdicted"], 0);

    // A verdict keyed by the location id keeps working while the line stays.
    let by_location = run_survivors(
        work.path(),
        &after_json,
        &[after[0]["finding_id"].as_str().expect("finding id")],
    );
    assert_eq!(by_location.code, 0, "stderr: {}", by_location.stderr);
}

#[test]
fn security_stable_finding_id_separates_identical_sinks_and_tracks_sink_text() {
    let project = tempfile::tempdir().expect("project dir");
    let twin = "import { exec } from 'node:child_process'\n\nexport function run(name) {\n\texec(`git log ${name}`)\n\texec(`git log ${name}`)\n}\n";
    write_line_move_project(project.path(), twin);
    let (twin_json, twins) = security_findings_in(project.path());
    assert_eq!(twins.len(), 2);
    let first = stable_id(&twins[0]);
    let second = stable_id(&twins[1]);
    assert_ne!(first, second);
    assert_eq!(second, format!("{first}~1"));

    // One verdict for each id: both resolve, and naming one candidate twice
    // (by its location id and its stable id) is a duplicate.
    let work = tempfile::tempdir().expect("work dir");
    let both = run_survivors(work.path(), &twin_json, &[&first, &second]);
    assert_eq!(both.code, 0, "stderr: {}", both.stderr);
    assert_eq!(parse_json(&both)["summary"]["unverdicted"], 0);
    let duplicate = run_survivors(
        work.path(),
        &twin_json,
        &[&first, twins[0]["finding_id"].as_str().expect("finding id")],
    );
    assert_eq!(duplicate.code, 2);
    assert!(
        duplicate.stdout.contains("duplicate verdict"),
        "stdout: {}",
        duplicate.stdout
    );

    write_line_move_project(
        project.path(),
        &LINE_MOVE_SOURCE.replace("git log", "git show"),
    );
    let (_, changed) = security_findings_in(project.path());
    assert_eq!(changed.len(), 1);
    assert_ne!(stable_id(&changed[0]), first);
}

/// Two assignment sinks with the same target and different values must get
/// different base ids. A verdict for a deleted sink then fails with exit 2,
/// and does not move to the other sink.
#[test]
fn security_stable_finding_id_hashes_the_assigned_value() {
    let project = tempfile::tempdir().expect("project dir");
    let two_sinks = "export function render(safe, html) {\n\tdocument.getElementById('a').innerHTML = safe;\n\tdocument.getElementById('a').innerHTML =\n\t\thtml;\n}\n";
    write_line_move_project(project.path(), two_sinks);
    let (_, both) = security_findings_in(project.path());
    assert_eq!(both.len(), 2);
    let safe_id = stable_id(&both[0]);
    let html_id = stable_id(&both[1]);
    assert!(!safe_id.contains('~'), "safe id: {safe_id}");
    assert!(!html_id.contains('~'), "html id: {html_id}");

    // A reformat of the wrapped assignment keeps its id.
    write_line_move_project(
        project.path(),
        &two_sinks.replace("innerHTML =\n\t\thtml", "innerHTML = html"),
    );
    let (_, reformatted) = security_findings_in(project.path());
    assert_eq!(stable_id(&reformatted[1]), html_id);

    write_line_move_project(
        project.path(),
        &two_sinks.replace("\tdocument.getElementById('a').innerHTML = safe;\n", ""),
    );
    let (only_html_json, only_html) = security_findings_in(project.path());
    assert_eq!(only_html.len(), 1);
    assert_eq!(stable_id(&only_html[0]), html_id);

    let work = tempfile::tempdir().expect("work dir");
    let stale = run_survivors(work.path(), &only_html_json, &[&safe_id]);
    assert_eq!(stale.code, 2, "stdout: {}", stale.stdout);
}
