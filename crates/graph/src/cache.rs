//! Persisted graph-cache identity contracts.
//!
//! This module does not read or write graph caches yet. It defines the
//! invalidation surface a future persisted graph cache must satisfy before a
//! cached graph can be trusted.

use std::path::Path;

use fallow_types::discover::{DiscoveredFile, StableFileKey};
use fallow_types::source_fingerprint::SourceFingerprint;

/// Persisted graph cache schema version.
pub const GRAPH_CACHE_VERSION: u32 = 1;

/// Option dimensions that affect graph construction.
///
/// The hashes are intentionally opaque to this crate. Callers decide which
/// resolver/plugin/entry-point inputs feed each hash, while this contract keeps
/// graph-cache validation explicit and typed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GraphCacheMode {
    /// Import resolver and tsconfig-relevant options.
    pub resolver_options_hash: u64,
    /// Entry point set and reachability root options.
    pub entry_points_hash: u64,
    /// Plugin-derived graph-affecting configuration.
    pub plugin_config_hash: u64,
}

impl GraphCacheMode {
    /// Build a mode from explicit hash dimensions.
    #[must_use]
    pub const fn new(
        resolver_options_hash: u64,
        entry_points_hash: u64,
        plugin_config_hash: u64,
    ) -> Self {
        Self {
            resolver_options_hash,
            entry_points_hash,
            plugin_config_hash,
        }
    }
}

/// Source freshness for one file in a graph-cache manifest.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GraphCacheFile {
    /// Persistable identity for the file.
    pub key: StableFileKey,
    /// Metadata fingerprint for cache invalidation.
    pub fingerprint: SourceFingerprint,
}

impl GraphCacheFile {
    /// Build a graph-cache file row from a discovered file and fingerprint.
    #[must_use]
    pub fn from_discovered_file(
        root: &Path,
        file: &DiscoveredFile,
        fingerprint: SourceFingerprint,
    ) -> Self {
        Self {
            key: StableFileKey::from_root_relative(root, &file.path),
            fingerprint,
        }
    }
}

/// Manifest inputs required to trust a persisted graph cache entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphCacheManifest {
    /// Schema version used by the persisted graph-cache entry.
    pub version: u32,
    /// Graph-affecting option dimensions.
    pub mode: GraphCacheMode,
    /// Stable file identities and freshness metadata.
    pub files: Vec<GraphCacheFile>,
}

impl GraphCacheManifest {
    /// Build a manifest and sort files by stable key for deterministic compare.
    #[must_use]
    pub fn new(mode: GraphCacheMode, mut files: Vec<GraphCacheFile>) -> Self {
        sort_files(&mut files);
        Self {
            version: GRAPH_CACHE_VERSION,
            mode,
            files,
        }
    }

    /// Build a manifest from discovered files plus a fingerprint provider.
    pub fn from_discovered_files(
        root: &Path,
        files: &[DiscoveredFile],
        mode: GraphCacheMode,
        mut fingerprint_for_path: impl FnMut(&Path) -> SourceFingerprint,
    ) -> Self {
        let rows = files
            .iter()
            .map(|file| {
                GraphCacheFile::from_discovered_file(root, file, fingerprint_for_path(&file.path))
            })
            .collect();
        Self::new(mode, rows)
    }

    /// True when a persisted manifest matches the current graph inputs.
    #[must_use]
    pub fn matches_inputs(&self, current: &Self) -> bool {
        self.version == GRAPH_CACHE_VERSION
            && current.version == GRAPH_CACHE_VERSION
            && self.mode == current.mode
            && self.files == current.files
    }
}

fn sort_files(files: &mut [GraphCacheFile]) {
    files.sort_unstable_by(|a, b| a.key.cmp(&b.key));
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use fallow_types::discover::FileId;
    use rustc_hash::FxHashMap;

    use super::*;

    fn file(id: u32, path: &str) -> DiscoveredFile {
        DiscoveredFile {
            id: FileId(id),
            path: PathBuf::from(path),
            size_bytes: 1,
        }
    }

    fn mode() -> GraphCacheMode {
        GraphCacheMode::new(1, 2, 3)
    }

    fn fingerprints(pairs: &[(&str, SourceFingerprint)]) -> FxHashMap<PathBuf, SourceFingerprint> {
        pairs
            .iter()
            .map(|(path, fingerprint)| (PathBuf::from(path), *fingerprint))
            .collect()
    }

    fn manifest(
        files: &[DiscoveredFile],
        mode: GraphCacheMode,
        map: &FxHashMap<PathBuf, SourceFingerprint>,
    ) -> GraphCacheManifest {
        GraphCacheManifest::from_discovered_files(Path::new("/project"), files, mode, |path| {
            *map.get(path).unwrap()
        })
    }

    #[test]
    fn manifest_sorts_by_stable_file_key() {
        let files = vec![file(0, "/project/src/z.ts"), file(1, "/project/src/a.ts")];
        let map = fingerprints(&[
            ("/project/src/z.ts", SourceFingerprint::new(10, 1)),
            ("/project/src/a.ts", SourceFingerprint::new(20, 1)),
        ]);

        let manifest = manifest(&files, mode(), &map);

        let keys: Vec<&str> = manifest
            .files
            .iter()
            .map(|file| file.key.as_str())
            .collect();
        assert_eq!(keys, vec!["src/a.ts", "src/z.ts"]);
    }

    #[test]
    fn manifest_matches_across_file_id_shift() {
        let before = vec![file(0, "/project/src/a.ts"), file(1, "/project/src/c.ts")];
        let after = vec![file(9, "/project/src/c.ts"), file(2, "/project/src/a.ts")];
        let map = fingerprints(&[
            ("/project/src/a.ts", SourceFingerprint::new(10, 1)),
            ("/project/src/c.ts", SourceFingerprint::new(20, 1)),
        ]);

        let cached = manifest(&before, mode(), &map);
        let current = manifest(&after, mode(), &map);

        assert!(cached.matches_inputs(&current));
    }

    #[test]
    fn manifest_misses_on_fingerprint_change() {
        let files = vec![file(0, "/project/src/a.ts")];
        let cached_map = fingerprints(&[("/project/src/a.ts", SourceFingerprint::new(10, 1))]);
        let current_map = fingerprints(&[("/project/src/a.ts", SourceFingerprint::new(11, 1))]);

        let cached = manifest(&files, mode(), &cached_map);
        let current = manifest(&files, mode(), &current_map);

        assert!(!cached.matches_inputs(&current));
    }

    #[test]
    fn manifest_misses_on_mode_change() {
        let files = vec![file(0, "/project/src/a.ts")];
        let map = fingerprints(&[("/project/src/a.ts", SourceFingerprint::new(10, 1))]);

        let cached = manifest(&files, mode(), &map);
        let current = manifest(&files, GraphCacheMode::new(1, 99, 3), &map);

        assert!(!cached.matches_inputs(&current));
    }

    #[test]
    fn manifest_misses_on_version_change() {
        let files = vec![file(0, "/project/src/a.ts")];
        let map = fingerprints(&[("/project/src/a.ts", SourceFingerprint::new(10, 1))]);
        let mut cached = manifest(&files, mode(), &map);
        let current = manifest(&files, mode(), &map);

        cached.version = GRAPH_CACHE_VERSION + 1;

        assert!(!cached.matches_inputs(&current));
    }
}
