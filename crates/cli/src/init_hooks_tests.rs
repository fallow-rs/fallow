//! Tests for the default branch fallback of `fallow hooks install`.
//!
//! The remote default branch name is repository metadata that a remote
//! controls. The generated hook writes it into a shell script, so a name that
//! fails the ref validator must not reach the hook.

use std::path::Path;
use std::process::{Command, ExitCode};

use crate::init::{GitHooksInstallOptions, run_git_hooks_install};

fn git_in(root: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(root)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .status()
        .expect("git runs");
    assert!(status.success(), "git {args:?} failed");
}

/// Create a repository whose `origin/HEAD` points at `origin/<branch>`, and
/// return the generated hook.
fn install_with_remote_default_branch(branch: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git_in(root, &["init", "-q"]);
    git_in(
        root,
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.invalid",
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "init",
        ],
    );
    let remote_ref = format!("refs/remotes/origin/{branch}");
    git_in(root, &["update-ref", &remote_ref, "HEAD"]);
    std::fs::write(
        root.join(".git/refs/remotes/origin").join("HEAD"),
        format!("ref: {remote_ref}\n"),
    )
    .unwrap();

    assert_eq!(
        run_git_hooks_install(&GitHooksInstallOptions {
            root,
            branch: None,
            dry_run: false,
            force: false,
            prune_baselines: false,
        }),
        ExitCode::SUCCESS
    );
    std::fs::read_to_string(root.join(".git/hooks/pre-commit")).unwrap()
}

#[test]
fn rejects_shell_payload_in_remote_default_branch() {
    for branch in ["$(touch${IFS}PWNED)", "`touch${IFS}PWNED`"] {
        let hook = install_with_remote_default_branch(branch);
        assert!(!hook.contains("PWNED"), "{hook}");
        assert!(hook.contains("  BASE=\"main\"\n"), "{hook}");
    }
}

#[test]
fn keeps_a_valid_remote_default_branch() {
    let hook = install_with_remote_default_branch("release/2.x");
    assert!(hook.contains("  BASE=\"release/2.x\"\n"), "{hook}");
}
