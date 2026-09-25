#![allow(
    clippy::expect_used,
    reason = "integration tests use expect to keep fixture setup concise"
)]

//! The `complexity-*` rules decide if a complexity finding blocks.
//!
//! The health thresholds decide if a finding exists. The rules
//! `complexity-cyclomatic`, `complexity-cognitive` and `complexity-crap`
//! decide if it fails the run. The most severe rule of the kinds that
//! contributed wins, and `off` on all contributing kinds drops the finding.
//! The gate severity sets the level in github-annotations, SARIF and
//! CodeClimate. The band stays in the title and the message.

use std::path::Path;

use crate::common::{
    CommandOutput, commit_all, git, parse_json, run_fallow_in_root, run_fallow_raw,
};
use serde_json::Value;

/// Cyclomatic 6, cognitive 5: a moderate-band finding above `maxCyclomatic: 5`.
const BRANCHY: &str = "export function branchy(x: number): number {
  let r = 0;
  if (x > 0) { r += 1; }
  if (x > 1) { r += 2; }
  if (x > 2) { r += 3; }
  if (x > 3) { r += 4; }
  if (x > 4) { r += 5; }
  return r;
}
";

/// Cyclomatic 9 and an estimated CRAP score of 90 without coverage.
const RISKY: &str = "export function risky(x: number): number {
  let r = 0;
  if (x > 0) { r += 0; }
  if (x > 1) { r += 1; }
  if (x > 2) { r += 2; }
  if (x > 3) { r += 3; }
  if (x > 4) { r += 4; }
  if (x > 5) { r += 5; }
  if (x > 6) { r += 6; }
  if (x > 7) { r += 7; }
  return r;
}
";

const DEFAULT_RULES: &str = r#"{
  "entry": ["src/**/*.ts"],
  "health": { "maxCyclomatic": 5, "maxCognitive": 50, "maxCrap": 1000 }
}
"#;

const CRAP_WARN: &str = r#"{
  "entry": ["src/**/*.ts"],
  "rules": { "complexity-crap": "warn" },
  "health": { "maxCyclomatic": 50, "maxCognitive": 50, "maxCrap": 10 }
}
"#;

const LEGACY_WARN: &str = r#"{
  "entry": ["src/**/*.ts"],
  "health": { "maxCyclomatic": 5, "maxCognitive": 50, "maxCrap": 1000 },
  "overrides": [
    { "files": ["src/legacy/**"], "rules": { "complexity-cyclomatic": "warn" } }
  ]
}
"#;

const MIXED: &str = r#"{
  "entry": ["src/**/*.ts"],
  "rules": { "complexity-cyclomatic": "warn", "complexity-crap": "error" },
  "health": { "maxCyclomatic": 5, "maxCognitive": 50, "maxCrap": 10 }
}
"#;

const ALL_OFF: &str = r#"{
  "entry": ["src/**/*.ts"],
  "rules": {
    "complexity-cyclomatic": "off",
    "complexity-cognitive": "off",
    "complexity-crap": "off"
  },
  "health": { "maxCyclomatic": 5, "maxCognitive": 5, "maxCrap": 10 }
}
"#;

const CORE: &str = "src/core.ts";
const LEGACY: &str = "src/legacy/old.ts";
const INDEX: &str = "src/index.ts";

fn write(root: &Path, relative: &str, content: &str) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("create directory");
    std::fs::write(path, content).expect("write fixture file");
}

fn write_base(root: &Path, config: &str) {
    write(
        root,
        "package.json",
        r#"{"name":"complexity-gate","private":true}"#,
    );
    write(root, ".fallowrc.json", config);
    write(root, INDEX, "export const entry = true;\n");
}

fn project(config: &str, files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("fixture tempdir");
    write_base(dir.path(), config);
    for (path, content) in files {
        write(dir.path(), path, content);
    }
    dir
}

/// A git repository whose `main` commit has no complexity finding and whose
/// worktree adds `files`.
fn audit_project(config: &str, files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("audit tempdir");
    let root = dir.path();
    git(root, &["init", "-q", "-b", "main"]);
    write_base(root, config);
    commit_all(root, "base");
    for (path, content) in files {
        write(root, path, content);
    }
    dir
}

fn run(command: &str, root: &Path, format: &str, extra: &[&str]) -> CommandOutput {
    let mut args = vec!["--quiet", "--format", format];
    if command == "audit" {
        args.extend_from_slice(&["--base", "main"]);
    }
    args.extend_from_slice(extra);
    let output = run_fallow_in_root(command, root, &args);
    assert!(
        matches!(output.code, 0 | 1),
        "{command} --format {format} failed: {}",
        output.stderr
    );
    output
}

fn report_from(root: &Path, saved: &Path, format: &str) -> CommandOutput {
    let output = run_fallow_raw(&[
        "report",
        "--from",
        saved.to_str().expect("utf-8 path"),
        "--root",
        root.to_str().expect("utf-8 path"),
        "--quiet",
        "--format",
        format,
    ]);
    assert_eq!(
        output.code, 0,
        "report --from --format {format} failed: {}",
        output.stderr
    );
    output
}

