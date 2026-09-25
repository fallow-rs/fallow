//! Parsed modules that a long-lived process keeps across analysis sessions.
//!
//! A typed MCP tool call builds a new [`AnalysisSession`] for each call. Each
//! session loads the persisted parse cache, checks each file against it, and
//! writes the cache back. A process that installs a [`WarmParseStore`] keeps
//! the parsed modules of recent sessions in memory. A later session with the
//! same file list and the same file fingerprints takes its modules from the
//! store and does no parse work.
//!
//! The store is safe to share between sessions with different configs,
//! because a parse depends only on the file path, the file content, and the
//! config hash of the persisted parse cache. The key holds all three: the
//! project root and the cache config hash, the ordered file list (the file ids
//! follow from it), and one fingerprint per file. The store keeps a module
//! only when each fingerprint can stand in for the file content without a
//! content check, which needs a known ctime. On a platform with no ctime, such
//! as Windows, each session parses through the persisted cache as before.
//!
//! [`AnalysisSession`]: crate::session::AnalysisSession

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError, RwLock};

use fallow_types::discover::DiscoveredFile;
use fallow_types::extract::{ModuleInfo, SourceParseDegradation, SourceReadFailure};
use fallow_types::source_fingerprint::SourceFingerprint;

/// The default number of parsed file lists that a store keeps.
pub const DEFAULT_MAX_ENTRIES: usize = 4;

/// The default limit on the summed source size of the kept file lists.
pub const DEFAULT_MAX_SOURCE_BYTES: u64 = 256 * 1024 * 1024;

/// Memory limits of a [`WarmParseStore`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WarmParseLimits {
    /// The most parsed file lists that the store keeps. The store removes the
    /// least recently used list first.
    pub max_entries: usize,
    /// The limit on the summed source size, in bytes, of the kept file lists.
    /// A file list that is larger than this limit is not kept.
    pub max_source_bytes: u64,
}

impl Default for WarmParseLimits {
    fn default() -> Self {
        Self {
            max_entries: DEFAULT_MAX_ENTRIES,
            max_source_bytes: DEFAULT_MAX_SOURCE_BYTES,
        }
    }
}

/// The parse work of the sessions that used a store.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WarmParseCounts {
    /// Parse passes over a full file list.
    pub parse_runs: usize,
    /// Files parsed from source.
    pub modules_parsed: usize,
    /// Files served from the persisted parse cache.
    pub disk_cache_hits: usize,
    /// Files served from the modules in the store.
    pub modules_reused: usize,
}

/// Parsed modules kept across analysis sessions in one process.
#[derive(Debug)]
pub struct WarmParseStore {
    limits: WarmParseLimits,
    entries: Mutex<Vec<WarmEntry>>,
    parse_runs: AtomicUsize,
    modules_parsed: AtomicUsize,
    disk_cache_hits: AtomicUsize,
    modules_reused: AtomicUsize,
}

#[derive(Debug)]
struct WarmEntry {
    root: PathBuf,
    cache_config_hash: u64,
    paths: Vec<PathBuf>,
    fingerprints: Vec<SourceFingerprint>,
    has_complexity: bool,
    source_bytes: u64,
    parse: WarmParse,
}

/// The identity of one parse: the project, the file list, and the file
/// fingerprints.
#[derive(Debug, Clone, Copy)]
pub(crate) struct WarmParseKey<'a> {
    pub(crate) root: &'a Path,
    pub(crate) cache_config_hash: u64,
    pub(crate) files: &'a [DiscoveredFile],
    pub(crate) fingerprints: &'a [SourceFingerprint],
}

impl WarmParseKey<'_> {
    /// Whether the store may keep the modules of this parse. Each fingerprint
    /// must stand in for the file content without a content check.
    pub(crate) fn is_reusable(&self) -> bool {
        self.files.len() == self.fingerprints.len()
            && self
                .fingerprints
                .iter()
                .all(|fingerprint| fingerprint.is_trustworthy_without_content())
    }

    fn source_bytes(&self) -> u64 {
        self.fingerprints
            .iter()
            .map(|fingerprint| fingerprint.file_size)
            .sum()
    }
}

/// The output of one parse that a later session can use again.
#[derive(Debug, Clone)]
pub(crate) struct WarmParse {
    pub(crate) modules: Arc<[ModuleInfo]>,
    pub(crate) read_failures: Arc<[SourceReadFailure]>,
    pub(crate) parse_degradations: Arc<[SourceParseDegradation]>,
}

impl WarmEntry {
    fn matches(&self, key: &WarmParseKey<'_>) -> bool {
        self.matches_files(key) && self.fingerprints == key.fingerprints
    }

    fn matches_files(&self, key: &WarmParseKey<'_>) -> bool {
        self.cache_config_hash == key.cache_config_hash
            && self.root == key.root
            && self
                .paths
                .iter()
                .eq(key.files.iter().map(|file| &file.path))
    }
}

