//! Temp-dir scans of the cache sweep: the enumeration of cache entries, and
//! the marker file that limits the per-audit scans to one per interval.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use rustc_hash::FxHashSet;

use super::{
    REUSABLE_LAST_USED_SUFFIX, REUSABLE_LOCK_SUFFIX, REUSABLE_SHA_SUFFIX,
    open_or_create_owned_sidecar, sidecar_metadata_is_trusted,
};

/// Minimum time between two temp-dir scans of the per-audit sweep.
///
/// The scans visit every `fallow-audit-base-cache-*` entry in the temp dir.
/// On a long-lived host the entries of deleted repos stay until they age out,
/// and tens of thousands of them make each audit pay seconds of IO. The
/// requested root's own entry is swept on every audit; the scans run at most
/// once per interval. `fallow audit-cache prune` always scans.
pub(super) const FULL_SWEEP_INTERVAL: Duration = Duration::from_hours(1);

/// Which candidates a cache sweep visits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SweepScope {
    /// Every pass, including the temp-dir scans for legacy SHA-suffixed and
    /// cross-repo entries.
    All,
    /// Only the passes that need no temp-dir scan: the requested root's own
    /// entry and the git-registered legacy entries of this repo.
    OwnedOnly,
}

/// Path of the marker whose mtime records the last full per-audit sweep of
/// `scan_root`. The name carries the effective uid, so users that share a
/// temp dir each keep their own interval.
pub fn full_sweep_marker_path(scan_root: &Path) -> PathBuf {
    #[cfg(unix)]
    let name = format!(
        "fallow-audit-cache-sweep-{}.stamp",
        rustix::process::geteuid().as_raw()
    );
    #[cfg(not(unix))]
    let name = String::from("fallow-audit-cache-sweep.stamp");
    scan_root.join(name)
}

/// Decide the scope of a per-audit sweep and claim the interval.
///
/// Returns [`SweepScope::OwnedOnly`] only when a trusted marker shows a full
/// sweep less than [`FULL_SWEEP_INTERVAL`] ago. Every other state (no marker,
/// an old marker, a marker from the future, a marker of another user) gives
/// [`SweepScope::All`] and stamps the marker before the sweep starts, so
/// concurrent audits do not all scan. A marker that cannot be written never
/// stops the sweep.
pub(super) fn claim_full_sweep(scan_root: &Path, now: SystemTime) -> SweepScope {
    let marker = full_sweep_marker_path(scan_root);
    let recent = std::fs::symlink_metadata(&marker)
        .ok()
        .filter(sidecar_metadata_is_trusted)
        .and_then(|metadata| metadata.modified().ok())
        .and_then(|stamped| now.duration_since(stamped).ok())
        .is_some_and(|elapsed| elapsed < FULL_SWEEP_INTERVAL);
    if recent {
        return SweepScope::OwnedOnly;
    }
    if let Err(err) = open_or_create_owned_sidecar(&marker).and_then(|file| file.set_modified(now))
    {
        tracing::debug!(
            path = %marker.display(),
            error = %err,
            "failed to stamp the audit cache sweep marker; the next audit scans again",
        );
    }
    SweepScope::All
}

/// Enumerate every reusable cache entry in the temp dir, regardless of repo
/// hash, for the cross-repo GC pass. Only names matching the two shapes
/// fallow has ever minted are accepted: `<repo16>-<sha16>` (legacy) and
/// `<repo16>-root-<root16>` (current). Sidecar entries fold back to their
/// owning cache path like the repo-scoped scan.
pub(super) fn scan_all_reusable_cache_paths(scan_root: &Path) -> Vec<PathBuf> {
    const GLOBAL_CACHE_PREFIX: &str = "fallow-audit-base-cache-";

    let Ok(entries) = std::fs::read_dir(scan_root) else {
        return Vec::new();
    };
    let mut seen: FxHashSet<PathBuf> = FxHashSet::default();
    let mut paths = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let cache_name = strip_cache_sidecar_suffix(name);
        let Some(hash_suffix) = cache_name.strip_prefix(GLOBAL_CACHE_PREFIX) else {
            continue;
        };
        if !cache_hash_suffix_is_valid(hash_suffix) {
            continue;
        }
        let path = scan_root.join(cache_name);
        if seen.insert(path.clone()) {
            paths.push(path);
        }
    }
    paths
}

fn cache_hash_suffix_is_valid(suffix: &str) -> bool {
    fn is_hex16(part: &str) -> bool {
        part.len() == 16 && part.bytes().all(|byte| byte.is_ascii_hexdigit())
    }
    if let Some((repo, root)) = suffix.split_once("-root-") {
        return is_hex16(repo) && is_hex16(root);
    }
    suffix
        .split_once('-')
        .is_some_and(|(repo, sha)| is_hex16(repo) && is_hex16(sha))
}

pub(super) fn scan_cache_paths_with_hex_suffix(prefix: &str, scan_root: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(scan_root) else {
        return Vec::new();
    };
    let mut seen: FxHashSet<PathBuf> = FxHashSet::default();
    let mut paths = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let cache_name = strip_cache_sidecar_suffix(name);
        let Some(hash_suffix) = cache_name.strip_prefix(prefix) else {
            continue;
        };
        if hash_suffix.len() != 16 || !hash_suffix.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            continue;
        }
        let path = scan_root.join(cache_name);
        if seen.insert(path.clone()) {
            paths.push(path);
        }
    }
    paths
}

/// Strip a known reusable-cache sidecar suffix so a sidecar entry maps to its
/// owning cache directory name. Cache dir names end in a hex SHA prefix and
/// never contain these suffixes, so the mapping is unambiguous.
fn strip_cache_sidecar_suffix(name: &str) -> &str {
    for suffix in [
        REUSABLE_LAST_USED_SUFFIX,
        REUSABLE_SHA_SUFFIX,
        REUSABLE_LOCK_SUFFIX,
    ] {
        if let Some(stripped) = name.strip_suffix(suffix) {
            return stripped;
        }
    }
    name
}
