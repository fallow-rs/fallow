//! `fallow audit` in a clone whose history does not reach a base.
//!
//! A CI checkout with the default `fetch-depth: 1` has only the HEAD commit.
//! The auto-detected base is then HEAD itself, so the audit compares HEAD
//! with itself and finds no changed file. The run must say so on stderr and
//! in `gate_outcomes`, so a CI job does not read the pass as a check.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

use std::fs;
use std::path::{Path, PathBuf};

use tempfile::TempDir;

use crate::common::{CommandOutput, commit_all, git, git_capture, parse_json, run_fallow_raw};

/// A repository with one commit on `main`, and a clone of it in the same
/// temporary directory. `depth` limits the history of the clone.
fn cloned_project(depth: Option<&str>) -> (TempDir, PathBuf) {
    let tmp = TempDir::new().expect("failed to create temp dir");
    let origin = tmp.path().join("origin");
    fs::create_dir_all(origin.join("src")).unwrap();
    fs::write(origin.join("package.json"), r#"{"name": "shallow-audit"}"#).unwrap();
    fs::write(origin.join("src/index.ts"), "export const used = 1;\n").unwrap();
    git(&origin, &["init", "-b", "main"]);
    git(&origin, &["config", "core.autocrlf", "false"]);
    commit_all(&origin, "initial");
    fs::write(origin.join("src/second.ts"), "export const second = 2;\n").unwrap();
    commit_all(&origin, "second");

    let mut args = vec!["clone", "--no-local"];
    if let Some(depth) = depth {
        args.extend(["--depth", depth]);
    }
    let origin_arg = origin.to_str().unwrap().to_owned();
    args.extend([origin_arg.as_str(), "clone"]);
    git(tmp.path(), &args);
    let clone = tmp.path().join("clone");
    (tmp, clone)
}

fn audit(root: &Path, extra: &[&str]) -> CommandOutput {
    let mut args = vec![
        "audit",
        "--root",
        root.to_str().unwrap(),
        "--format",
        "json",
        "--quiet",
    ];
    args.extend_from_slice(extra);
    run_fallow_raw(&args)
}

fn audit_verdict_status(output: &CommandOutput) -> String {
    let json = parse_json(output);
    json["gate_outcomes"]["audit-verdict"]["status"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

#[test]
fn shallow_clone_without_base_warns_and_marks_the_verdict_skipped() {
    let (_tmp, clone) = cloned_project(Some("1"));
    assert_eq!(
        git_capture(&clone, &["rev-parse", "--is-shallow-repository"]),
        "true"
    );

    let output = audit(&clone, &[]);

    assert_eq!(
        output.code, 0,
        "an auto-detected base keeps exit 0: {}",
        output.stderr
    );
    assert!(
        output.stderr.contains("Warning: this is a shallow clone")
            && output.stderr.contains("fetch-depth: 0")
            && output.stderr.contains("--base <ref>"),
        "the warning must name the shallow clone and both fixes: {}",
        output.stderr
    );
    assert!(
        !output
            .stderr
            .contains("Note: the audit compared no changed files"),
        "the shallow warning replaces the generic note: {}",
        output.stderr
    );
    assert_eq!(audit_verdict_status(&output), "skipped");
    assert_eq!(parse_json(&output)["verdict"].as_str(), Some("pass"));
}

#[test]
fn shallow_clone_with_an_explicit_base_keeps_the_verdict() {
    let (_tmp, clone) = cloned_project(Some("1"));

    let output = audit(&clone, &["--base", "HEAD"]);

    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    assert!(
        !output.stderr.contains("shallow clone"),
        "an explicit base is the choice of the user: {}",
        output.stderr
    );
    assert_eq!(audit_verdict_status(&output), "pass");
}

#[test]
fn full_clone_without_changes_keeps_the_verdict() {
    let (_tmp, clone) = cloned_project(None);

    let output = audit(&clone, &[]);

    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    assert!(
        !output.stderr.contains("shallow clone"),
        "a full clone is not shallow: {}",
        output.stderr
    );
    assert_eq!(audit_verdict_status(&output), "pass");
}

#[test]
fn shallow_clone_with_a_local_change_keeps_the_verdict() {
    let (_tmp, clone) = cloned_project(Some("1"));
    fs::write(clone.join("src/index.ts"), "export const used = 2;\n").unwrap();

    let output = audit(&clone, &[]);

    assert!(
        !output.stderr.contains("shallow clone"),
        "an audit with a changed file judged that file: {}",
        output.stderr
    );
    assert_ne!(audit_verdict_status(&output), "skipped");
}
