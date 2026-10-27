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
#[expect(
    clippy::disallowed_methods,
    reason = "engine-owned git spawn wrapper clears ambient git env before every git subprocess"
)]
pub fn git_command(cwd: &Path) -> Command {
    let mut command = Command::new("git");
    clear_ambient_git_env(&mut command);
    crate::base_worktree_registration::confine_git_to_host(&mut command, cwd);
    command
        .stdin(Stdio::null())
        .args(["-c", "safe.bareRepository=explicit"])
        .current_dir(cwd);
    command
}
