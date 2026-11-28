//! The disk walk of a glob with a `**` segment.
//!
//! The `glob` crate reads a folder again for each way that leads to it: each
//! split of the path over several `**` segments, and each folder link. The
//! work then grows with the number of these ways, and a folder link loop has
//! no end. This walk handles each pair of a real folder and a pattern
//! segment once, so the work is at most the number of folders times the
//! number of segments. The matches are the same as those of the `glob`
//! crate, except that the walk does not follow a link to a folder outside
//! the root.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use rustc_hash::FxHashSet;

/// One segment of the pattern.
enum Segment {
    /// A name without glob syntax.
    Literal(String),
    /// A name with glob syntax.
    Wildcard(glob::Pattern),
    /// `**`: zero or more folders.
    Recursive,
}

/// The paths below `root` that the root-relative `pattern` matches, sorted.
/// Returns `None` for a pattern that is not valid glob syntax or a root that
/// does not resolve.
pub(super) fn walk_recursive_glob(root: &Path, pattern: &str) -> Option<Vec<PathBuf>> {
    let segments = parse_segments(pattern)?;
    let canonical_root = dunce::canonicalize(root).ok()?;
    let mut walk = Walk {
        segments: &segments,
        canonical_root: &canonical_root,
        visited: FxHashSet::default(),
        pending: Vec::new(),
        matches: BTreeSet::new(),
    };
    if segments.is_empty() {
        return Some(Vec::new());
    }
    walk.pending
        .push((root.to_path_buf(), canonical_root.clone(), 0));
    while let Some((dir, real, index)) = walk.pending.pop() {
        if walk.visited.insert((real, index)) {
            walk.step(&dir, index);
        }
    }
    Some(walk.matches.into_iter().collect())
}

/// The segments of `pattern`, with `.` and empty segments dropped and
/// consecutive `**` segments joined.
fn parse_segments(pattern: &str) -> Option<Vec<Segment>> {
    glob::Pattern::new(pattern).ok()?;
    let mut segments = Vec::new();
    for text in pattern.split(std::path::is_separator) {
        if text.is_empty() || text == "." {
            continue;
        }
        if text == "**" {
            if !matches!(segments.last(), Some(Segment::Recursive)) {
                segments.push(Segment::Recursive);
            }
        } else if text.contains(['*', '?', '[']) {
            segments.push(Segment::Wildcard(glob::Pattern::new(text).ok()?));
        } else {
            segments.push(Segment::Literal(text.to_owned()));
        }
    }
    Some(segments)
}

struct Walk<'a> {
    segments: &'a [Segment],
    canonical_root: &'a Path,
    /// The pairs of a real folder and a segment index that the walk handled.
    visited: FxHashSet<(PathBuf, usize)>,
    /// The folders that wait for a segment: the path as the walk reached it,
    /// its real path, and the segment index.
    pending: Vec<(PathBuf, PathBuf, usize)>,
    matches: BTreeSet<PathBuf>,
}

impl Walk<'_> {
    /// Match the segment `index` against the entries of `dir`.
    fn step(&mut self, dir: &Path, index: usize) {
        match &self.segments[index] {
            Segment::Literal(name) => {
                let path = dir.join(name);
                if std::fs::symlink_metadata(&path).is_ok() {
                    self.advance(path, index + 1);
                }
            }
            Segment::Wildcard(pattern) => {
                for (path, name) in sorted_entries(dir) {
                    if name.to_str().is_some_and(|name| pattern.matches(name)) {
                        self.advance(path, index + 1);
                    }
                }
            }
            Segment::Recursive => {
                let last = index + 1 == self.segments.len();
                if !last {
                    self.advance(dir.to_path_buf(), index + 1);
                }
                for (path, _) in sorted_entries(dir) {
                    if !path.is_dir() {
                        continue;
                    }
                    if last {
                        self.matches.insert(path.clone());
                    }
                    self.queue_folder(path, index);
                }
            }
        }
    }

    /// `path` matched the segments before `index`.
    fn advance(&mut self, path: PathBuf, index: usize) {
        if index == self.segments.len() {
            self.matches.insert(path);
        } else if path.is_dir() {
            self.queue_folder(path, index);
        }
    }

    /// Queue the folder `path` for the segment `index`, when its real path
    /// is inside the root.
    fn queue_folder(&mut self, path: PathBuf, index: usize) {
        let Ok(real) = dunce::canonicalize(&path) else {
            return;
        };
        if real.starts_with(self.canonical_root) && !self.visited.contains(&(real.clone(), index)) {
            self.pending.push((path, real, index));
        }
    }
}

/// The entries of `dir` with their names, sorted by name. An unreadable
/// folder has no entries.
fn sorted_entries(dir: &Path) -> Vec<(PathBuf, std::ffi::OsString)> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut entries: Vec<(PathBuf, std::ffi::OsString)> = entries
        .filter_map(Result::ok)
        .map(|entry| (entry.path(), entry.file_name()))
        .collect();
    entries.sort_by(|left, right| left.1.cmp(&right.1));
    entries
}
