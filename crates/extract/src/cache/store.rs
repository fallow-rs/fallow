//! Cache store: load, save, and query cached module data.

use std::path::Path;

#[cfg(test)]
use std::cell::Cell;

use fallow_types::cache_rejection::CacheRejection;
use rustc_hash::FxHashMap;

use bitcode::{Decode, Encode};

use super::types::{
    CACHE_VERSION, CachedModule, DEFAULT_CACHE_MAX_SIZE, EVICTION_SIGNIFICANT_BPS,
    EVICTION_TARGET_BPS, EVICTION_TRIGGER_BPS,
};

#[cfg(test)]
thread_local! {
    static FULL_STORE_ENCODE_COUNT: Cell<usize> = const { Cell::new(0) };
}

/// Cached module information stored on disk.
///
/// Entries are keyed on the ROOT-RELATIVE, forward-slash-normalised path, and
/// the root is recorded once in the header. Absolute keys made the blob
/// unusable anywhere but the directory that wrote it: a container job, a
/// matrix over roots, a GitLab shell executor, or a plain `cp -Rp` to a
/// sibling path paid the full decode of a multi-megabyte file and then missed
/// every single lookup, with nothing on stderr to say so.
#[derive(Debug, Encode, Decode)]
pub struct CacheStore {
    version: u32,
    /// Stable hash of extraction-affecting config fields.
    config_hash: u64,
    /// Project root the entries are relative to, forward-slash normalised and
    /// without a trailing separator. Informational after load: the loader
    /// re-anchors to the CURRENT root, because a blob restored under another
    /// path is exactly the case root-relative keys exist to serve.
    root: String,
    /// Map from root-relative file path to cached module data.
    entries: FxHashMap<String, CachedModule>,
}

impl CacheStore {
    /// Create a new empty cache anchored at `root`.
    #[must_use]
    pub fn new(root: &Path) -> Self {
        Self {
            version: CACHE_VERSION,
            config_hash: 0,
            root: normalise_root(root),
            entries: FxHashMap::default(),
        }
    }

    /// Load cache from disk.
    ///
    /// # Errors
    ///
    /// Returns the [`CacheRejection`] that decided against reuse. Every branch
    /// names itself instead of collapsing into a bare miss: a run that read a
    /// multi-megabyte blob and then refused it costs the same as a cold run
    /// but used to be indistinguishable from having no cache at all, and the
    /// config-hash branch in particular said nothing whatsoever. Callers carry
    /// the reason into the perf table and `fallow doctor`.
    ///
    /// The version is read from the file header BEFORE the payload is
    /// decoded, because the two are decided by different things. A format bump
    /// changes the encoded shape, so decoding a blob from the previous release
    /// fails outright and never reaches a version comparison made afterwards:
    /// the most ordinary event there is (upgrading fallow) then reported
    /// "cache file could not be decoded", which reads as corruption and sent
    /// people looking for a damaged disk. With the version in front, an upgrade
    /// says the format changed. The framing is checked separately from the
    /// version it carries, so an unframed or unreadable payload reports `Undecodable`
    /// rather than borrowing the upgrade message.
    ///
    /// Every branch that refuses a file that DID exist logs at warn, because
    /// the user paid the read and got nothing back. Only the missing-file case
    /// stays quiet.
    pub fn load(
        cache_dir: &Path,
        root: &Path,
        expected_config_hash: u64,
        max_size_bytes: usize,
    ) -> Result<Self, CacheRejection> {
        Self::load_counting_bytes(cache_dir, root, expected_config_hash, max_size_bytes).0
    }

    /// [`Self::load`], plus the number of cache bytes read from disk.
    ///
    /// The count is returned also when the cache is refused, because the run
    /// paid for the read either way. It is zero when no file was read.
    pub fn load_counting_bytes(
        cache_dir: &Path,
        root: &Path,
        expected_config_hash: u64,
        max_size_bytes: usize,
    ) -> (Result<Self, CacheRejection>, u64) {
        let cache_file = cache_dir.join("cache.bin");
        let data = match std::fs::read(&cache_file) {
            Ok(data) => data,
            Err(error) => {
                if error.kind() != std::io::ErrorKind::NotFound {
                    tracing::warn!("Cache file could not be read; check the path and permissions");
                    return (Err(CacheRejection::Unreadable), 0);
                }
                return (Err(CacheRejection::Absent), 0);
            }
        };
        let bytes_read = data.len() as u64;
        (
            Self::decode_loaded(&data, root, expected_config_hash, max_size_bytes),
            bytes_read,
        )
    }

