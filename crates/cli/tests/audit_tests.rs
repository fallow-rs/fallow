#[path = "common/mod.rs"]
mod common;

use common::{parse_json, run_fallow_raw};
use std::fs;
use std::process::Command;

/// Create a temp git repo with a commit, suitable for audit testing.
fn create_audit_fixture(suffix: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "fallow-audit-test-{}-{}",
        std::process::id(),
        suffix
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("src")).unwrap();

    fs::write(
        dir.join("package.json"),
        r#"{"name": "audit-test", "main": "src/index.ts", "dependencies": {"unused-pkg": "1.0.0"}}"#,
    )
    .unwrap();

    fs::write(
        dir.join("src/index.ts"),
        "import { used } from './utils';\nused();\n",
    )
    .unwrap();
    fs::write(
        dir.join("src/utils.ts"),
        "export const used = () => 42;\nexport const unused = () => 0;\n",
    )
    .unwrap();
    fs::write(
        dir.join("src/orphan.ts"),
        "export const orphaned = 'nobody';\n",
    )
    .unwrap();

    let git = |args: &[&str]| {
        Command::new("git")
            .args(args)
            .current_dir(&dir)
            .env("GIT_AUTHOR_NAME", "test")
            .env("GIT_AUTHOR_EMAIL", "test@test.com")
            .env("GIT_COMMITTER_NAME", "test")
            .env("GIT_COMMITTER_EMAIL", "test@test.com")
            .output()
            .expect("git command failed")
    };

    git(&["init", "-b", "main"]);
    git(&["add", "."]);
    git(&["-c", "commit.gpgsign=false", "commit", "-m", "initial"]);

    dir
}

fn cleanup(dir: &std::path::Path) {
    let _ = fs::remove_dir_all(dir);
}

// ---------------------------------------------------------------------------
// Audit JSON output structure
// ---------------------------------------------------------------------------

#[test]
fn audit_json_has_verdict_and_schema() {
    let dir = create_audit_fixture("verdict");
    let output = run_fallow_raw(&[
        "audit",
        "--root",
        dir.to_str().unwrap(),
        "--base",
        "HEAD",
        "--format",
        "json",
        "--quiet",
    ]);

    assert_eq!(
        output.code, 0,
        "audit with no changes should exit 0. stderr: {}",
        output.stderr
    );

    let json = parse_json(&output);
    assert_eq!(
        json["verdict"].as_str(),
        Some("pass"),
        "no changes should give pass verdict"
    );
    assert_eq!(
        json["command"].as_str(),
        Some("audit"),
        "command should be 'audit'"
    );
    assert!(
        json.get("schema_version").is_some(),
        "audit JSON should have schema_version"
    );

    cleanup(&dir);
}

#[test]
fn audit_pass_verdict_when_no_changes() {
    let dir = create_audit_fixture("nochanges");
    let output = run_fallow_raw(&[
        "audit",
        "--root",
        dir.to_str().unwrap(),
        "--base",
        "HEAD",
        "--format",
        "json",
        "--quiet",
    ]);

    assert_eq!(output.code, 0, "no changes should give exit 0");

    let json = parse_json(&output);
    assert_eq!(
        json["verdict"].as_str(),
        Some("pass"),
        "no changes should give pass verdict"
    );
    assert_eq!(
        json["changed_files_count"].as_u64(),
        Some(0),
        "should report 0 changed files"
    );

    cleanup(&dir);
}

#[test]
fn audit_json_has_summary_with_changes() {
    let dir = create_audit_fixture("summary");

    fs::write(dir.join("src/new.ts"), "export const newThing = 'added';\n").unwrap();

    Command::new("git")
        .args(["add", "."])
        .current_dir(&dir)
        .output()
        .unwrap();
    Command::new("git")
        .args(["-c", "commit.gpgsign=false", "commit", "-m", "add new file"])
        .current_dir(&dir)
        .env("GIT_AUTHOR_NAME", "test")
        .env("GIT_AUTHOR_EMAIL", "test@test.com")
        .env("GIT_COMMITTER_NAME", "test")
        .env("GIT_COMMITTER_EMAIL", "test@test.com")
        .output()
        .unwrap();

    let output = run_fallow_raw(&[
        "audit",
        "--root",
        dir.to_str().unwrap(),
        "--base",
        "HEAD~1",
        "--format",
        "json",
        "--quiet",
    ]);

    assert!(
        output.code == 0 || output.code == 1,
        "audit should not crash, got exit {}. stderr: {}",
        output.code,
        output.stderr
    );

    let json = parse_json(&output);
    assert!(
        json.get("summary").is_some(),
        "audit JSON should have summary"
    );
    let summary = &json["summary"];
    assert!(
        summary.get("dead_code_issues").is_some(),
        "summary should have dead_code_issues"
    );

    cleanup(&dir);
}

// ---------------------------------------------------------------------------
// Audit error handling
// ---------------------------------------------------------------------------

#[test]
fn audit_badge_format_exits_2() {
    let dir = create_audit_fixture("badge");
    let output = run_fallow_raw(&[
        "audit",
        "--root",
        dir.to_str().unwrap(),
        "--base",
        "HEAD",
        "--format",
        "badge",
        "--quiet",
    ]);
    assert_eq!(
        output.code, 2,
        "audit with --format badge should exit 2 (unsupported)"
    );
    cleanup(&dir);
}
