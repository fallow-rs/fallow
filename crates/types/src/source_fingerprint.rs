//! Shared source-file fingerprint inputs for cache invalidation.

use std::fs::Metadata;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

/// File metadata used to decide whether a source-derived cache entry is fresh.
///
/// This is intentionally metadata-only. Callers that need content validation
/// can combine it with their existing content hash, while cheap caches can use
/// the same freshness shape without inventing their own `(mtime, size)` tuple.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SourceFingerprint {
    /// Source file modification time as nanoseconds since the Unix epoch.
    ///
    /// A value of `0` means the timestamp could not be read. Fast metadata-only
    /// cache hits should treat that as unknown and miss conservatively.
    pub mtime_ns: u64,
    /// Source file size in bytes.
    pub file_size: u64,
}

impl SourceFingerprint {
    /// Build a fingerprint from explicit metadata parts.
    #[must_use]
    pub const fn new(mtime_ns: u64, file_size: u64) -> Self {
        Self {
            mtime_ns,
            file_size,
        }
    }

    /// Build a fingerprint from filesystem metadata.
    #[must_use]
    pub fn from_metadata(metadata: &Metadata) -> Self {
        Self {
            mtime_ns: metadata_mtime_ns(metadata),
            file_size: metadata.len(),
        }
    }

    /// Returns true when the modification time is known.
    #[must_use]
    pub const fn has_known_mtime(self) -> bool {
        self.mtime_ns > 0
    }
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "filesystem mtimes used for cache invalidation fit in u64 nanoseconds for supported dates"
)]
fn metadata_mtime_ns(metadata: &Metadata) -> u64 {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map_or(0, |duration| duration.as_nanos() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_fingerprint_preserves_explicit_parts() {
        let fingerprint = SourceFingerprint::new(123, 456);
        assert_eq!(fingerprint.mtime_ns, 123);
        assert_eq!(fingerprint.file_size, 456);
        assert!(fingerprint.has_known_mtime());
    }

    #[test]
    fn source_fingerprint_zero_mtime_is_unknown() {
        let fingerprint = SourceFingerprint::new(0, 456);
        assert!(!fingerprint.has_known_mtime());
    }

    #[test]
    #[cfg_attr(miri, ignore = "filesystem metadata is blocked by Miri isolation")]
    fn source_fingerprint_from_metadata_sets_size() {
        let metadata = std::fs::metadata(".").expect("metadata");

        let fingerprint = SourceFingerprint::from_metadata(&metadata);

        assert_eq!(fingerprint.file_size, metadata.len());
    }
}
