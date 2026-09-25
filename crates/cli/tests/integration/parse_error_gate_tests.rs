//! The opt-in `parse-error` gate (issue #2727).
//!
//! A file the parser rejects keeps its `source-parse-degraded` diagnostic on
//! every run. With `--fail-on-parse-error` or the `failOnParseError` config
//! key, the same file also fails the run and is named in
//! `gate_outcomes["parse-error"]`. Without either, the exit code and the
//! envelope do not change.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

use crate::common::{CommandOutput, commit_all, git, parse_json, run_fallow_raw};
use serde_json::Value;
use tempfile::TempDir;

/// Invalid JSX from the issue: two sibling elements in one expression
/// container. The parser stops in this file.
const BROKEN_TSX: &str = r"import { usedOnlyByBroken } from './helper';

export function Broken({ show }: { show: boolean }) {
  return (
    <div>
      {show && (
        <span>one</span>
        <span>two</span>
      )}
      <p>{usedOnlyByBroken()}</p>
    </div>
  );
}
";

/// A project with one file the parser rejects. `unused-files` is `warn`, so the
/// file the broken import hides does not fail the run by itself: only the
/// parse-error gate can decide the exit code.
fn broken_project(config: &str) -> TempDir {
    let dir = TempDir::new().expect("temp project");
    let root = dir.path();
    std::fs::create_dir_all(root.join("src")).expect("src dir");
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"parse-gate-fx","version":"1.0.0","private":true,"main":"src/index.ts"}"#,
    )
    .expect("package.json");
    std::fs::write(
        root.join("src/index.ts"),
        "import { Broken } from './Broken';\nconsole.log(Broken);\n",
    )
    .expect("entry");
    std::fs::write(
        root.join("src/helper.ts"),
        "export function usedOnlyByBroken(): number {\n  return 42;\n}\n",
    )
    .expect("helper");
    std::fs::write(root.join("src/Broken.tsx"), BROKEN_TSX).expect("broken file");
    std::fs::write(root.join(".fallowrc.json"), config).expect("config");
    dir
}

const WARN_UNUSED_FILES: &str = r#"{"rules":{"unused-files":"warn"}}"#;
const ARMED_BY_CONFIG: &str = r#"{"rules":{"unused-files":"warn"},"failOnParseError":true}"#;

fn root_arg(dir: &TempDir) -> &str {
    dir.path().to_str().expect("temp path is UTF-8")
}

/// Run a command as JSON with `--quiet`. `command` is empty for the bare run.
fn run_json(dir: &TempDir, command: &str, extra: &[&str]) -> (CommandOutput, Value) {
    let mut args: Vec<&str> = Vec::new();
    if !command.is_empty() {
        args.push(command);
    }
    args.extend([
        "--root",
        root_arg(dir),
        "--format",
        "json",
        "--quiet",
        "--no-cache",
    ]);
    args.extend_from_slice(extra);
    let output = run_fallow_raw(&args);
    let json = parse_json(&output);
    (output, json)
}

fn assert_names_the_broken_file(entry: &Value) {
    assert_eq!(entry["status"], "fail", "entry: {entry}");
    assert_eq!(entry["enforced"], true, "entry: {entry}");
    assert_eq!(entry["observed"], 1.0, "entry: {entry}");
    assert_eq!(
        entry["files"],
        serde_json::json!([{ "path": "src/Broken.tsx", "error_count": 1, "panicked": true }]),
        "entry: {entry}"
    );
}

const COMMANDS: [&str; 3] = ["dead-code", "health", ""];

#[test]
fn an_unarmed_run_keeps_its_exit_code_and_envelope() {
    let project = broken_project(WARN_UNUSED_FILES);
    for command in COMMANDS {
        let (output, json) = run_json(&project, command, &[]);
        assert_eq!(output.code, 0, "`{command}` stderr: {}", output.stderr);
        assert!(
            json["gate_outcomes"].get("parse-error").is_none(),
            "`{command}` publishes no parse-error entry: {}",
            json["gate_outcomes"]
        );
        let kinds: Vec<&str> = json["workspace_diagnostics"]
            .as_array()
            .expect("diagnostics array")
            .iter()
            .filter_map(|diagnostic| diagnostic["kind"].as_str())
            .collect();
        assert!(
            kinds.contains(&"source-parse-degraded"),
            "`{command}` keeps the diagnostic: {kinds:?}"
        );
    }
}

#[test]
fn the_flag_fails_every_command_and_names_the_file() {
    let project = broken_project(WARN_UNUSED_FILES);
    for command in COMMANDS {
        let (output, json) = run_json(&project, command, &["--fail-on-parse-error"]);
        assert_eq!(output.code, 1, "`{command}` stderr: {}", output.stderr);
        assert_names_the_broken_file(&json["gate_outcomes"]["parse-error"]);
    }
}

