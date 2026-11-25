//! A shell that changed into a directory through a committed link keeps the
//! link only in `PWD`. A root that is the current directory, or relative to
//! it, is checked with that spelling too.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests use unwrap/expect to keep fixture setup concise"
)]

use crate::common::{CommandOutput, commit_all, git, run_fallow_raw_in_dir_with_env};
use std::fs;
use std::path::{Path, PathBuf};

/// A repository `repo` with a committed link `packages` to `outside`, which
/// holds a project `app`. Returns the temp dir, the linked spelling of
/// `app`, and the real path of `app`.
fn linked_project() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let base = fs::canonicalize(temp.path()).unwrap();
    let repo = base.join("repo");
    fs::create_dir_all(&repo).unwrap();
    let app = base.join("outside").join("app");
    fs::create_dir_all(app.join("src")).unwrap();
    fs::write(app.join("package.json"), r#"{"name":"app","private":true}"#).unwrap();
    fs::write(app.join("src/index.ts"), "export const a = 1;\n").unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    std::os::unix::fs::symlink(base.join("outside"), repo.join("packages")).unwrap();
    commit_all(&repo, "link");
    (temp, repo.join("packages").join("app"), app)
}

fn run(cwd: &Path, pwd: &Path, args: &[&str]) -> CommandOutput {
    run_fallow_raw_in_dir_with_env(cwd, args, &[("PWD", pwd.to_str().unwrap())])
}

#[track_caller]
fn assert_refused(output: &CommandOutput) {
    assert_eq!(
        output.code, 2,
        "stdout: {}\nstderr: {}",
        output.stdout, output.stderr
    );
    let text = format!("{}{}", output.stdout, output.stderr);
    assert!(text.contains("outside the checkout"), "got: {text}");
}

#[test]
fn doctor_with_no_root_refuses_a_current_directory_reached_through_a_link() {
    let (_temp, linked, real) = linked_project();

    let output = run(&real, &linked, &["doctor", "--format", "json"]);
    assert_refused(&output);
}

#[test]
fn a_relative_root_is_checked_with_its_pwd_spelling() {
    let (_temp, linked, real) = linked_project();
    let parent = real.parent().unwrap();
    let linked_parent = linked.parent().unwrap();

    assert_refused(&run(&real, &linked, &["dead-code", "--quiet"]));
    assert_refused(&run(
        &real,
        &linked,
        &["dead-code", "--quiet", "--root", "."],
    ));
    assert_refused(&run(
        parent,
        linked_parent,
        &["dead-code", "--quiet", "--root", "app"],
    ));
    // The resolved absolute path is allowed, as the hint of the refusal says.
    let output = run(
        &real,
        &linked,
        &["dead-code", "--quiet", "--root", real.to_str().unwrap()],
    );
    assert_ne!(output.code, 2, "stderr: {}", output.stderr);
}
