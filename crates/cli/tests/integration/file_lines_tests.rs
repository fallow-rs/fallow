//! The `health.maxFileLines` threshold: a file-level complexity finding for a
//! file with more lines than the limit, and its audit attribution.

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

/// A module with `lines` physical lines and a final newline.
fn module_of(lines: usize) -> String {
    let mut source = String::new();
    for line in 0..lines {
        writeln!(source, "export const value{line} = {line};").unwrap();
    }
    source
}

fn project(config: &str, files: &[(&str, String)]) -> TempDir {
    let tmp = TempDir::new().expect("create temp dir");
    let dir = tmp.path();
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("package.json"),
        r#"{"name": "file-lines", "private": true}"#,
    )
    .unwrap();
    fs::write(dir.join(".fallowrc.json"), config).unwrap();
    for (path, contents) in files {
        fs::write(dir.join(path), contents).unwrap();
    }
    tmp
}

fn health_json(dir: &Path, extra: &[&str]) -> serde_json::Value {
    let mut args = vec![
        "health",
        "--root",
        dir.to_str().unwrap(),
        "--complexity",
        "--format",
        "json",
        "--quiet",
        "--no-cache",
    ];
    args.extend_from_slice(extra);
    let output = run_fallow_raw(&args);
    assert!(
        output.code == 0 || output.code == 1,
        "health failed. stdout: {}\nstderr: {}",
        output.stdout,
        output.stderr
    );
    parse_json(&output)
}

fn file_findings(json: &serde_json::Value) -> Vec<&serde_json::Value> {
    json["findings"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|finding| finding["exceeded"] == "file_lines")
        .collect()
}

const CONFIG: &str = r#"{"entry": ["src/*.js"]}"#;

#[test]
fn file_lines_is_off_by_default() {
    let tmp = project(CONFIG, &[("src/big.js", module_of(800))]);
    let json = health_json(tmp.path(), &[]);
    assert!(file_findings(&json).is_empty(), "{json:#}");
    assert!(json["summary"].get("max_file_lines_threshold").is_none());
}

#[test]
fn flag_reports_each_file_over_the_limit() {
    let tmp = project(
        CONFIG,
        &[
            ("src/big.js", module_of(12)),
            ("src/exact.js", module_of(10)),
            ("src/small.js", module_of(3)),
        ],
    );
    let json = health_json(tmp.path(), &["--max-file-lines", "10"]);
    let findings = file_findings(&json);
    assert_eq!(findings.len(), 1, "{json:#}");
    let finding = findings[0];
    assert_eq!(finding["path"], "src/big.js");
    assert_eq!(finding["name"], "<file>");
    assert_eq!(finding["line"], 1);
    assert_eq!(finding["line_count"], 12);
    assert_eq!(finding["max_file_lines"], 10);
    assert_eq!(finding["effective_severity"], "error");
    assert_eq!(json["summary"]["max_file_lines_threshold"], 10);
}

#[test]
fn final_line_without_newline_counts_once() {
    let mut source = module_of(11);
    source.push_str("export const last = 1;");
    let tmp = project(CONFIG, &[("src/big.js", source)]);
    let json = health_json(tmp.path(), &["--max-file-lines", "10"]);
    assert_eq!(file_findings(&json)[0]["line_count"], 12, "{json:#}");
}

#[test]
fn config_limit_and_override_apply() {
    let config = r#"{
        "entry": ["src/*.js"],
        "health": {
            "maxFileLines": 10,
            "thresholdOverrides": [{"files": ["src/generated.js"], "maxFileLines": 100}]
        }
    }"#;
    let tmp = project(
        config,
        &[
            ("src/big.js", module_of(20)),
            ("src/generated.js", module_of(50)),
        ],
    );
    let json = health_json(tmp.path(), &[]);
    let paths: Vec<&str> = file_findings(&json)
        .iter()
        .map(|finding| finding["path"].as_str().unwrap())
        .collect();
    assert_eq!(paths, ["src/big.js"], "{json:#}");
}

#[test]
fn rule_warn_reports_without_failure() {
    let config = r#"{
        "entry": ["src/*.js"],
        "rules": {"complexity-file-lines": "warn"},
        "health": {"maxFileLines": 10}
    }"#;
    let tmp = project(config, &[("src/big.js", module_of(20))]);
    let json = health_json(tmp.path(), &[]);
    assert_eq!(file_findings(&json)[0]["effective_severity"], "warn");
}

