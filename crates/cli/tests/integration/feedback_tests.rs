#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

use std::path::Path;
use std::process::Command;

use crate::common::{CommandOutput, fallow_bin, parse_json};

/// Run `fallow feedback` with a `PATH` that the test controls, so the test
/// decides whether a `gh` binary exists.
fn feedback(bin: &Path, args: &[&str], stdin: Option<&str>) -> CommandOutput {
    let home = tempfile::tempdir().expect("temp home");
    let mut cmd = Command::new(fallow_bin());
    // Only `bin`: a CI runner can have a real `gh` in `/usr/bin`, and a test
    // must never reach it.
    cmd.env("PATH", bin)
        .env_remove("GH_TOKEN")
        .env_remove("GITHUB_TOKEN")
        .env_remove("GH_HOST")
        .env_remove("GH_ENTERPRISE_TOKEN")
        .env("HOME", home.path())
        .env_remove("DO_NOT_TRACK")
        .env_remove("FALLOW_TELEMETRY_DISABLED")
        .env("NO_COLOR", "1")
        .arg("feedback")
        .args(args);
    if let Some(input) = stdin {
        use std::io::Write as _;
        cmd.stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        let mut child = cmd.spawn().expect("spawn fallow");
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        return CommandOutput::from_process(child.wait_with_output().unwrap());
    }
    CommandOutput::from_process(cmd.output().expect("run fallow"))
}

#[cfg(unix)]
fn fake_gh(dir: &Path, script: &str) {
    use std::os::unix::fs::PermissionsExt as _;
    let path = dir.join("gh");
    std::fs::write(&path, format!("#!/bin/sh\n{script}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn preview_sends_nothing_and_prints_a_prefilled_link() {
    let bin = tempfile::tempdir().unwrap();
    let output = feedback(
        bin.path(),
        &[
            "dead-code flags a used export",
            "--type",
            "false-positive",
            "--format",
            "json",
            "--quiet",
        ],
        None,
    );
    assert_eq!(output.code, 0, "{}", output.stderr);
    let json = parse_json(&output);
    let feedback = &json["feedback"];
    assert_eq!(feedback["status"], "preview");
    assert_eq!(
        feedback["title"],
        "Feedback (false positive): dead-code flags a used export"
    );
    assert!(
        feedback["new_issue_url"]
            .as_str()
            .unwrap()
            .starts_with("https://github.com/fallow-rs/fallow/issues/new?title=")
    );
    assert!(feedback["issue_url"].is_null());
    assert_eq!(feedback["diagnostics_included"], true);
}

#[test]
fn json_input_from_stdin_and_flag_precedence() {
    let bin = tempfile::tempdir().unwrap();
    let report = r#"{"message":"slow audit","type":"bug","impact":"worked_around",
        "agent":{"name":"claude-code","commands":["fallow audit"]}}"#;
    let output = feedback(
        bin.path(),
        &[
            "--input-json",
            "-",
            "--type",
            "friction",
            "--no-diagnostics",
            "--format",
            "json",
        ],
        Some(report),
    );
    assert_eq!(output.code, 0, "{}", output.stderr);
    let json = parse_json(&output);
    let body = json["feedback"]["body"].as_str().unwrap();
    assert_eq!(
        json["feedback"]["title"],
        "Agent feedback (friction): slow audit"
    );
    assert!(body.contains("**Impact:** worked around"), "{body}");
    assert!(body.contains("claude-code"), "{body}");
    assert!(!body.contains("### Environment"), "{body}");
}

#[test]
fn missing_message_is_invalid_input() {
    let bin = tempfile::tempdir().unwrap();
    let output = feedback(bin.path(), &["--format", "json"], None);
    assert_eq!(output.code, 2, "{}", output.stdout);
    assert!(
        output.stdout.contains("feedback message"),
        "{}",
        output.stdout
    );
}

#[test]
fn yes_without_gh_returns_the_link_and_exits_3() {
    let bin = tempfile::tempdir().unwrap();
    let output = feedback(bin.path(), &["note", "--yes", "--format", "json"], None);
    assert_eq!(output.code, 3, "{}", output.stderr);
    let json = parse_json(&output);
    assert_eq!(json["feedback"]["status"], "manual");
    assert!(
        json["feedback"]["reason"]
            .as_str()
            .unwrap()
            .contains("not installed")
    );
}

#[cfg(unix)]
#[test]
fn yes_creates_the_issue_with_gh() {
    let bin = tempfile::tempdir().unwrap();
    let log = bin.path().join("args.log");
    let body = bin.path().join("body.md");
    fake_gh(
        bin.path(),
        &format!(
            "printf '%s\\n' \"$@\" > '{}'\n/bin/cat > '{}'\necho https://github.com/fallow-rs/fallow/issues/4242",
            log.display(),
            body.display()
        ),
    );
    let output = feedback(
        bin.path(),
        &["gh path works", "--yes", "--format", "json"],
        None,
    );
    assert_eq!(output.code, 0, "{}", output.stderr);
    let json = parse_json(&output);
    assert_eq!(json["feedback"]["status"], "created");
    assert_eq!(
        json["feedback"]["issue_url"],
        "https://github.com/fallow-rs/fallow/issues/4242"
    );
    let args = std::fs::read_to_string(&log).unwrap();
    assert!(
        args.starts_with("issue\ncreate\n--repo\ngithub.com/fallow-rs/fallow\n--title\nFeedback (friction): gh path works\n--body-file\n-\n"),
        "{args}"
    );
    let sent = std::fs::read_to_string(&body).unwrap();
    assert_eq!(sent, json["feedback"]["body"].as_str().unwrap());
}

#[cfg(unix)]
#[test]
fn failed_gh_run_is_manual() {
    let bin = tempfile::tempdir().unwrap();
    fake_gh(
        bin.path(),
        "/bin/cat > /dev/null\necho 'To get started with GitHub CLI, please run:  gh auth login' >&2\nexit 4",
    );
    let output = feedback(bin.path(), &["note", "--yes"], None);
    assert_eq!(output.code, 3, "{}", output.stdout);
    assert!(output.stderr.contains("gh auth login"), "{}", output.stderr);
    assert!(
        output.stdout.contains("issues/new?title="),
        "{}",
        output.stdout
    );
}

#[test]
fn a_field_value_may_start_with_a_dash() {
    let bin = tempfile::tempdir().unwrap();
    let output = feedback(
        bin.path(),
        &["note", "--actual", "- first item", "--format", "json"],
        None,
    );
    assert_eq!(output.code, 0, "{}", output.stderr);
    let json = parse_json(&output);
    let body = json["feedback"]["body"].as_str().unwrap();
    assert!(body.contains("- first item"), "{body}");
}

#[test]
fn a_stale_confirm_digest_creates_nothing() {
    let bin = tempfile::tempdir().unwrap();
    let preview = parse_json(&feedback(bin.path(), &["note", "--format", "json"], None));
    let digest = preview["feedback"]["digest"].as_str().unwrap().to_owned();
    let output = feedback(
        bin.path(),
        &[
            "other note",
            "--yes",
            "--confirm",
            &digest,
            "--format",
            "json",
        ],
        None,
    );
    assert_eq!(output.code, 2, "{}", output.stdout);
    assert!(
        output.stdout.contains("changed since the preview"),
        "{}",
        output.stdout
    );
}
