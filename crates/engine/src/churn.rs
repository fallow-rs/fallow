//! Git churn helpers and types exposed through the engine boundary.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

pub use fallow_types::churn::ChurnTrend;
use rustc_hash::FxHashMap;

use crate::core_backend;

/// Function pointer signature used to intercept git churn subprocesses.
pub type ChurnSpawnHook = fn(&mut Command) -> std::io::Result<Output>;

/// Parsed duration for the `--since` flag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SinceDuration {
    /// Value to pass to `git log --after`.
    pub git_after: String,
    /// Human-readable display string.
    pub display: String,
}

/// Per-author commit aggregation for a single file.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AuthorContribution {
    /// Total commits by this author touching this file in the analysis window.
    pub commits: u32,
    /// Recency-weighted commit sum.
    pub weighted_commits: f64,
    /// Earliest commit timestamp by this author.
    pub first_commit_ts: u64,
    /// Latest commit timestamp by this author.
    pub last_commit_ts: u64,
}

/// Per-file churn data collected from git history.
#[derive(Debug, Clone)]
pub struct FileChurn {
    /// Absolute file path.
    pub path: PathBuf,
    /// Total number of commits touching this file in the analysis window.
    pub commits: u32,
    /// Recency-weighted commit count.
    pub weighted_commits: f64,
    /// Total lines added across all commits.
    pub lines_added: u32,
    /// Total lines deleted across all commits.
    pub lines_deleted: u32,
    /// Churn trend: accelerating, stable, or cooling.
    pub trend: ChurnTrend,
    /// Per-author contributions keyed by interned author index.
    pub authors: FxHashMap<u32, AuthorContribution>,
}

/// Result of churn analysis.
#[derive(Debug, Clone)]
pub struct ChurnResult {
    /// Per-file churn data, keyed by absolute path.
    pub files: FxHashMap<PathBuf, FileChurn>,
    /// Whether the repository is a shallow clone.
    pub shallow_clone: bool,
    /// Author email pool.
    pub author_pool: Vec<String>,
}

/// Install a spawn hook for git churn analysis.
pub fn set_spawn_hook(hook: ChurnSpawnHook) {
    core_backend::set_churn_spawn_hook(hook);
}

/// Parse a `--since` value into a git-compatible duration.
///
/// # Errors
///
/// Returns an error if the input is not a supported duration or ISO date.
pub fn parse_since(input: &str) -> Result<SinceDuration, String> {
    core_backend::parse_since(input)
}

/// Analyze git churn for files under `root`.
#[must_use]
pub fn analyze_churn(root: &Path, since: &SinceDuration) -> Option<ChurnResult> {
    core_backend::analyze_churn(root, since)
}

/// Analyze churn from a normalized `fallow-churn/v1` file.
///
/// # Errors
///
/// Returns an error when the import file cannot be read, parsed, or validated.
pub fn analyze_churn_from_file(path: &Path, root: &Path) -> Result<ChurnResult, String> {
    core_backend::analyze_churn_from_file(path, root)
}

/// Check whether `root` is inside a git repository.
#[must_use]
pub fn is_git_repo(root: &Path) -> bool {
    core_backend::is_git_repo(root)
}

/// Analyze churn with disk caching.
#[must_use]
pub fn analyze_churn_cached(
    root: &Path,
    since: &SinceDuration,
    cache_dir: &Path,
    no_cache: bool,
) -> Option<(ChurnResult, bool)> {
    core_backend::analyze_churn_cached(root, since, cache_dir, no_cache)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_since_returns_engine_owned_duration() {
        let duration = parse_since("6m").expect("duration should parse");
        assert_eq!(duration.git_after, "6 months ago");
        assert_eq!(duration.display, "6 months");
    }
}