#[test]
fn override_with_functions_rejects_max_file_lines() {
    let config = r#"{
        "entry": ["src/*.js"],
        "health": {"thresholdOverrides": [{"files": ["src/big.js"], "functions": ["f"], "maxFileLines": 100}]}
    }"#;
    let tmp = project(config, &[("src/big.js", module_of(3))]);
    let output = run_fallow_raw(&[
        "health",
        "--root",
        tmp.path().to_str().unwrap(),
        "--quiet",
        "--no-cache",
    ]);
    assert_eq!(output.code, 2, "stdout: {}", output.stdout);
    assert!(
        output.stderr.contains("maxFileLines"),
        "stderr: {}",
        output.stderr
    );
}

/// A repository with `base` on `main` and `head` committed on a `head` branch.
fn audit_fixture(base: &[(&str, String)], head: &[(&str, String)]) -> TempDir {
    let config = r#"{"entry": ["src/*.js"], "health": {"maxFileLines": 20}}"#;
    let tmp = project(config, base);
    let dir = tmp.path();
    git(dir, &["init", "-b", "main"]);
    git(dir, &["config", "core.autocrlf", "false"]);
    commit_all(dir, "baseline");
    git(dir, &["checkout", "-b", "head"]);
    for (path, contents) in head {
        fs::write(dir.join(path), contents).unwrap();
    }
    commit_all(dir, "head");
    tmp
}

fn audit_json(dir: &Path, extra: &[&str]) -> (i32, serde_json::Value) {
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
        "--skip",
        "dupes",
    ];
    args.extend_from_slice(extra);
    let output = run_fallow_raw(&args);
    assert!(
        output.code == 0 || output.code == 1,
        "audit failed. stdout: {}\nstderr: {}",
        output.stdout,
        output.stderr
    );
    (output.code, parse_json(&output))
}

fn audit_file_finding<'a>(json: &'a serde_json::Value, path: &str) -> &'a serde_json::Value {
    json["complexity"]["findings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|finding| finding["exceeded"] == "file_lines" && finding["path"] == path)
        .unwrap_or_else(|| panic!("no file finding for {path}: {json:#}"))
}

#[test]
fn audit_keeps_an_edited_long_file_of_the_same_length_inherited() {
    let edited = module_of(30).replace("value0 = 0", "value0 = 100");
    let tmp = audit_fixture(&[("src/big.js", module_of(30))], &[("src/big.js", edited)]);
    let (code, json) = audit_json(tmp.path(), &[]);
    assert_eq!(code, 0, "{json:#}");
    let finding = audit_file_finding(&json, "src/big.js");
    assert_eq!(finding["attribution"], "inherited");
}

#[test]
fn audit_keeps_small_growth_inherited() {
    let tmp = audit_fixture(
        &[("src/big.js", module_of(30))],
        &[("src/big.js", module_of(33))],
    );
    let (code, json) = audit_json(tmp.path(), &[]);
    assert_eq!(code, 0, "{json:#}");
    assert_eq!(
        audit_file_finding(&json, "src/big.js")["attribution"],
        "inherited"
    );
}

#[test]
fn audit_reports_a_grown_long_file_as_worsened() {
    let tmp = audit_fixture(
        &[("src/big.js", module_of(30))],
        &[("src/big.js", module_of(40))],
    );
    let (code, json) = audit_json(tmp.path(), &[]);
    let finding = audit_file_finding(&json, "src/big.js");
    assert_eq!(finding["attribution"], "worsened", "{json:#}");
    assert_eq!(finding["worsened_metrics"][0]["metric"], "line_count");
    assert_eq!(finding["worsened_metrics"][0]["base"], 30);
    assert_eq!(finding["worsened_metrics"][0]["head"], 40);
    assert_eq!(code, 0, "worsened gives warn by default");

    let (code, _) = audit_json(tmp.path(), &["--worsened", "error"]);
    assert_eq!(code, 1, "--worsened error fails the audit");
}

#[test]
fn audit_reports_a_file_that_crosses_the_limit_as_introduced() {
    let tmp = audit_fixture(
        &[("src/big.js", module_of(15))],
        &[("src/big.js", module_of(25)), ("src/new.js", module_of(30))],
    );
    let (code, json) = audit_json(tmp.path(), &[]);
    assert_eq!(code, 1, "{json:#}");
    assert_eq!(
        audit_file_finding(&json, "src/big.js")["attribution"],
        "introduced"
    );
    assert_eq!(
        audit_file_finding(&json, "src/new.js")["attribution"],
        "introduced"
    );
}

#[test]
fn audit_worsened_off_keeps_growth_of_a_long_file_visible() {
    let tmp = audit_fixture(
        &[("src/big.js", module_of(30))],
        &[("src/big.js", module_of(40))],
    );
    let (code, json) = audit_json(tmp.path(), &["--worsened", "off"]);
    assert_eq!(code, 0, "{json:#}");
    assert_eq!(
        audit_file_finding(&json, "src/big.js")["attribution"],
        "worsened",
        "the line count is the exceeded metric, so its growth stays visible"
    );
}