/// The workflow-command level of the first annotation for `path` whose title
/// starts with `title`.
fn annotation_level<'a>(stdout: &'a str, path: &str, title: &str) -> &'a str {
    let needle = format!(" file={path}");
    let line = stdout
        .lines()
        .find(|line| line.contains(&needle) && line.contains(&format!("title={title}")))
        .unwrap_or_else(|| panic!("no {title} annotation for {path} in:\n{stdout}"));
    line.trim_start_matches("::")
        .split(' ')
        .next()
        .expect("annotation level")
}

/// The level of the first result for `rule_id` and `path` in any run. The
/// audit writes one SARIF run for each analysis.
fn sarif_level(sarif: &Value, rule_id: &str, path: &str) -> String {
    sarif["runs"]
        .as_array()
        .expect("SARIF runs")
        .iter()
        .flat_map(|run| run["results"].as_array().into_iter().flatten())
        .find(|result| {
            result["ruleId"] == rule_id
                && result["locations"][0]["physicalLocation"]["artifactLocation"]["uri"] == path
        })
        .unwrap_or_else(|| panic!("no SARIF {rule_id} result for {path} in {sarif}"))["level"]
        .as_str()
        .expect("SARIF level")
        .to_owned()
}

fn codeclimate_severity(issues: &Value, check_name: &str, path: &str) -> String {
    issues
        .as_array()
        .expect("CodeClimate issues")
        .iter()
        .find(|issue| issue["check_name"] == check_name && issue["location"]["path"] == path)
        .unwrap_or_else(|| panic!("no CodeClimate {check_name} issue for {path}"))["severity"]
        .as_str()
        .expect("CodeClimate severity")
        .to_owned()
}

fn health_finding<'a>(findings: &'a Value, path: &str) -> &'a Value {
    findings
        .as_array()
        .expect("findings array")
        .iter()
        .find(|finding| finding["path"] == path)
        .unwrap_or_else(|| panic!("no finding for {path} in {findings}"))
}

#[test]
fn default_rules_give_a_blocking_moderate_finding_an_error_annotation() {
    let dir = audit_project(DEFAULT_RULES, &[(CORE, BRANCHY)]);
    let root = dir.path();

    let json = run("audit", root, "json", &[]);
    let envelope = parse_json(&json);
    assert_eq!(json.code, 1, "a default complexity rule fails the audit");
    assert_eq!(envelope["verdict"], "fail");
    let finding = health_finding(&envelope["complexity"]["findings"], CORE);
    assert_eq!(finding["severity"], "moderate");
    assert_eq!(finding["effective_severity"], "error");

    let annotations = run("audit", root, "github-annotations", &[]).stdout;
    assert_eq!(
        annotation_level(&annotations, CORE, "High cyclomatic complexity (moderate)"),
        "error"
    );
    let sarif = parse_json(&run("audit", root, "sarif", &[]));
    assert_eq!(
        sarif_level(&sarif, "fallow/high-cyclomatic-complexity", CORE),
        "error"
    );
}

#[test]
fn a_warn_crap_rule_reports_without_failing_the_audit_or_health() {
    let dir = audit_project(CRAP_WARN, &[(CORE, RISKY)]);
    let root = dir.path();

    let json = run("audit", root, "json", &[]);
    let envelope = parse_json(&json);
    assert_eq!(json.code, 0, "a warn finding does not fail the audit");
    assert_eq!(envelope["verdict"], "warn");
    let finding = health_finding(&envelope["complexity"]["findings"], CORE);
    assert_eq!(finding["exceeded"], "crap");
    assert_eq!(finding["effective_severity"], "warn");

    let annotations = run("audit", root, "github-annotations", &[]).stdout;
    assert_eq!(
        annotation_level(&annotations, CORE, "High CRAP score"),
        "warning"
    );
    let sarif = parse_json(&run("audit", root, "sarif", &[]));
    assert_eq!(
        sarif_level(&sarif, "fallow/high-crap-score", CORE),
        "warning"
    );
    let issues = parse_json(&run("audit", root, "codeclimate", &[]));
    assert_eq!(
        codeclimate_severity(&issues, "fallow/high-crap-score", CORE),
        "minor"
    );

    let health = run("health", root, "json", &[]);
    assert_eq!(health.code, 0, "a warn finding does not fail health");
    assert_eq!(
        health_finding(&parse_json(&health)["findings"], CORE)["effective_severity"],
        "warn"
    );
    let gated = run("health", root, "json", &["--min-severity", "moderate"]);
    assert_eq!(
        gated.code, 0,
        "the rule applies before --min-severity, so a warn finding stays non-blocking"
    );
}