impl WarmParseStore {
    /// Create an empty store with the given memory limits.
    #[must_use]
    pub fn new(limits: WarmParseLimits) -> Self {
        Self {
            limits,
            entries: Mutex::new(Vec::new()),
            parse_runs: AtomicUsize::new(0),
            modules_parsed: AtomicUsize::new(0),
            disk_cache_hits: AtomicUsize::new(0),
            modules_reused: AtomicUsize::new(0),
        }
    }

    /// The parse work of the sessions that used this store.
    #[must_use]
    pub fn counts(&self) -> WarmParseCounts {
        WarmParseCounts {
            parse_runs: self.parse_runs.load(Ordering::Relaxed),
            modules_parsed: self.modules_parsed.load(Ordering::Relaxed),
            disk_cache_hits: self.disk_cache_hits.load(Ordering::Relaxed),
            modules_reused: self.modules_reused.load(Ordering::Relaxed),
        }
    }

    /// The number of parsed file lists in the store.
    #[must_use]
    pub fn len(&self) -> usize {
        self.lock().len()
    }

    /// Whether the store keeps no parsed file list.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.lock().is_empty()
    }

    /// The kept parse for `key`, when it has complexity or the caller needs
    /// none. A hit becomes the most recently used entry.
    pub(crate) fn get(&self, key: &WarmParseKey<'_>, need_complexity: bool) -> Option<WarmParse> {
        if !key.is_reusable() {
            return None;
        }
        let mut entries = self.lock();
        let position = entries
            .iter()
            .position(|entry| entry.matches(key) && (entry.has_complexity || !need_complexity))?;
        let entry = entries.remove(position);
        let parse = entry.parse.clone();
        entries.push(entry);
        drop(entries);
        self.modules_reused
            .fetch_add(parse.modules.len(), Ordering::Relaxed);
        Some(parse)
    }

    /// Keep the parse for `key`. It replaces an older parse of the same file
    /// list, and the least recently used entries leave the store until it is
    /// within its limits.
    pub(crate) fn put(&self, key: &WarmParseKey<'_>, has_complexity: bool, parse: WarmParse) {
        let source_bytes = key.source_bytes();
        let keep = key.is_reusable()
            && self.limits.max_entries > 0
            && source_bytes <= self.limits.max_source_bytes;
        let entry = keep.then(|| WarmEntry {
            root: key.root.to_path_buf(),
            cache_config_hash: key.cache_config_hash,
            paths: key.files.iter().map(|file| file.path.clone()).collect(),
            fingerprints: key.fingerprints.to_vec(),
            has_complexity,
            source_bytes,
            parse,
        });

        let mut entries = self.lock();
        entries.retain(|entry| !entry.matches_files(key));
        entries.extend(entry);
        let mut total_bytes: u64 = entries.iter().map(|entry| entry.source_bytes).sum();
        while entries.len() > self.limits.max_entries || total_bytes > self.limits.max_source_bytes
        {
            let removed = entries.remove(0);
            total_bytes -= removed.source_bytes;
        }
        drop(entries);
    }

    /// Count one parse pass over a full file list.
    pub(crate) fn record_parse(&self, cache_misses: usize, cache_hits: usize) {
        self.parse_runs.fetch_add(1, Ordering::Relaxed);
        self.modules_parsed
            .fetch_add(cache_misses, Ordering::Relaxed);
        self.disk_cache_hits
            .fetch_add(cache_hits, Ordering::Relaxed);
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<WarmEntry>> {
        self.entries.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

static INSTALLED: RwLock<Option<Arc<WarmParseStore>>> = RwLock::new(None);

/// Make `store` the store of each session that this process creates from now
/// on. `None` removes the store, so new sessions parse as before.
///
/// Only a long-lived process that runs many analyses of the same project
/// installs a store, such as the MCP server. A session keeps the store that
/// was installed when the session was created.
pub fn install(store: Option<Arc<WarmParseStore>>) {
    *INSTALLED.write().unwrap_or_else(PoisonError::into_inner) = store;
}

/// The store that new sessions use, if a process installed one.
#[must_use]
pub fn installed() -> Option<Arc<WarmParseStore>> {
    INSTALLED
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
}

#[cfg(test)]
mod tests {
    use fallow_types::discover::FileId;

    use super::*;

    fn files(paths: &[&str]) -> Vec<DiscoveredFile> {
        paths
            .iter()
            .enumerate()
            .map(|(index, path)| DiscoveredFile {
                id: FileId(u32::try_from(index).expect("small index")),
                path: PathBuf::from(path),
                size_bytes: 1,
            })
            .collect()
    }

    fn fingerprints(count: usize, size: u64) -> Vec<SourceFingerprint> {
        (0..count)
            .map(|index| SourceFingerprint::with_ctime(10 + index as u64, 20, size))
            .collect()
    }

    fn parse() -> WarmParse {
        WarmParse {
            modules: Arc::from(Vec::new()),
            read_failures: Arc::from(Vec::new()),
            parse_degradations: Arc::from(Vec::new()),
        }
    }

    fn key<'a>(
        root: &'a Path,
        files: &'a [DiscoveredFile],
        fingerprints: &'a [SourceFingerprint],
    ) -> WarmParseKey<'a> {
        WarmParseKey {
            root,
            cache_config_hash: 7,
            files,
            fingerprints,
        }
    }

    #[test]
    fn a_hit_needs_the_same_files_and_fingerprints() {
        let store = WarmParseStore::new(WarmParseLimits::default());
        let root = Path::new("/project");
        let listed = files(&["/project/a.ts", "/project/b.ts"]);
        let marks = fingerprints(2, 5);
        store.put(&key(root, &listed, &marks), true, parse());

        assert!(store.get(&key(root, &listed, &marks), true).is_some());

        let mut edited = marks.clone();
        edited[1].mtime_ns += 1;
        assert!(store.get(&key(root, &listed, &edited), false).is_none());

        let added = files(&["/project/a.ts", "/project/b.ts", "/project/c.ts"]);
        assert!(
            store
                .get(&key(root, &added, &fingerprints(3, 5)), false)
                .is_none()
        );

        let mut other_config = key(root, &listed, &marks);
        other_config.cache_config_hash = 8;
        assert!(store.get(&other_config, false).is_none());
    }

    #[test]
    fn a_parse_without_complexity_does_not_serve_a_request_for_it() {
        let store = WarmParseStore::new(WarmParseLimits::default());
        let root = Path::new("/project");
        let listed = files(&["/project/a.ts"]);
        let marks = fingerprints(1, 5);
        store.put(&key(root, &listed, &marks), false, parse());

        assert!(store.get(&key(root, &listed, &marks), true).is_none());
        assert!(store.get(&key(root, &listed, &marks), false).is_some());
    }

    #[test]
    fn fingerprints_without_ctime_are_not_kept() {
        let store = WarmParseStore::new(WarmParseLimits::default());
        let root = Path::new("/project");
        let listed = files(&["/project/a.ts"]);
        let marks = [SourceFingerprint::new(10, 5)];
        store.put(&key(root, &listed, &marks), true, parse());

        assert!(store.is_empty());
        assert!(store.get(&key(root, &listed, &marks), false).is_none());
    }

    #[test]
    fn a_new_parse_of_the_same_files_replaces_the_old_one() {
        let store = WarmParseStore::new(WarmParseLimits::default());
        let root = Path::new("/project");
        let listed = files(&["/project/a.ts"]);
        let before = fingerprints(1, 5);
        let after = fingerprints(1, 6);
        store.put(&key(root, &listed, &before), true, parse());
        store.put(&key(root, &listed, &after), true, parse());

        assert_eq!(store.len(), 1);
        assert!(store.get(&key(root, &listed, &after), true).is_some());
    }

    #[test]
    fn the_least_recently_used_entry_leaves_first() {
        let store = WarmParseStore::new(WarmParseLimits {
            max_entries: 2,
            max_source_bytes: u64::MAX,
        });
        let marks = fingerprints(1, 5);
        let first = files(&["/first/a.ts"]);
        let second = files(&["/second/a.ts"]);
        let third = files(&["/third/a.ts"]);
        store.put(&key(Path::new("/first"), &first, &marks), true, parse());
        store.put(&key(Path::new("/second"), &second, &marks), true, parse());
        assert!(
            store
                .get(&key(Path::new("/first"), &first, &marks), true)
                .is_some()
        );
        store.put(&key(Path::new("/third"), &third, &marks), true, parse());

        assert_eq!(store.len(), 2);
        assert!(
            store
                .get(&key(Path::new("/second"), &second, &marks), true)
                .is_none()
        );
        assert!(
            store
                .get(&key(Path::new("/first"), &first, &marks), true)
                .is_some()
        );
    }

    #[test]
    fn the_source_size_limit_bounds_the_store() {
        let store = WarmParseStore::new(WarmParseLimits {
            max_entries: 8,
            max_source_bytes: 10,
        });
        let small = files(&["/small/a.ts"]);
        let large = files(&["/large/a.ts", "/large/b.ts"]);
        store.put(
            &key(Path::new("/small"), &small, &fingerprints(1, 6)),
            true,
            parse(),
        );
        store.put(
            &key(Path::new("/large"), &large, &fingerprints(2, 6)),
            true,
            parse(),
        );
        assert_eq!(store.len(), 1, "a list over the limit is not kept");

        let other = files(&["/other/a.ts"]);
        store.put(
            &key(Path::new("/other"), &other, &fingerprints(1, 6)),
            true,
            parse(),
        );
        assert_eq!(
            store.len(),
            1,
            "the older list leaves to keep the sum within the limit"
        );
        assert!(
            store
                .get(&key(Path::new("/other"), &other, &fingerprints(1, 6)), true)
                .is_some()
        );
    }
}
