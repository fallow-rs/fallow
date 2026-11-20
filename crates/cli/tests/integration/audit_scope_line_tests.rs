//! The human `audit` scope line names the commit range that the audit
//! compared. The range starts at the base commit, which is the merge-base of
//! the base ref and `HEAD`. A range that starts at `HEAD` reads as an empty
//! comparison (#4838).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

use std::fs;

use tempfile::TempDir;

use crate::common::{commit_all, git, git_capture, run_fallow_raw};

#[test]
fn scope_line_range_starts_at_the_base_commit() {
    let tmp = TempDir::new().expect("failed to create temp dir");
    let dir = tmp.path();
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("package.json"),
        r#"{"name": "scope-line", "private": true}"#,
    )
    .unwrap();
    fs::write(dir.join(".gitignore"), ".fallow\n").unwrap();
    fs::write(dir.join("src/index.js"), "export const a = 1;\n").unwrap();
    git(dir, &["init", "-b", "main"]);
    commit_all(dir, "base");
    let base_sha = git_capture(dir, &["rev-parse", "--short", "HEAD"]);
    git(dir, &["checkout", "-b", "feature"]);
    fs::write(dir.join("src/index.js"), "export const a = 2;\n").unwrap();
    commit_all(dir, "change");
    let head_sha = git_capture(dir, &["rev-parse", "--short", "HEAD"]);

    let output = run_fallow_raw(&["audit", "--root", dir.to_str().unwrap(), "--base", "main"]);

    let scope = output
        .stderr
        .lines()
        .find(|line| line.starts_with("Audit scope:"))
        .unwrap_or_else(|| panic!("scope line missing: {}", output.stderr));
    assert!(
        scope.ends_with(&format!("vs main ({base_sha}..HEAD)")),
        "scope line must start the range at the base commit {base_sha}, not HEAD {head_sha}: {scope}"
    );
}
