//! `fallow health --mutation-report`: the mutation score of a saved report
//! next to the CRAP score of each complexity finding.

use std::fs;
use std::path::Path;

use sha2::{Digest, Sha256};

use super::common::{CommandOutput, commit_all, git, parse_json, run_fallow_in_root};

const CLAMP: &str = "export function clamp(n: number, lo: number, hi: number): number {\n  if (n < lo) {\n    return lo;\n  }\n  if (n > hi) {\n    return hi;\n  }\n  return n;\n}\n";

fn sha256_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    Sha256::digest(bytes)
        .iter()
        .fold(String::with_capacity(64), |mut hex, byte| {
            let _ = write!(hex, "{byte:02x}");
            hex
        })
}

/// A git project with one source file and no test file.
fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    fs::write(
        root.join("package.json"),
        r#"{"name":"health-mutation-fixture","private":true,"type":"module","main":"src/clamp.ts"}"#,
    )
    .expect("package.json");
    fs::create_dir_all(root.join("src")).expect("src");
    fs::write(root.join("src/clamp.ts"), CLAMP).expect("clamp.ts");
    fs::write(root.join(".gitignore"), "node_modules\n.fallow\n").expect(".gitignore");
    git(root, &["init", "-q"]);
    commit_all(root, "init");
    dir
}

/// A saved `fallow mutation` report with one function result for `clamp`.
fn write_report(path: &Path, digest: &str) {
    let report = serde_json::json!({
        "kind": "mutation",
        "schema_version": "1",
        "completion": { "status": "complete", "reasons": [] },
        "mutants": [],
        "functions": [{
            "file": "src/clamp.ts",
            "name": "clamp",
            "line": 1,
            "end_line": 9,
            "in_diff": false,
            "cyclomatic": 3,
            "counts": {
                "total": 5, "killed": 3, "timeout": 0, "survived": 1,
                "no_coverage": 1, "runtime_error": 0, "flaky": 0,
                "pruned": 0, "pending": 0, "equivalent": 0
            },
            "score": 0.75,
            "survived": ["aaaaaaaaaaaaaaaa"]
        }],
        "files": [{ "path": "src/clamp.ts", "source_sha256": digest, "static_test_files": [] }]
    });
    fs::write(path, serde_json::to_vec_pretty(&report).unwrap()).expect("report");
}

fn health_json(root: &Path, extra: &[&str]) -> (CommandOutput, serde_json::Value) {
    let mut args = vec!["--format", "json", "--quiet", "--max-cyclomatic", "1"];
    args.extend_from_slice(extra);
    let output = run_fallow_in_root("health", root, &args);
    let value = parse_json(&output);
    (output, value)
}

fn clamp_finding(value: &serde_json::Value) -> &serde_json::Value {
    value["findings"]
        .as_array()
        .expect("findings")
        .iter()
        .find(|finding| finding["name"] == "clamp")
        .unwrap_or_else(|| panic!("no clamp finding: {value}"))
}

#[test]
fn a_fresh_report_adds_the_mutation_score_and_keeps_crap_and_the_health_score() {
    let project = project();
    let reports = tempfile::tempdir().expect("reports dir");
    let report = reports.path().join("mutation.json");
    write_report(&report, &sha256_hex(CLAMP.as_bytes()));

    let (plain_output, plain) = health_json(project.path(), &[]);
    let (output, value) = health_json(
        project.path(),
        &["--mutation-report", report.to_str().unwrap()],
    );
    assert_eq!(output.code, plain_output.code, "{}", output.stderr);

    let finding = clamp_finding(&value);
    assert_eq!(finding["mutation"]["state"], "fresh", "{finding}");
    assert_eq!(finding["mutation"]["score"], 0.75, "{finding}");
    assert_eq!(finding["mutation"]["counts"]["killed"], 3, "{finding}");
    assert_eq!(finding["mutation"]["counts"]["survived"], 1, "{finding}");
    assert_eq!(finding["crap"], clamp_finding(&plain)["crap"]);
    assert!(clamp_finding(&plain).get("mutation").is_none());

    assert_eq!(value["health_score"], plain["health_score"]);
    assert_eq!(value["vital_signs"], plain["vital_signs"]);
    assert_eq!(value["mutation"]["completion"], "complete", "{value}");
    assert_eq!(value["mutation"]["findings_fresh"], 1, "{value}");
    assert_eq!(value["mutation"]["findings_stale"], 0, "{value}");
    assert!(plain.get("mutation").is_none());
}

#[test]
fn a_changed_source_file_is_stale_and_reports_no_score() {
    let project = project();
    let reports = tempfile::tempdir().expect("reports dir");
    let report = reports.path().join("mutation.json");
    write_report(&report, &sha256_hex(b"an older version of the file"));

    let (output, value) = health_json(
        project.path(),
        &["--mutation-report", report.to_str().unwrap()],
    );
    assert!(output.code <= 1, "{}", output.stderr);
    let finding = clamp_finding(&value);
    assert_eq!(finding["mutation"]["state"], "stale", "{finding}");
    assert_eq!(finding["mutation"]["stale_reason"], "source-changed");
    assert!(finding["mutation"].get("score").is_none(), "{finding}");
    assert!(finding["mutation"].get("counts").is_none(), "{finding}");
    assert_eq!(value["mutation"]["findings_stale"], 1, "{value}");
    assert_eq!(
        value["mutation"]["stale_files"][0]["path"], "src/clamp.ts",
        "{value}"
    );
}

