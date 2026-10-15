//! Persisted graph-cache store: coarse all-or-nothing load / save of a
//! previously-built [`ModuleGraph`].
//!
//! Mirrors the extraction cache store (`fallow_extract::cache::store`): the
//! payload is postcard-encoded, written atomically via a uniquely named temp
//! file plus best-effort fsync and rename, and a `.gitignore` is written alongside
//! so `.fallow/` is never committed. Every IO error is swallowed (the graph
//! cache is best-effort and must never fail analysis); a corrupt or
//! version-mismatched file misses and the graph is rebuilt fresh, but the
//! loader names WHY it missed through [`CacheRejection`] so the run can report
//! a refusal instead of leaving it indistinguishable from a first run.

use std::path::Path;

use fallow_types::cache_rejection::CacheRejection;
use serde::{Deserialize, Serialize};

use super::{CachedResolvedProject, GRAPH_CACHE_VERSION, GraphCacheManifest};
use crate::graph::ModuleGraph;

/// Filename of the persisted graph cache inside the cache directory.
pub const GRAPH_CACHE_FILE: &str = "graph-cache.bin";

/// On-disk graph cache entry: a manifest plus the graph it validates.
#[derive(Serialize, Deserialize)]
pub struct GraphCacheStore {
    /// Schema version. Checked on load; a mismatch misses so a stale file from
    /// an older binary is never deserialized into the wrong shape.
    pub version: u32,
    /// Inputs that must match the current run for the graph to be trusted.
    pub manifest: GraphCacheManifest,
    /// The previously-built graph. Its `namespace_imported` bitset is
    /// `#[serde(skip)]`, so the loader reconstructs it from the edge set.
    pub graph: ModuleGraph,
    /// Resolver output aligned with the cached graph. Exact manifest hits use
    /// it alongside the graph; stable-key resolver hits remap it and rebuild
    /// the graph with current `FileId`s.
    pub resolved_project: CachedResolvedProject,
}

impl GraphCacheStore {
    /// Load the persisted graph cache from `cache_dir`.
    ///
    /// # Errors
    ///
    /// Returns the [`CacheRejection`] that decided against reuse: the file is
    /// missing, undecodable, or written for a different
    /// `GRAPH_CACHE_VERSION`. The caller compares the loaded manifest against
    /// the current inputs before trusting the graph or resolver payload, and
    /// reports its own rejection reason for that comparison.
    ///
    /// The version is read from the file header BEFORE the payload is
    /// decoded. A format bump changes the encoded shape, so a blob
    /// from the previous release fails to decode and a version comparison made
    /// afterwards is unreachable on the one event that triggers it most: an
    /// upgrade. `fallow doctor` reports this reason verbatim, so a routine
    /// version bump must not read as corruption, and a file that is not a
    /// fallow cache at all must not read as a version bump.
    ///
    /// A file that existed and was then refused logs at warn: the run paid the
    /// read and the decode and reused nothing. A missing file stays quiet.
    pub fn load(cache_dir: &Path) -> Result<Self, CacheRejection> {
        let cache_file = cache_dir.join(GRAPH_CACHE_FILE);
        let data = std::fs::read(&cache_file).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                return CacheRejection::Absent;
            }
            tracing::warn!("Cache file could not be read; check the path and permissions");
            CacheRejection::Unreadable
        })?;
        let payload = read_header(&data)?;
        let mut store: Self = match postcard::from_bytes(payload) {
            Ok(store) => store,
            Err(_) => {
                tracing::warn!(
                    "Graph cache carries the current format version but its payload could not be \
                     decoded, rebuilding"
                );
                return Err(CacheRejection::Undecodable);
            }
        };
        // The header already agreed with `GRAPH_CACHE_VERSION`, so this catches
        // only a file whose header and payload disagree.
        if store.version != GRAPH_CACHE_VERSION {
            tracing::warn!(
                cached_version = store.version,
                expected_version = GRAPH_CACHE_VERSION,
                "Graph cache header and payload declare different format versions, rebuilding"
            );
            return Err(CacheRejection::VersionMismatch);
        }
        // `namespace_imported` is `#[serde(skip)]`; rebuild it from the persisted
        // edges so the loaded graph is byte-identical to a fresh build.
        store.graph.reconstruct_namespace_imported();
        Ok(store)
    }

    /// Persist this graph cache to `cache_dir`, best-effort.
    ///
    /// Creates the cache directory, writes a `.gitignore`, encodes the store
    /// with postcard, and writes `graph-cache.bin` atomically. Every IO error
    /// is logged at debug and swallowed; the graph cache must never fail the
    /// surrounding analysis run.
    pub fn save(&self, cache_dir: &Path) {
        if let Err(error) = std::fs::create_dir_all(cache_dir) {
            tracing::debug!("Failed to create graph cache dir: {error}");
            return;
        }
        if let Err(error) = write_cache_gitignore(cache_dir) {
            tracing::debug!("Failed to write graph cache .gitignore: {error}");
            // Continue: a missing .gitignore does not invalidate the cache file.
        }

        let encoded = match postcard::to_allocvec(self) {
            Ok(bytes) => bytes,
            Err(error) => {
                tracing::debug!("Failed to encode graph cache: {error}");
                return;
            }
        };

        let cache_file = cache_dir.join(GRAPH_CACHE_FILE);
        if let Err(error) = atomic_write(&cache_file, &framed(self.version, &encoded)) {
            tracing::debug!("Failed to write graph cache: {error}");
        }
    }
}

