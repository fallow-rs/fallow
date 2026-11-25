//! The last known content of each file that a save or a watched-file event
//! reported.
//!
//! A watched-file event does not prove that the content of a file changed.
//! A client reports one event for a save that `didSave` already reported, and
//! a file watcher also reports a changed timestamp or attribute, for example
//! when a virus scanner, a sync client or another tool touches the file. When
//! such events keep coming, each one starts a new run, and the server never
//! comes to rest (#5278). The server therefore compares the content of the
//! file with the content that it recorded last, and a changed event for
//! unchanged content starts no run.

use std::hash::Hasher as _;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

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

/// The length and a hash of the content of one file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fingerprint {
    len: u64,
    hash: u64,
}

/// Read the fingerprint of `path`. `None` when the path is not a readable
/// file of at most [`MAX_FINGERPRINT_BYTES`].
pub fn read_fingerprint(path: &Path) -> Option<Fingerprint> {
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
    /// Each fingerprint with the number of the read that gave it.
    known: FxHashMap<PathBuf, (Fingerprint, u64)>,
    reads: u64,
}

impl DiskFingerprints {
    /// Number a read that starts now. Two notifications can read the same
    /// file at the same time, and the newer read must win.
    pub fn begin_read(&mut self) -> u64 {
        self.reads += 1;
        self.reads
    }

    /// Store the fingerprint that read `read` gave for `path`. Returns
    /// whether the content differs from the last record, or whether the
    /// store cannot tell.
    pub fn update(&mut self, path: &Path, fingerprint: Option<Fingerprint>, read: u64) -> bool {
        let Some(fingerprint) = fingerprint else {
            self.known.remove(path);
            return true;
        };
        match self.known.get(path) {
            Some((_, newer)) if *newer > read => return true,
            Some((known, _)) if *known == fingerprint => return false,
            _ => {}
        }
        if self.known.len() >= MAX_RECORDED_FILES && !self.known.contains_key(path) {
            self.known.clear();
        }
        self.known.insert(path.to_path_buf(), (fingerprint, read));
        true
    }

    /// Forget `path` and every recorded path below it. A client can report
    /// one event for a deleted directory and none for its files.
    pub fn forget(&mut self, path: &Path) {
        self.known.retain(|known, _| !known.starts_with(path));
    }

    /// Forget every record.
    pub fn clear(&mut self) {
        self.known.clear();
    }
}

/// Shortest time between two log lines about skipped events. A steady
/// stream of events would otherwise write one line per notification.
const SKIPPED_LOG_INTERVAL: Duration = Duration::from_secs(10);

/// Counts the skipped events between two log lines.
#[derive(Debug, Default)]
pub struct SkippedEventLog {
    pending: usize,
    last_logged: Option<Instant>,
}

impl SkippedEventLog {
    /// Add `skipped` events. Returns the count to log now, or `None` while
    /// the last log line is newer than [`SKIPPED_LOG_INTERVAL`].
    pub fn note(&mut self, skipped: usize, now: Instant) -> Option<usize> {
        self.pending += skipped;
        if self.pending == 0 {
            return None;
        }
        if self
            .last_logged
            .is_some_and(|last| now.duration_since(last) < SKIPPED_LOG_INTERVAL)
        {
            return None;
        }
        self.last_logged = Some(now);
        Some(std::mem::take(&mut self.pending))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skipped_events_are_logged_at_most_once_per_interval() {
        let mut log = SkippedEventLog::default();
        let start = Instant::now();
        assert_eq!(log.note(1, start), Some(1));
        assert_eq!(log.note(2, start + Duration::from_secs(1)), None);
        assert_eq!(log.note(3, start + Duration::from_secs(5)), None);
        assert_eq!(log.note(0, start + SKIPPED_LOG_INTERVAL), Some(5));
        assert_eq!(log.note(0, start + SKIPPED_LOG_INTERVAL * 3), None);
    }

    fn record(store: &mut DiskFingerprints, path: &Path) -> bool {
        let read = store.begin_read();
        store.update(path, read_fingerprint(path), read)
    }

    #[test]
    fn an_older_read_does_not_replace_a_newer_record() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("a.ts");
        std::fs::write(&path, "export const a = 1;\n").expect("write");
        let mut store = DiskFingerprints::default();
        let older = store.begin_read();
        let old_content = read_fingerprint(&path);
        std::fs::write(&path, "export const a = 2;\n").expect("edit");
        record(&mut store, &path);
        assert!(
            store.update(&path, old_content, older),
            "the older read counts"
        );
        assert!(!record(&mut store, &path), "the newer record stays");
    }

    #[test]
    fn the_first_record_counts_as_a_change() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("a.ts");
        std::fs::write(&path, "export const a = 1;\n").expect("write");
        assert!(record(&mut DiskFingerprints::default(), &path));
    }

    #[test]
    fn unchanged_content_is_no_change() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("a.ts");
        std::fs::write(&path, "export const a = 1;\n").expect("write");
        let mut store = DiskFingerprints::default();
        assert!(record(&mut store, &path));
        std::fs::write(&path, "export const a = 1;\n").expect("rewrite same content");
        assert!(!record(&mut store, &path));
    }

    #[test]
    fn new_content_of_the_same_size_is_a_change() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("a.ts");
        std::fs::write(&path, "export const a = 1;\n").expect("write");
        let mut store = DiskFingerprints::default();
        record(&mut store, &path);
        std::fs::write(&path, "export const a = 2;\n").expect("edit");
        assert!(record(&mut store, &path));
        assert!(!record(&mut store, &path));
    }

    #[test]
    fn a_missing_file_is_a_change_and_is_forgotten() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("a.ts");
        std::fs::write(&path, "export const a = 1;\n").expect("write");
        let mut store = DiskFingerprints::default();
        record(&mut store, &path);
        std::fs::remove_file(&path).expect("remove");
        assert!(record(&mut store, &path));
        std::fs::write(&path, "export const a = 1;\n").expect("create again");
        assert!(
            record(&mut store, &path),
            "a created file counts as a change"
        );
    }

    #[test]
    fn forgetting_a_directory_forgets_its_files() {
        let dir = tempfile::tempdir().expect("temp dir");
        let nested = dir.path().join("src");
        std::fs::create_dir(&nested).expect("dir");
        let path = nested.join("a.ts");
        std::fs::write(&path, "export const a = 1;\n").expect("write");
        let mut store = DiskFingerprints::default();
        record(&mut store, &path);
        store.forget(&nested);
        assert!(record(&mut store, &path));
    }
}
