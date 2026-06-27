//! Persisted graph-cache identity contracts and on-disk store.
//!
//! The manifest types here define the invalidation surface a persisted graph
//! cache must satisfy before a cached graph can be trusted; the store implements
//! the coarse all-or-nothing load / save of a previously-built `ModuleGraph`
//! keyed by that manifest.

use std::path::Path;

use fallow_types::discover::{DiscoveredFile, StableFileKey};
use fallow_types::source_fingerprint::SourceFingerprint;

mod store;

pub use store::GraphCacheStore;

/// Persisted graph cache schema version.
///
/// Bump this whenever the serialized shape of the persisted graph (any of the
/// graph types that derive serde for the cache, the manifest types, or the
/// store envelope) changes, so a stale `graph-cache.bin` written by an older
/// binary is rejected rather than deserialized into the wrong shape.
pub const GRAPH_CACHE_VERSION: u32 = 1;

/// Serialize an [`oxc_span::Span`] as a `[start, end]` `u32` pair.
///
/// `oxc_span::Span` does not enable its own serde feature in this workspace, so
/// the graph types that carry spans route them through this module via
/// `#[serde(with = "crate::cache::span_serde")]`. A 2-element array keeps the
/// postcard encoding compact (two varints) and is trivially lossless: a `Span`
/// is fully described by its `start` / `end` offsets.
pub(crate) mod span_serde {
    use oxc_span::Span;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    #[expect(
        clippy::trivially_copy_pass_by_ref,
        reason = "serde `serialize_with` / `with` requires a `&T` signature"
    )]
    pub fn serialize<S: Serializer>(span: &Span, serializer: S) -> Result<S::Ok, S::Error> {
        [span.start, span.end].serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Span, D::Error> {
        let [start, end] = <[u32; 2]>::deserialize(deserializer)?;
        Ok(Span::new(start, end))
    }
}

/// Lossless cache (de)serialization for `Vec<MemberInfo>`.
///
/// `fallow_types::extract::MemberInfo` derives only `serde::Serialize`, and its
/// `span` field uses `serialize_with` with no matching deserializer, so it
/// cannot be deserialized through a plain derive. Rather than change the shared
/// type's serde shape (which would ripple into JSON output), the cache mirrors
/// it field-for-field into a dedicated `CachedMemberInfo` and converts both
/// ways. Every `MemberInfo` field is carried, so the round-trip is lossless.
pub(crate) mod member_serde {
    use fallow_types::extract::{MemberInfo, MemberKind};
    use oxc_span::Span;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    #[derive(Serialize, Deserialize)]
    struct CachedMemberInfo {
        name: String,
        kind: MemberKind,
        span: [u32; 2],
        has_decorator: bool,
        decorator_names: Vec<String>,
        is_instance_returning_static: bool,
        is_self_returning: bool,
    }

    impl From<&MemberInfo> for CachedMemberInfo {
        fn from(member: &MemberInfo) -> Self {
            Self {
                name: member.name.clone(),
                kind: member.kind,
                span: [member.span.start, member.span.end],
                has_decorator: member.has_decorator,
                decorator_names: member.decorator_names.clone(),
                is_instance_returning_static: member.is_instance_returning_static,
                is_self_returning: member.is_self_returning,
            }
        }
    }

    impl From<CachedMemberInfo> for MemberInfo {
        fn from(cached: CachedMemberInfo) -> Self {
            Self {
                name: cached.name,
                kind: cached.kind,
                span: Span::new(cached.span[0], cached.span[1]),
                has_decorator: cached.has_decorator,
                decorator_names: cached.decorator_names,
                is_instance_returning_static: cached.is_instance_returning_static,
                is_self_returning: cached.is_self_returning,
            }
        }
    }

    pub fn serialize<S: Serializer>(
        members: &[MemberInfo],
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        let mirror: Vec<CachedMemberInfo> = members.iter().map(CachedMemberInfo::from).collect();
        mirror.serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Vec<MemberInfo>, D::Error> {
        let mirror = Vec::<CachedMemberInfo>::deserialize(deserializer)?;
        Ok(mirror.into_iter().map(MemberInfo::from).collect())
    }
}

/// Option dimensions that affect graph construction.
///
/// The hashes are intentionally opaque to this crate. Callers decide which
/// resolver/plugin/entry-point inputs feed each hash, while this contract keeps
/// graph-cache validation explicit and typed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
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
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
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
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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
    fn manifest_misses_on_file_deletion() {
        let before = vec![
            file(0, "/project/src/a.ts"),
            file(1, "/project/src/deleted.ts"),
        ];
        let after = vec![file(0, "/project/src/a.ts")];
        let map = fingerprints(&[
            ("/project/src/a.ts", SourceFingerprint::new(10, 1)),
            ("/project/src/deleted.ts", SourceFingerprint::new(20, 1)),
        ]);

        let cached = manifest(&before, mode(), &map);
        let current = manifest(&after, mode(), &map);

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