/// Marker written ahead of new graph-cache payload so the format version can
/// be read without decoding the payload it describes.
///
/// Constant across format bumps: only the version field beside it moves. That
/// lets future upgrades report an explicit version mismatch; older unframed
/// caches still report an ambiguous decode failure.
const GRAPH_CACHE_MAGIC: [u8; 4] = *b"FLWG";

/// Bytes the framing adds ahead of the payload: the magic plus a little-endian
/// `u32` format version.
const GRAPH_CACHE_HEADER_LEN: usize = GRAPH_CACHE_MAGIC.len() + 4;

/// Prepend the format header to an encoded payload.
///
/// The version comes from the store being written rather than from the
/// constant, so the header always describes the payload behind it.
fn framed(version: u32, payload: &[u8]) -> Vec<u8> {
    let mut framed = Vec::with_capacity(GRAPH_CACHE_HEADER_LEN + payload.len());
    framed.extend_from_slice(&GRAPH_CACHE_MAGIC);
    framed.extend_from_slice(&version.to_le_bytes());
    framed.extend_from_slice(payload);
    framed
}

/// Split a cache file into its declared version and its payload, refusing
/// anything this binary cannot read WITHOUT decoding it first.
///
/// A recognized header exposes a version mismatch without decoding. Releases
/// before framing wrote raw payloads, so a missing header cannot distinguish
/// an older cache from foreign or damaged data. `Undecodable` keeps that
/// uncertainty explicit and the next successful run replaces the blob.
fn read_header(data: &[u8]) -> Result<&[u8], CacheRejection> {
    let Some((header, payload)) = data.split_at_checked(GRAPH_CACHE_HEADER_LEN) else {
        tracing::warn!("Graph cache is too short to carry a format header, rebuilding");
        return Err(CacheRejection::Undecodable);
    };
    let (declared_magic, declared_version) = header.split_at(GRAPH_CACHE_MAGIC.len());
    if declared_magic != GRAPH_CACHE_MAGIC {
        tracing::warn!("Graph cache does not carry fallow's cache framing, rebuilding");
        return Err(CacheRejection::Undecodable);
    }
    // The slice is exactly four bytes; the fallback only has to be a version
    // this binary never writes, so an impossible header is refused rather than
    // trusted.
    let declared = declared_version.try_into().map_or(0, u32::from_le_bytes);
    if declared != GRAPH_CACHE_VERSION {
        tracing::warn!(
            cached_version = declared,
            expected_version = GRAPH_CACHE_VERSION,
            "Graph cache format upgraded, rebuilding (one-time cost after version bump)"
        );
        return Err(CacheRejection::VersionMismatch);
    }
    Ok(payload)
}

/// Write `.fallow/.gitignore` (`*\n`) so the cache directory is never committed.
fn write_cache_gitignore(cache_dir: &Path) -> std::io::Result<()> {
    std::fs::write(cache_dir.join(".gitignore"), "*\n")
}

/// Replace `cache_file` with `data` through a unique temp file. The extraction
/// cache store uses the same helper, so concurrent fallow processes that share
/// `.fallow/` never interleave a write to either cache.
fn atomic_write(cache_file: &Path, data: &[u8]) -> std::io::Result<()> {
    fallow_types::atomic_file::write_atomic(cache_file, data)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Unframed data may come from a release predating the header. Its origin
    /// is unknown, so the error does not establish corruption.
    #[test]
    fn a_blob_without_fallows_framing_is_undecodable() {
        assert_eq!(
            read_header(b"written-by-an-older-build").err(),
            Some(CacheRejection::Undecodable)
        );
    }

    #[test]
    fn a_blob_too_short_to_carry_a_header_is_undecodable() {
        assert_eq!(
            read_header(&[0_u8; 3]).err(),
            Some(CacheRejection::Undecodable)
        );
    }

    #[test]
    fn a_header_declaring_another_version_is_refused_without_reading_the_payload() {
        let blob = framed(GRAPH_CACHE_VERSION + 1, b"payload");

        assert_eq!(
            read_header(&blob).err(),
            Some(CacheRejection::VersionMismatch)
        );
    }

    #[test]
    fn a_header_at_the_current_version_hands_back_the_payload_it_frames() {
        let blob = framed(GRAPH_CACHE_VERSION, b"payload");

        assert_eq!(read_header(&blob), Ok(b"payload".as_slice()));
    }
}