    fn decode_loaded(
        data: &[u8],
        root: &Path,
        expected_config_hash: u64,
        max_size_bytes: usize,
    ) -> Result<Self, CacheRejection> {
        let safety_ceiling = max_size_bytes.max(DEFAULT_CACHE_MAX_SIZE);
        if data.len() > safety_ceiling {
            tracing::warn!(
                size_mb = data.len() / (1024 * 1024),
                ceiling_mb = safety_ceiling / (1024 * 1024),
                "Cache file exceeds safety ceiling, ignoring"
            );
            return Err(CacheRejection::Oversize {
                size_bytes: data.len() as u64,
                ceiling_bytes: safety_ceiling as u64,
            });
        }
        let payload = read_header(data)?;
        let mut store: Self = match bitcode::decode(payload) {
            Ok(s) => s,
            Err(_) => {
                tracing::warn!(
                    "Cache file carries the current format version but its payload could not be \
                     decoded, rebuilding"
                );
                return Err(CacheRejection::Undecodable);
            }
        };
        // The header already agreed with `CACHE_VERSION`, so this catches only a
        // file whose header and payload disagree: a spliced or hand-edited blob.
        if store.version != CACHE_VERSION {
            tracing::warn!(
                cached_version = store.version,
                expected_version = CACHE_VERSION,
                "Cache header and payload declare different format versions, rebuilding"
            );
            return Err(CacheRejection::VersionMismatch);
        }
        if store.config_hash != expected_config_hash {
            tracing::warn!(
                "Cache was built under different extraction config, rebuilding from cold"
            );
            return Err(CacheRejection::ConfigHashMismatch);
        }
        let current_root = normalise_root(root);
        if store.root != current_root {
            tracing::debug!(
                cached_root = %store.root,
                "Reusing a cache written under a different project root"
            );
            store.root = current_root;
        }
        Ok(store)
    }

    /// Save cache to disk with write-time size enforcement and atomic rename.
    pub fn save(
        &mut self,
        cache_dir: &Path,
        config_hash: u64,
        max_size_bytes: usize,
    ) -> Result<(), String> {
        std::fs::create_dir_all(cache_dir)
            .map_err(|e| format!("Failed to create cache dir: {e}"))?;
        write_cache_gitignore(cache_dir)?;

        self.config_hash = config_hash;
        let initial_entries = self.entries.len();
        let mut encoded = self.encode();

        let trigger = (max_size_bytes / 10_000).saturating_mul(EVICTION_TRIGGER_BPS);
        if encoded.len().saturating_add(CACHE_HEADER_LEN) > trigger {
            // The cap is a promise about the file, and the file carries the
            // header as well as the payload, so eviction aims below both.
            let target = (max_size_bytes / 10_000)
                .saturating_mul(EVICTION_TARGET_BPS)
                .saturating_sub(CACHE_HEADER_LEN);
            encoded = self.evict_lru_to_target(target, encoded);
            let evicted = initial_entries.saturating_sub(self.entries.len());
            let final_size = encoded.len();
            let significant_evicted =
                initial_entries.saturating_mul(EVICTION_SIGNIFICANT_BPS) / 10_000;
            if evicted >= significant_evicted && initial_entries > 0 {
                tracing::info!(
                    evicted_entries = evicted,
                    remaining_entries = self.entries.len(),
                    final_size_kb = final_size / 1024,
                    max_size_kb = max_size_bytes / 1024,
                    "Cache eviction: removed oldest entries to stay under cap"
                );
            } else {
                tracing::debug!(
                    evicted_entries = evicted,
                    remaining_entries = self.entries.len(),
                    final_size_kb = final_size / 1024,
                    max_size_kb = max_size_bytes / 1024,
                    "Cache eviction"
                );
            }
        }

        let cache_file = cache_dir.join("cache.bin");
        atomic_write(&cache_file, &framed(self.version, &encoded))?;
        Ok(())
    }

