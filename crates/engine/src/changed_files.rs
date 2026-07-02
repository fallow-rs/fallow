//! Changed-file helpers owned by the engine boundary.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::OnceLock;

use fallow_types::results::AnalysisResults;
use rustc_hash::FxHashSet;

use crate::core_backend;
use crate::duplicates::DuplicationReport;

pub use crate::git_env::{AMBIENT_GIT_ENV_VARS, clear_ambient_git_env};

/// Function pointer signature used to intercept short-running git
/// subprocesses spawned by changed-file helpers.
pub type ChangedFilesSpawnHook = fn(&mut std::process::Command) -> std::io::Result<Output>;

static SPAWN_HOOK: OnceLock<ChangedFilesSpawnHook> = OnceLock::new();

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
    let _ = SPAWN_HOOK.set(hook);
}

/// Validate a user-supplied git ref before passing it to git.
pub fn validate_git_ref(s: &str) -> Result<&str, String> {
    if s.is_empty() {
        return Err("git ref cannot be empty".to_string());
    }
    if s.starts_with('-') {
        return Err("git ref cannot start with '-'".to_string());
    }
    let mut in_braces = false;
    for c in s.chars() {
        match c {
            '{' => in_braces = true,
            '}' => in_braces = false,
            ':' | ' ' if in_braces => {}
            c if c.is_ascii_alphanumeric()
                || matches!(c, '.' | '_' | '-' | '/' | '~' | '^' | '@' | '{' | '}') => {}
            _ => return Err(format!("git ref contains disallowed character: '{c}'")),
        }
    }
    if in_braces {
        return Err("git ref has unclosed '{'".to_string());
    }
    Ok(s)
}

/// Resolve the canonical git toplevel for `cwd`.
pub fn resolve_git_toplevel(cwd: &Path) -> Result<PathBuf, ChangedFilesError> {
    let output = spawn_output(&mut git_command(cwd, &["rev-parse", "--show-toplevel"]))
        .map_err(|e| ChangedFilesError::GitMissing(e.to_string()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(if stderr.contains("not a git repository") {
            ChangedFilesError::NotARepository
        } else {
            ChangedFilesError::GitFailed(stderr.trim().to_owned())
        });
    }

    let raw = String::from_utf8_lossy(&output.stdout);
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(ChangedFilesError::GitFailed(
            "git rev-parse --show-toplevel returned empty output".to_owned(),
        ));
    }

    let path = PathBuf::from(trimmed);
    Ok(dunce::canonicalize(&path).unwrap_or(path))
}

/// Resolve the canonical git common directory for `cwd`.
pub fn resolve_git_common_dir(cwd: &Path) -> Result<PathBuf, ChangedFilesError> {
    let output = spawn_output(&mut git_command(
        cwd,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    ))
    .map_err(|e| ChangedFilesError::GitMissing(e.to_string()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(if stderr.contains("not a git repository") {
            ChangedFilesError::NotARepository
        } else {
            ChangedFilesError::GitFailed(stderr.trim().to_owned())
        });
    }

    let raw = String::from_utf8_lossy(&output.stdout);
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(ChangedFilesError::GitFailed(
            "git rev-parse --git-common-dir returned empty output".to_owned(),
        ));
    }

    let path = PathBuf::from(trimmed);
    Ok(dunce::canonicalize(&path).unwrap_or(path))
}

/// Get files changed since a git ref.
pub fn try_get_changed_files(
    root: &Path,
    git_ref: &str,
) -> Result<FxHashSet<PathBuf>, ChangedFilesError> {
    validate_git_ref(git_ref).map_err(ChangedFilesError::InvalidRef)?;
    let toplevel = resolve_git_toplevel(root)?;
    try_get_changed_files_with_toplevel(root, &toplevel, git_ref)
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
    validate_git_ref(git_ref).map_err(ChangedFilesError::InvalidRef)?;

    let mut files = collect_git_paths(
        cwd,
        toplevel,
        &[
            "diff",
            "--name-only",
            "--end-of-options",
            &format!("{git_ref}...HEAD"),
        ],
    )?;
    files.extend(collect_git_paths(
        cwd,
        toplevel,
        &["diff", "--name-only", "HEAD"],
    )?);
    files.extend(collect_git_paths(
        cwd,
        toplevel,
        &["ls-files", "--full-name", "--others", "--exclude-standard"],
    )?);
    Ok(files)
}