#[test]
fn a_stale_report_prints_no_score_in_the_human_output() {
    let project = project();
    let reports = tempfile::tempdir().expect("reports dir");
    let report = reports.path().join("mutation.json");
    write_report(&report, &sha256_hex(b"an older version of the file"));

    let output = run_fallow_in_root(
        "health",
        project.path(),
        &[
            "--quiet",
            "--complexity",
            "--max-cyclomatic",
            "1",
            "--mutation-report",
            report.to_str().unwrap(),
        ],
    );
    assert!(
        output.stdout.contains("mutation score: stale"),
        "{}",
        output.stdout
    );
    assert!(!output.stdout.contains("75%"), "{}", output.stdout);
    assert!(
        output.stdout.contains("1 stale file has no score"),
        "{}",
        output.stdout
    );
}

#[test]
fn a_fresh_report_prints_the_score_under_crap_in_the_human_output() {
    let project = project();
    let reports = tempfile::tempdir().expect("reports dir");
    let report = reports.path().join("mutation.json");
    write_report(&report, &sha256_hex(CLAMP.as_bytes()));

    let output = run_fallow_in_root(
        "health",
        project.path(),
        &[
            "--quiet",
            "--complexity",
            "--max-cyclomatic",
            "1",
            "--mutation-report",
            report.to_str().unwrap(),
        ],
    );
    assert!(
        output
            .stdout
            .contains("75% mutation score (3 killed, 1 survived, 1 no coverage)"),
        "{}",
        output.stdout
    );
}

#[test]
fn a_plan_or_a_missing_file_exits_2() {
    let project = project();
    let reports = tempfile::tempdir().expect("reports dir");
    let plan = reports.path().join("plan.json");
    fs::write(&plan, r#"{"kind":"mutation-plan","mutants":[],"files":[]}"#).expect("plan");

    let (output, value) = health_json(
        project.path(),
        &["--mutation-report", plan.to_str().unwrap()],
    );
    assert_eq!(output.code, 2, "{}", output.stdout);
    assert_eq!(value["code"], "FALLOW_MUTATION_REPORT_INVALID", "{value}");

    let missing = reports.path().join("missing.json");
    let (output, value) = health_json(
        project.path(),
        &["--mutation-report", missing.to_str().unwrap()],
    );
    assert_eq!(output.code, 2, "{}", output.stdout);
    assert_eq!(value["code"], "FALLOW_MUTATION_REPORT_INVALID", "{value}");
}

#[test]
fn a_report_from_a_real_run_joins_by_function() {
    let project = project();
    let reports = tempfile::tempdir().expect("reports dir");
    let report = reports.path().join("mutation.json");
    let run = run_fallow_in_root(
        "mutation",
        project.path(),
        &[
            "--all",
            "--format",
            "json",
            "--quiet",
            "-o",
            report.to_str().unwrap(),
        ],
    );
    assert_eq!(run.code, 0, "{}", run.stderr);

    let (output, value) = health_json(
        project.path(),
        &["--mutation-report", report.to_str().unwrap()],
    );
    assert!(output.code <= 1, "{}", output.stderr);
    let finding = clamp_finding(&value);
    // The project has no test file: each mutant is `no_coverage`, so the
    // function has counts and no score.
    assert_eq!(finding["mutation"]["state"], "fresh", "{finding}");
    assert!(finding["mutation"].get("score").is_none(), "{finding}");
    assert!(
        finding["mutation"]["counts"]["no_coverage"]
            .as_u64()
            .unwrap()
            > 0,
        "{finding}"
    );
}

#[test]
fn group_by_findings_get_the_same_evidence() {
    let project = project();
    let reports = tempfile::tempdir().expect("reports dir");
    let report = reports.path().join("mutation.json");
    write_report(&report, &sha256_hex(CLAMP.as_bytes()));

    let (output, value) = health_json(
        project.path(),
        &[
            "--complexity",
            "--group-by",
            "directory",
            "--mutation-report",
            report.to_str().unwrap(),
        ],
    );
    assert!(output.code <= 1, "{}", output.stderr);
    let group_finding = value["groups"]
        .as_array()
        .unwrap_or_else(|| panic!("no groups: {value}"))
        .iter()
        .flat_map(|group| group["findings"].as_array().cloned().unwrap_or_default())
        .find(|finding| finding["name"] == "clamp")
        .unwrap_or_else(|| panic!("no grouped clamp finding: {value}"));
    assert_eq!(group_finding["mutation"]["score"], 0.75, "{group_finding}");
}
