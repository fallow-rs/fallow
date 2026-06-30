//! Git churn helpers and types exposed through the engine boundary.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

pub use fallow_types::churn::ChurnTrend;
use rustc_hash::FxHashMap;

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

impl From<fallow_core::churn::SinceDuration> for SinceDuration {
    fn from(duration: fallow_core::churn::SinceDuration) -> Self {
        Self {
            git_after: duration.git_after,
            display: duration.display,
        }
    }
}

impl From<&SinceDuration> for fallow_core::churn::SinceDuration {
    fn from(duration: &SinceDuration) -> Self {
        Self {
            git_after: duration.git_after.clone(),
            display: duration.display.clone(),
        }
    }
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

impl From<fallow_core::churn::AuthorContribution> for AuthorContribution {
    fn from(author: fallow_core::churn::AuthorContribution) -> Self {
        Self {
            commits: author.commits,
            weighted_commits: author.weighted_commits,
            first_commit_ts: author.first_commit_ts,
            last_commit_ts: author.last_commit_ts,
        }
    }
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

impl From<fallow_core::churn::FileChurn> for FileChurn {
    fn from(file: fallow_core::churn::FileChurn) -> Self {
        Self {
            path: file.path,
            commits: file.commits,
            weighted_commits: file.weighted_commits,
            lines_added: file.lines_added,
            lines_deleted: file.lines_deleted,
            trend: file.trend,
            authors: file
                .authors
                .into_iter()
                .map(|(index, author)| (index, AuthorContribution::from(author)))
                .collect(),
        }
    }
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

impl From<fallow_core::churn::ChurnResult> for ChurnResult {
    fn from(result: fallow_core::churn::ChurnResult) -> Self {
        Self {
            files: result
                .files
                .into_iter()
                .map(|(path, file)| (path, FileChurn::from(file)))
                .collect(),
            shallow_clone: result.shallow_clone,
            author_pool: result.author_pool,
        }
    }
}

/// Install a spawn hook for git churn analysis.
pub fn set_spawn_hook(hook: ChurnSpawnHook) {
    fallow_core::churn::set_spawn_hook(hook);
}

/// Parse a `--since` value into a git-compatible duration.
///
/// # Errors
///
/// Returns an error if the input is not a supported duration or ISO date.
pub fn parse_since(input: &str) -> Result<SinceDuration, String> {
    fallow_core::churn::parse_since(input).map(SinceDuration::from)
}

/// Analyze git churn for files under `root`.
#[must_use]
pub fn analyze_churn(root: &Path, since: &SinceDuration) -> Option<ChurnResult> {
    let since = fallow_core::churn::SinceDuration::from(since);
    fallow_core::churn::analyze_churn(root, &since).map(ChurnResult::from)
}

/// Analyze churn from a normalized `fallow-churn/v1` file.
///
/// # Errors
///
/// Returns an error when the import file cannot be read, parsed, or validated.
pub fn analyze_churn_from_file(path: &Path, root: &Path) -> Result<ChurnResult, String> {
    fallow_core::churn::analyze_churn_from_file(path, root).map(ChurnResult::from)
}

/// Check whether `root` is inside a git repository.
#[must_use]
pub fn is_git_repo(root: &Path) -> bool {
    fallow_core::churn::is_git_repo(root)
}

/// Analyze churn with disk caching.
#[must_use]
pub fn analyze_churn_cached(
    root: &Path,
    since: &SinceDuration,
    cache_dir: &Path,
    no_cache: bool,
) -> Option<(ChurnResult, bool)> {
    let since = fallow_core::churn::SinceDuration::from(since);
    fallow_core::churn::analyze_churn_cached(root, &since, cache_dir, no_cache)
        .map(|(result, cache_hit)| (ChurnResult::from(result), cache_hit))
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

    #[test]
    fn churn_result_converts_from_core_without_leaking_type() {
        let mut authors = FxHashMap::default();
        authors.insert(
            0,
            fallow_core::churn::AuthorContribution {
                commits: 2,
                weighted_commits: 1.5,
                first_commit_ts: 10,
                last_commit_ts: 20,
            },
        );
        let mut files = FxHashMap::default();
        files.insert(
            PathBuf::from("/repo/src/a.ts"),
            fallow_core::churn::FileChurn {
                path: PathBuf::from("/repo/src/a.ts"),
                commits: 2,
                weighted_commits: 1.5,
                lines_added: 10,
                lines_deleted: 4,
                trend: ChurnTrend::Stable,
                authors,
            },
        );
        let result = ChurnResult::from(fallow_core::churn::ChurnResult {
            files,
            shallow_clone: true,
            author_pool: vec!["dev@example.com".to_string()],
        });

        let file = result
            .files
            .get(&PathBuf::from("/repo/src/a.ts"))
            .expect("converted file churn");
        assert!(result.shallow_clone);
        assert_eq!(result.author_pool, ["dev@example.com"]);
        assert_eq!(file.commits, 2);
        assert_eq!(file.authors[&0].last_commit_ts, 20);
    }
}
