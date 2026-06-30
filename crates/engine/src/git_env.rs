//! Git process environment helpers owned by the engine boundary.

use std::process::Command;

/// Environment variables that describe an enclosing git operation's repository
/// state and should not leak into fallow-owned git subprocesses.
pub const AMBIENT_GIT_ENV_VARS: &[&str] = fallow_core::git_env::AMBIENT_GIT_ENV_VARS;

/// Strip ambient git repository-state environment variables from a `Command`.
///
/// Returns the `Command` for fluent chaining alongside `.args()` and
/// `.current_dir()`.
pub fn clear_ambient_git_env(cmd: &mut Command) -> &mut Command {
    fallow_core::git_env::clear_ambient_git_env(cmd)
}