    /// Evict LRU entries until the re-encoded size is under `target_bytes`
    /// or only one entry remains.
    fn evict_lru_to_target(&mut self, target_bytes: usize, mut encoded: Vec<u8>) -> Vec<u8> {
        let mut order: Vec<(u64, String, usize)> = self
            .entries
            .iter()
            .map(|(key, entry)| {
                (
                    entry.last_access_secs,
                    key.clone(),
                    bitcode::encode(entry)
                        .len()
                        .saturating_add(key.len())
                        .max(1),
                )
            })
            .collect();
        order.sort();

        const MAX_REFINEMENT_PASSES: usize = 2;
        const ESTIMATE_SAFETY_BPS: usize = 9_800;
        let mut idx = 0;
        let mut estimated_remaining: usize = order
            .iter()
            .map(|(_, _, estimated_bytes)| estimated_bytes)
            .sum();
        for _ in 0..MAX_REFINEMENT_PASSES {
            if encoded.len() <= target_bytes || self.entries.len() <= 1 {
                break;
            }

            let estimated_budget = estimated_eviction_budget(
                estimated_remaining,
                target_bytes,
                encoded.len(),
                ESTIMATE_SAFETY_BPS,
            );
            let start_idx = idx;
            while idx + 1 < order.len() && estimated_remaining > estimated_budget {
                let (_, key, estimated_bytes) = &order[idx];
                self.entries.remove(key);
                estimated_remaining = estimated_remaining.saturating_sub(*estimated_bytes);
                idx += 1;
            }
            if idx == start_idx && idx + 1 < order.len() {
                let (_, key, estimated_bytes) = &order[idx];
                self.entries.remove(key);
                estimated_remaining = estimated_remaining.saturating_sub(*estimated_bytes);
                idx += 1;
            }
            encoded = self.encode();
        }

        if encoded.len() > target_bytes && self.entries.len() > 1 {
            let conservative_budget = target_bytes / 2;
            while idx + 1 < order.len() && estimated_remaining > conservative_budget {
                let (_, key, estimated_bytes) = &order[idx];
                self.entries.remove(key);
                estimated_remaining = estimated_remaining.saturating_sub(*estimated_bytes);
                idx += 1;
            }
            encoded = self.encode();
        }

        // Per-entry encodings are deliberately conservative, but keep the
        // configured cap exact if a future bitcode layout violates that
        // estimate. This safety path runs only after byte-aware retention had
        // three opportunities to preserve a recent suffix.
        if encoded.len() > target_bytes && self.entries.len() > 1 {
            let keep_newest_from = order.len().saturating_sub(1);
            for (_, key, _) in &order[idx..keep_newest_from] {
                self.entries.remove(key);
            }
            encoded = self.encode();
        }

        if encoded.len() > target_bytes && self.entries.len() == 1 {
            tracing::warn!(
                encoded_kb = encoded.len() / 1024,
                target_kb = target_bytes / 1024,
                "Single cache entry exceeds configured max; cache will overshoot the cap"
            );
        }
        encoded
    }

    fn encode(&self) -> Vec<u8> {
        #[cfg(test)]
        FULL_STORE_ENCODE_COUNT.with(|count| count.set(count.get() + 1));
        bitcode::encode(self)
    }

    #[cfg(test)]
    pub(super) fn reset_full_store_encode_count() {
        FULL_STORE_ENCODE_COUNT.with(|count| count.set(0));
    }

    #[cfg(test)]
    pub(super) fn full_store_encode_count() -> usize {
        FULL_STORE_ENCODE_COUNT.with(Cell::get)
    }

