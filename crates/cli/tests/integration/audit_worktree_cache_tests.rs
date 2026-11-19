//! The base pass of `fallow audit` analyzes a temporary worktree. It reads
//! the parse cache of the checkout and never writes the caches of the
//! checkout, also when `FALLOW_CACHE_DIR` names a shared directory.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

use std::fs;
use std::path::Path;
use std::process::Command;

use tempfile::TempDir;

use crate::common::{commit_all, fallow_bin, git, scrub_analysis_env};

fn fallow(dir: &Path, cache_dir: &Path, args: &[&str]) -> (i32, String) {
    let mut command = Command::new(fallow_bin());
    command.env("RUST_LOG", "").env("NO_COLOR", "1");
    scrub_analysis_env(&mut command);
    command
        .env_remove("GIT_INDEX_FILE")
        .env("FALLOW_CACHE_DIR", cache_dir)
        .arg("--root")
        .arg(dir)
        .args(args)
        .args(["--format", "json", "--quiet"]);
    let output = command.output().expect("failed to run fallow binary");
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stderr).to_string(),
    )
}

/// A branch audit with a shared `FALLOW_CACHE_DIR` keeps the module graph
/// cache and the parse cache of the checkout valid. The base pass analyzes
/// another root and other file content, so its caches must stay in the base
/// worktree.
#[test]
fn a_branch_audit_keeps_the_shared_caches_of_the_checkout() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    let cache = TempDir::new().unwrap();
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("package.json"),
        r#"{"name": "audit-base-cache", "private": true, "main": "src/index.ts"}"#,
    )
    .unwrap();
    fs::write(dir.join("src/index.ts"), "export const a = 1;\n").unwrap();
    git(dir, &["init", "-b", "main"]);
    git(dir, &["config", "core.autocrlf", "false"]);
    commit_all(dir, "baseline");
    fs::write(dir.join("src/index.ts"), "export const a = 2;\n").unwrap();
    commit_all(dir, "change");

    let (code, stderr) = fallow(dir, cache.path(), &["dead-code"]);
    assert!(code <= 1, "stderr: {stderr}");
    let (code, stderr) = fallow(dir, cache.path(), &["audit", "--base", "HEAD~1"]);
    assert!(code <= 1, "stderr: {stderr}");

    let (_, stderr) = fallow(dir, cache.path(), &["dead-code", "--performance"]);
    let perf = stderr
        .lines()
        .find_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .unwrap_or_else(|| panic!("no performance JSON: {stderr}"));
    assert!(
        perf.get("graph_cache_rejection")
            .is_none_or(serde_json::Value::is_null),
        "the graph cache of the checkout must stay valid: {perf}"
    );
    assert_eq!(perf["cache_misses"], 0, "{perf}");
}
