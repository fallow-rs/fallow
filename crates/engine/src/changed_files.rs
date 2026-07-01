//! Changed-file helpers owned by the engine boundary.

use std::path::{Path, PathBuf};
use std::process::Output;

use fallow_types::results::AnalysisResults;
use rustc_hash::FxHashSet;

use crate::core_backend;
use crate::duplicates::DuplicationReport;

pub use crate::git_env::{AMBIENT_GIT_ENV_VARS, clear_ambient_git_env};

/// Function pointer signature used to intercept short-running git
/// subprocesses spawned by changed-file helpers.
pub type ChangedFilesSpawnHook = fn(&mut std::process::Command) -> std::io::Result<Output>;

/// Classification of a changed-file git failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChangedFilesError {
    /// Git ref failed validation before invoking `git`.
    InvalidRef(String),
    /// `git` binary not found or not executable.
    GitMissing(String),
    /// Command ran but the directory is not a git repository.
    NotARepository,
    /// Command ran but the ref is invalid or another git error occurred.
    GitFailed(String),
}

impl ChangedFilesError {
    /// Human-readable clause suitable for embedding in an error message.
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            Self::InvalidRef(err) => format!("invalid git ref: {err}"),
            Self::GitMissing(err) => format!("failed to run git: {err}"),
            Self::NotARepository => "not a git repository".to_owned(),
            Self::GitFailed(stderr) => augment_git_failed(stderr),
        }
    }
}

fn augment_git_failed(stderr: &str) -> String {
    let lower = stderr.to_ascii_lowercase();
    if lower.contains("not a valid object name")
        || lower.contains("unknown revision")
        || lower.contains("ambiguous argument")
    {
        format!(
            "{stderr} (shallow clone? try `git fetch --unshallow`, or set `fetch-depth: 0` on actions/checkout / `GIT_DEPTH: 0` in GitLab CI)"
        )
    } else {
        stderr.to_owned()
    }
}

/// Install a spawn-hook for changed-file git subprocesses.
pub fn set_spawn_hook(hook: ChangedFilesSpawnHook) {
    core_backend::set_changed_files_spawn_hook(hook);
}

/// Validate a user-supplied git ref before passing it to git.
pub fn validate_git_ref(s: &str) -> Result<&str, String> {
    core_backend::validate_git_ref(s)
}

/// Resolve the canonical git toplevel for `cwd`.
pub fn resolve_git_toplevel(cwd: &Path) -> Result<PathBuf, ChangedFilesError> {
    core_backend::resolve_git_toplevel(cwd)
}

/// Resolve the canonical git common directory for `cwd`.
pub fn resolve_git_common_dir(cwd: &Path) -> Result<PathBuf, ChangedFilesError> {
    core_backend::resolve_git_common_dir(cwd)
}

/// Get files changed since a git ref.
pub fn try_get_changed_files(
    root: &Path,
    git_ref: &str,
) -> Result<FxHashSet<PathBuf>, ChangedFilesError> {
    core_backend::try_get_changed_files(root, git_ref)
}

/// Resolve changed files for a git ref relative to a project root.
///
/// # Errors
///
/// Returns an error when git cannot resolve the ref or repository state.
pub fn changed_files(root: &Path, git_ref: &str) -> Result<FxHashSet<PathBuf>, ChangedFilesError> {
    try_get_changed_files(root, git_ref)
}

/// Get changed files and the git toplevel used to resolve them.
pub fn try_get_changed_files_with_toplevel(
    cwd: &Path,
    toplevel: &Path,
    git_ref: &str,
) -> Result<FxHashSet<PathBuf>, ChangedFilesError> {
    core_backend::try_get_changed_files_with_toplevel(cwd, toplevel, git_ref)
}

/// Return the raw git diff for a ref.
pub fn try_get_changed_diff(root: &Path, git_ref: &str) -> Result<String, ChangedFilesError> {
    core_backend::try_get_changed_diff(root, git_ref)
}

/// Get changed files if git can resolve them, otherwise return `None`.
#[must_use]
pub fn get_changed_files(root: &Path, git_ref: &str) -> Option<FxHashSet<PathBuf>> {
    core_backend::get_changed_files(root, git_ref)
}

/// Scope dead-code results to findings affected by changed files.
#[expect(
    clippy::implicit_hasher,
    reason = "fallow standardizes on FxHashSet across the workspace"
)]
pub fn filter_results_by_changed_files(
    results: &mut AnalysisResults,
    changed_files: &FxHashSet<PathBuf>,
) {
    core_backend::filter_results_by_changed_files(results, changed_files);
}

/// Scope duplication groups to clone groups touching at least one changed file.
#[expect(
    clippy::implicit_hasher,
    reason = "fallow standardizes on FxHashSet across the workspace"
)]
pub fn filter_duplication_by_changed_files(
    report: &mut DuplicationReport,
    changed_files: &FxHashSet<PathBuf>,
    root: &Path,
) {
    core_backend::filter_duplication_by_changed_files(report, changed_files, root);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changed_files_error_describe_matches_core_contract() {
        assert_eq!(
            ChangedFilesError::InvalidRef("bad ref".to_string()).describe(),
            "invalid git ref: bad ref"
        );
        assert_eq!(
            ChangedFilesError::GitMissing("not found".to_string()).describe(),
            "failed to run git: not found"
        );
        assert_eq!(
            ChangedFilesError::NotARepository.describe(),
            "not a git repository"
        );
        assert!(
            ChangedFilesError::GitFailed("unknown revision main".to_string())
                .describe()
                .contains("fetch-depth: 0")
        );
    }
}
