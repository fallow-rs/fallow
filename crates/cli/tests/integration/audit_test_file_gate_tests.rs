//! `audit`: complexity findings in test files do not gate the verdict.
//!
//! A complexity finding in a test file (`*.test.*`, `*.spec.*`, `__tests__/`
//! and the other paths of the shared test-path classifier) gets
//! `effective_severity: warn` and `gate_exemption: test-file`. It stays in the
//! report. `fallow health` does not change.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use serde_json::Value;
use tempfile::TempDir;

use crate::common::{commit_all, git, parse_json, run_fallow_raw};

const TEST_FILE: &str = "src/__tests__/app.test.js";
const SPEC_FILE: &str = "src/app.spec.js";
const PRODUCTION_FILE: &str = "src/index.js";

/// A function with `branches` early returns: cyclomatic `branches + 1`,
/// cognitive `branches`.
fn hotspot(name: &str, branches: usize) -> String {
    let mut source = format!("export function {name}(x) {{\n");
    for branch in 0..branches {
        writeln!(source, "  if (x === {branch}) return {branch};").unwrap();
    }
    source.push_str("  return -1;\n}\n");
    source
}

/// A repository with a small `src/index.js` on the base commit, and a `head`
/// branch checked out on top of it.
fn fixture() -> TempDir {
    let tmp = TempDir::new().expect("failed to create temp dir");
    let dir = tmp.path();
    fs::create_dir_all(dir.join("src/__tests__")).unwrap();
    fs::write(
        dir.join("package.json"),
        r#"{"name": "audit-test-file-gate", "private": true}"#,
    )
    .unwrap();
    fs::write(
        dir.join(".fallowrc.json"),
        r#"{"entry": ["src/**/*.js"], "health": {"maxCyclomatic": 15, "maxCognitive": 15, "maxCrap": 999999999, "maxUnitSize": 999999}}"#,
    )
    .unwrap();
    fs::write(dir.join(PRODUCTION_FILE), "export const value = 1;\n").unwrap();
    git(dir, &["init", "-b", "main"]);
    git(dir, &["config", "core.autocrlf", "false"]);
    commit_all(dir, "baseline");
    git(dir, &["checkout", "-b", "head"]);
    tmp
}

fn write(dir: &Path, path: &str, source: &str) {
    fs::write(dir.join(path), source).unwrap();
}

fn run_audit(dir: &Path, gate: &str, format: &str) -> crate::common::CommandOutput {
    run_fallow_raw(&[
        "audit",
        "--root",
        dir.to_str().unwrap(),
        "--base",
        "main",
        "--gate",
        gate,
        "--format",
        format,
        "--quiet",
        "--no-cache",
        "--no-css",
    ])
}

fn audit_json(dir: &Path, gate: &str) -> (i32, Value) {
    let output = run_audit(dir, gate, "json");
    assert!(
        output.code == 0 || output.code == 1,
        "audit failed. stdout: {}\nstderr: {}",
        output.stdout,
        output.stderr
    );
    (output.code, parse_json(&output))
}

fn finding<'a>(json: &'a Value, path: &str) -> &'a Value {
    let findings = if json.get("complexity").is_some() {
        &json["complexity"]["findings"]
    } else {
        &json["findings"]
    };
    findings
        .as_array()
        .expect("complexity findings")
        .iter()
        .find(|item| item["path"].as_str() == Some(path))
        .unwrap_or_else(|| panic!("no finding for {path}: {json:#}"))
}

#[test]
fn new_test_file_finding_is_reported_but_does_not_gate() {
    let tmp = fixture();
    write(tmp.path(), TEST_FILE, &hotspot("testHotspot", 20));
    write(tmp.path(), SPEC_FILE, &hotspot("specHotspot", 20));
    commit_all(tmp.path(), "head");

    let (code, json) = audit_json(tmp.path(), "new-only");

    assert_eq!(code, 0, "{json:#}");
    assert_eq!(json["verdict"], "warn", "{json:#}");
    assert_eq!(json["attribution"]["complexity_introduced"], 2, "{json:#}");
    for path in [TEST_FILE, SPEC_FILE] {
        let item = finding(&json, path);
        assert_eq!(item["introduced"], true, "{item:#}");
        assert_eq!(item["effective_severity"], "warn", "{item:#}");
        assert_eq!(item["gate_exemption"], "test-file", "{item:#}");
    }
}