    /// Key `path` the way entries are stored: root-relative where possible,
    /// forward-slash normalised.
    ///
    /// A path outside the root keeps its own normalised spelling. It is still
    /// stable within one root, which is all a lookup needs, and no root-
    /// relative spelling of it exists to prefer.
    fn key_for(&self, path: &Path) -> String {
        let text = path.to_string_lossy().replace('\\', "/");
        if self.root.is_empty() {
            return text;
        }
        match text
            .strip_prefix(&self.root)
            .and_then(|rest| rest.strip_prefix('/'))
        {
            Some(relative) => relative.to_owned(),
            None => text,
        }
    }

    /// Rebuild the absolute path an entry key refers to under the current root.
    fn path_for_key(&self, key: &str) -> std::path::PathBuf {
        if self.root.is_empty() {
            return std::path::PathBuf::from(key);
        }
        let candidate = Path::new(key);
        if candidate.is_absolute() {
            return candidate.to_path_buf();
        }
        Path::new(&self.root).join(key)
    }

    /// Look up a cached module by path and content hash.
    /// Returns None if not cached or hash mismatch.
    #[must_use]
    pub fn get(&self, path: &Path, content_hash: u64) -> Option<&CachedModule> {
        let entry = self.entries.get(&self.key_for(path))?;
        if entry.content_hash == content_hash {
            Some(entry)
        } else {
            None
        }
    }

    /// Insert or update a cached module.
    pub fn insert(&mut self, path: &Path, module: CachedModule) {
        let key = self.key_for(path);
        self.entries.insert(key, module);
    }

    /// Look up a cached module by path only (ignoring hash).
    #[must_use]
    pub fn get_by_path_only(&self, path: &Path) -> Option<&CachedModule> {
        self.entries.get(&self.key_for(path))
    }

    /// Remove cache entries for files that no longer exist on disk.
    ///
    /// Returns `true` when any entry was removed.
    ///
    /// The predicate is deliberately "still exists", not "was discovered by
    /// this run". Discovery is scoped: `--production` drops test and story
    /// files, `--root` narrows to a subtree, and `ignorePatterns` differs per
    /// command. Evicting whatever the current scope did not walk meant one
    /// `--production` run threw away the entries for every test file, and the
    /// next full run reparsed them from cold. Entries are keyed by absolute
    /// path, so the check is one `symlink_metadata` per undiscovered entry
    /// (`symlink_metadata`, not `metadata`, so a broken symlink still counts as
    /// present rather than being evicted as missing). Size is not this
    /// method's concern: `evict_lru_to_target` remains the only guard on how
    /// large the blob may grow.
    pub fn retain_paths(&mut self, files: &[fallow_types::discover::DiscoveredFile]) -> bool {
        use rustc_hash::FxHashSet;
        let current_keys: FxHashSet<String> = files.iter().map(|f| self.key_for(&f.path)).collect();
        let before = self.entries.len();
        let retained: FxHashSet<String> = self
            .entries
            .keys()
            .filter(|key| {
                current_keys.contains(*key)
                    || std::fs::symlink_metadata(self.path_for_key(key)).is_ok()
            })
            .cloned()
            .collect();
        self.entries.retain(|key, _| retained.contains(key));
        self.entries.len() != before
    }