#[test]
fn an_override_lowers_one_directory_while_another_stays_error() {
    let dir = project(LEGACY_WARN, &[(CORE, BRANCHY), (LEGACY, BRANCHY)]);
    let root = dir.path();

    let health = run("health", root, "json", &[]);
    let envelope = parse_json(&health);
    assert_eq!(health.code, 1, "the core finding stays blocking");
    assert_eq!(
        health_finding(&envelope["findings"], CORE)["effective_severity"],
        "error"
    );
    assert_eq!(
        health_finding(&envelope["findings"], LEGACY)["effective_severity"],
        "warn"
    );

    let annotations = run("health", root, "github-annotations", &[]).stdout;
    assert_eq!(
        annotation_level(&annotations, CORE, "High cyclomatic complexity"),
        "error"
    );
    assert_eq!(
        annotation_level(&annotations, LEGACY, "High cyclomatic complexity"),
        "warning"
    );

    let only_legacy = project(LEGACY_WARN, &[(LEGACY, BRANCHY)]);
    let health = run("health", only_legacy.path(), "json", &[]);
    assert_eq!(
        health.code, 0,
        "a finding that the override lowers to warn does not fail health"
    );
}

#[test]
fn the_most_severe_contributing_rule_wins() {
    let dir = project(MIXED, &[(CORE, RISKY)]);
    let health = run("health", dir.path(), "json", &[]);
    let envelope = parse_json(&health);
    let finding = health_finding(&envelope["findings"], CORE);

    assert_eq!(finding["exceeded"], "cyclomatic_crap");
    assert_eq!(finding["effective_severity"], "error");
    assert_eq!(health.code, 1);
}

#[test]
fn off_on_every_contributing_kind_drops_the_finding() {
    let dir = project(ALL_OFF, &[(CORE, RISKY)]);
    let health = run("health", dir.path(), "json", &[]);
    let envelope = parse_json(&health);

    assert_eq!(health.code, 0);
    assert_eq!(
        envelope["findings"].as_array().map_or(0, Vec::len),
        0,
        "no finding survives: {}",
        envelope["findings"]
    );
}

fn assert_saved_parity(command: &str, root: &Path) {
    let json = run(command, root, "json", &[]);
    let saved_dir = tempfile::tempdir().expect("saved report tempdir");
    let saved = saved_dir.path().join("results.json");
    std::fs::write(&saved, &json.stdout).expect("write saved report");

    for format in ["sarif", "codeclimate", "github-annotations"] {
        let direct = run(command, root, format, &[]);
        let rendered = report_from(root, &saved, format);
        assert_eq!(
            rendered.stdout, direct.stdout,
            "{command}: report --from --format {format} must be byte-identical to the direct run"
        );
    }
}

#[test]
fn saved_health_reports_match_the_direct_run() {
    let dir = project(LEGACY_WARN, &[(CORE, BRANCHY), (LEGACY, BRANCHY)]);
    assert_saved_parity("health", dir.path());
}

#[test]
fn saved_audit_reports_match_the_direct_run() {
    let dir = audit_project(CRAP_WARN, &[(CORE, RISKY)]);
    assert_saved_parity("audit", dir.path());
}

#[test]
fn saved_json_without_the_field_keeps_the_band_levels() {
    let dir = project(DEFAULT_RULES, &[(CORE, BRANCHY)]);
    let root = dir.path();
    let mut envelope = parse_json(&run("health", root, "json", &[]));
    for finding in envelope["findings"].as_array_mut().expect("findings") {
        finding
            .as_object_mut()
            .expect("finding object")
            .remove("effective_severity");
    }
    let saved_dir = tempfile::tempdir().expect("saved report tempdir");
    let saved = saved_dir.path().join("results.json");
    std::fs::write(&saved, envelope.to_string()).expect("write saved report");

    let annotations = report_from(root, &saved, "github-annotations").stdout;
    assert_eq!(
        annotation_level(&annotations, CORE, "High cyclomatic complexity (moderate)"),
        "warning"
    );
    let sarif = parse_json(&report_from(root, &saved, "sarif"));
    assert_eq!(
        sarif_level(&sarif, "fallow/high-cyclomatic-complexity", CORE),
        "note"
    );
}

/// Rule `off` hides the finding but does not settle the threshold override.
/// The override is still too low for the unit, so its row keeps the
/// dimensions that the unit breaches.
#[test]
fn an_off_rule_keeps_the_outstanding_dimensions_of_a_threshold_override() {
    let config = r#"{
  "entry": ["src/**/*.ts"],
  "rules": {
    "complexity-cyclomatic": "off",
    "complexity-cognitive": "off",
    "complexity-crap": "off"
  },
  "health": {
    "maxCyclomatic": 5,
    "maxCognitive": 50,
    "maxCrap": 1000,
    "thresholdOverrides": [{ "files": ["src/core.ts"], "maxCyclomatic": 7 }]
  }
}
"#;
    let dir = project(config, &[(CORE, RISKY)]);
    let envelope = parse_json(&run("health", dir.path(), "json", &[]));

    assert_eq!(envelope["findings"].as_array().map_or(0, Vec::len), 0);
    let row = envelope["threshold_overrides"]
        .as_array()
        .expect("threshold_overrides array")
        .iter()
        .find(|row| row["dimension"] == "complexity")
        .unwrap_or_else(|| panic!("no complexity row in {envelope}"));
    assert_eq!(row["status"], "insufficient");
    assert!(
        row["outstanding"]
            .as_array()
            .is_some_and(|dims| dims.iter().any(|dim| dim == "complexity")),
        "an insufficient row names what the unit still breaches: {row}"
    );
}