#[test]
fn production_finding_next_to_a_test_file_finding_still_gates() {
    let tmp = fixture();
    write(tmp.path(), TEST_FILE, &hotspot("testHotspot", 20));
    write(tmp.path(), PRODUCTION_FILE, &hotspot("hotspot", 20));
    commit_all(tmp.path(), "head");

    let (code, json) = audit_json(tmp.path(), "new-only");

    assert_eq!(code, 1, "{json:#}");
    assert_eq!(json["verdict"], "fail", "{json:#}");
    let production = finding(&json, PRODUCTION_FILE);
    assert_eq!(production["effective_severity"], "error", "{production:#}");
    assert!(production.get("gate_exemption").is_none(), "{production:#}");
    assert_eq!(finding(&json, TEST_FILE)["gate_exemption"], "test-file");
}

#[test]
fn gate_all_does_not_gate_test_file_findings() {
    let tmp = fixture();
    write(tmp.path(), TEST_FILE, &hotspot("testHotspot", 20));
    commit_all(tmp.path(), "head");

    let (code, json) = audit_json(tmp.path(), "all");

    assert_eq!(code, 0, "{json:#}");
    assert_eq!(json["verdict"], "warn", "{json:#}");
    assert_eq!(finding(&json, TEST_FILE)["gate_exemption"], "test-file");
}

#[test]
fn health_still_reports_test_file_findings_at_rule_severity() {
    let tmp = fixture();
    write(tmp.path(), TEST_FILE, &hotspot("testHotspot", 20));
    commit_all(tmp.path(), "head");

    let output = run_fallow_raw(&[
        "health",
        "--root",
        tmp.path().to_str().unwrap(),
        "--complexity",
        "--format",
        "json",
        "--quiet",
        "--no-cache",
    ]);
    assert_eq!(output.code, 1, "stdout: {}", output.stdout);
    let json = parse_json(&output);
    let item = finding(&json, TEST_FILE);
    assert_eq!(item["effective_severity"], "error", "{item:#}");
    assert!(item.get("gate_exemption").is_none(), "{item:#}");
}

#[test]
fn every_output_format_marks_the_test_file_finding() {
    let tmp = fixture();
    write(tmp.path(), TEST_FILE, &hotspot("testHotspot", 20));
    commit_all(tmp.path(), "head");

    let human = run_audit(tmp.path(), "new-only", "human");
    assert_eq!(human.code, 0, "stderr: {}", human.stderr);
    let text = format!("{}{}", human.stdout, human.stderr);
    assert!(
        text.contains(&format!(
            "{TEST_FILE}:1 testHotspot  (test file: warn only, audit does not gate it)"
        )),
        "{text}"
    );

    let markdown = run_audit(tmp.path(), "new-only", "markdown");
    assert!(
        markdown.stdout.contains("### Test files, not gated"),
        "{}",
        markdown.stdout
    );
    assert!(
        markdown.stdout.contains(&format!("`{TEST_FILE}:1`")),
        "{}",
        markdown.stdout
    );

    let compact = run_audit(tmp.path(), "new-only", "compact");
    assert!(
        compact.stdout.contains(&format!(
            "gate-exemption:{TEST_FILE}:1:testHotspot:test-file"
        )),
        "{}",
        compact.stdout
    );

    let sarif = run_audit(tmp.path(), "new-only", "sarif");
    let sarif: Value = serde_json::from_str(&sarif.stdout).expect("sarif json");
    let result = sarif["runs"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|run| run["results"].as_array())
        .flatten()
        .find(|result| {
            result["locations"][0]["physicalLocation"]["artifactLocation"]["uri"].as_str()
                == Some(TEST_FILE)
        })
        .expect("sarif result for the test file");
    assert_eq!(result["level"], "warning", "{result:#}");
    assert_eq!(
        result["properties"]["gate_exemption"], "test-file",
        "{result:#}"
    );
}
