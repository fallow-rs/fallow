//! Shared source-file fingerprint inputs for cache invalidation.

use std::fs::Metadata;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

/// File metadata recorded alongside source-derived cache entries.
///
/// These values are useful for cache accounting and diagnostics. They do not
/// prove that the source content is unchanged, so cache hits must validate the
/// content separately.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SourceFingerprint {
    /// Source file modification time as nanoseconds since the Unix epoch.
    ///
    /// A value of `0` means the timestamp could not be read.
    pub mtime_ns: u64,
    /// Source file inode change time (ctime) as nanoseconds since the Unix
    /// epoch, or `0` when the platform does not expose it.
    ///
    /// Only Unix reports it (`stat.st_ctime`). Other platforms use `0`. Like
    /// modification time, ctime is metadata and does not replace a content
    /// check when deciding whether parsed source is current.
    pub ctime_ns: u64,
    /// Source file size in bytes.
    pub file_size: u64,
}

impl SourceFingerprint {
    /// Build a fingerprint from explicit metadata parts, with no known ctime.
    #[must_use]
    pub const fn new(mtime_ns: u64, file_size: u64) -> Self {
        Self {
            mtime_ns,
            ctime_ns: 0,
            file_size,
        }
    }

    /// Build a fingerprint from explicit metadata parts, including ctime.
    #[must_use]
    pub const fn with_ctime(mtime_ns: u64, ctime_ns: u64, file_size: u64) -> Self {
        Self {
            mtime_ns,
            ctime_ns,
            file_size,
        }
    }

    /// Build a fingerprint from filesystem metadata.
    #[must_use]
    pub fn from_metadata(metadata: &Metadata) -> Self {
        Self {
            mtime_ns: metadata_mtime_ns(metadata),
            ctime_ns: metadata_ctime_ns(metadata),
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

/// Unix inode change time in nanoseconds since the epoch.
///
/// Pre-epoch and unreadable values collapse to `0`.
#[cfg(unix)]
fn metadata_ctime_ns(metadata: &Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;

    let seconds = u64::try_from(metadata.ctime()).unwrap_or(0);
    let nanos = u64::try_from(metadata.ctime_nsec()).unwrap_or(0);
    seconds.saturating_mul(1_000_000_000).saturating_add(nanos)
}

/// Non-Unix platforms do not expose an inode change time.
#[cfg(not(unix))]
fn metadata_ctime_ns(_metadata: &Metadata) -> u64 {
    0
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
    fn source_fingerprint_differs_when_only_ctime_moved() {
        let before = SourceFingerprint::with_ctime(123, 700, 456);
        let after = SourceFingerprint::with_ctime(123, 800, 456);
        assert_ne!(before, after);
    }

    #[test]
    #[cfg_attr(miri, ignore = "filesystem metadata is blocked by Miri isolation")]
    fn source_fingerprint_from_metadata_sets_size() {
        let metadata = std::fs::metadata(".").expect("metadata");

        let fingerprint = SourceFingerprint::from_metadata(&metadata);

        assert_eq!(fingerprint.file_size, metadata.len());
    }

    #[cfg(unix)]
    #[test]
    #[cfg_attr(miri, ignore = "filesystem metadata is blocked by Miri isolation")]
    fn source_fingerprint_from_metadata_reads_unix_ctime() {
        let metadata = std::fs::metadata(".").expect("metadata");

        let fingerprint = SourceFingerprint::from_metadata(&metadata);

        assert!(fingerprint.ctime_ns > 0);
    }
}
