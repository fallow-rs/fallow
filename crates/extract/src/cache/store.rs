//! Cache store: load, save, and query cached module data.

use std::path::Path;

use rustc_hash::FxHashMap;

use bitcode::{Decode, Encode};

use super::types::{
    CACHE_VERSION, CachedModule, DEFAULT_CACHE_MAX_SIZE, EVICTION_SIGNIFICANT_BPS,
    EVICTION_TARGET_BPS, EVICTION_TRIGGER_BPS,
};

/// Cached module information stored on disk.
#[derive(Debug, Encode, Decode)]
pub struct CacheStore {
    version: u32,
    /// Stable u64 hash of extraction-affecting config fields (currently the
    /// active external plugin names + inline framework definition names).
    /// A mismatch at load time discards the cache, matching how
    /// `CACHE_VERSION` works but invalidating on a user-driven config change
    /// rather than on a fallow upgrade. See ADR-009 for the ingredient list.
    config_hash: u64,
    /// Map from file path to cached module data.
    entries: FxHashMap<String, CachedModule>,
}

impl CacheStore {
    /// Create a new empty cache.
    #[must_use]
    pub fn new() -> Self {
        Self {
            version: CACHE_VERSION,
            config_hash: 0,
            entries: FxHashMap::default(),
        }
    }

    /// Load cache from disk.
    ///
    /// Returns `None` when:
    /// - the file does not exist or cannot be read,
    /// - the on-disk size exceeds the configured `max_size_bytes` (matches the
    ///   user's `cache.maxSizeMb` / `FALLOW_CACHE_MAX_SIZE` setting, with the
    ///   built-in `DEFAULT_CACHE_MAX_SIZE` as the lower bound so a misconfigured
    ///   tiny cap cannot push a still-valid larger cache into discard),
    /// - bitcode decoding fails or the decoded version differs from
    ///   `CACHE_VERSION` (emits a one-line `tracing::info!` so the user sees the
    ///   one-time rebuild cost; decode failure is the common case across
    ///   `CACHE_VERSION` bumps because the on-disk schema changes shape and
    ///   the new struct cannot deserialize the old bytes),
    /// - the on-disk `config_hash` differs from `expected_config_hash` (silent;
    ///   config changes are user-driven and routine, no log noise).
    #[must_use]
    pub fn load(
        cache_dir: &Path,
        expected_config_hash: u64,
        max_size_bytes: usize,
    ) -> Option<Self> {
        let cache_file = cache_dir.join("cache.bin");
        let data = std::fs::read(&cache_file).ok()?;
        // Honour both the user's cap AND the built-in default so a
        // misconfigured tiny cap (e.g. `FALLOW_CACHE_MAX_SIZE=1`) does NOT
        // throw away a valid existing cache on the load path; the user's
        // cap takes effect at the NEXT save via the eviction logic.
        let safety_ceiling = max_size_bytes.max(DEFAULT_CACHE_MAX_SIZE);
        if data.len() > safety_ceiling {
            tracing::warn!(
                size_mb = data.len() / (1024 * 1024),
                ceiling_mb = safety_ceiling / (1024 * 1024),
                "Cache file exceeds safety ceiling, ignoring"
            );
            return None;
        }
        let store: Self = match bitcode::decode(&data) {
            Ok(s) => s,
            Err(_) => {
                tracing::info!(
                    "Cache format upgraded, rebuilding (one-time cost after version bump)"
                );
                return None;
            }
        };
        if store.version != CACHE_VERSION {
            tracing::info!("Cache format upgraded, rebuilding (one-time cost after version bump)");
            return None;
        }
        if store.config_hash != expected_config_hash {
            return None;
        }
        Some(store)
    }

    /// Save cache to disk with write-time size enforcement and atomic rename.
    ///
    /// Algorithm:
    /// 1. Set `self.config_hash = config_hash`.
    /// 2. Encode once.
    /// 3. If the encoded size exceeds 80% of `max_size_bytes`, evict LRU
    ///    entries (oldest `last_access_secs` first, path-tiebroken) until
    ///    the size is below 60% of `max_size_bytes` OR only one entry
    ///    remains. Re-encode after eviction.
    /// 4. Write the bytes to `cache.bin.tmp` then `rename` to `cache.bin`.
    ///    This bounds the partial-truncate window that a plain
    ///    `std::fs::write` would expose mid-write.
    ///
    /// # Errors
    ///
    /// Returns an error string when the cache directory cannot be created
    /// or the temporary file cannot be written or renamed.
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
        let mut encoded = bitcode::encode(self);

        // Divide-first ordering keeps the multiplication from saturating at
        // pathologically-large caps. At most 0.008% rounding error per
        // operation, negligible for a soft size threshold.
        let trigger = (max_size_bytes / 10_000).saturating_mul(EVICTION_TRIGGER_BPS);
        if encoded.len() > trigger {
            let target = (max_size_bytes / 10_000).saturating_mul(EVICTION_TARGET_BPS);
            self.evict_lru_to_target(target);
            encoded = bitcode::encode(self);
            let evicted = initial_entries.saturating_sub(self.entries.len());
            let final_size = encoded.len();
            // `initial_entries` is bounded by the file count, so
            // `usize` saturation is not a concern here. Use the
            // multiply-then-divide ordering so small caches (< 10k
            // entries) still produce a non-zero significance threshold.
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
        atomic_write(&cache_file, &encoded)?;
        Ok(())
    }

