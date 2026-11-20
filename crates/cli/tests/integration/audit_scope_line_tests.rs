//! The human audit scope line names the commit range that the audit compared:
//! the merge base of the base ref and `HEAD`, up to `HEAD` (issue #4838).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

use std::fs;

use tempfile::TempDir;

use crate::common::{commit_all, git, git_capture, run_fallow_raw};

#[test]
fn scope_line_starts_the_range_at_the_merge_base() {
    let tmp = TempDir::new().expect("failed to create temp dir");
    let dir = tmp.path();
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("package.json"),
        r#"{"name": "audit-scope", "private": true, "main": "src/index.js"}"#,
    )
    .unwrap();
    fs::write(dir.join("src/index.js"), "export const a = 1;\n").unwrap();
    git(dir, &["init", "-b", "main"]);
    git(dir, &["config", "core.autocrlf", "false"]);
    commit_all(dir, "baseline");
    git(dir, &["checkout", "-b", "head"]);
    fs::write(dir.join("src/index.js"), "export const a = 2;\n").unwrap();
    commit_all(dir, "change");
    // A later commit on main makes the main tip differ from the merge base.
    git(dir, &["checkout", "main"]);
    fs::write(dir.join("README.md"), "main moved on\n").unwrap();
    commit_all(dir, "main moves");
    git(dir, &["checkout", "head"]);

    let merge_base = git_capture(dir, &["merge-base", "main", "HEAD"]);
    let base_short = git_capture(dir, &["rev-parse", "--short", merge_base.trim()]);
    let head_short = git_capture(dir, &["rev-parse", "--short", "HEAD"]);

    let output = run_fallow_raw(&[
        "audit",
        "--root",
        dir.to_str().unwrap(),
        "--base",
        "main",
        "--no-cache",
        "--no-css",
        "--skip",
        "dupes",
    ]);

    let expected = format!("vs main ({}..HEAD)", base_short.trim());
    assert!(
        output.stderr.contains(&expected),
        "scope line lacks {expected}:\n{}",
        output.stderr
    );
    assert!(
        !output
            .stderr
            .contains(&format!("({}..HEAD)", head_short.trim())),
        "scope line starts the range at HEAD:\n{}",
        output.stderr
    );
}
