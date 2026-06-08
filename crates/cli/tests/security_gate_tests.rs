//! End-to-end exit-code contract for the `fallow security --gate new` regression
//! gate (issue #886): a new security-sink candidate on a changed LINE exits 8; a
//! diff that touches the file but not the sink line exits 0; a gate with no diff
//! source hard-errors (exit 2), never a green gate.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests use unwrap/expect to keep fixture setup concise"
)]

#[path = "common/mod.rs"]
mod common;

use common::{fallow_bin, fixture_path};
use std::io::Write as _;
use std::path::Path;
use std::process::{Command, Stdio};

/// Run `fallow security --gate new` against `root`, optionally piping `stdin`
/// (a unified diff for `--diff-stdin`). Returns `(exit_code, stdout)`.
fn run_security_gate(root: &Path, extra: &[&str], stdin: Option<&str>) -> (i32, String) {
    let mut cmd = Command::new(fallow_bin());
    cmd.args([
        "security", "--gate", "new", "--format", "json", "--quiet", "--root",
    ])
    .arg(root)
    .args(extra)
    .env("RUST_LOG", "")
    .env("NO_COLOR", "1")
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());
    if stdin.is_some() {
        cmd.stdin(Stdio::piped());
    }
    let mut child = cmd.spawn().unwrap();
    if let Some(text) = stdin {
        child
            .stdin
            .take()
            .unwrap()
            .write_all(text.as_bytes())
            .unwrap();
    }
    let out = child.wait_with_output().unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
    )
}

/// Adds the `dangerouslySetInnerHTML` sink line (the fixture's `src/component.tsx`
/// anchor is line 3) as a `+` line, so the gate sees a NEW sink.
const SINK_DIFF: &str = "diff --git a/src/component.tsx b/src/component.tsx\n\
--- a/src/component.tsx\n\
+++ b/src/component.tsx\n\
@@ -2,0 +3,1 @@\n\
+  return <div dangerouslySetInnerHTML={{ __html: props.html }} />;\n";

/// Touches the same file but on line 1 (a comment), NOT the sink line: the
/// pre-existing sink must NOT trip the gate.
const NON_SINK_DIFF: &str = "diff --git a/src/component.tsx b/src/component.tsx\n\
--- a/src/component.tsx\n\
+++ b/src/component.tsx\n\
@@ -0,0 +1,1 @@\n\
+// a fresh comment line\n";

#[test]
fn gate_exits_8_when_diff_adds_a_new_sink() {
    let root = fixture_path("security-dangerous-html");
    let (code, stdout) = run_security_gate(&root, &["--diff-stdin"], Some(SINK_DIFF));
    assert_eq!(
        code, 8,
        "a new sink in changed lines must exit 8; stdout: {stdout}"
    );
    assert!(stdout.contains("\"verdict\": \"fail\""), "stdout: {stdout}");
    assert!(stdout.contains("\"new_count\": 1"), "stdout: {stdout}");
}

#[test]
fn gate_exits_0_when_diff_touches_file_but_not_sink_line() {
    let root = fixture_path("security-dangerous-html");
    let (code, stdout) = run_security_gate(&root, &["--diff-stdin"], Some(NON_SINK_DIFF));
    assert_eq!(
        code, 0,
        "a pre-existing sink in a touched file (anchor not added) must exit 0; stdout: {stdout}"
    );
    assert!(stdout.contains("\"verdict\": \"pass\""), "stdout: {stdout}");
    assert!(stdout.contains("\"new_count\": 0"), "stdout: {stdout}");
}

#[test]
fn gate_exits_2_without_a_diff_source() {
    let root = fixture_path("security-dangerous-html");
    let (code, _) = run_security_gate(&root, &[], None);
    assert_eq!(
        code, 2,
        "a gate with no diff source must hard-error (exit 2), never a green gate"
    );
}

#[test]
fn gate_supersedes_fail_on_issues_when_no_new_sink() {
    // `--fail-on-issues` alone would exit 1 (the fixture has pre-existing
    // candidates). In gate mode the gate is authoritative: no NEW sink in the
    // changed lines exits 0, NOT 1 (the gate must not re-gate the backlog).
    let root = fixture_path("security-dangerous-html");
    let (code, stdout) = run_security_gate(
        &root,
        &["--diff-stdin", "--fail-on-issues"],
        Some(NON_SINK_DIFF),
    );
    assert_eq!(
        code, 0,
        "gate must supersede --fail-on-issues when no new sink; stdout: {stdout}"
    );
}