#[test]
fn the_config_key_arms_the_same_gate() {
    let project = broken_project(ARMED_BY_CONFIG);
    for command in COMMANDS {
        let (output, json) = run_json(&project, command, &[]);
        assert_eq!(output.code, 1, "`{command}` stderr: {}", output.stderr);
        assert_names_the_broken_file(&json["gate_outcomes"]["parse-error"]);
    }
}

#[test]
fn an_armed_gate_passes_when_every_file_parses() {
    let project = broken_project(ARMED_BY_CONFIG);
    let fixed = BROKEN_TSX.replace(
        "<span>one</span>\n        <span>two</span>",
        "<>\n          <span>one</span>\n          <span>two</span>\n        </>",
    );
    assert_ne!(
        fixed, BROKEN_TSX,
        "the fix wraps the siblings in a fragment"
    );
    std::fs::write(project.path().join("src/Broken.tsx"), fixed).expect("fixed file");
    for command in COMMANDS {
        let (output, json) = run_json(&project, command, &[]);
        assert_eq!(output.code, 0, "`{command}` stderr: {}", output.stderr);
        assert_eq!(
            json["gate_outcomes"]["parse-error"],
            serde_json::json!({ "status": "pass", "enforced": true, "observed": 0.0 }),
            "`{command}`"
        );
    }
}

#[test]
fn health_report_only_publishes_the_verdict_and_does_not_fail() {
    let project = broken_project(ARMED_BY_CONFIG);
    let (output, json) = run_json(&project, "health", &["--report-only"]);
    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    let entry = &json["gate_outcomes"]["parse-error"];
    assert_eq!(entry["status"], "fail", "entry: {entry}");
    assert_eq!(entry["enforced"], false, "entry: {entry}");
}

#[test]
fn audit_applies_the_gate_over_the_whole_run() {
    let project = broken_project(WARN_UNUSED_FILES);
    let root = project.path();
    git(root, &["init", "-q", "-b", "main"]);
    commit_all(root, "base");
    git(root, &["checkout", "-q", "-b", "change"]);
    std::fs::write(root.join("src/extra.ts"), "export const extra = 1;\n").expect("change");
    commit_all(root, "change");

    let (unarmed, unarmed_json) = run_json(&project, "audit", &["--base", "main"]);
    assert_eq!(unarmed.code, 0, "stderr: {}", unarmed.stderr);
    assert!(unarmed_json["gate_outcomes"].get("parse-error").is_none());

    let (armed, armed_json) = run_json(
        &project,
        "audit",
        &["--base", "main", "--fail-on-parse-error"],
    );
    assert_eq!(armed.code, 1, "stderr: {}", armed.stderr);
    assert_names_the_broken_file(&armed_json["gate_outcomes"]["parse-error"]);
}

#[test]
fn the_human_run_names_each_file_and_the_parser_outcome() {
    let project = broken_project(WARN_UNUSED_FILES);
    for command in COMMANDS {
        let mut args: Vec<&str> = Vec::new();
        if !command.is_empty() {
            args.push(command);
        }
        args.extend([
            "--root",
            root_arg(&project),
            "--no-cache",
            "--fail-on-parse-error",
        ]);
        let output = run_fallow_raw(&args);
        assert_eq!(output.code, 1, "`{command}` stderr: {}", output.stderr);
        assert!(
            output.stderr.contains(
                "Parse-error gate failed: fallow could not parse 1 file cleanly.\n  src/Broken.tsx: 1 parser error, the parser stopped"
            ),
            "`{command}` stderr: {}",
            output.stderr
        );
    }
}

#[test]
fn the_health_body_names_the_degraded_file_without_the_gate() {
    let project = broken_project(WARN_UNUSED_FILES);
    let output = run_fallow_raw(&[
        "health",
        "--root",
        root_arg(&project),
        "--no-cache",
        "--score",
    ]);
    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    let score = output
        .stdout
        .find("Health score:")
        .expect("the score is shown");
    let note = output
        .stdout
        .find("Parse errors: fallow could not fully parse 1 file.")
        .expect("the body names the parse errors");
    assert!(
        score < note,
        "the note follows the score: {}",
        output.stdout
    );
    assert!(
        output
            .stdout
            .contains("    src/Broken.tsx: 1 parser error, the parser stopped"),
        "stdout: {}",
        output.stdout
    );
}

#[test]
fn the_ci_comment_names_the_file_in_the_gate_summary() {
    let project = broken_project(WARN_UNUSED_FILES);
    let output = run_fallow_raw(&[
        "dead-code",
        "--root",
        root_arg(&project),
        "--no-cache",
        "--quiet",
        "--fail-on-parse-error",
        "--format",
        "pr-comment-github",
    ]);
    assert_eq!(output.code, 1, "stderr: {}", output.stderr);
    assert!(
        output
            .stdout
            .contains("failed parse-error (src/Broken.tsx: 1 parser error, the parser stopped)"),
        "stdout: {}",
        output.stdout
    );
}

