//! Cache store: load, save, and query cached module data.

use std::path::Path;

use fallow_types::source_fingerprint::SourceFingerprint;
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
    /// Stable hash of extraction-affecting config fields.
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
    /// Returns `None` when the file is missing, too large, undecodable, or
    /// built for a different `config_hash`.
    #[must_use]
    pub fn load(
        cache_dir: &Path,
        expected_config_hash: u64,
        max_size_bytes: usize,
    ) -> Option<Self> {
        let cache_file = cache_dir.join("cache.bin");
        let data = std::fs::read(&cache_file).ok()?;
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

        let trigger = (max_size_bytes / 10_000).saturating_mul(EVICTION_TRIGGER_BPS);
        if encoded.len() > trigger {
            let target = (max_size_bytes / 10_000).saturating_mul(EVICTION_TARGET_BPS);
            self.evict_lru_to_target(target);
            encoded = bitcode::encode(self);
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
        atomic_write(&cache_file, &encoded)?;
        Ok(())
    }

    /// Evict LRU entries until the re-encoded size is under `target_bytes`
    /// or only one entry remains.
    fn evict_lru_to_target(&mut self, target_bytes: usize) {
        let mut order: Vec<(u64, String)> = self
            .entries
            .iter()
            .map(|(k, v)| (v.last_access_secs, k.clone()))
            .collect();
        order.sort();

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
    #[must_use]
    pub fn get_by_metadata(
        &self,
        path: &Path,
        fingerprint: SourceFingerprint,
    ) -> Option<&CachedModule> {
        let key = path.to_string_lossy();
        let entry = self.entries.get(key.as_ref())?;
        if entry.source_fingerprint() == fingerprint && fingerprint.has_known_mtime() {
            Some(entry)
        } else {
            None
        }
    }

    /// Look up a cached module by path only (ignoring hash).
    #[must_use]
    pub fn get_by_path_only(&self, path: &Path) -> Option<&CachedModule> {
        let key = path.to_string_lossy();
        self.entries.get(key.as_ref())
    }

    /// Remove cache entries for files that are no longer in the project.
    ///
    /// Returns `true` when any entry was removed.
    pub fn retain_paths(&mut self, files: &[fallow_types::discover::DiscoveredFile]) -> bool {
        use rustc_hash::FxHashSet;
        let current_paths: FxHashSet<String> = files
            .iter()
            .map(|f| f.path.to_string_lossy().to_string())
            .collect();
        let before = self.entries.len();
        self.entries.retain(|key, _| current_paths.contains(key));
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
        Self::new()
    }
}
