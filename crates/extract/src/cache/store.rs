//! Cache store: load, save, and query cached module data.

use std::path::Path;

#[cfg(test)]
use std::cell::Cell;

use fallow_types::cache_rejection::CacheRejection;
use fallow_types::path_guard::CacheTarget;
use rustc_hash::FxHashMap;

use bitcode::{Decode, Encode};

use super::conversion::module_to_cached;
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
    /// multi-megabyte blob and then refused it costs the same as a cold run,
    /// and without a named reason it is indistinguishable from having no cache
    /// at all. Callers carry
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
    /// A symlink that leaves the base of `target` stops the save before any
    /// write.
    pub fn save(
        &mut self,
        target: CacheTarget<'_>,
        config_hash: u64,
        max_size_bytes: usize,
    ) -> Result<(), String> {
        target
            .write_gitignore()
            .map_err(|e| format!("Failed to write cache .gitignore: {e}"))?;

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
            // The working set of the latest run may fill the whole cap.
            let working_set = max_size_bytes.saturating_sub(CACHE_HEADER_LEN);
            encoded = self.evict_lru_to_target(target, working_set, encoded);
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

        target
            .write_file("cache.bin", &framed(self.version, &encoded))
            .map_err(|e| format!("Failed to write cache file: {e}"))
    }

    /// Evict the entries that runs saw least recently until the re-encoded
    /// size is under `target_bytes` or only one entry remains.
    ///
    /// The entries with the newest seen stamp are the working set of the
    /// latest run, and they all share that one stamp. Eviction removes older
    /// entries down to `target_bytes` first. It removes working-set entries
    /// only down to `working_set_bytes`, which is the cap itself. The low
    /// target gives headroom for entries that other scopes add. The working
    /// set must not go down to the low target: a working set a little over
    /// the trigger then loses about 40 percent of its entries on every save,
    /// and the next run parses those files again. The tie order of the keys
    /// is stable, so the same entries go on each save and the rest stay hits.
    fn evict_lru_to_target(
        &mut self,
        target_bytes: usize,
        working_set_bytes: usize,
        encoded: Vec<u8>,
    ) -> Vec<u8> {
        let mut order: Vec<(u64, String, usize)> = self
            .entries
            .iter()
            .map(|(key, entry)| {
                (
                    entry.last_seen_secs,
                    key.clone(),
                    bitcode::encode(entry)
                        .len()
                        .saturating_add(key.len())
                        .max(1),
                )
            })
            .collect();
        order.sort();
        let newest = order.last().map_or(0, |(seen, _, _)| *seen);
        let working_set_from = order.partition_point(|(seen, _, _)| *seen < newest);

        let mut cursor = EvictionCursor {
            idx: 0,
            estimated_remaining: order
                .iter()
                .map(|(_, _, estimated_bytes)| estimated_bytes)
                .sum(),
        };
        let mut encoded = encoded;
        if working_set_from > 0 {
            encoded = self.evict_order_range(
                &order,
                &mut cursor,
                working_set_from,
                target_bytes,
                encoded,
            );
        }
        let working_set_target = target_bytes.max(working_set_bytes);
        if encoded.len() > working_set_target {
            let keep_newest_from = order.len().saturating_sub(1);
            encoded = self.evict_order_range(
                &order,
                &mut cursor,
                keep_newest_from,
                working_set_target,
                encoded,
            );
        }

        if encoded.len() > working_set_target && self.entries.len() == 1 {
            tracing::warn!(
                encoded_kb = encoded.len() / 1024,
                target_kb = working_set_target / 1024,
                "Single cache entry exceeds configured max; cache will overshoot the cap"
            );
        }
        encoded
    }

    /// Remove entries in `order` from `cursor.idx` up to (not including)
    /// `end` until the re-encoded size is under `target_bytes`.
    fn evict_order_range(
        &mut self,
        order: &[(u64, String, usize)],
        cursor: &mut EvictionCursor,
        end: usize,
        target_bytes: usize,
        mut encoded: Vec<u8>,
    ) -> Vec<u8> {
        const MAX_REFINEMENT_PASSES: usize = 2;
        const ESTIMATE_SAFETY_BPS: usize = 9_800;
        for _ in 0..MAX_REFINEMENT_PASSES {
            if encoded.len() <= target_bytes || cursor.idx >= end {
                return encoded;
            }

            let estimated_budget = estimated_eviction_budget(
                cursor.estimated_remaining,
                target_bytes,
                encoded.len(),
                ESTIMATE_SAFETY_BPS,
            );
            let start_idx = cursor.idx;
            while cursor.idx < end && cursor.estimated_remaining > estimated_budget {
                self.evict_at(order, cursor);
            }
            if cursor.idx == start_idx {
                self.evict_at(order, cursor);
            }
            encoded = self.encode();
        }

        if encoded.len() > target_bytes && cursor.idx < end {
            let conservative_budget = target_bytes / 2;
            while cursor.idx < end && cursor.estimated_remaining > conservative_budget {
                self.evict_at(order, cursor);
            }
            encoded = self.encode();
        }

        // Per-entry encodings are deliberately conservative, but keep the
        // configured cap exact if a future bitcode layout violates that
        // estimate. This safety path runs only after byte-aware retention had
        // three opportunities to preserve a recent suffix.
        if encoded.len() > target_bytes && cursor.idx < end {
            while cursor.idx < end {
                self.evict_at(order, cursor);
            }
            encoded = self.encode();
        }
        encoded
    }

    fn evict_at(&mut self, order: &[(u64, String, usize)], cursor: &mut EvictionCursor) {
        let (_, key, estimated_bytes) = &order[cursor.idx];
        self.entries.remove(key);
        cursor.estimated_remaining = cursor.estimated_remaining.saturating_sub(*estimated_bytes);
        cursor.idx += 1;
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

    /// Replace the source fingerprint of the entry for `path`, and keep the
    /// rest of the entry.
    ///
    /// A fresh checkout or a restored cache moves the file timestamps but not
    /// the content. Rebuilding such an entry from the module copies every
    /// extracted fact again, so a caller whose content hash still matches
    /// refreshes only the metadata. Returns `false` when no entry exists.
    pub fn refresh_fingerprint(
        &mut self,
        path: &Path,
        fingerprint: fallow_types::source_fingerprint::SourceFingerprint,
    ) -> bool {
        let key = self.key_for(path);
        let Some(entry) = self.entries.get_mut(&key) else {
            return false;
        };
        entry.mtime_ns = fingerprint.mtime_ns;
        entry.ctime_ns = fingerprint.ctime_ns;
        entry.file_size = fingerprint.file_size;
        true
    }

    /// Store the module of this run for `path`. Returns `true` when the store
    /// changed.
    ///
    /// An entry whose content hash still matches is rewritten only when its
    /// metadata fingerprint moved or when this run adds complexity or call
    /// facts that the entry lacks. A rewrite keeps the complexity and the call
    /// facts that an earlier run stored and this run did not extract: a
    /// module of such a run has none of them by design, so a copy would strip
    /// the entry and make each later run that reads them parse the file again.
    pub fn record_module(
        &mut self,
        path: &Path,
        module: &crate::ModuleInfo,
        fingerprint: fallow_types::source_fingerprint::SourceFingerprint,
        need_complexity: bool,
        need_call_facts: bool,
    ) -> bool {
        let Some(cached) = self
            .get_by_path_only(path)
            .filter(|cached| cached.content_hash == module.content_hash)
        else {
            self.insert(
                path,
                module_to_cached(module, fingerprint, need_complexity, need_call_facts),
            );
            return true;
        };
        let adds_complexity = need_complexity && !cached.complexity_extracted;
        let adds_call_facts = need_call_facts && cached.call_facts.is_none();
        if !adds_complexity && !adds_call_facts {
            return cached.source_fingerprint() != fingerprint
                && self.refresh_fingerprint(path, fingerprint);
        }
        let mut refreshed = module_to_cached(module, fingerprint, need_complexity, need_call_facts);
        if !need_complexity && cached.complexity_extracted {
            refreshed.complexity.clone_from(&cached.complexity);
            refreshed.complexity_extracted = true;
        }
        if !need_call_facts {
            refreshed.call_facts.clone_from(&cached.call_facts);
        }
        self.insert(path, refreshed);
        true
    }

    /// Look up a cached module by path only (ignoring hash).
    #[must_use]
    pub fn get_by_path_only(&self, path: &Path) -> Option<&CachedModule> {
        self.entries.get(&self.key_for(path))
    }

    /// Remove cache entries for files that no longer exist on disk, and mark
    /// the entries for every discovered file as seen by this run.
    ///
    /// Returns `true` when any entry was removed. The seen stamp alone does
    /// not make the store dirty: a run that saves nothing else does not
    /// rewrite the whole blob to persist it, and the next run that saves
    /// stamps the same files again.
    ///
    /// The seen stamp orders eviction. A file that every run discovers keeps a
    /// current stamp even when it never changes, so `evict_lru_to_target`
    /// removes the entries that only another scope reads before it removes
    /// the files that this scope reads on every run.
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
        let now = super::conversion::current_unix_seconds();
        for key in &current_keys {
            if let Some(entry) = self.entries.get_mut(key) {
                entry.last_seen_secs = now;
            }
        }
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

/// Position in the sorted eviction order, and the estimated size of the
/// entries that remain.
struct EvictionCursor {
    idx: usize,
    estimated_remaining: usize,
}

/// Marker written ahead of every cache payload so the format version can be
/// read without decoding the payload it describes.
///
/// Constant across format bumps: only the version field beside it moves. That
/// lets future upgrades report an explicit version mismatch; older unframed
/// caches still report an ambiguous decode failure.
const CACHE_MAGIC: [u8; 4] = *b"FLWX";

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

impl Default for CacheStore {
    fn default() -> Self {
        Self::new(Path::new(""))
    }
}
