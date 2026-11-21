//! Repository state and history coverage that the churn cache keys on.

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

/// Hash of the `.mailmap` file at the repository top level, or 0 when the
/// file does not exist, is not a regular file, or the top level is unknown.
/// Git does not follow a symlinked in-tree `.mailmap`, and a symlink to a
/// FIFO or a device would block the read.
#[must_use]
pub fn mailmap_fingerprint(root: &Path) -> u64 {
    let mut command = crate::git_env::git_command(root);
    command
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(root);
    let Ok(output) = command.output() else {
        return 0;
    };
    if !output.status.success() {
        return 0;
    }
    let toplevel = String::from_utf8_lossy(&output.stdout);
    let path = Path::new(toplevel.trim()).join(".mailmap");
    let is_regular_file = std::fs::symlink_metadata(&path).is_ok_and(|meta| meta.is_file());
    if !is_regular_file {
        return 0;
    }
    std::fs::read(path).map_or(0, |bytes| xxhash_rust::xxh3::xxh3_64(&bytes))
}

/// Get the full HEAD SHA for cache keying.
pub fn get_head_sha(root: &Path) -> Option<String> {
    crate::repo_refs::head_sha(root).ok().flatten()
}

/// Check whether `ancestor` is still reachable from `descendant`.
pub fn is_ancestor(root: &Path, ancestor: &str, descendant: &str) -> bool {
    let mut command = crate::git_env::git_command(root);
    command
        .args(["merge-base", "--is-ancestor", ancestor, descendant])
        .current_dir(root);
    command.status().is_ok_and(|s| s.success())
}

/// What a churn history covers: the project root, the revision, the shallow
/// state of the clone, and the oldest committer time that the walk loaded.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChurnCoverage {
    /// Hash of the project root path. Cached file paths are joined to the
    /// root, so an entry of another root holds the wrong files.
    pub root_key: u64,
    /// The commit that the walk started from.
    pub head_sha: String,
    /// Shallow state of the clone.
    pub shallow: ShallowState,
    /// [`mailmap_fingerprint`] of the repository. `git log --use-mailmap`
    /// reads the mailmap from the worktree, so an edit changes author
    /// emails without a change of HEAD.
    pub mailmap: u64,
    /// Oldest committer time that the history holds. `None` means that the
    /// walk had no cutoff.
    pub earliest_cutoff_secs: Option<u64>,
}

impl ChurnCoverage {
    /// True when a history with this coverage holds every event that
    /// `wanted` needs, for a `wanted.head_sha` that is this revision or a
    /// descendant of it. The caller compares the revisions.
    #[must_use]
    pub fn covers(&self, wanted: &Self) -> bool {
        self.root_key == wanted.root_key
            && self.shallow == wanted.shallow
            && self.mailmap == wanted.mailmap
            && cutoff_covers(self.earliest_cutoff_secs, wanted.earliest_cutoff_secs)
    }

    /// The coverage after an extension to `head_sha` that prunes the old
    /// events to `rolled_cutoff_secs` and walks the new commits with it.
    #[must_use]
    pub fn extended_to(&self, head_sha: &str, rolled_cutoff_secs: Option<u64>) -> Self {
        let earliest_cutoff_secs = match (self.earliest_cutoff_secs, rolled_cutoff_secs) {
            (Some(have), Some(rolled)) => Some(have.max(rolled)),
            (have, rolled) => have.or(rolled),
        };
        Self {
            root_key: self.root_key,
            head_sha: head_sha.to_owned(),
            shallow: self.shallow,
            mailmap: self.mailmap,
            earliest_cutoff_secs,
        }
    }
}

/// True when a history that starts at `have` holds every event from `want`.
fn cutoff_covers(have: Option<u64>, want: Option<u64>) -> bool {
    match (have, want) {
        (None, _) => true,
        (Some(_), None) => false,
        (Some(have), Some(want)) => have <= want,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn coverage(root_key: u64, cutoff: Option<u64>) -> ChurnCoverage {
        ChurnCoverage {
            root_key,
            head_sha: "a".to_owned(),
            shallow: ShallowState::default(),
            mailmap: 0,
            earliest_cutoff_secs: cutoff,
        }
    }

    #[cfg(unix)]
    #[test]
    fn mailmap_fingerprint_reads_only_a_regular_file() {
        let dir = tempfile::tempdir().expect("repo tempdir");
        let root = dir.path();
        let status = crate::git_env::git_command(root)
            .args(["init", "--quiet"])
            .current_dir(root)
            .status()
            .expect("run git init");
        assert!(status.success());
        assert_eq!(mailmap_fingerprint(root), 0);

        std::fs::write(root.join("aliases"), "A <a@x.test> <b@x.test>\n").expect("write aliases");
        std::os::unix::fs::symlink(root.join("aliases"), root.join(".mailmap"))
            .expect("symlink mailmap");
        assert_eq!(mailmap_fingerprint(root), 0);

        std::fs::remove_file(root.join(".mailmap")).expect("remove symlink");
        std::fs::copy(root.join("aliases"), root.join(".mailmap")).expect("copy mailmap");
        assert_ne!(mailmap_fingerprint(root), 0);
    }

    #[test]
    fn covers_requires_the_same_mailmap() {
        let have = coverage(1, None);
        let mut wanted = coverage(1, None);
        wanted.mailmap = 7;
        assert!(!have.covers(&wanted));
    }

    #[test]
    fn covers_requires_the_same_root() {
        assert!(!coverage(1, Some(100)).covers(&coverage(2, Some(100))));
    }

    #[test]
    fn covers_requires_the_same_shallow_state() {
        let wanted = ChurnCoverage {
            shallow: ShallowState {
                shallow: true,
                boundary: 7,
            },
            ..coverage(1, Some(100))
        };
        assert!(!coverage(1, Some(100)).covers(&wanted));
    }

    #[test]
    fn covers_compares_the_cutoffs() {
        let have = coverage(1, Some(100));
        assert!(have.covers(&coverage(1, Some(100))));
        assert!(have.covers(&coverage(1, Some(150))));
        assert!(!have.covers(&coverage(1, Some(99))));
        assert!(!have.covers(&coverage(1, None)));
        let unbounded = coverage(1, None);
        assert!(unbounded.covers(&coverage(1, Some(1))));
        assert!(unbounded.covers(&coverage(1, None)));
    }

    #[test]
    fn covers_ignores_the_revision() {
        let wanted = ChurnCoverage {
            head_sha: "b".to_owned(),
            ..coverage(1, Some(100))
        };
        assert!(coverage(1, Some(100)).covers(&wanted));
    }

    #[test]
    fn extended_to_keeps_the_later_cutoff() {
        let cases = [
            (Some(100), Some(150), Some(150)),
            (Some(150), Some(100), Some(150)),
            (None, Some(5), Some(5)),
            (Some(5), None, Some(5)),
            (None, None, None),
        ];
        for (have, rolled, expected) in cases {
            let extended = coverage(1, have).extended_to("b", rolled);
            assert_eq!(
                extended.earliest_cutoff_secs, expected,
                "{have:?} {rolled:?}"
            );
            assert_eq!(extended.head_sha, "b");
        }
    }
}