    /// Evict LRU entries until the re-encoded size is under `target_bytes`
    /// OR only one entry remains. The single-entry floor exists so the
    /// cache stays useful under extremely tight caps; if even one entry
    /// busts the cap, the call site logs a warning and the cap is
    /// overshot intentionally rather than silently lying about respecting
    /// it (the alternative is dropping the entry and rebuilding the cache
    /// from scratch every run, which is worse).
    fn evict_lru_to_target(&mut self, target_bytes: usize) {
        // Collect (key, last_access_secs) pairs and sort ascending so the
        // oldest leave first. Ties break on path string for reproducible
        // eviction order across runs (FxHashMap iteration order is not
        // stable across processes).
        let mut order: Vec<(u64, String)> = self
            .entries
            .iter()
            .map(|(k, v)| (v.last_access_secs, k.clone()))
            .collect();
        order.sort();

        // Drop in batches of 100 to amortize the re-encode cost: 100k
        // entries with one re-encode per eviction would be O(n^2 * encode).
        const BATCH: usize = 100;
        let mut idx = 0;
        while idx < order.len() {
            let batch_end = (idx + BATCH).min(order.len());
            for (_, key) in &order[idx..batch_end] {
                if self.entries.len() <= 1 {
                    break;
                }
                self.entries.remove(key);
            }
            idx = batch_end;

            // Cheap progress check: re-encode and bail if we're already
            // under target. This costs one extra encode per 100 evictions,
            // but avoids over-evicting when the bulk of the size came from
            // a small number of large entries near the front.
            let encoded_size = bitcode::encode(self).len();
            if encoded_size <= target_bytes || self.entries.len() <= 1 {
                if encoded_size > target_bytes && self.entries.len() <= 1 {
                    tracing::warn!(
                        encoded_kb = encoded_size / 1024,
                        target_kb = target_bytes / 1024,
                        "Single cache entry exceeds configured max; cache will overshoot the cap"
                    );
                }
                return;
            }
        }
    }

    /// Look up a cached module by path and content hash.
    /// Returns None if not cached or hash mismatch.
    #[must_use]
    pub fn get(&self, path: &Path, content_hash: u64) -> Option<&CachedModule> {
        let key = path.to_string_lossy();
        let entry = self.entries.get(key.as_ref())?;
        if entry.content_hash == content_hash {
            Some(entry)
        } else {
            None
        }
    }

    /// Insert or update a cached module.
    pub fn insert(&mut self, path: &Path, module: CachedModule) {
        let key = path.to_string_lossy().into_owned();
        self.entries.insert(key, module);
    }

    /// Fast cache lookup using only file metadata (mtime + size).
    ///
    /// If the cached entry has matching mtime and size, the file content
    /// almost certainly has not changed, so we can skip reading the file
    /// entirely. This turns a cache hit from `stat() + read() + hash`
    /// into just `stat()`.
    #[must_use]
    pub fn get_by_metadata(
        &self,
        path: &Path,
        mtime_secs: u64,
        file_size: u64,
    ) -> Option<&CachedModule> {
        let key = path.to_string_lossy();
        let entry = self.entries.get(key.as_ref())?;
        if entry.mtime_secs == mtime_secs && entry.file_size == file_size && mtime_secs > 0 {
            Some(entry)
        } else {
            None
        }
    }

    /// Look up a cached module by path only (ignoring hash).
    /// Used to check whether a module's content hash matches without
    /// requiring the caller to know the hash upfront.
    #[must_use]
    pub fn get_by_path_only(&self, path: &Path) -> Option<&CachedModule> {
        let key = path.to_string_lossy();
        self.entries.get(key.as_ref())
    }

    /// Remove cache entries for files that are no longer in the project.
    /// Keeps the cache from growing unboundedly as files are deleted.
    pub fn retain_paths(&mut self, files: &[fallow_types::discover::DiscoveredFile]) {
        use rustc_hash::FxHashSet;
        let current_paths: FxHashSet<String> = files
            .iter()
            .map(|f| f.path.to_string_lossy().to_string())
            .collect();
        self.entries.retain(|key, _| current_paths.contains(key));
    }

    /// Number of cached entries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether cache is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

fn write_cache_gitignore(cache_dir: &Path) -> Result<(), String> {
    std::fs::write(cache_dir.join(".gitignore"), "*\n")
        .map_err(|e| format!("Failed to write cache .gitignore: {e}"))
}

/// Write `data` to `cache_file` atomically: write to a sibling `.tmp` file,
/// best-effort fsync, then rename over the destination. Bounds the
/// partial-truncate window that a plain `std::fs::write` exposes, which
/// matters more once the eviction path encodes twice for large caches.
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
        // Best-effort fsync. Failures here are non-fatal because the
        // rename below is still atomic on every platform fallow targets;
        // the fsync just reduces the chance of post-power-loss corruption.
        let _ = f.sync_all();
    }

    std::fs::rename(&tmp_file, cache_file)
        .map_err(|e| format!("Failed to rename cache tmp into place: {e}"))?;
    Ok(())
}

impl Default for CacheStore {
    fn default() -> Self {
        Self::new()
    }
}
