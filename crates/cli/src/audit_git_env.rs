//! A hint for audit worktree failures that git hook environments cause.

/// If fallow's process inherited any ambient git repo-state env vars (typical
/// when invoked from a `pre-commit` / `pre-push` hook or a tool wrapping git),
/// surface the most likely culprit so a user hitting an unexpected worktree
/// failure can short-circuit the diagnosis. Returns `None` otherwise.
pub fn ambient_git_env_hint() -> Option<String> {
    use fallow_engine::changed_files::AMBIENT_GIT_ENV_VARS;
    for var in AMBIENT_GIT_ENV_VARS {
        if let Ok(value) = std::env::var(var)
            && !value.is_empty()
        {
            return Some(format!(
                "{var}={value} is set in the environment; if fallow is being \
invoked from a git hook this can interfere with worktree operations. Re-run \
with `env -u {var} fallow audit` to confirm."
            ));
        }
    }
    None
}
