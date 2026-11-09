use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};

use super::common::{fallow_bin, scrub_analysis_env};

const FOREIGN: &str = "--- a/nowhere/x.ts\n+++ b/nowhere/x.ts\n@@ -1 +1 @@\n-a\n+b\n";

fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::write(
        root.join("package.json"),
        r#"{"name":"p","main":"src/math.ts"}"#,
    )
    .unwrap();
    fs::create_dir(root.join("src")).unwrap();
    fs::write(
        root.join("src/math.ts"),
        "export function add(a: number, b: number) { return a + b; }\n",
    )
    .unwrap();
    super::common::git(root, &["init", "-q"]);
    super::common::commit_all(root, "init");
    fs::write(
        root.join("src/math.ts"),
        "export function add(a: number, b: number) { return a - b; }\n",
    )
    .unwrap();
    dir
}

fn invoke(root: &std::path::Path, args: &[&str], stdin: &str) -> (i32, serde_json::Value, String) {
    invoke_env(root, args, stdin, None, None)
}

fn invoke_env(
    root: &std::path::Path,
    args: &[&str],
    stdin: &str,
    ambient: Option<&std::path::Path>,
    temp: Option<&std::path::Path>,
) -> (i32, serde_json::Value, String) {
    let mut command = Command::new(fallow_bin());
    scrub_analysis_env(&mut command);
    command.env_remove("FALLOW_DIFF_FILE");
    if let Some(path) = ambient {
        command.env("FALLOW_DIFF_FILE", path);
    }
    if let Some(path) = temp {
        command
            .env("TMPDIR", path)
            .env("TMP", path)
            .env("TEMP", path);
    }
    let mut child = command
        .args(["mutation", "--root"])
        .arg(root)
        .args([
            "--changed-since",
            "HEAD",
            "--format",
            "json",
            "--quiet",
            "--no-cache",
        ])
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    let value = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|_| panic!("{} {stderr}", String::from_utf8_lossy(&output.stdout)));
    (output.status.code().unwrap(), value, stderr)
}

#[test]
fn explicit_unplaceable_diff_file_and_stdin_refuse_generated_scope() {
    let project = project();
    let root = project.path();
    let patch = tempfile::NamedTempFile::new().unwrap();
    fs::write(patch.path(), FOREIGN).unwrap();
    let (code, generated, stderr) = invoke(root, &["--plan"], "");
    assert_eq!(code, 0, "{stderr}");
    assert!(
        generated["mutants"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["file"] == "src/math.ts")
    );
    for flags in [
        vec!["--diff-file", patch.path().to_str().unwrap()],
        vec!["--diff-stdin"],
        vec!["--diff-file", "-"],
    ] {
        let mut args = vec!["--plan"];
        args.extend(flags);
        let (code, value, stderr) = invoke(root, &args, FOREIGN);
        assert_eq!(code, 2, "{value} {stderr}");
        assert_eq!(value["code"], "FALLOW_MUTATION_DIFF_NOT_APPLIED", "{value}");
        assert!(stderr.contains("cannot place"), "{stderr}");
    }
}

#[test]
fn stdin_preserves_empty_and_placed_hunks_over_ambient_diff() {
    let project = project();
    let root = project.path();
    fs::write(
        root.join("src/other.ts"),
        "export function twice(n: number) { return n * 2; }\n",
    )
    .unwrap();
    let ambient = tempfile::NamedTempFile::new().unwrap();
    fs::write(ambient.path(), FOREIGN).unwrap();
    let placed = "--- a/src/other.ts\n+++ b/src/other.ts\n@@ -1 +1 @@\n-a\n+b\n";
    for flags in [
        vec!["--plan", "--diff-stdin"],
        vec!["--plan", "--diff-file", "-"],
    ] {
        let (code, report, stderr) = invoke_env(root, &flags, placed, Some(ambient.path()), None);
        assert_eq!(code, 0, "{report} {stderr}");
        assert!(
            report["mutants"]
                .as_array()
                .unwrap()
                .iter()
                .any(|m| m["file"] == "src/other.ts")
        );
        assert!(
            report["mutants"]
                .as_array()
                .unwrap()
                .iter()
                .all(|m| m["file"] == "src/other.ts")
        );
        let (code, report, stderr) = invoke_env(root, &flags, "", Some(ambient.path()), None);
        assert_eq!(code, 0, "{report} {stderr}");
        assert!(report["mutants"].as_array().unwrap().is_empty());
        let (code, report, stderr) = invoke_env(root, &flags, FOREIGN, Some(ambient.path()), None);
        assert_eq!(code, 2, "{report} {stderr}");
        assert_eq!(report["code"], "FALLOW_MUTATION_DIFF_NOT_APPLIED");
        assert!(
            report["message"].as_str().unwrap().contains("--diff-stdin"),
            "{report}"
        );
    }
    let (code, report, stderr) = invoke_env(root, &["--plan"], "", Some(ambient.path()), None);
    assert_eq!(code, 0, "{report} {stderr}");
    assert!(
        report["mutants"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["file"] == "src/math.ts")
    );
    let (code, _, stderr) = invoke(
        root,
        &[
            "--plan",
            "--diff-stdin",
            "--diff-file",
            ambient.path().to_str().unwrap(),
        ],
        "",
    );
    assert_eq!(code, 2, "{stderr}");
}

