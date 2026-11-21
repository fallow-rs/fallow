//! Shared git fixture for the `audit` and `decision_surface` tests that pass
//! a `root` the base commit does not contain (#2699): a repository whose
//! branch commit adds `apps/new`, with the remote default left on the commit
//! before it, so the auto-detected base has no counterpart for that root.
//!
//! This file also gives the MCP tool tests their shared git runner, from
//! `fallow_api::git_fixture`. The runner removes the ambient repository state
//! and ignores the global and system config, so a test that runs inside a git
//! hook or a `git rebase -x` step cannot write into the enclosing repository.

use std::path::Path;

/// Project-relative path of the package the branch commit adds.
pub(super) const NEW_PACKAGE: &str = "apps/new";

/// Build the fixture. Drop the guard to delete the directory.
pub(super) fn new_package_repo() -> tempfile::TempDir {
    let project = tempfile::tempdir().expect("project");
    let root = project.path();
    std::fs::create_dir_all(root.join("src")).expect("create src");
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"base-root-fixture","type":"module","main":"src/index.ts"}"#,
    )
    .expect("write package");
    std::fs::write(root.join("src/index.ts"), "console.log('root');\n").expect("write entry");
    git(root, &["init", "-b", "main"]);
    commit_all(root, "initial");

    let base_commit = git_capture(root, &["rev-parse", "HEAD"]);
    git_capture(
        root,
        &["update-ref", "refs/remotes/origin/main", &base_commit],
    );
    git_capture(
        root,
        &[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/main",
        ],
    );

    let package = root.join(NEW_PACKAGE);
    std::fs::create_dir_all(package.join("src")).expect("create package src");
    std::fs::write(
        package.join("package.json"),
        r#"{"name":"new-package","type":"module","main":"src/index.ts"}"#,
    )
    .expect("write package manifest");
    std::fs::write(package.join("src/index.ts"), "console.log('entry');\n")
        .expect("write package entry");
    std::fs::write(package.join("src/dead.ts"), "export const unused = 1;\n")
        .expect("write unused file");
    commit_all(root, "add package");
    project
}

fn commit_all(root: &Path, message: &str) {
    git(root, &["add", "."]);
    git(
        root,
        &[
            "-c",
            "user.email=test@example.com",
            "-c",
            "user.name=Test",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-m",
            message,
        ],
    );
}

pub(super) use fallow_api::git_fixture::{fixture_git as git, fixture_git_capture as git_capture};
