//! `fallow audit --staged` audits the staged index, not the working tree.
//!
//! The changed files are the staged changes against HEAD. The analysis reads
//! each file from the index, so unstaged edits and untracked files do not
//! change the verdict, and a staged file that is deleted from the working
//! tree is still audited.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

use std::fs;
use std::path::Path;
use std::process::Command;

use tempfile::TempDir;

use crate::common::{
    CommandOutput, commit_all, fallow_bin, git, git_command, parse_json, scrub_analysis_env,
};

/// A repository with one commit on `main`: `src/index.ts` is the entry and
/// exports `a`. A `node_modules` package is present and not ignored, so the
/// staged worktree links it.
fn fixture() -> TempDir {
    let tmp = TempDir::new().expect("failed to create temp dir");
    let dir = tmp.path();
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("package.json"),
        r#"{"name": "audit-staged", "private": true, "main": "src/index.ts"}"#,
    )
    .unwrap();
    fs::write(dir.join("src/index.ts"), "export const a = 1;\n").unwrap();
    git(dir, &["init", "-b", "main"]);
    git(dir, &["config", "core.autocrlf", "false"]);
    commit_all(dir, "baseline");
    fs::create_dir_all(dir.join("node_modules/left-pad")).unwrap();
    fs::write(
        dir.join("node_modules/left-pad/package.json"),
        r#"{"name": "left-pad", "version": "1.0.0"}"#,
    )
    .unwrap();
    tmp
}

/// Run `fallow audit --staged` in `dir`. `index_file` is the
/// `GIT_INDEX_FILE` of the run. Without it, the run gets no
/// `GIT_INDEX_FILE`, also when the test suite runs inside a Git hook.
fn audit_staged_with_index(dir: &Path, extra: &[&str], index_file: Option<&Path>) -> CommandOutput {
    let mut command = audit_staged_command(dir, extra);
    if let Some(index_file) = index_file {
        command.env("GIT_INDEX_FILE", index_file);
    }
    run(&mut command)
}

/// A `fallow audit --staged` command for `dir` with no `GIT_INDEX_FILE`.
fn audit_staged_command(dir: &Path, extra: &[&str]) -> Command {
    let mut command = Command::new(fallow_bin());
    command.env("RUST_LOG", "").env("NO_COLOR", "1");
    scrub_analysis_env(&mut command);
    command.env_remove("GIT_INDEX_FILE");
    command
        .args(["audit", "--root"])
        .arg(dir)
        .args(["--staged", "--format", "json", "--quiet"])
        .args(extra);
    command
}

fn run(command: &mut Command) -> CommandOutput {
    let output = command.output().expect("failed to run fallow binary");
    CommandOutput {
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        code: output.status.code().unwrap_or(-1),
    }
}

fn audit_staged(dir: &Path, extra: &[&str]) -> CommandOutput {
    audit_staged_with_index(dir, extra, None)
}

