//! Git process environment helpers owned by the engine boundary.

use std::path::Path;
use std::process::{Command, Stdio};

/// Environment variables that describe an enclosing git operation's repository
/// state and should not leak into fallow-owned git subprocesses.
pub const AMBIENT_GIT_ENV_VARS: &[&str] = &[
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_OBJECT_DIRECTORY",
    "GIT_COMMON_DIR",
    "GIT_PREFIX",
];

/// Strip ambient git repository-state environment variables from a `Command`.
///
/// Returns the `Command` for fluent chaining alongside `.args()` and
/// `.current_dir()`.
pub fn clear_ambient_git_env(cmd: &mut Command) -> &mut Command {
    for var in AMBIENT_GIT_ENV_VARS {
        cmd.env_remove(var);
    }
    cmd
}

/// Build a `git` command that runs in `cwd`.
///
/// Every Fallow-owned Git process starts here:
/// - The ambient repository-state environment is cleared.
/// - Stdin is closed. Long-lived embedders keep protocol stdin open, which Git
///   for Windows can inherit and hold.
/// - Inside a deregistered base snapshot, which holds an untrusted commit, the
///   command gets a `GIT_DIR` that does not exist, so Git finds no repository.
/// - `safe.bareRepository=explicit` stops Git from using a directory shaped
///   as a bare repository that it finds on its own. Checkouts, their
///   subdirectories and linked worktrees are not affected.
/// - `core.fsmonitor=false` stops Git from running a file system monitor
///   program that the config of the repository names.
/// - `core.hooksPath` names the null device, so Git runs no hook of the
///   repository, for example during `git worktree add`.
pub fn git_command(cwd: &Path) -> Command {
    let mut command = git_command_with_repository_hooks(cwd);
    command
        .arg("-c")
        .arg(format!("core.hooksPath={NO_HOOKS_DIR}"));
    command
}

/// Hooks folder that holds no hook. Git looks for `<folder>/<hook name>`,
/// and no path below the null device exists.
#[cfg(not(windows))]
const NO_HOOKS_DIR: &str = "/dev/null";
#[cfg(windows)]
const NO_HOOKS_DIR: &str = "NUL";

/// Build a `git` command as [`git_command`] does, but keep the hooks folder
/// that the config of the repository names.
///
/// Use it only for a command that reads the hooks folder setting and runs
/// no hook, such as `git rev-parse --git-path hooks`.
#[expect(
    clippy::disallowed_methods,
    reason = "engine-owned git spawn wrapper clears ambient git env before every git subprocess"
)]
pub fn git_command_with_repository_hooks(cwd: &Path) -> Command {
    let mut command = Command::new("git");
    clear_ambient_git_env(&mut command);
    crate::base_worktree_registration::confine_git_to_host(&mut command, cwd);
    command
        .stdin(Stdio::null())
        .args([
            "-c",
            "safe.bareRepository=explicit",
            "-c",
            "core.fsmonitor=false",
        ])
        .current_dir(cwd);
    command
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::git_fixture::{fixture_git, fixture_git_command};

    /// A repository whose config names a file system monitor program that
    /// creates `marker`.
    #[cfg(unix)]
    fn repo_with_fsmonitor(dir: &Path, marker: &Path) {
        std::fs::create_dir_all(dir).expect("repo dir");
        fixture_git(dir, &["init", "-q", "-b", "main"]);
        std::fs::write(dir.join("tracked.txt"), "a\n").expect("file");
        fixture_git(dir, &["add", "."]);
        let program = format!("touch '{}'; false", marker.display());
        fixture_git(dir, &["config", "core.fsmonitor", &program]);
    }

    /// A `git worktree add` updates refs, and git then runs the
    /// `reference-transaction` hook of the repository.
    #[cfg(unix)]
    #[test]
    fn git_command_runs_no_hook_of_the_repository() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().expect("temp dir");
        let repo = temp.path().join("repo");
        let marker = temp.path().join("hook-ran");
        std::fs::create_dir_all(&repo).expect("repo dir");
        fixture_git(&repo, &["init", "-q", "-b", "main"]);
        std::fs::write(repo.join("tracked.txt"), "a\n").expect("file");
        fixture_git(&repo, &["add", "."]);
        fixture_git(&repo, &["commit", "-q", "--no-verify", "-m", "base"]);
        let hook = repo.join(".git/hooks/reference-transaction");
        std::fs::write(&hook, format!("#!/bin/sh\ntouch '{}'\n", marker.display())).expect("hook");
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).expect("chmod");

        fixture_git(&repo, &["branch", "probe"]);
        assert!(marker.exists(), "the fixture repository runs its hook");
        std::fs::remove_file(&marker).expect("remove marker");

        let destination = temp.path().join("worktree");
        let output = git_command(&repo)
            .args([
                "worktree",
                "add",
                "--detach",
                "--quiet",
                "--no-checkout",
                "--",
            ])
            .arg(&destination)
            .arg("HEAD")
            .output()
            .expect("git starts");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!marker.exists(), "git worktree add ran the hook");
    }

    #[cfg(unix)]
    #[test]
    fn git_command_runs_no_fsmonitor_program_of_the_repository() {
        let temp = tempfile::tempdir().expect("temp dir");
        let repo = temp.path().join("repo");
        let marker = temp.path().join("fsmonitor-ran");
        repo_with_fsmonitor(&repo, &marker);

        // The fixture runs the program, so the config works as written.
        let _ = fixture_git_command(&repo)
            .args(["status", "--porcelain"])
            .output()
            .expect("git starts");
        assert!(marker.exists(), "the fixture repository runs its program");
        std::fs::remove_file(&marker).expect("remove marker");

        for args in [
            &["status", "--porcelain"][..],
            &["ls-files", "-s", "-z"][..],
        ] {
            let _ = git_command(&repo).args(args).output().expect("git starts");
            assert!(!marker.exists(), "git {args:?} ran the program");
        }
    }
}
