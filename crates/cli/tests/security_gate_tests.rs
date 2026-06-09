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

/// Run `fallow security --gate <gate>` against `root`, optionally piping
/// `stdin` (a unified diff for `--diff-stdin`). Returns `(exit_code, stdout)`.
fn run_security_gate(
    root: &Path,
    gate: &str,
    extra: &[&str],
    stdin: Option<&str>,
) -> (i32, String) {
    let mut cmd = Command::new(fallow_bin());
    cmd.args(["security", "--gate", gate, "--format", "json", "--quiet"])
        .arg("--root")
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

fn run_security_gate_new(root: &Path, extra: &[&str], stdin: Option<&str>) -> (i32, String) {
    run_security_gate(root, "new", extra, stdin)
}

fn write_reachability_project(root: &Path, imports_component: bool) {
    std::fs::create_dir_all(root.join("src")).expect("src dir");
    std::fs::write(
        root.join("package.json"),
        r#"{
  "name": "security-newly-reachable-fixture",
  "version": "0.0.0",
  "private": true,
  "main": "src/index.ts",
  "dependencies": {
    "react": "19.0.0"
  }
}
"#,
    )
    .expect("package");
    std::fs::write(
        root.join("tsconfig.json"),
        r#"{
  "compilerOptions": {
    "jsx": "react-jsx"
  },
  "include": ["src"]
}
"#,
    )
    .expect("tsconfig");
    std::fs::write(
        root.join("src/component.tsx"),
        "export const Markup = (props: { html: string }): JSX.Element => {\n  return <div dangerouslySetInnerHTML={{ __html: props.html }} />;\n};\n",
    )
    .expect("component");
    let index = if imports_component {
        "import { Markup } from './component';\n\nexport const render = Markup;\n"
    } else {
        "export const render = () => 'ok';\n"
    };
    std::fs::write(root.join("src/index.ts"), index).expect("index");
}

fn git(root: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(root)
        .status()
        .expect("git should run");
    assert!(status.success(), "git {args:?} should succeed");
}

fn commit(root: &Path, message: &str) {
    git(root, &["add", "."]);
    git(
        root,
        &[
            "-c",
            "user.name=Fallow Test",
            "-c",
            "user.email=fallow-test@example.com",
            "commit",
            "-m",
            message,
        ],
    );
}

fn newly_reachable_repo(imports_in_base: bool, imports_in_head: bool) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    git(dir.path(), &["init", "-q"]);
    write_reachability_project(dir.path(), imports_in_base);
    commit(dir.path(), "base");
    write_reachability_project(dir.path(), imports_in_head);
    std::fs::write(
        dir.path().join("src/change.ts"),
        format!("export const marker = '{imports_in_base}-{imports_in_head}';\n"),
    )
    .expect("head marker");
    commit(dir.path(), "head");
    dir
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
    let (code, stdout) = run_security_gate_new(&root, &["--diff-stdin"], Some(SINK_DIFF));
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
    let (code, stdout) = run_security_gate_new(&root, &["--diff-stdin"], Some(NON_SINK_DIFF));
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
    let (code, _) = run_security_gate_new(&root, &[], None);
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
    let (code, stdout) = run_security_gate_new(
        &root,
        &["--diff-stdin", "--fail-on-issues"],
        Some(NON_SINK_DIFF),
    );
    assert_eq!(
        code, 0,
        "gate must supersede --fail-on-issues when no new sink; stdout: {stdout}"
    );
}

#[test]
fn newly_reachable_gate_exits_8_when_existing_sink_becomes_entry_reachable() {
    let repo = newly_reachable_repo(false, true);
    let (code, stdout) = run_security_gate(
        repo.path(),
        "newly-reachable",
        &["--changed-since", "HEAD~1", "--no-cache"],
        None,
    );
    assert_eq!(
        code, 8,
        "existing sink becoming entry-reachable must exit 8; stdout: {stdout}"
    );
    assert!(
        stdout.contains("\"mode\": \"newly-reachable\""),
        "stdout: {stdout}"
    );
    assert!(stdout.contains("\"new_count\": 1"), "stdout: {stdout}");
}

#[test]
fn newly_reachable_gate_exits_0_when_sink_was_already_reachable_in_base() {
    let repo = newly_reachable_repo(true, true);
    let (code, stdout) = run_security_gate(
        repo.path(),
        "newly-reachable",
        &["--changed-since", "HEAD~1", "--no-cache"],
        None,
    );
    assert_eq!(
        code, 0,
        "already-reachable sink must not trip newly-reachable gate; stdout: {stdout}"
    );
    assert!(stdout.contains("\"verdict\": \"pass\""), "stdout: {stdout}");
}

#[test]
fn newly_reachable_gate_exits_0_when_sink_remains_unreachable_in_head() {
    let repo = newly_reachable_repo(false, false);
    let (code, stdout) = run_security_gate(
        repo.path(),
        "newly-reachable",
        &["--changed-since", "HEAD~1", "--no-cache"],
        None,
    );
    assert_eq!(
        code, 0,
        "unreachable sink must not trip newly-reachable gate; stdout: {stdout}"
    );
    assert!(stdout.contains("\"new_count\": 0"), "stdout: {stdout}");
}

#[test]
fn newly_reachable_gate_exits_2_with_diff_only_input() {
    let root = fixture_path("security-dangerous-html");
    let (code, _) = run_security_gate(
        &root,
        "newly-reachable",
        &["--diff-stdin"],
        Some(NON_SINK_DIFF),
    );
    assert_eq!(
        code, 2,
        "newly-reachable gate requires a base ref, not only a diff"
    );
}
