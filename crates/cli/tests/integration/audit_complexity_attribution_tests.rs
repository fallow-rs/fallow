//! `audit --gate new-only` attribution of complexity findings (#3277).
//!
//! A head finding is introduced when no base finding matches it, or when a
//! metric that the head finding exceeds has a higher value than in the base
//! finding. Unchanged and decreased metrics stay inherited, also when the
//! exceeded category changes.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use tempfile::TempDir;

use crate::common::{commit_all, git, parse_json, run_fallow_raw};

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

/// A class with one method `run` that has `branches` early returns.
fn class_with_run(class: &str, branches: usize) -> String {
    let mut source = format!("export class {class} {{\n  run(x) {{\n");
    for branch in 0..branches {
        writeln!(source, "    if (x === {branch}) return {branch};").unwrap();
    }
    source.push_str("    return -1;\n  }\n}\n");
    source
}

/// A repository with `src/index.js` holding `source` on the base commit, and a
/// `head` branch checked out on top of it.
fn fixture(source: &str) -> TempDir {
    let tmp = TempDir::new().expect("failed to create temp dir");
    let dir = tmp.path();
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("package.json"),
        r#"{"name": "complexity-attribution", "private": true}"#,
    )
    .unwrap();
    fs::write(
        dir.join(".fallowrc.json"),
        r#"{"entry": ["src/*.js"], "health": {"maxCyclomatic": 15, "maxCognitive": 15, "maxCrap": 999999999, "maxUnitSize": 999999}}"#,
    )
    .unwrap();
    fs::write(dir.join("src/index.js"), source).unwrap();
    git(dir, &["init", "-b", "main"]);
    git(dir, &["config", "core.autocrlf", "false"]);
    commit_all(dir, "baseline");
    git(dir, &["checkout", "-b", "head"]);
    tmp
}

struct Attribution {
    code: i32,
    verdict: String,
    introduced: u64,
    inherited: u64,
    /// `(cyclomatic, cognitive, exceeded, introduced)` of each finding.
    findings: Vec<(u64, u64, String, bool)>,
}