#[test]
fn file_level_suppression_hides_the_finding() {
    let mut source = String::from("// fallow-ignore-file complexity\n");
    source.push_str(&module_of(20));
    let tmp = project(CONFIG, &[("src/big.js", source)]);
    let json = health_json(tmp.path(), &["--max-file-lines", "10"]);
    assert!(file_findings(&json).is_empty(), "{json:#}");
}

#[test]
fn saved_baseline_hides_a_known_long_file() {
    let tmp = project(CONFIG, &[("src/big.js", module_of(20))]);
    let baseline = tmp.path().join("health-baseline.json");
    let baseline = baseline.to_str().unwrap();
    health_json(
        tmp.path(),
        &["--max-file-lines", "10", "--save-baseline", baseline],
    );
    let json = health_json(
        tmp.path(),
        &["--max-file-lines", "10", "--baseline", baseline],
    );
    assert!(file_findings(&json).is_empty(), "{json:#}");

    fs::write(tmp.path().join("src/other.js"), module_of(15)).unwrap();
    let json = health_json(
        tmp.path(),
        &["--max-file-lines", "10", "--baseline", baseline],
    );
    let paths: Vec<&str> = file_findings(&json)
        .iter()
        .map(|finding| finding["path"].as_str().unwrap())
        .collect();
    assert_eq!(paths, ["src/other.js"], "{json:#}");
}

#[test]
fn override_row_reports_an_active_file_lines_entry() {
    let config = r#"{
        "entry": ["src/*.js"],
        "health": {
            "maxFileLines": 10,
            "thresholdOverrides": [
                {"files": ["src/generated.js"], "maxFileLines": 100, "reason": "generated"},
                {"files": ["src/missing.js"], "maxFileLines": 100}
            ]
        }
    }"#;
    let tmp = project(config, &[("src/generated.js", module_of(50))]);
    let json = health_json(tmp.path(), &[]);
    let rows = json["threshold_overrides"].as_array().unwrap();
    let status_of = |index: u64| {
        rows.iter()
            .find(|row| row["override_index"] == index)
            .unwrap_or_else(|| panic!("no row for override {index}: {json:#}"))["status"]
            .as_str()
            .unwrap()
    };
    assert_eq!(status_of(0), "active", "{json:#}");
    assert_eq!(status_of(1), "no_match", "{json:#}");
}

fn health_text(dir: &Path, format: &str) -> String {
    let output = run_fallow_raw(&[
        "health",
        "--root",
        dir.to_str().unwrap(),
        "--complexity",
        "--max-file-lines",
        "10",
        "--format",
        format,
        "--quiet",
        "--no-cache",
    ]);
    assert!(output.code == 0 || output.code == 1, "{}", output.stderr);
    output.stdout
}

#[test]
fn markdown_and_review_name_a_long_file_as_a_file() {
    let tmp = project(CONFIG, &[("src/big.js", module_of(20))]);
    let markdown = health_text(tmp.path(), "markdown");
    assert!(
        markdown.contains("## Fallow: 1 high complexity finding\n"),
        "{markdown}"
    );
    assert!(
        markdown.contains("| - | - | - | 20 **!** (threshold: 10) |"),
        "{markdown}"
    );
    let review = health_text(tmp.path(), "review-github");
    assert!(review.contains("fallow/long-file"), "{review}");
    assert!(
        review.contains("Fix intent: Move cohesive groups of exports"),
        "{review}"
    );
}

#[test]
fn byte_order_mark_adds_no_line() {
    let mut source = String::from("\u{feff}");
    source.push_str(&module_of(10));
    let tmp = project(CONFIG, &[("src/bom.js", source)]);
    let json = health_json(tmp.path(), &["--max-file-lines", "9"]);
    assert_eq!(file_findings(&json)[0]["line_count"], 10, "{json:#}");
    let json = health_json(tmp.path(), &["--max-file-lines", "10"]);
    assert!(file_findings(&json).is_empty(), "{json:#}");
}

#[test]
fn audit_counts_comment_only_growth() {
    let comments = |count: usize| "// note\n".repeat(count);
    let tmp = audit_fixture(
        &[
            ("src/grows.js", module_of(15)),
            ("src/long.js", module_of(30)),
        ],
        &[
            ("src/grows.js", format!("{}{}", module_of(15), comments(8))),
            ("src/long.js", format!("{}{}", module_of(30), comments(8))),
        ],
    );
    let (code, json) = audit_json(tmp.path(), &[]);
    assert_eq!(
        audit_file_finding(&json, "src/grows.js")["attribution"],
        "introduced",
        "comment lines that cross the limit make the finding new"
    );
    assert_eq!(
        audit_file_finding(&json, "src/long.js")["attribution"],
        "worsened"
    );
    assert_eq!(code, 1, "{json:#}");
}