#[test]
fn invalid_explicit_scope_stops_before_project_tests_execute() {
    let project = project();
    let root = project.path();
    fs::write(
        root.join("package.json"),
        r#"{"name":"p","type":"module","main":"src/math.ts","scripts":{"test":"node --test"}}"#,
    )
    .unwrap();
    fs::create_dir(root.join("test")).unwrap();
    fs::write(root.join("test/math.test.ts"), "import { test } from 'node:test';\nimport { writeFileSync } from 'node:fs';\nimport { add } from '../src/math.ts';\ntest('add', () => { writeFileSync('runner-started', 'yes'); if (add(1, 2) !== -1) throw new Error('wrong sum'); });\n").unwrap();
    let patch = tempfile::NamedTempFile::new().unwrap();
    fs::write(patch.path(), FOREIGN).unwrap();
    for flags in [
        vec!["--diff-file", patch.path().to_str().unwrap()],
        vec!["--diff-stdin"],
        vec!["--diff-file", "-"],
    ] {
        let mut args = vec!["--allow-dirty", "--max-mutants", "1"];
        args.extend(flags);
        let (code, report, stderr) = invoke(root, &args, FOREIGN);
        assert_eq!(code, 2, "{report} {stderr}");
        assert_eq!(report["code"], "FALLOW_MUTATION_DIFF_NOT_APPLIED");
        assert!(!root.join("runner-started").exists());
    }
    let (_, report, stderr) = invoke(root, &["--allow-dirty", "--max-mutants", "1"], "");
    assert_eq!(
        fs::read_to_string(root.join("runner-started")).unwrap_or_default(),
        "yes",
        "{report} {stderr}"
    );
    assert_eq!(report["runner"]["name"], "node-test", "{report}");
}

#[test]
fn ambiguous_stdin_and_file_refuse_a_nested_root() {
    let project = project();
    let root = project.path();
    let nested = root.join("packages/app");
    fs::create_dir_all(nested.join("src")).unwrap();
    fs::copy(root.join("package.json"), nested.join("package.json")).unwrap();
    fs::copy(root.join("src/math.ts"), nested.join("src/math.ts")).unwrap();
    let patch = tempfile::NamedTempFile::new().unwrap();
    let text = "--- a/src/math.ts\n+++ b/src/math.ts\n@@ -1 +1 @@\n-a\n+b\n";
    fs::write(patch.path(), text).unwrap();
    let placed = text.replace("src/math.ts", "packages/app/src/math.ts");
    let (code, generated, stderr) = invoke(&nested, &["--plan", "--diff-stdin"], &placed);
    assert_eq!(code, 0, "{generated} {stderr}");
    assert!(
        generated["mutants"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["file"] == "src/math.ts")
    );
    for flags in [
        vec!["--plan", "--diff-file", patch.path().to_str().unwrap()],
        vec!["--plan", "--diff-stdin"],
    ] {
        let (code, report, stderr) = invoke(&nested, &flags, text);
        assert_eq!(code, 2, "{report} {stderr}");
        assert_eq!(report["code"], "FALLOW_MUTATION_DIFF_NOT_APPLIED");
        assert!(
            report["message"]
                .as_str()
                .unwrap()
                .contains("ambiguous-base")
        );
        assert!(stderr.contains("ambiguous"), "{stderr}");
    }
}

#[test]
fn unreadable_explicit_file_keeps_its_input_error() {
    let project = project();
    let (code, report, stderr) = invoke(
        project.path(),
        &["--plan", "--diff-file", "missing.diff"],
        "",
    );
    assert_eq!(code, 2, "{report} {stderr}");
    assert_eq!(report["code"], "FALLOW_INVALID_DIFF_FILE");
}

#[path = "mutation_stdin_temp_tests.rs"]
mod stdin_temp;
