//! The base pass of `fallow audit` and `fallow security --changed-since`
//! does not follow a link of the base commit out of the base worktree.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests use unwrap/expect to keep fixture setup concise"
)]

use crate::common::{CommandOutput, commit_all, git, run_fallow_raw};
use std::fs;
use std::path::Path;

fn write_app(dir: &Path) {
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("package.json"),
        r#"{"name":"app","private":true,"main":"src/index.ts"}"#,
    )
    .unwrap();
    fs::write(
        dir.join("src/index.ts"),
        "export const run = (html: string): void => {\n  document.body.innerHTML = html;\n};\n",
    )
    .unwrap();
}

/// A repository whose base commit has a link `packages` with the absolute
/// path of `outside` as its target,
/// and whose head commit has a real `packages/app`. Returns the temp dir;
/// the repository is at `repo` and the link target at `outside`.
fn repo_with_a_linked_base() -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    write_app(&temp.path().join("outside").join("app"));
    git(&repo, &["init", "-q", "-b", "main"]);
    fs::write(repo.join("README.md"), "base\n").unwrap();
    // An absolute target, so the copy of the link in the base worktree still
    // names this directory.
    let outside = fs::canonicalize(temp.path().join("outside")).unwrap();
    std::os::unix::fs::symlink(&outside, repo.join("packages")).unwrap();
    commit_all(&repo, "base with a link");
    fs::remove_file(repo.join("packages")).unwrap();
    write_app(&repo.join("packages").join("app"));
    commit_all(&repo, "head with a real directory");
    temp
}

fn assert_outside_untouched(temp: &Path, output: &CommandOutput) {
    assert_ne!(
        output.code, 2,
        "stdout: {}\nstderr: {}",
        output.stdout, output.stderr
    );
    let outside = temp.join("outside").join("app");
    assert!(
        !outside.join(".fallow").exists(),
        "the base pass wrote a cache into the link target"
    );
}

#[test]
fn audit_base_pass_does_not_follow_a_base_link_out_of_the_worktree() {
    let temp = repo_with_a_linked_base();
    let root = temp.path().join("repo").join("packages").join("app");
    let output = run_fallow_raw(&[
        "audit",
        "--root",
        root.to_str().unwrap(),
        "--base",
        "HEAD~1",
        "--format",
        "json",
        "--quiet",
        "--no-css",
    ]);
    assert_outside_untouched(temp.path(), &output);
}

#[test]
fn security_base_pass_does_not_follow_a_base_link_out_of_the_worktree() {
    let temp = repo_with_a_linked_base();
    let root = temp.path().join("repo").join("packages").join("app");
    let output = run_fallow_raw(&[
        "security",
        "--root",
        root.to_str().unwrap(),
        "--gate",
        "newly-reachable",
        "--changed-since",
        "HEAD~1",
        "--format",
        "json",
        "--quiet",
    ]);
    assert_outside_untouched(temp.path(), &output);
}
