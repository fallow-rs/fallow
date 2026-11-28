//! Git process environment helpers owned by the engine boundary.

use std::ffi::OsString;
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
/// - `core.hooksPath` names a folder that holds no hook, and
///   `hook.<event>.enabled=false` turns off each hook that the config names
///   (Git 2.54 and later). Git thus runs no hook of the repository, for
///   example during `git worktree add`. Older Git ignores the `hook.*` keys.
pub fn git_command(cwd: &Path) -> Command {
    let mut command = git_command_with_repository_hooks(cwd);
    let mut hooks_path = OsString::from("core.hooksPath=");
    hooks_path.push(no_hooks_dir());
    command.arg("-c").arg(hooks_path);
    for event in HOOK_EVENTS {
        command.arg("-c").arg(format!("hook.{event}.enabled=false"));
    }
    command
}

/// Each hook event of githooks(5).
const HOOK_EVENTS: &[&str] = &[
    "applypatch-msg",
    "pre-applypatch",
    "post-applypatch",
    "pre-commit",
    "pre-merge-commit",
    "prepare-commit-msg",
    "commit-msg",
    "post-commit",
    "pre-rebase",
    "post-checkout",
    "post-merge",
    "pre-push",
    "pre-receive",
    "update",
    "proc-receive",
    "post-receive",
    "post-update",
    "reference-transaction",
    "push-to-checkout",
    "pre-auto-gc",
    "post-rewrite",
    "sendemail-validate",
    "fsmonitor-watchman",
    "p4-changelist",
    "p4-prepare-changelist",
    "p4-post-changelist",
    "p4-pre-submit",
    "post-index-change",
];

/// Hooks folder that holds no hook. Git looks for `<folder>/<hook name>`,
/// and no path below the null device exists.
#[cfg(not(windows))]
fn no_hooks_dir() -> &'static Path {
    Path::new("/dev/null")
}

/// Hooks folder that holds no hook. Git reads a relative `core.hooksPath`
/// against the work tree, and Git for Windows reads `/dev/null` as a folder
/// on the current drive, so the folder is a path below the fallow
/// executable. A regular file has no children, so no hook can exist there.
/// If the executable path is not known, the folder is in the temporary
/// directory of the user. The `hook.<event>.enabled` keys still apply then.
#[cfg(windows)]
fn no_hooks_dir() -> &'static Path {
    static DIR: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();
    DIR.get_or_init(|| {
        std::env::current_exe()
            .unwrap_or_else(|_| std::env::temp_dir().join("fallow-no-hooks"))
            .join("no-hooks")
    })
}

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

#[cfg(test)]
mod path_tests {
    use std::path::Path;

    /// Git reads a relative `core.hooksPath` against the work tree, so the
    /// value must be absolute. No path below it may exist, so a folder that
    /// someone makes later still holds no hook.
    #[test]
    fn the_hooks_folder_is_absolute_and_holds_nothing() {
        let command = super::git_command(Path::new("."));
        let value = command
            .get_args()
            .filter_map(|arg| arg.to_str())
            .find_map(|arg| arg.strip_prefix("core.hooksPath="))
            .expect("git_command sets core.hooksPath");
        let folder = Path::new(value);
        assert!(folder.is_absolute(), "{value} is not absolute");
        assert!(
            folder
                .ancestors()
                .any(|ancestor| ancestor.exists() && !ancestor.is_dir()),
            "a folder can exist at {value}"
        );
    }
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

    /// Git 2.54 added hooks that the config names (`hook.<name>.command`
    /// with `hook.<name>.event`). Older Git has no such hooks.
    fn git_runs_config_hooks() -> bool {
        let version = crate::git_fixture::fixture_git_capture(Path::new("."), &["--version"]);
        let mut numbers = version
            .trim_start_matches("git version ")
            .split(|c: char| !c.is_ascii_digit())
            .filter_map(|part| part.parse::<u32>().ok());
        let major = numbers.next().unwrap_or(0);
        let minor = numbers.next().unwrap_or(0);
        (major, minor) >= (2, 54)
    }

    #[cfg(unix)]
    #[test]
    fn git_command_runs_no_config_hook() {
        if !git_runs_config_hooks() {
            return;
        }
        let temp = tempfile::tempdir().expect("temp dir");
        let repo = temp.path().join("repo");
        let marker = temp.path().join("config-hook-ran");
        std::fs::create_dir_all(&repo).expect("repo dir");
        fixture_git(&repo, &["init", "-q", "-b", "main"]);
        std::fs::write(repo.join("tracked.txt"), "a\n").expect("file");
        fixture_git(&repo, &["add", "."]);
        fixture_git(&repo, &["commit", "-q", "--no-verify", "-m", "base"]);
        let program = format!("touch '{}'", marker.display());
        fixture_git(&repo, &["config", "hook.probe.command", &program]);
        for event in [
            "reference-transaction",
            "post-index-change",
            "post-checkout",
        ] {
            fixture_git(&repo, &["config", "--add", "hook.probe.event", event]);
        }

        fixture_git(&repo, &["branch", "probe"]);
        assert!(marker.exists(), "the fixture repository runs its hook");
        std::fs::remove_file(&marker).expect("remove marker");

        let destination = temp.path().join("worktree");
        let commands: [Vec<std::ffi::OsString>; 3] = [
            [
                "worktree",
                "add",
                "--detach",
                "--quiet",
                "--no-checkout",
                "--",
            ]
            .iter()
            .map(Into::into)
            .chain([destination.into_os_string(), "HEAD".into()])
            .collect(),
            ["read-tree", "--reset", "HEAD"]
                .iter()
                .map(Into::into)
                .collect(),
            ["checkout", "--quiet", "--detach", "HEAD"]
                .iter()
                .map(Into::into)
                .collect(),
        ];
        for args in commands {
            let output = git_command(&repo).args(&args).output().expect("git starts");
            assert!(
                output.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(!marker.exists(), "git {args:?} ran the config hook");
        }
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
