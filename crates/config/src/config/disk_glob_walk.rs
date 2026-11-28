//! The disk walk of a glob with a `**` segment.
//!
//! The `glob` crate reads a folder again for each way that leads to it: each
//! split of the path over several `**` segments, and each folder link. The
//! work then grows with the number of these ways, and a folder link loop has
//! no end. This walk handles each pair of a real folder and a pattern
//! segment once, so the work is at most the number of folders times the
//! number of segments.
//!
//! The matches are the same as those of the `glob` crate, with one
//! exception: the walk does not enter a folder whose real path is outside
//! the read scope of the root ([`crate::project_scope_base`]), such as a
//! link to a folder above the git checkout. The walk is lazy, so a caller
//! that stops at the first match does not read the rest of the tree.

use std::collections::VecDeque;
use std::ffi::OsString;
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

impl Segment {
    /// Whether the entry name `name` matches this segment. The `glob` crate
    /// compares a name with its letter case also on a file system that
    /// ignores the case.
    fn matches_name(&self, name: &OsString) -> bool {
        match self {
            Self::Literal(literal) => name.to_str() == Some(literal.as_str()),
            Self::Wildcard(pattern) => name.to_str().is_some_and(|name| pattern.matches(name)),
            Self::Recursive => false,
        }
    }
}

/// The paths below `root` that the root-relative `pattern` matches, each
/// path once. Returns `None` for a pattern that is not valid glob syntax or
/// a root that does not resolve.
pub(super) fn walk_recursive_glob(root: &Path, pattern: &str) -> Option<RecursiveGlobWalk> {
    let segments = parse_segments(pattern)?;
    let canonical_root = dunce::canonicalize(root).ok()?;
    let scope = crate::project_scope_base(&canonical_root);
    let scope = dunce::canonicalize(&scope).unwrap_or(scope);
    let mut pending = Vec::new();
    if !segments.is_empty() {
        pending.push(Folder {
            path: root.to_path_buf(),
            real: canonical_root,
            segment: 0,
        });
    }
    Some(RecursiveGlobWalk {
        segments,
        scope,
        visited: FxHashSet::default(),
        pending,
        ready: VecDeque::new(),
        yielded: FxHashSet::default(),
    })
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

/// A folder that waits for the segment `segment`.
struct Folder {
    /// The path as the walk reached it.
    path: PathBuf,
    /// The real path.
    real: PathBuf,
    segment: usize,
}

/// One entry of a folder that the walk read.
struct Entry {
    path: PathBuf,
    name: OsString,
    real: PathBuf,
    is_dir: bool,
}

/// The lazy walk of a glob with a `**` segment. It yields each matching path
/// once.
pub(super) struct RecursiveGlobWalk {
    segments: Vec<Segment>,
    scope: PathBuf,
    /// The pairs of a real folder and a segment index that the walk handled.
    visited: FxHashSet<(PathBuf, usize)>,
    /// The folders still to read, the next one last.
    pending: Vec<Folder>,
    /// The matches of the last folder that the walk read.
    ready: VecDeque<PathBuf>,
    yielded: FxHashSet<PathBuf>,
}

impl Iterator for RecursiveGlobWalk {
    type Item = PathBuf;

    fn next(&mut self) -> Option<PathBuf> {
        loop {
            while let Some(path) = self.ready.pop_front() {
                if self.yielded.insert(path.clone()) {
                    return Some(path);
                }
            }
            let folder = self.pending.pop()?;
            if self.visited.insert((folder.real.clone(), folder.segment)) {
                self.step(&folder);
            }
        }
    }
}

impl RecursiveGlobWalk {
    /// Match the segment of `folder` against its entries.
    fn step(&mut self, folder: &Folder) {
        let index = folder.segment;
        let mut queued = Vec::new();
        match &self.segments[index] {
            Segment::Literal(name) => {
                let path = folder.path.join(name);
                if let Some(entry) = literal_entry(path, name, &folder.real) {
                    self.advance(entry, index + 1, &mut queued);
                }
            }
            Segment::Wildcard(_) => {
                for entry in read_entries(folder) {
                    if self.segments[index].matches_name(&entry.name) {
                        self.advance(entry, index + 1, &mut queued);
                    }
                }
            }
            Segment::Recursive => {
                let last = index + 1 == self.segments.len();
                for entry in read_entries(folder) {
                    if entry.is_dir {
                        if last {
                            self.ready.push_back(entry.path.clone());
                        }
                        self.queue(&entry, index, &mut queued);
                    }
                    if !last && self.segments[index + 1].matches_name(&entry.name) {
                        self.advance(entry, index + 2, &mut queued);
                    }
                }
            }
        }
        // The first entry in name order is read first.
        self.pending.extend(queued.into_iter().rev());
    }

    /// `entry` matched the segments before `index`.
    fn advance(&mut self, entry: Entry, index: usize, queued: &mut Vec<Folder>) {
        if index == self.segments.len() {
            self.ready.push_back(entry.path);
        } else if entry.is_dir {
            self.queue(&entry, index, queued);
        }
    }

    /// Queue the folder `entry` for the segment `index`, when its real path
    /// is inside the read scope.
    fn queue(&self, entry: &Entry, index: usize, queued: &mut Vec<Folder>) {
        if entry.real.starts_with(&self.scope)
            && !self.visited.contains(&(entry.real.clone(), index))
        {
            queued.push(Folder {
                path: entry.path.clone(),
                real: entry.real.clone(),
                segment: index,
            });
        }
    }
}

/// The entry `name` of the folder with the real path `parent_real`, when it
/// exists. The `glob` crate checks a literal segment after another literal
/// segment with a file system lookup, so this check does too.
fn literal_entry(path: PathBuf, name: &str, parent_real: &Path) -> Option<Entry> {
    let metadata = std::fs::symlink_metadata(&path).ok()?;
    let (real, is_dir) = if metadata.file_type().is_symlink() {
        link_target(&path)
    } else {
        (parent_real.join(name), metadata.is_dir())
    };
    Some(Entry {
        path,
        name: OsString::from(name),
        real,
        is_dir,
    })
}

/// The real path of the link `path`, and whether it leads to a folder. A
/// link that does not resolve leads to no folder.
fn link_target(path: &Path) -> (PathBuf, bool) {
    match dunce::canonicalize(path) {
        Ok(real) => {
            let is_dir = real.is_dir();
            (real, is_dir)
        }
        Err(_) => (path.to_path_buf(), false),
    }
}

/// The entries of `folder`, sorted by name. Only a link costs a lookup of
/// its real path. An unreadable folder has no entries.
fn read_entries(folder: &Folder) -> Vec<Entry> {
    let Ok(entries) = std::fs::read_dir(&folder.path) else {
        return Vec::new();
    };
    let mut entries: Vec<Entry> = entries
        .filter_map(Result::ok)
        .map(|entry| {
            let path = entry.path();
            let name = entry.file_name();
            let kind = entry.file_type().ok();
            let (real, is_dir) = match kind {
                Some(kind) if kind.is_symlink() => link_target(&path),
                Some(kind) => (folder.real.join(&name), kind.is_dir()),
                None => (folder.real.join(&name), false),
            };
            Entry {
                path,
                name,
                real,
                is_dir,
            }
        })
        .collect();
    entries.sort_by(|left, right| left.name.cmp(&right.name));
    entries
}