fn audit(dir: &Path) -> Attribution {
    let output = run_fallow_raw(&[
        "audit",
        "--root",
        dir.to_str().unwrap(),
        "--base",
        "main",
        "--gate",
        "new-only",
        "--format",
        "json",
        "--quiet",
        "--no-cache",
        "--no-css",
    ]);
    assert!(
        output.code == 0 || output.code == 1,
        "audit failed. stdout: {}\nstderr: {}",
        output.stdout,
        output.stderr
    );
    let json = parse_json(&output);
    let findings: Vec<(u64, u64, String, bool)> = json["complexity"]["findings"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .map(|item| {
                    (
                        item["cyclomatic"].as_u64().unwrap(),
                        item["cognitive"].as_u64().unwrap(),
                        item["exceeded"].as_str().unwrap().to_string(),
                        item["introduced"].as_bool().expect("introduced flag"),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    let attribution = Attribution {
        code: output.code,
        verdict: json["verdict"].as_str().unwrap().to_string(),
        introduced: json["attribution"]["complexity_introduced"]
            .as_u64()
            .unwrap(),
        inherited: json["attribution"]["complexity_inherited"]
            .as_u64()
            .unwrap(),
        findings,
    };
    // Verdict, attribution counts and the flag of each finding agree.
    let flagged = attribution
        .findings
        .iter()
        .filter(|finding| finding.3)
        .count() as u64;
    assert_eq!(
        flagged, attribution.introduced,
        "introduced flags and complexity_introduced disagree: {json:#}"
    );
    assert_eq!(
        attribution.findings.len() as u64 - flagged,
        attribution.inherited,
        "inherited flags and complexity_inherited disagree: {json:#}"
    );
    attribution
}

fn assert_inherited(result: &Attribution, findings: u64) {
    assert_eq!(result.code, 0, "expected a pass: {:?}", result.findings);
    assert_eq!(result.verdict, "pass");
    assert_eq!(result.introduced, 0, "{:?}", result.findings);
    assert_eq!(result.inherited, findings, "{:?}", result.findings);
}

fn assert_introduced(result: &Attribution, introduced: u64, inherited: u64) {
    assert_eq!(result.code, 1, "expected a fail: {:?}", result.findings);
    assert_eq!(result.verdict, "fail");
    assert_eq!(result.introduced, introduced, "{:?}", result.findings);
    assert_eq!(result.inherited, inherited, "{:?}", result.findings);
}

/// Replace `src/index.js` on the head branch and commit it.
fn edit(dir: &Path, source: &str) {
    fs::write(dir.join("src/index.js"), source).unwrap();
    commit_all(dir, "head");
}

#[test]
fn unchanged_metrics_stay_inherited() {
    let tmp = fixture(&hotspot("hotspot", 16));
    edit(
        tmp.path(),
        &format!("{}// edited file\n", hotspot("hotspot", 16)),
    );

    let result = audit(tmp.path());

    assert_inherited(&result, 1);
    assert_eq!(result.findings[0].0, 17);
}

#[test]
fn increased_metric_of_existing_finding_is_introduced() {
    let tmp = fixture(&hotspot("hotspot", 16));
    edit(tmp.path(), &hotspot("hotspot", 17));

    let result = audit(tmp.path());

    assert_introduced(&result, 1, 0);
    assert_eq!(
        result.findings,
        vec![(18, 17, "both".to_string(), true)],
        "the worse hotspot is introduced"
    );
}

#[test]
fn decreased_metrics_in_same_category_stay_inherited() {
    let tmp = fixture(&hotspot("hotspot", 17));
    edit(tmp.path(), &hotspot("hotspot", 16));

    let result = audit(tmp.path());

    assert_inherited(&result, 1);
    assert_eq!(result.findings[0].2, "both");
}

#[test]
fn decreased_metrics_with_category_change_stay_inherited() {
    let tmp = fixture(&hotspot("hotspot", 16));
    edit(tmp.path(), &hotspot("hotspot", 15));

    let result = audit(tmp.path());

    assert_inherited(&result, 1);
    assert_eq!(
        result.findings,
        vec![(16, 15, "cyclomatic".to_string(), false)],
        "an improvement that changes the exceeded category is inherited"
    );
}

#[test]
fn new_threshold_crossing_is_introduced() {
    let tmp = fixture(&hotspot("hotspot", 14));
    edit(tmp.path(), &hotspot("hotspot", 16));

    let result = audit(tmp.path());

    assert_introduced(&result, 1, 0);
}

#[test]
fn line_shift_keeps_unchanged_finding_inherited() {
    let tmp = fixture(&hotspot("hotspot", 16));
    let shifted = format!(
        "export const first = 1;\nexport const second = 2;\n\n{}",
        hotspot("hotspot", 16)
    );
    edit(tmp.path(), &shifted);

    let result = audit(tmp.path());

    assert_inherited(&result, 1);
}

#[test]
fn line_shift_with_increase_is_introduced() {
    let tmp = fixture(&hotspot("hotspot", 16));
    let shifted = format!("export const first = 1;\n\n{}", hotspot("hotspot", 17));
    edit(tmp.path(), &shifted);

    let result = audit(tmp.path());

    assert_introduced(&result, 1, 0);
}

#[test]
fn renamed_file_compares_metrics_with_the_old_path() {
    let tmp = fixture(&hotspot("hotspot", 17));
    let dir = tmp.path();
    git(dir, &["mv", "src/index.js", "src/moved.js"]);
    commit_all(dir, "rename");

    assert_inherited(&audit(dir), 1);

    fs::write(dir.join("src/moved.js"), hotspot("hotspot", 16)).unwrap();
    commit_all(dir, "rename plus improvement");

    assert_inherited(&audit(dir), 1);

    fs::write(dir.join("src/moved.js"), hotspot("hotspot", 18)).unwrap();
    commit_all(dir, "rename plus regression");

    assert_introduced(&audit(dir), 1, 0);
}

#[test]
fn same_named_methods_count_and_compare_separately() {
    let base = format!(
        "{}{}",
        class_with_run("First", 16),
        class_with_run("Second", 20)
    );
    let tmp = fixture(&base);
    let dir = tmp.path();
    edit(dir, &format!("{base}// edited file\n"));

    // Each same-named method counts once, and both are unchanged.
    assert_inherited(&audit(dir), 2);

    // Worsen the first method only. The second method still matches its own
    // unchanged base finding.
    edit(
        dir,
        &format!(
            "{}{}",
            class_with_run("First", 17),
            class_with_run("Second", 20)
        ),
    );
    let result = audit(dir);
    assert_introduced(&result, 1, 1);
    assert_eq!(
        result
            .findings
            .iter()
            .find(|finding| finding.3)
            .map(|finding| finding.0),
        Some(18),
        "the worsened method carries the introduced flag: {:?}",
        result.findings
    );
}

#[test]
fn removing_a_same_named_method_does_not_fail_the_other() {
    let base = format!(
        "{}{}",
        class_with_run("First", 16),
        class_with_run("Second", 20)
    );
    let tmp = fixture(&base);
    let dir = tmp.path();
    edit(dir, &class_with_run("Second", 20));

    assert_inherited(&audit(dir), 1);
}

#[test]
fn adding_a_same_named_method_is_introduced() {
    let base = class_with_run("Second", 20);
    let tmp = fixture(&base);
    let dir = tmp.path();
    edit(dir, &format!("{}{base}", class_with_run("First", 16)));

    let result = audit(dir);

    assert_introduced(&result, 1, 1);
    assert_eq!(
        result
            .findings
            .iter()
            .find(|finding| finding.3)
            .map(|finding| finding.0),
        Some(17),
        "the new method carries the introduced flag: {:?}",
        result.findings
    );
}