    /// Number of cached entries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the cache is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Marker written ahead of every cache payload so the format version can be
/// read without decoding the payload it describes.
///
/// Constant across format bumps: only the version field beside it moves. That
/// lets future upgrades report an explicit version mismatch; older unframed
/// caches still report an ambiguous decode failure.
pub(super) const CACHE_MAGIC: [u8; 4] = *b"FLWX";

/// Bytes the framing adds ahead of the payload: the magic plus a little-endian
/// `u32` format version.
pub(super) const CACHE_HEADER_LEN: usize = CACHE_MAGIC.len() + 4;

/// Prepend the format header to an encoded payload.
///
/// The version comes from the store being written rather than from the
/// constant, so the header always describes the payload behind it.
pub(super) fn framed(version: u32, payload: &[u8]) -> Vec<u8> {
    let mut framed = Vec::with_capacity(CACHE_HEADER_LEN + payload.len());
    framed.extend_from_slice(&CACHE_MAGIC);
    framed.extend_from_slice(&version.to_le_bytes());
    framed.extend_from_slice(payload);
    framed
}

/// Split a cache file into its declared version and its payload, refusing
/// anything this binary cannot read WITHOUT decoding it first.
///
/// The version check has to come first: a format bump changes the encoded
/// shape, so a blob from the previous release fails to decode and a version
/// comparison made after the decode is unreachable on the one event that
/// triggers it most, an upgrade.
///
/// A recognized header exposes a version mismatch without decoding. Releases
/// before framing wrote raw payloads, so a missing header cannot distinguish
/// an older cache from foreign or damaged data. `Undecodable` keeps that
/// uncertainty explicit and the next successful run replaces the blob.
fn read_header(data: &[u8]) -> Result<&[u8], CacheRejection> {
    let Some((header, payload)) = data.split_at_checked(CACHE_HEADER_LEN) else {
        tracing::warn!("Cache file is too short to carry a format header, rebuilding");
        return Err(CacheRejection::Undecodable);
    };
    let (declared_magic, declared_version) = header.split_at(CACHE_MAGIC.len());
    if declared_magic != CACHE_MAGIC {
        tracing::warn!("Cache file does not carry fallow's cache framing, rebuilding");
        return Err(CacheRejection::Undecodable);
    }
    // The slice is exactly four bytes; the fallback only has to be a version
    // this binary never writes, so an impossible header is refused rather than
    // trusted.
    let declared = declared_version.try_into().map_or(0, u32::from_le_bytes);
    if declared != CACHE_VERSION {
        tracing::warn!(
            cached_version = declared,
            expected_version = CACHE_VERSION,
            "Cache format upgraded, rebuilding (one-time cost after version bump)"
        );
        return Err(CacheRejection::VersionMismatch);
    }
    Ok(payload)
}

pub(super) fn estimated_eviction_budget(
    estimated_remaining: usize,
    target_bytes: usize,
    encoded_bytes: usize,
    safety_bps: usize,
) -> usize {
    if encoded_bytes == 0 {
        return 0;
    }

    const BASIS_POINTS: u128 = 10_000;
    let scaled = estimated_remaining as u128 * target_bytes as u128 / encoded_bytes as u128;
    let safety = (safety_bps as u128).min(BASIS_POINTS);
    let budget = scaled / BASIS_POINTS * safety + scaled % BASIS_POINTS * safety / BASIS_POINTS;
    budget.min(usize::MAX as u128) as usize
}

/// Normalise a project root for storage and prefix stripping: forward slashes,
/// no trailing separator. An empty root disables stripping, which is what a
/// default-constructed store gets.
fn normalise_root(root: &Path) -> String {
    let text = root.to_string_lossy().replace('\\', "/");
    match text.strip_suffix('/') {
        Some(trimmed) => trimmed.to_owned(),
        None => text,
    }
}

fn write_cache_gitignore(cache_dir: &Path) -> Result<(), String> {
    std::fs::write(cache_dir.join(".gitignore"), "*\n")
        .map_err(|e| format!("Failed to write cache .gitignore: {e}"))
}

/// Write `data` atomically via a sibling `.tmp` file, best-effort fsync, then rename.
fn atomic_write(cache_file: &Path, data: &[u8]) -> Result<(), String> {
    let tmp_file = match cache_file.file_name() {
        Some(name) => cache_file.with_file_name({
            let mut s = name.to_os_string();
            s.push(".tmp");
            s
        }),
        None => return Err("Cache file path has no filename component".to_owned()),
    };

    {
        use std::io::Write as _;
        let mut f = std::fs::File::create(&tmp_file)
            .map_err(|e| format!("Failed to create cache tmp: {e}"))?;
        f.write_all(data)
            .map_err(|e| format!("Failed to write cache tmp: {e}"))?;
        let _ = f.sync_all();
    }

    std::fs::rename(&tmp_file, cache_file)
        .map_err(|e| format!("Failed to rename cache tmp into place: {e}"))?;
    Ok(())
}

impl Default for CacheStore {
    fn default() -> Self {
        Self::new(Path::new(""))
    }
}
