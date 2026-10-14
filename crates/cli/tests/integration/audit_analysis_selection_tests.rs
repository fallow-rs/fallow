//! `fallow audit --only` / `--skip` and the `audit.deadCode`, `audit.health`
//! and `audit.dupes` config keys choose the audit sub-analyses, with the bare
//! `fallow --only` / `--skip` semantics (#3381). A skipped sub-analysis leaves
//! the verdict, and the summary marks it as skipped.

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

use crate::common::{CommandOutput, commit_all, git, parse_json, run_fallow_raw};

const BASE_CONFIG: &str = r#"{
  "entry": ["src/index.js"],
  "health": {"maxCyclomatic": 15, "maxCognitive": 15, "maxCrap": 999999999, "maxUnitSize": 999999},
  "rules": {"complexity-cyclomatic": "warn", "complexity-cognitive": "warn", "complexity-crap": "warn"}
}"#;

/// `BASE_CONFIG` with an `audit` section.
fn config_with_audit(audit: &str) -> String {
    BASE_CONFIG.replacen('{', &format!(r#"{{"audit": {audit},"#), 1)
}

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

/// A function body long enough to form a clone group when two files hold it.
fn cloned(name: &str) -> String {
    let mut source = format!("export function {name}(items) {{\n  const out = [];\n");
    for step in 0..12 {
        writeln!(
            source,
            "  out.push(items.map((item) => item.value * {step}).filter((value) => value > {step}));"
        )
        .unwrap();
    }
    source.push_str("  return out;\n}\n");
    source
}

/// A repository whose `head` branch introduces one complexity warning, one
/// unused file (a dead-code error), one import cycle (an architecture
/// finding) and one clone group on top of a clean `main`.
fn fixture(config: &str) -> TempDir {
    let tmp = TempDir::new().expect("failed to create temp dir");
    let dir = tmp.path();
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("package.json"),
        r#"{"name": "audit-analysis-selection", "private": true}"#,
    )
    .unwrap();
    fs::write(dir.join(".fallowrc.json"), config).unwrap();
    fs::write(dir.join("src/index.js"), "export const ready = true;\n").unwrap();
    git(dir, &["init", "-b", "main"]);
    git(dir, &["config", "core.autocrlf", "false"]);
    commit_all(dir, "baseline");
    git(dir, &["checkout", "-b", "head"]);
    fs::write(
        dir.join("src/index.js"),
        format!(
            "import {{ a }} from './a.js';\nimport {{ first }} from './first.js';\nimport {{ second }} from './second.js';\nexport const ready = [a, first, second];\n{}",
            hotspot("route", 20)
        ),
    )
    .unwrap();
    fs::write(
        dir.join("src/a.js"),
        "import { b } from './b.js';\nexport const a = () => b;\n",
    )
    .unwrap();
    fs::write(
        dir.join("src/b.js"),
        "import { a } from './a.js';\nexport const b = () => a;\n",
    )
    .unwrap();
    fs::write(dir.join("src/first.js"), cloned("first")).unwrap();
    fs::write(dir.join("src/second.js"), cloned("second")).unwrap();
    fs::write(dir.join("src/orphan.js"), "export const orphan = 1;\n").unwrap();
    commit_all(dir, "head change");
    tmp
}

fn audit(dir: &Path, extra: &[&str]) -> CommandOutput {
    let mut args = vec![
        "audit",
        "--root",
        dir.to_str().unwrap(),
        "--base",
        "main",
        "--format",
        "json",
        "--quiet",
        "--no-cache",
        "--no-css",
    ];
    args.extend_from_slice(extra);
    run_fallow_raw(&args)
}

/// The JSON report of an audit that exits 0 or 1.
fn report(output: &CommandOutput) -> Value {
    assert!(
        output.code == 0 || output.code == 1,
        "audit failed. stdout: {}\nstderr: {}",
        output.stdout,
        output.stderr
    );
    parse_json(output)
}

fn count(json: &Value, section: &str, key: &str) -> usize {
    json[section][key].as_array().map_or(0, Vec::len)
}

/// Which of the three sub-analysis blocks the report holds, and which the
/// summary marks as skipped. Each pair must agree.
fn sections(json: &Value) -> [bool; 3] {
    let present = [
        json.get("dead_code").is_some(),
        json.get("complexity").is_some(),
        json.get("duplication").is_some(),
    ];
    for (ran, marker) in present.iter().zip([
        "dead_code_skipped",
        "complexity_skipped",
        "duplication_skipped",
    ]) {
        assert_eq!(
            json["summary"][marker].as_bool().unwrap_or(false),
            !ran,
            "{marker} disagrees with the report: {json:#}"
        );
    }
    present
}

#[test]
fn every_sub_analysis_runs_by_default() {
    let tmp = fixture(BASE_CONFIG);
    let output = audit(tmp.path(), &[]);
    let json = report(&output);

    assert_eq!(output.code, 1, "{json:#}");
    assert_eq!(sections(&json), [true, true, true], "{json:#}");
    assert!(count(&json, "dead_code", "unused_files") >= 1, "{json:#}");
    assert!(
        count(&json, "dead_code", "circular_dependencies") >= 1,
        "{json:#}"
    );
    assert!(count(&json, "duplication", "clone_groups") >= 1, "{json:#}");
}

#[test]
fn skip_dead_code_leaves_complexity_and_duplication_to_decide() {
    let tmp = fixture(BASE_CONFIG);
    let output = audit(tmp.path(), &["--skip", "dead-code"]);
    let json = report(&output);

    assert_eq!(output.code, 0, "a warn verdict exits 0: {json:#}");
    assert_eq!(json["verdict"], "warn", "{json:#}");
    assert_eq!(sections(&json), [false, true, true], "{json:#}");
    assert_eq!(json["summary"]["dead_code_issues"], 0, "{json:#}");
    assert_eq!(json["attribution"]["complexity_introduced"], 1, "{json:#}");
}

#[test]
fn config_keys_match_the_skip_flag() {
    let tmp = fixture(&config_with_audit(r#"{"deadCode": false}"#));
    let json = report(&audit(tmp.path(), &[]));
    assert_eq!(sections(&json), [false, true, true], "{json:#}");

    let tmp = fixture(&config_with_audit(r#"{"health": false, "dupes": false}"#));
    let flag = fixture(BASE_CONFIG);
    let from_config = report(&audit(tmp.path(), &[]));
    let from_flag = report(&audit(flag.path(), &["--skip", "health,dupes"]));
    for json in [&from_config, &from_flag] {
        assert_eq!(sections(json), [true, false, false], "{json:#}");
        assert_eq!(json["summary"]["complexity_findings"], 0, "{json:#}");
    }
}

#[test]
fn only_replaces_the_config_selection() {
    let tmp = fixture(&config_with_audit(r#"{"deadCode": false}"#));
    let json = report(&audit(tmp.path(), &["--only", "dead-code"]));
    assert_eq!(sections(&json), [true, false, false], "{json:#}");

    let json = report(&audit(tmp.path(), &["--only", "health"]));
    assert_eq!(sections(&json), [false, true, false], "{json:#}");
}

#[test]
fn architecture_narrows_the_dead_code_findings() {
    let tmp = fixture(BASE_CONFIG);

    let json = report(&audit(tmp.path(), &["--skip", "architecture"]));
    assert_eq!(sections(&json), [true, true, true], "{json:#}");
    assert_eq!(
        count(&json, "dead_code", "circular_dependencies"),
        0,
        "{json:#}"
    );
    assert!(count(&json, "dead_code", "unused_files") >= 1, "{json:#}");

    let json = report(&audit(tmp.path(), &["--only", "architecture"]));
    assert_eq!(sections(&json), [true, false, false], "{json:#}");
    assert!(
        count(&json, "dead_code", "circular_dependencies") >= 1,
        "{json:#}"
    );
    assert_eq!(count(&json, "dead_code", "unused_files"), 0, "{json:#}");
}

#[test]
fn a_skipped_sub_analysis_ignores_its_configured_baseline() {
    // The baseline file does not exist: a pass that read it would exit 2.
    let tmp = fixture(&config_with_audit(
        r#"{"deadCode": false, "deadCodeBaseline": "missing-baseline.json"}"#,
    ));
    let json = report(&audit(tmp.path(), &[]));
    assert_eq!(sections(&json), [false, true, true], "{json:#}");
}

#[test]
fn an_explicit_baseline_for_a_skipped_sub_analysis_is_an_error() {
    let tmp = fixture(BASE_CONFIG);
    for (skip, flag) in [
        ("dead-code", "--dead-code-baseline"),
        ("health", "--health-baseline"),
        ("dupes", "--dupes-baseline"),
    ] {
        let output = audit(tmp.path(), &["--skip", skip, flag, "baseline.json"]);
        assert_eq!(output.code, 2, "{flag}: {}", output.stdout);
        assert!(output.stdout.contains(flag), "{}", output.stdout);
    }
}

#[test]
fn only_and_skip_are_mutually_exclusive() {
    let tmp = fixture(BASE_CONFIG);
    let output = audit(tmp.path(), &["--only", "health", "--skip", "dupes"]);
    assert_eq!(output.code, 2, "stdout: {}", output.stdout);
}

#[test]
fn review_brief_keeps_every_sub_analysis() {
    let tmp = fixture(&config_with_audit(r#"{"deadCode": false, "dupes": false}"#));
    let dir = tmp.path().to_str().unwrap();
    let output = run_fallow_raw(&[
        "review",
        "--root",
        dir,
        "--base",
        "main",
        "--format",
        "json",
        "--quiet",
        "--no-cache",
        "--no-css",
        "--skip",
        "health",
    ]);

    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    let json = parse_json(&output);
    assert!(json.get("dead_code").is_some(), "{json:#}");
    assert!(json.get("complexity").is_some(), "{json:#}");
    assert!(json.get("duplication").is_some(), "{json:#}");
    assert!(
        output
            .stderr
            .contains("--only and --skip have no effect on the review brief"),
        "stderr: {}",
        output.stderr
    );
}

#[test]
fn type_aware_require_says_it_did_not_run_without_dead_code() {
    let tmp = fixture(BASE_CONFIG);
    let output = audit(
        tmp.path(),
        &["--skip", "dead-code", "--type-aware-require", "complete"],
    );

    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    assert!(
        output.stderr.contains("--type-aware-require did not run"),
        "stderr: {}",
        output.stderr
    );
}

#[test]
fn bare_only_before_the_subcommand_points_at_audit() {
    let tmp = fixture(BASE_CONFIG);
    let output = run_fallow_raw(&[
        "--skip",
        "dead-code",
        "audit",
        "--root",
        tmp.path().to_str().unwrap(),
        "--format",
        "json",
    ]);
    assert_eq!(output.code, 2);
    assert!(
        output.stdout.contains("pass them after `audit`"),
        "{}",
        output.stdout
    );
}

#[test]
fn a_selection_without_any_analysis_is_an_error() {
    let tmp = fixture(BASE_CONFIG);
    let output = audit(tmp.path(), &["--skip", "dead-code,dupes,health"]);
    assert_eq!(output.code, 2, "audit: {}", output.stdout);
    assert!(
        output.stdout.contains("selects no sub-analysis"),
        "{}",
        output.stdout
    );

    let output = run_fallow_raw(&[
        "--root",
        tmp.path().to_str().unwrap(),
        "--format",
        "json",
        "--quiet",
        "--skip",
        "dead-code,dupes,health",
    ]);
    assert_eq!(output.code, 2, "bare: {}", output.stdout);
    assert!(
        output.stdout.contains("no analysis to run"),
        "{}",
        output.stdout
    );
}

#[test]
fn health_only_flags_say_they_did_not_run_when_health_is_skipped() {
    let tmp = fixture(BASE_CONFIG);
    let output = audit(tmp.path(), &["--skip", "health", "--max-crap", "5"]);
    report(&output);
    assert!(
        output
            .stderr
            .contains("Note: --max-crap did not run: this audit skips the health sub-analysis"),
        "stderr: {}",
        output.stderr
    );
}

#[test]
fn an_audit_without_changes_still_marks_skipped_sub_analyses() {
    let tmp = fixture(BASE_CONFIG);
    let dir = tmp.path().to_str().unwrap();
    let output = run_fallow_raw(&[
        "audit",
        "--root",
        dir,
        "--base",
        "HEAD",
        "--format",
        "json",
        "--quiet",
        "--no-cache",
        "--skip",
        "dupes",
    ]);
    let json = report(&output);
    assert_eq!(json["changed_files_count"], 0, "{json:#}");
    assert_eq!(json["summary"]["duplication_skipped"], true, "{json:#}");
    assert!(
        json["summary"].get("dead_code_skipped").is_none(),
        "{json:#}"
    );
}