#[test]
fn security_rejects_the_flag() {
    let project = broken_project(WARN_UNUSED_FILES);
    let output = run_fallow_raw(&[
        "security",
        "--root",
        root_arg(&project),
        "--fail-on-parse-error",
    ]);
    assert_eq!(output.code, 2, "stderr: {}", output.stderr);
    assert!(
        output.stderr.contains("--fail-on-parse-error")
            || output.stdout.contains("--fail-on-parse-error"),
        "stdout: {}\nstderr: {}",
        output.stdout,
        output.stderr
    );
}

/// `--ci` implies `--quiet`, and a CI log must still say why the run failed.
#[test]
fn a_quiet_or_ci_run_still_names_the_files_on_stderr() {
    let project = broken_project(WARN_UNUSED_FILES);
    for extra in [&["--quiet"][..], &["--ci"][..]] {
        for command in COMMANDS {
            let mut args: Vec<&str> = Vec::new();
            if !command.is_empty() {
                args.push(command);
            }
            args.extend([
                "--root",
                root_arg(&project),
                "--no-cache",
                "--fail-on-parse-error",
            ]);
            args.extend_from_slice(extra);
            let output = run_fallow_raw(&args);
            assert_eq!(output.code, 1, "`{command}` {extra:?}: {}", output.stderr);
            assert!(
                output.stderr.contains(
                    "Parse-error gate failed: fallow could not parse 1 file cleanly.\n  src/Broken.tsx: 1 parser error, the parser stopped"
                ),
                "`{command}` {extra:?} stderr: {}",
                output.stderr
            );
        }
    }
}

/// A command the gate cannot apply to rejects the flag instead of ignoring it.
#[test]
fn commands_without_the_gate_reject_the_flag() {
    let project = broken_project(WARN_UNUSED_FILES);
    let root = root_arg(&project);
    for args in [
        vec!["dupes", "--root", root, "--fail-on-parse-error"],
        vec!["fix", "--dry-run", "--root", root, "--fail-on-parse-error"],
        vec!["--root", root, "--only", "dupes", "--fail-on-parse-error"],
    ] {
        let output = run_fallow_raw(&args);
        assert_eq!(output.code, 2, "{args:?} stderr: {}", output.stderr);
        assert!(
            output.stderr.contains("--fail-on-parse-error")
                || output.stdout.contains("--fail-on-parse-error"),
            "{args:?} stdout: {}\nstderr: {}",
            output.stdout,
            output.stderr
        );
    }
}

/// With no finding and a failed parse-error gate, the final status line must
/// not claim a clean run.
#[test]
fn a_failed_gate_replaces_the_clean_status_line() {
    let project = broken_project(r#"{"rules":{"unused-files":"off"}}"#);
    for command in ["dead-code", ""] {
        let mut args: Vec<&str> = Vec::new();
        if !command.is_empty() {
            args.push(command);
        }
        args.extend([
            "--root",
            root_arg(&project),
            "--no-cache",
            "--fail-on-parse-error",
        ]);
        let output = run_fallow_raw(&args);
        assert_eq!(output.code, 1, "`{command}` stderr: {}", output.stderr);
        assert!(
            !output.stderr.contains("No issues found"),
            "`{command}` stderr: {}",
            output.stderr
        );
        assert!(
            output
                .stderr
                .contains("0 issues, parse-error gate failed: 1 file did not parse"),
            "`{command}` stderr: {}",
            output.stderr
        );
    }
}

#[test]
fn a_failed_gate_replaces_the_clean_audit_status_line() {
    let project = broken_project(r#"{"rules":{"unused-files":"off"}}"#);
    let root = project.path();
    git(root, &["init", "-q", "-b", "main"]);
    commit_all(root, "base");
    git(root, &["checkout", "-q", "-b", "change"]);
    std::fs::write(root.join("src/extra.ts"), "export const extra = 1;\n").expect("change");
    commit_all(root, "change");
    let output = run_fallow_raw(&[
        "audit",
        "--root",
        root_arg(&project),
        "--no-cache",
        "--base",
        "main",
        "--fail-on-parse-error",
    ]);
    assert_eq!(output.code, 1, "stderr: {}", output.stderr);
    assert!(
        !output.stderr.contains("No issues in"),
        "stderr: {}",
        output.stderr
    );
    assert!(
        output
            .stderr
            .contains("0 issues in 1 changed file, parse-error gate failed: 1 file did not parse"),
        "stderr: {}",
        output.stderr
    );
}

#[test]
fn a_failed_gate_replaces_the_clean_grouped_status_line() {
    let project = broken_project(r#"{"rules":{"unused-files":"off"}}"#);
    let output = run_fallow_raw(&[
        "dead-code",
        "--root",
        root_arg(&project),
        "--no-cache",
        "--group-by",
        "directory",
        "--fail-on-parse-error",
    ]);
    assert_eq!(output.code, 1, "stderr: {}", output.stderr);
    assert!(
        !output.stderr.contains("No issues found"),
        "stderr: {}",
        output.stderr
    );
    assert!(
        output
            .stderr
            .contains("0 issues, parse-error gate failed: 1 file did not parse"),
        "stderr: {}",
        output.stderr
    );
}