/// Return the raw git diff for a ref.
pub fn try_get_changed_diff(root: &Path, git_ref: &str) -> Result<String, ChangedFilesError> {
    validate_git_ref(git_ref).map_err(ChangedFilesError::InvalidRef)?;
    let output = spawn_output(&mut git_command(
        root,
        &[
            "diff",
            "--relative",
            "--unified=0",
            "--end-of-options",
            &format!("{git_ref}...HEAD"),
        ],
    ))
    .map_err(|e| ChangedFilesError::GitMissing(e.to_string()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(if stderr.contains("not a git repository") {
            ChangedFilesError::NotARepository
        } else {
            ChangedFilesError::GitFailed(stderr.trim().to_owned())
        });
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Get changed files if git can resolve them, otherwise return `None`.
#[must_use]
#[expect(
    clippy::print_stderr,
    reason = "intentional user-facing warning for the CLI's --changed-since fallback path; typed callers use try_get_changed_files instead"
)]
pub fn get_changed_files(root: &Path, git_ref: &str) -> Option<FxHashSet<PathBuf>> {
    match try_get_changed_files(root, git_ref) {
        Ok(files) => Some(files),
        Err(ChangedFilesError::InvalidRef(e)) => {
            eprintln!("Warning: --changed-since ignored: invalid git ref: {e}");
            None
        }
        Err(ChangedFilesError::GitMissing(e)) => {
            eprintln!("Warning: --changed-since ignored: failed to run git: {e}");
            None
        }
        Err(ChangedFilesError::NotARepository) => {
            eprintln!("Warning: --changed-since ignored: not a git repository");
            None
        }
        Err(ChangedFilesError::GitFailed(stderr)) => {
            eprintln!("Warning: --changed-since failed for ref '{git_ref}': {stderr}");
            None
        }
    }
}

fn spawn_output(command: &mut Command) -> std::io::Result<Output> {
    if let Some(hook) = SPAWN_HOOK.get() {
        hook(command)
    } else {
        command.output()
    }
}

fn collect_git_paths(
    cwd: &Path,
    toplevel: &Path,
    args: &[&str],
) -> Result<FxHashSet<PathBuf>, ChangedFilesError> {
    let output = spawn_output(&mut git_command(cwd, args))
        .map_err(|e| ChangedFilesError::GitMissing(e.to_string()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(if stderr.contains("not a git repository") {
            ChangedFilesError::NotARepository
        } else {
            ChangedFilesError::GitFailed(stderr.trim().to_owned())
        });
    }

    #[cfg(windows)]
    let normalise_segment = |line: &str| line.replace('/', "\\");
    #[cfg(not(windows))]
    let normalise_segment = |line: &str| line.to_owned();

    let files = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| toplevel.join(normalise_segment(line)))
        .collect();

    Ok(files)
}

#[expect(
    clippy::disallowed_methods,
    reason = "canonical engine-owned git spawn wrapper for changed-file orchestration"
)]
fn git_command(cwd: &Path, args: &[&str]) -> Command {
    let mut command = Command::new("git");
    clear_ambient_git_env(&mut command);
    command.args(args).current_dir(cwd);
    command
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
    fn validate_git_ref_rejects_option_like_ref() {
        assert!(validate_git_ref("--upload-pack=evil").is_err());
        assert!(validate_git_ref("-flag").is_err());
    }

    #[test]
    fn validate_git_ref_allows_reflog_relative_date() {
        assert!(validate_git_ref("HEAD@{1 week ago}").is_ok());
    }

    #[test]
    fn git_command_clears_parent_git_environment() {
        let command = git_command(Path::new("."), &["status"]);
        let envs: Vec<_> = command.get_envs().collect();

        for var in AMBIENT_GIT_ENV_VARS {
            assert!(
                envs.iter()
                    .any(|(key, value)| key.to_str() == Some(*var) && value.is_none()),
                "{var} should be cleared from the command env",
            );
        }
    }

    #[test]
    fn try_get_changed_files_not_a_repository() {
        let temp = tempfile::tempdir().expect("tempdir");
        let result = try_get_changed_files(temp.path(), "main");
        assert!(matches!(result, Err(ChangedFilesError::NotARepository)));
    }

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
