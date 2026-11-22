#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

//! `fallow hooks install --target git` writes the hook where Git runs it:
//! `core.hooksPath` when set, and the common hooks folder of a linked
//! worktree.

use std::fs;
use std::path::{Path, PathBuf};

use crate::common::{commit_all, git, parse_json, run_fallow_raw};

fn repo(suffix: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "fallow-git-hook-location-{}-{suffix}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    let main = dir.join("main");
    fs::create_dir_all(&main).unwrap();
    fs::write(main.join("package.json"), r#"{"name":"hook-test"}"#).unwrap();
    git(&main, &["init", "-q"]);
    commit_all(&main, "initial");
    dir
}

fn hooks(root: &Path, args: &[&str]) -> crate::common::CommandOutput {
    let mut full = vec!["--root", root.to_str().unwrap(), "hooks"];
    full.extend_from_slice(args);
    run_fallow_raw(&full)
}

fn status_path(root: &Path) -> (bool, String) {
    let output = hooks(root, &["status", "--format", "json"]);
    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    let json = parse_json(&output);
    (
        json["hooks"]["git"]["installed"].as_bool().unwrap(),
        json["hooks"]["git"]["path"].as_str().unwrap().to_string(),
    )
}

#[test]
fn install_status_and_uninstall_follow_core_hooks_path() {
    let dir = repo("hooks-path");
    let main = dir.join("main");
    git(&main, &["config", "core.hooksPath", ".githooks"]);

    let install = hooks(&main, &["install", "--target", "git"]);
    assert_eq!(install.code, 0, "stderr: {}", install.stderr);
    assert!(main.join(".githooks/pre-commit").is_file());
    assert!(!main.join(".git/hooks/pre-commit").exists());
    assert_eq!(
        status_path(&main),
        (true, ".githooks/pre-commit".to_string())
    );

    let uninstall = hooks(&main, &["uninstall", "--target", "git"]);
    assert_eq!(uninstall.code, 0, "stderr: {}", uninstall.stderr);
    assert!(!main.join(".githooks/pre-commit").exists());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn install_in_a_linked_worktree_writes_the_common_hook() {
    let dir = repo("worktree");
    let main = dir.join("main");
    let linked = dir.join("linked");
    git(&main, &["worktree", "add", "-q", linked.to_str().unwrap()]);

    let install = hooks(&linked, &["install", "--target", "git"]);
    assert_eq!(install.code, 0, "stderr: {}", install.stderr);
    assert!(main.join(".git/hooks/pre-commit").is_file());
    assert!(status_path(&linked).0, "status must see the installed hook");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn install_refuses_a_shared_hooks_folder_outside_the_repository() {
    let dir = repo("shared");
    let main = dir.join("main");
    let shared = dir.join("shared-hooks");
    git(
        &main,
        &["config", "core.hooksPath", shared.to_str().unwrap()],
    );

    let install = hooks(&main, &["install", "--target", "git"]);
    assert_eq!(install.code, 2, "stderr: {}", install.stderr);
    assert!(
        install.stderr.contains("core.hooksPath"),
        "{}",
        install.stderr
    );
    assert!(!shared.join("pre-commit").exists());
    let _ = fs::remove_dir_all(&dir);
}
