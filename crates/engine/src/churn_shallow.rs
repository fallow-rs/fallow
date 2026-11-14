//! Shallow clone state that the churn cache keys on.

use std::path::Path;

/// Shallow state of a repository.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ShallowState {
    /// Whether the repository is a shallow clone.
    pub shallow: bool,
    /// Hash of the shallow boundary file, or 0 when the file does not exist.
    /// `git fetch --unshallow` and `git fetch --deepen` change this file
    /// without a change of HEAD, so the churn cache keys on it.
    pub boundary: u64,
}

impl ShallowState {
    /// Read the shallow state with one `git rev-parse` call.
    #[must_use]
    pub fn read(root: &Path) -> Self {
        let mut command = crate::git_env::git_command(root);
        command
            .args([
                "rev-parse",
                "--is-shallow-repository",
                "--git-path",
                "shallow",
            ])
            .current_dir(root);
        let Ok(output) = command.output() else {
            return Self::default();
        };
        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut lines = stdout.lines();
        let shallow = lines
            .next()
            .is_some_and(|line| line.trim().eq_ignore_ascii_case("true"));
        let boundary = lines
            .next()
            .and_then(|path| std::fs::read(root.join(path.trim())).ok())
            .map_or(0, |bytes| xxhash_rust::xxh3::xxh3_64(&bytes));
        Self { shallow, boundary }
    }
}

/// Repository state that a churn cache entry must match.
#[derive(Clone, Copy)]
pub struct RepoSnapshot<'a> {
    pub head_sha: &'a str,
    pub shallow: ShallowState,
}
