#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

//! `fallow hooks install --target git` writes the hook where Git runs it:
//! `core.hooksPath` when set, and the common hooks folder of a linked
//! worktree.

use std::fs;
use std::path::Path;

use crate::common::{commit_all, git, parse_json, run_fallow_raw_with_env};

/// A null Git config, so a global `core.hooksPath` of the machine does not
/// change the result.
const NULL_CONFIG: &str = if cfg!(windows) { "NUL" } else { "/dev/null" };

/// A temp folder with a committed repository in `main`.
fn repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let main = dir.path().join("main");
    fs::create_dir_all(&main).unwrap();
    fs::write(main.join("package.json"), r#"{"name":"hook-test"}"#).unwrap();
    git(&main, &["init", "-q"]);
    commit_all(&main, "initial");
    dir
}

fn hooks(root: &Path, args: &[&str]) -> crate::common::CommandOutput {
    let mut full = vec!["--root", root.to_str().unwrap(), "hooks"];
    full.extend_from_slice(args);
    run_fallow_raw_with_env(
        &full,
        &[
            ("GIT_CONFIG_GLOBAL", NULL_CONFIG),
            ("GIT_CONFIG_SYSTEM", NULL_CONFIG),
        ],
    )
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
    let dir = repo();
    let main = dir.path().join("main");
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
}

#[test]
fn install_in_a_linked_worktree_writes_the_common_hook() {
    let dir = repo();
    let main = dir.path().join("main");
    let linked = dir.path().join("linked");
    git(&main, &["worktree", "add", "-q", linked.to_str().unwrap()]);

    let install = hooks(&linked, &["install", "--target", "git"]);
    assert_eq!(install.code, 0, "stderr: {}", install.stderr);
    assert!(main.join(".git/hooks/pre-commit").is_file());
    assert!(status_path(&linked).0, "status must see the installed hook");
}

#[test]
fn install_refuses_a_shared_hooks_folder_outside_the_repository() {
    let dir = repo();
    let main = dir.path().join("main");
    let shared = dir.path().join("shared-hooks");
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
}

/// A clone can commit `.githooks/pre-commit` as a link, so the hook file
/// gets the same link guard as a Husky hook.
#[cfg(unix)]
#[test]
fn install_refuses_a_tracked_hook_link_that_leaves_the_repository() {
    let dir = repo();
    let main = dir.path().join("main");
    let outside = dir.path().join("outside.sh");
    fs::write(&outside, "keep\n").unwrap();
    fs::create_dir_all(main.join(".githooks")).unwrap();
    std::os::unix::fs::symlink(&outside, main.join(".githooks/pre-commit")).unwrap();
    git(&main, &["config", "core.hooksPath", ".githooks"]);

    let install = hooks(&main, &["install", "--target", "git", "--force"]);
    assert_eq!(install.code, 2, "stderr: {}", install.stderr);
    assert_eq!(fs::read_to_string(&outside).unwrap(), "keep\n");

    let uninstall = hooks(&main, &["uninstall", "--target", "git", "--force"]);
    assert_eq!(uninstall.code, 2, "stderr: {}", uninstall.stderr);
    assert!(
        fs::symlink_metadata(main.join(".githooks/pre-commit"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
}

#[test]
fn install_from_a_package_folder_uses_the_hooks_of_the_repository() {
    let dir = repo();
    let main = dir.path().join("main");
    let package = main.join("packages/web");
    fs::create_dir_all(&package).unwrap();
    git(&main, &["config", "core.hooksPath", ".githooks"]);

    let install = hooks(&package, &["install", "--target", "git"]);
    assert_eq!(install.code, 0, "stderr: {}", install.stderr);
    assert!(main.join(".githooks/pre-commit").is_file());
}

#[test]
fn uninstall_with_a_shared_hooks_folder_removes_the_old_hook() {
    let dir = repo();
    let main = dir.path().join("main");
    let install = hooks(&main, &["install", "--target", "git"]);
    assert_eq!(install.code, 0, "stderr: {}", install.stderr);
    let shared = dir.path().join("shared-hooks");
    git(
        &main,
        &["config", "core.hooksPath", shared.to_str().unwrap()],
    );

    let uninstall = hooks(&main, &["uninstall", "--target", "git"]);
    assert_eq!(uninstall.code, 0, "stderr: {}", uninstall.stderr);
    assert!(!main.join(".git/hooks/pre-commit").exists());
}
