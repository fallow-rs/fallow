//! The last known content of each file that a save or a watched-file event
//! reported.
//!
//! A watched-file event does not prove that the content of a file changed.
//! A client reports one event for a save that `didSave` already reported, and
//! a file watcher also reports a changed timestamp or attribute, for example
//! when a virus scanner, a sync client or another tool touches the file. When
//! such events keep coming, each one starts a new run, and the server never
//! comes to rest (#5278). The server therefore compares the content of the
//! file with the content that it recorded last, and an event for unchanged
//! content starts no run.

use std::hash::Hasher as _;
use std::io::Read as _;
use std::path::{Path, PathBuf};

use rustc_hash::{FxHashMap, FxHasher};

/// Most paths that the store keeps. The store starts again when it is full,
/// so memory stays bounded in a large workspace.
const MAX_RECORDED_FILES: usize = 4_096;

/// Largest file that the store reads. An event for a larger file always
/// counts as a change.
const MAX_FINGERPRINT_BYTES: u64 = 8 * 1024 * 1024;

/// Most events of one notification that the server compares. A larger batch,
/// for example a branch switch, always counts as a change.
pub const MAX_COMPARED_EVENTS: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Fingerprint {
    len: u64,
    hash: u64,
}

fn read_fingerprint(path: &Path) -> Option<Fingerprint> {
    let file = std::fs::File::open(path).ok()?;
    let metadata = file.metadata().ok()?;
    if !metadata.is_file() || metadata.len() > MAX_FINGERPRINT_BYTES {
        return None;
    }
    let mut content = Vec::new();
    file.take(MAX_FINGERPRINT_BYTES + 1)
        .read_to_end(&mut content)
        .ok()?;
    let mut hasher = FxHasher::default();
    hasher.write(&content);
    Some(Fingerprint {
        len: content.len() as u64,
        hash: hasher.finish(),
    })
}

/// The last known content of the reported files.
#[derive(Debug, Default)]
pub struct DiskFingerprints {
    known: FxHashMap<PathBuf, Fingerprint>,
}

impl DiskFingerprints {
    /// Record the current content of `path`. Returns whether the content
    /// differs from the last record, or whether the store cannot tell.
    pub fn record(&mut self, path: &Path) -> bool {
        let Some(fingerprint) = read_fingerprint(path) else {
            self.known.remove(path);
            return true;
        };
        if self.known.get(path) == Some(&fingerprint) {
            return false;
        }
        if self.known.len() >= MAX_RECORDED_FILES && !self.known.contains_key(path) {
            self.known.clear();
        }
        self.known.insert(path.to_path_buf(), fingerprint);
        true
    }

    /// Forget `path`, so the next event for it counts as a change.
    pub fn forget(&mut self, path: &Path) {
        self.known.remove(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_record_counts_as_a_change() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("a.ts");
        std::fs::write(&path, "export const a = 1;\n").expect("write");
        assert!(DiskFingerprints::default().record(&path));
    }

    #[test]
    fn unchanged_content_is_no_change() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("a.ts");
        std::fs::write(&path, "export const a = 1;\n").expect("write");
        let mut store = DiskFingerprints::default();
        assert!(store.record(&path));
        std::fs::write(&path, "export const a = 1;\n").expect("rewrite same content");
        assert!(!store.record(&path));
    }

    #[test]
    fn new_content_of_the_same_size_is_a_change() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("a.ts");
        std::fs::write(&path, "export const a = 1;\n").expect("write");
        let mut store = DiskFingerprints::default();
        store.record(&path);
        std::fs::write(&path, "export const a = 2;\n").expect("edit");
        assert!(store.record(&path));
        assert!(!store.record(&path));
    }

    #[test]
    fn a_missing_file_is_a_change_and_is_forgotten() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("a.ts");
        std::fs::write(&path, "export const a = 1;\n").expect("write");
        let mut store = DiskFingerprints::default();
        store.record(&path);
        std::fs::remove_file(&path).expect("remove");
        assert!(store.record(&path));
        std::fs::write(&path, "export const a = 1;\n").expect("create again");
        assert!(store.record(&path), "a created file counts as a change");
    }

    #[test]
    fn a_forgotten_path_counts_as_a_change() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("a.ts");
        std::fs::write(&path, "export const a = 1;\n").expect("write");
        let mut store = DiskFingerprints::default();
        store.record(&path);
        store.forget(&path);
        assert!(store.record(&path));
    }
}