fn unused_file_paths(json: &serde_json::Value) -> Vec<String> {
    json["dead_code"]["unused_files"]
        .as_array()
        .map(|files| {
            files
                .iter()
                .filter_map(|file| file["path"].as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn untracked_and_unstaged_files_do_not_affect_the_verdict() {
    let tmp = fixture();
    let dir = tmp.path();
    fs::write(
        dir.join("src/index.ts"),
        "export const a = 1;\nexport const b = a;\n",
    )
    .unwrap();
    git(dir, &["add", "src/index.ts"]);
    fs::write(dir.join("src/untracked-orphan.ts"), "export const z = 3;\n").unwrap();
    fs::write(
        dir.join("src/index.ts"),
        "import './unstaged-orphan';\nexport const a = 1;\nexport const b = a;\n",
    )
    .unwrap();

    let output = audit_staged(dir, &[]);
    let json = parse_json(&output);

    assert_eq!(output.code, 0, "stderr: {}\njson: {json}", output.stderr);
    assert_eq!(json["verdict"], "pass", "{json}");
    assert_eq!(json["changed_files_count"], 1, "{json}");
    assert!(unused_file_paths(&json).is_empty(), "{json}");
}

#[test]
fn a_staged_file_deleted_from_the_working_tree_is_audited() {
    let tmp = fixture();
    let dir = tmp.path();
    fs::write(dir.join("src/staged-orphan.ts"), "export const q = 9;\n").unwrap();
    git(dir, &["add", "src/staged-orphan.ts"]);
    fs::remove_file(dir.join("src/staged-orphan.ts")).unwrap();

    let output = audit_staged(dir, &[]);
    let json = parse_json(&output);

    assert_eq!(output.code, 1, "stderr: {}\njson: {json}", output.stderr);
    assert_eq!(json["verdict"], "fail", "{json}");
    assert_eq!(
        unused_file_paths(&json),
        vec!["src/staged-orphan.ts".to_owned()],
        "{json}"
    );
}

/// `git commit -a` and `git commit <path>` give the hook another index file
/// through `GIT_INDEX_FILE`. The staged audit reads that index.
#[test]
fn the_index_file_that_git_gives_the_hook_is_audited() {
    let tmp = fixture();
    let dir = tmp.path();
    let index_file = dir.join(".git/next-index-test");
    fs::copy(dir.join(".git/index"), &index_file).unwrap();
    fs::write(dir.join("src/staged-orphan.ts"), "export const q = 9;\n").unwrap();
    let status = git_command(dir)
        .env("GIT_INDEX_FILE", &index_file)
        .args(["add", "src/staged-orphan.ts"])
        .status()
        .unwrap();
    assert!(status.success());
    fs::remove_file(dir.join("src/staged-orphan.ts")).unwrap();

    let default_index = audit_staged(dir, &[]);
    assert_eq!(default_index.code, 0, "stderr: {}", default_index.stderr);

    let output = audit_staged_with_index(dir, &[], Some(&index_file));
    let json = parse_json(&output);
    assert_eq!(output.code, 1, "stderr: {}\njson: {json}", output.stderr);
    assert_eq!(
        unused_file_paths(&json),
        vec!["src/staged-orphan.ts".to_owned()],
        "{json}"
    );
}

/// A plain `git commit` gives the hook the relative value `.git/index`. Git
/// resolves it against the top level of the work tree, also when the hook
/// runs `fallow` from a subdirectory.
#[test]
fn a_relative_index_file_resolves_against_the_top_level() {
    let tmp = fixture();
    let dir = tmp.path();
    fs::write(dir.join("src/staged-orphan.ts"), "export const q = 9;\n").unwrap();
    git(dir, &["add", "src/staged-orphan.ts"]);

    let output = run(audit_staged_command(dir, &[])
        .current_dir(dir.join("src"))
        .env("GIT_INDEX_FILE", ".git/index"));
    let json = parse_json(&output);

    assert_eq!(output.code, 1, "stderr: {}\njson: {json}", output.stderr);
    assert_eq!(
        unused_file_paths(&json),
        vec!["src/staged-orphan.ts".to_owned()],
        "{json}"
    );
}

#[test]
fn the_staged_worktree_is_removed_after_the_run() {
    let tmp = fixture();
    let dir = tmp.path();
    fs::write(dir.join("src/index.ts"), "export const a = 2;\n").unwrap();
    git(dir, &["add", "src/index.ts"]);

    let output = audit_staged(dir, &[]);
    assert_eq!(output.code, 0, "stderr: {}", output.stderr);

    let worktrees = git_command(dir)
        .args(["worktree", "list", "--porcelain"])
        .output()
        .unwrap();
    let listing = String::from_utf8_lossy(&worktrees.stdout);
    assert_eq!(
        listing.matches("worktree ").count(),
        1,
        "only the main worktree stays registered: {listing}"
    );
}

#[test]
fn staged_rejects_an_explicit_base() {
    let tmp = fixture();
    let output = audit_staged(tmp.path(), &["--base", "main"]);
    assert_eq!(output.code, 2, "stderr: {}", output.stderr);
    assert!(
        output.stdout.contains("does not take --base")
            || output.stderr.contains("does not take --base"),
        "stdout: {}\nstderr: {}",
        output.stdout,
        output.stderr
    );
}

/// Run `git commit` in `dir` with a `PATH` that holds only the built fallow
/// binary and the system tools, the way a Git client starts the hook.
#[cfg(unix)]
fn commit_through_hook(dir: &Path, bin: &Path, message: &str) -> std::process::Output {
    let path = format!("{}:/usr/bin:/bin", bin.display());
    git_command(dir)
        .env("PATH", path)
        .args(["commit", "-q", "-m", message])
        .output()
        .expect("git commit runs")
}

/// The generated hook gates the commit on the staged index. An untracked
/// orphan does not block a clean commit, and a staged orphan that is deleted
/// from the working tree blocks the commit.
#[cfg(unix)]
#[test]
fn the_generated_hook_audits_the_staged_index() {
    let tmp = fixture();
    let dir = tmp.path();
    let install = crate::common::run_fallow_raw(&[
        "--root",
        dir.to_str().unwrap(),
        "hooks",
        "install",
        "--target",
        "git",
    ]);
    assert_eq!(install.code, 0, "stderr: {}", install.stderr);
    let bin = TempDir::new().unwrap();
    std::os::unix::fs::symlink(fallow_bin(), bin.path().join("fallow")).unwrap();

    fs::write(
        dir.join("src/index.ts"),
        "export const a = 1;\nexport const b = a;\n",
    )
    .unwrap();
    git(dir, &["add", "src/index.ts"]);
    fs::write(dir.join("src/untracked-orphan.ts"), "export const z = 3;\n").unwrap();
    let clean = commit_through_hook(dir, bin.path(), "edit index");
    assert!(
        clean.status.success(),
        "an untracked file must not block the commit: {}",
        String::from_utf8_lossy(&clean.stderr)
    );

    fs::remove_file(dir.join("src/untracked-orphan.ts")).unwrap();
    fs::write(dir.join("src/staged-orphan.ts"), "export const q = 9;\n").unwrap();
    git(dir, &["add", "src/staged-orphan.ts"]);
    fs::remove_file(dir.join("src/staged-orphan.ts")).unwrap();
    let blocked = commit_through_hook(dir, bin.path(), "add staged orphan");
    assert_eq!(
        blocked.status.code(),
        Some(1),
        "a staged orphan must block the commit: {}",
        String::from_utf8_lossy(&blocked.stderr)
    );
}
