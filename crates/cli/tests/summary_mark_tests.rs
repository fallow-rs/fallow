#![allow(
    clippy::expect_used,
    reason = "integration tests use expect to keep fixture setup concise"
)]

//! The human summary line follows the gate result.
//!
//! A run whose findings are all at rule severity `warn` exits 0. Its summary
//! line shows a warning mark. Only a run that an `error` finding fails shows
//! the failure mark. The same holds for `dead-code`, `health` and the bare
//! combined run.

#[path = "common/mod.rs"]
mod common;

use std::path::Path;

use common::{CommandOutput, run_fallow_raw};

/// Cyclomatic 6: a finding above `maxCyclomatic: 5`, used from the entry.
const BRANCHY: &str = "export function branchy(x: number): number {
  let r = 0;
  if (x > 0) { r += 1; }
  if (x > 1) { r += 2; }
  if (x > 2) { r += 3; }
  if (x > 3) { r += 4; }
  if (x > 4) { r += 5; }
  return r;
}
export const unusedThing = 1;
";

const INDEX: &str = "import { branchy } from './lib';\nbranchy(1);\n";

const WARN_RULES: &str = r#"{
  "entry": ["src/index.ts"],
  "rules": {
    "unused-exports": "warn",
    "complexity-cyclomatic": "warn",
    "complexity-cognitive": "warn",
    "complexity-crap": "warn"
  },
  "health": { "maxCyclomatic": 5, "maxCognitive": 50, "maxCrap": 1000 }
}
"#;

const ERROR_RULES: &str = r#"{
  "entry": ["src/index.ts"],
  "rules": {
    "unused-exports": "error",
    "complexity-cyclomatic": "error",
    "complexity-cognitive": "warn",
    "complexity-crap": "warn"
  },
  "health": { "maxCyclomatic": 5, "maxCognitive": 50, "maxCrap": 1000 }
}
"#;

fn write(root: &Path, relative: &str, content: &str) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("create directory");
    std::fs::write(path, content).expect("write fixture file");
}

fn project(config: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("fixture tempdir");
    let root = dir.path();
    write(
        root,
        "package.json",
        r#"{"name":"summary-mark","private":true}"#,
    );
    write(root, ".fallowrc.json", config);
    write(root, "src/index.ts", INDEX);
    write(root, "src/lib.ts", BRANCHY);
    dir
}

/// Run a command in human format. `None` runs the bare combined command.
fn run_human(command: Option<&str>, root: &Path, extra: &[&str]) -> CommandOutput {
    let root = root.to_str().expect("utf-8 path");
    let mut args: Vec<&str> = command.into_iter().collect();
    args.extend_from_slice(&["--root", root]);
    args.extend_from_slice(extra);
    run_fallow_raw(&args)
}

/// The summary lines of a run: each stderr line that starts with a status
/// mark, with the elapsed time replaced so the snapshot is stable.
fn summary_lines(output: &CommandOutput) -> String {
    output
        .stderr
        .lines()
        .filter(|line| {
            line.starts_with('\u{2717}')
                || line.starts_with('\u{26a0}')
                || line.starts_with('\u{2713}')
        })
        .map(redact_elapsed)
        .collect::<Vec<_>>()
        .join("\n")
}

fn redact_elapsed(line: &str) -> String {
    match line.rfind(" (") {
        Some(index) if line.ends_with("s)") => format!("{} ([ELAPSED])", &line[..index]),
        _ => line.to_owned(),
    }
}

fn assert_exit(output: &CommandOutput, expected: i32) {
    assert_eq!(
        output.code, expected,
        "unexpected exit code\nstdout:\n{}\nstderr:\n{}",
        output.stdout, output.stderr
    );
}

#[test]
fn dead_code_warn_only_shows_warning_mark() {
    let dir = project(WARN_RULES);
    let output = run_human(Some("dead-code"), dir.path(), &[]);
    assert_exit(&output, 0);
    insta::assert_snapshot!("summary_mark_dead_code_warn", summary_lines(&output));
}

#[test]
fn dead_code_error_shows_failure_mark() {
    let dir = project(ERROR_RULES);
    let output = run_human(Some("dead-code"), dir.path(), &[]);
    assert_exit(&output, 1);
    insta::assert_snapshot!("summary_mark_dead_code_error", summary_lines(&output));
}

#[test]
fn dead_code_summary_warn_only_shows_warning_mark() {
    let dir = project(WARN_RULES);
    let output = run_human(Some("dead-code"), dir.path(), &["--summary"]);
    assert_exit(&output, 0);
    insta::assert_snapshot!(
        "summary_mark_dead_code_summary_warn",
        summary_lines(&output)
    );
}

#[test]
fn dead_code_fail_on_issues_promotes_warn_to_failure_mark() {
    let dir = project(WARN_RULES);
    let output = run_human(Some("dead-code"), dir.path(), &["--fail-on-issues"]);
    assert_exit(&output, 1);
    insta::assert_snapshot!(
        "summary_mark_dead_code_fail_on_issues",
        summary_lines(&output)
    );
}

#[test]
fn health_warn_only_shows_warning_mark() {
    let dir = project(WARN_RULES);
    let output = run_human(Some("health"), dir.path(), &[]);
    assert_exit(&output, 0);
    insta::assert_snapshot!("summary_mark_health_warn", summary_lines(&output));
}

#[test]
fn health_error_shows_failure_mark() {
    let dir = project(ERROR_RULES);
    let output = run_human(Some("health"), dir.path(), &[]);
    assert_exit(&output, 1);
    insta::assert_snapshot!("summary_mark_health_error", summary_lines(&output));
}

#[test]
fn health_report_only_shows_warning_mark() {
    let dir = project(ERROR_RULES);
    let output = run_human(Some("health"), dir.path(), &["--report-only"]);
    assert_exit(&output, 0);
    insta::assert_snapshot!("summary_mark_health_report_only", summary_lines(&output));
}

#[test]
fn combined_warn_only_shows_warning_marks() {
    let dir = project(WARN_RULES);
    let output = run_human(None, dir.path(), &[]);
    assert_exit(&output, 0);
    insta::assert_snapshot!("summary_mark_combined_warn", summary_lines(&output));
}

#[test]
fn combined_error_shows_failure_marks() {
    let dir = project(ERROR_RULES);
    let output = run_human(None, dir.path(), &[]);
    assert_exit(&output, 1);
    insta::assert_snapshot!("summary_mark_combined_error", summary_lines(&output));
}
