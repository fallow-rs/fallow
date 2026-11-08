//! A generated project on disk, and the seeded choices that build it.

use std::path::Path;

use crate::common::{commit_all, git, git_capture};

/// A small SplitMix64 generator, so a seed always gives the same project.
pub struct Rng(u64);

impl Rng {
    pub const fn new(seed: u64) -> Self {
        Self(seed)
    }

    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A number in `low..=high`.
    pub fn range(&mut self, low: usize, high: usize) -> usize {
        low + usize::try_from(self.next() % (high - low + 1) as u64).expect("small range")
    }

    pub fn chance(&mut self, percent: u64) -> bool {
        self.next() % 100 < percent
    }

    pub fn shuffle<T>(&mut self, items: &mut [T]) {
        for upper in (1..items.len()).rev() {
            let pick = self.range(0, upper);
            items.swap(upper, pick);
        }
    }
}

/// The files of a generated project, and the change that `audit` reviews.
#[derive(Debug, Default)]
pub struct Project {
    /// The files of the base commit, as `(path, contents)`.
    pub files: Vec<(String, String)>,
    /// The files that the head commit adds or replaces.
    pub change: Vec<(String, String)>,
    /// A dependency for `trace --dependency` and `--trace-dependency`.
    pub dependency: &'static str,
    /// A `file:symbol` target for `trace`.
    pub symbol: &'static str,
}

impl Project {
    pub fn file(&mut self, path: impl Into<String>, contents: impl Into<String>) {
        self.files.push((path.into(), contents.into()));
    }

    pub fn changed(&mut self, path: impl Into<String>, contents: impl Into<String>) {
        self.change.push((path.into(), contents.into()));
    }
}

fn write_all(root: &Path, files: &[(String, String)]) {
    for (path, contents) in files {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().expect("file has a parent")).expect("create dir");
        std::fs::write(path, contents).expect("write project file");
    }
}

/// Write the project as two commits, and return the SHA of the base commit.
/// The files are written in a seeded order, so the directory entry order on
/// disk also changes with the seed.
pub fn materialize(root: &Path, project: &Project, rng: &mut Rng) -> String {
    let mut files: Vec<(String, String)> = project.files.clone();
    rng.shuffle(&mut files);
    write_all(root, &files);
    git(root, &["init", "-q", "-b", "main"]);
    commit_all(root, "base");
    let base = git_capture(root, &["rev-parse", "HEAD"]);
    write_all(root, &project.change);
    commit_all(root, "change");
    base
}
