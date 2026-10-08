//! Persistent token cache for duplication analysis.

use std::path::{Path, PathBuf};

use bitcode::{Decode, Encode};
use fallow_config::ResolvedNormalization;
use fallow_types::source_fingerprint::SourceFingerprint;
use fallow_types::suppress::{PolicyRuleSuppression, SuppressionTarget};
use oxc_span::Span;
use rustc_hash::FxHashMap;
use tempfile::NamedTempFile;
use xxhash_rust::xxh3::xxh3_64;

use super::normalize::HashedToken;
use super::tokenize::{FileTokens, SourceToken, TokenKind};
use crate::suppress::{IssueKind, Suppression};
use fallow_extract::cache::DUPES_CACHE_VERSION;

const MAX_DUPES_CACHE_SIZE: usize = 512 * 1024 * 1024;

/// Extracted token payload cached for one file.
pub(super) struct TokenPayload<'a> {
    pub(super) hashed_tokens: &'a [HashedToken],
    pub(super) file_tokens: &'a FileTokens,
    pub(super) suppressions: &'a [Suppression],
}

#[derive(Debug, Encode, Decode)]
struct CacheStore {
    version: u32,
    entries: FxHashMap<String, CachedTokenFile>,
}

#[derive(Debug, Clone, Encode, Decode)]
struct CachedTokenFile {
    mtime_ns: u64,
    /// Inode change time, or `0` where the platform reports none (always the
    /// case on Windows). Paired with `mtime_ns` so a same-length rewrite with
    /// a restored mtime cannot serve the previous file's token stream. When
    /// it is unavailable, `TokenCache::get_by_fingerprint` falls back to
    /// comparing real file content against `source` instead of refusing the
    /// cache outright.
    ctime_ns: u64,
    file_size: u64,
    normalization_hash: u64,
    hashed_tokens: Vec<CachedHashedToken>,
    token_kinds: Vec<TokenKind>,
    token_spans: Vec<CachedSpan>,
    function_spans: Vec<CachedSpan>,
    atomic_invocation_spans: Vec<CachedSpan>,
    source: String,
    line_count: u64,
    suppressions: Vec<CachedSuppression>,
}

impl CachedTokenFile {
    fn source_fingerprint(&self) -> SourceFingerprint {
        SourceFingerprint::with_ctime(self.mtime_ns, self.ctime_ns, self.file_size)
    }
}

#[derive(Debug, Clone, Encode, Decode)]
struct CachedHashedToken {
    hash: u64,
    original_index: u64,
}

#[derive(Debug, Clone, Encode, Decode)]
struct CachedSpan {
    start: u32,
    end: u32,
}

#[derive(Debug, Clone, Encode, Decode)]
struct CachedSuppression {
    line: u32,
    comment_line: u32,
    kind: u8,
    policy_pack: String,
    policy_rule_id: String,
}

#[derive(Debug, Clone)]
pub(super) struct TokenCacheEntry {
    pub hashed_tokens: Vec<HashedToken>,
    pub file_tokens: FileTokens,
    pub suppressions: Vec<Suppression>,
}

#[derive(Debug)]
pub(super) struct TokenCache {
    dir: PathBuf,
    store: CacheStore,
    dirty: bool,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct TokenCacheMode {
    hash: u64,
}

impl TokenCacheMode {
    #[must_use]
    pub(super) fn new(
        normalization: ResolvedNormalization,
        strip_types: bool,
        skip_imports: bool,
    ) -> Self {
        let bytes = [
            u8::from(normalization.ignore_identifiers),
            u8::from(normalization.ignore_string_values),
            u8::from(normalization.ignore_numeric_values),
            u8::from(strip_types),
            u8::from(skip_imports),
        ];
        Self {
            hash: xxh3_64(&bytes),
        }
    }
}

impl TokenCache {
    #[must_use]
    pub(super) fn load(cache_root: &Path) -> Self {
        let dir = cache_root
            .join("cache")
            .join(format!("dupes-tokens-v{DUPES_CACHE_VERSION}"));
        let cache_file = dir.join("cache.bin");
        let store = std::fs::read(&cache_file)
            .ok()
            .filter(|data| data.len() <= MAX_DUPES_CACHE_SIZE)
            .and_then(|data| bitcode::decode::<CacheStore>(&data).ok())
            .filter(|store| store.version == DUPES_CACHE_VERSION)
            .unwrap_or_else(CacheStore::new);

        Self {
            dir,
            store,
            dirty: false,
        }
    }

    #[must_use]
    pub(super) fn get(
        &self,
        path: &Path,
        metadata: &std::fs::Metadata,
        mode: TokenCacheMode,
    ) -> Option<TokenCacheEntry> {
        self.get_by_fingerprint(path, SourceFingerprint::from_metadata(metadata), mode)
    }

    /// Cache validation strategy (fast path -> slow path), mirroring
    /// [`fallow_extract`]'s module cache:
    ///
    /// 1. If mtime, ctime, and size all match the cached entry -> hit
    ///    immediately, no read required.
    /// 2. Otherwise, when ctime is unavailable (always the case on Windows,
    ///    where [`SourceFingerprint::ctime_ns`] is permanently `0`) -> read the
    ///    file and compare content against the cached source instead. A
    ///    same-size rewrite with a restored mtime still misses here because
    ///    its content differs; an untouched file whose mtime moved for an
    ///    unrelated reason (or whose ctime this platform cannot report at
    ///    all) still hits.
    ///
    /// Step 1 requires ctime as well as mtime because mtime is
    /// writer-controlled: a same-length rewrite whose mtime is restored
    /// (`touch -r`, a codemod, a `git checkout` of an equal-length revision)
    /// leaves `(mtime, size)` unchanged, and serving the cached token stream
    /// for it means reporting duplicates against the OLD file's content.
    fn get_by_fingerprint(
        &self,
        path: &Path,
        fingerprint: SourceFingerprint,
        mode: TokenCacheMode,
    ) -> Option<TokenCacheEntry> {
        let entry = self.store.entries.get(&cache_key(path))?;
        if entry.normalization_hash != mode.hash {
            return None;
        }
        // Metadata is the fast path, not the verdict. A match settles it without
        // touching the disk; a mismatch only means the timestamps cannot settle
        // it, so content decides. Returning `None` on a metadata mismatch would
        // re-tokenize an untouched file after a `touch` or a checkout that
        // rewrites timestamps, and on a platform with no ctime (Windows) it
        // would skip the cache entirely, since the fingerprint is never
        // trustworthy there. Content is the same authority on every platform,
        // so a size-preserving edit with a restored mtime still misses.
        if fingerprint.is_trustworthy_without_content() && entry.source_fingerprint() == fingerprint
        {
            return Some(entry.to_entry());
        }
        let content = std::fs::read_to_string(path).ok()?;
        if xxh3_64(content.as_bytes()) != xxh3_64(entry.source.as_bytes()) {
            return None;
        }
        Some(entry.to_entry())
    }

    /// Store the tokens of `path`. `read_started_ns` is the time before the
    /// caller took `metadata` and read the source. A fingerprint inside the
    /// settle window of that time is stored without its ctime, so the next
    /// run compares content for the file.
    pub(super) fn insert(
        &mut self,
        path: &Path,
        metadata: &std::fs::Metadata,
        read_started_ns: u64,
        mode: TokenCacheMode,
        payload: &TokenPayload<'_>,
    ) {
        let fingerprint =
            SourceFingerprint::from_metadata(metadata).for_content_read_at(read_started_ns);
        self.store.entries.insert(
            cache_key(path),
            CachedTokenFile::from_tokens(
                fingerprint,
                mode.hash,
                payload.hashed_tokens,
                payload.file_tokens,
                payload.suppressions,
            ),
        );
        self.dirty = true;
    }

    /// Store the current fingerprint of a cache hit, when it differs from the
    /// stored one. A hit through the content check proves that the stored
    /// tokens match the source read after `read_started_ns`, so a settled
    /// fingerprint lets the next run skip that read.
    pub(super) fn refresh_fingerprint(
        &mut self,
        path: &Path,
        metadata: &std::fs::Metadata,
        read_started_ns: u64,
    ) {
        let fingerprint =
            SourceFingerprint::from_metadata(metadata).for_content_read_at(read_started_ns);
        let Some(entry) = self.store.entries.get_mut(&cache_key(path)) else {
            return;
        };
        if entry.source_fingerprint() == fingerprint {
            return;
        }
        entry.mtime_ns = fingerprint.mtime_ns;
        entry.ctime_ns = fingerprint.ctime_ns;
        entry.file_size = fingerprint.file_size;
        self.dirty = true;
    }

    pub(super) fn retain_paths(&mut self, files: &[crate::discover::DiscoveredFile]) {
        let current: rustc_hash::FxHashSet<String> =
            files.iter().map(|file| cache_key(&file.path)).collect();
        let before = self.store.entries.len();
        self.store.entries.retain(|path, _| current.contains(path));
        if self.store.entries.len() != before {
            self.dirty = true;
        }
    }

    pub(super) fn save_if_dirty(&self) -> Result<bool, String> {
        ensure_cache_gitignore(&self.dir)?;
        if !self.dirty {
            return Ok(false);
        }

        let data = bitcode::encode(&self.store);
        let mut tmp = NamedTempFile::new_in(&self.dir)
            .map_err(|e| format!("Failed to create duplication cache temp file: {e}"))?;
        std::io::Write::write_all(&mut tmp, &data)
            .map_err(|e| format!("Failed to write duplication cache temp file: {e}"))?;
        tmp.persist(self.dir.join("cache.bin"))
            .map_err(|e| format!("Failed to persist duplication cache: {}", e.error))?;
        Ok(true)
    }

    #[cfg(test)]
    fn save(&self) -> Result<(), String> {
        self.save_if_dirty().map(|_| ())
    }
}

fn ensure_cache_gitignore(cache_dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(cache_dir)
        .map_err(|e| format!("Failed to create duplication cache dir: {e}"))?;
    let path = cache_dir.join(".gitignore");
    if std::fs::read_to_string(&path).ok().as_deref() == Some("*\n") {
        return Ok(());
    }
    std::fs::write(path, "*\n")
        .map_err(|e| format!("Failed to write duplication cache .gitignore: {e}"))
}

impl CacheStore {
    fn new() -> Self {
        Self {
            version: DUPES_CACHE_VERSION,
            entries: FxHashMap::default(),
        }
    }
}

impl CachedTokenFile {
    fn from_tokens(
        fingerprint: SourceFingerprint,
        normalization_hash: u64,
        hashed_tokens: &[HashedToken],
        file_tokens: &FileTokens,
        suppressions: &[Suppression],
    ) -> Self {
        Self {
            mtime_ns: fingerprint.mtime_ns,
            ctime_ns: fingerprint.ctime_ns,
            file_size: fingerprint.file_size,
            normalization_hash,
            hashed_tokens: hashed_tokens
                .iter()
                .map(|token| CachedHashedToken {
                    hash: token.hash,
                    original_index: token.original_index as u64,
                })
                .collect(),
            token_kinds: file_tokens
                .tokens
                .iter()
                .map(|token| token.kind.clone())
                .collect(),
            token_spans: file_tokens
                .tokens
                .iter()
                .map(|token| cached_span(token.span))
                .collect(),
            function_spans: file_tokens
                .function_spans
                .iter()
                .map(|span| cached_span(*span))
                .collect(),
            atomic_invocation_spans: file_tokens
                .atomic_invocation_spans
                .iter()
                .map(|span| cached_span(*span))
                .collect(),
            source: file_tokens.source.clone(),
            line_count: file_tokens.line_count as u64,
            suppressions: suppressions.iter().map(cached_suppression).collect(),
        }
    }

    fn to_entry(&self) -> TokenCacheEntry {
        let file_tokens = FileTokens {
            tokens: self
                .token_spans
                .iter()
                .zip(&self.token_kinds)
                .map(|(span, kind)| SourceToken {
                    kind: kind.clone(),
                    span: Span::new(span.start, span.end),
                })
                .collect(),
            function_spans: self
                .function_spans
                .iter()
                .map(|span| Span::new(span.start, span.end))
                .collect(),
            atomic_invocation_spans: self
                .atomic_invocation_spans
                .iter()
                .map(|span| Span::new(span.start, span.end))
                .collect(),
            source: self.source.clone(),
            line_count: usize::try_from(self.line_count).unwrap_or(usize::MAX),
        };
        let hashed_tokens = self
            .hashed_tokens
            .iter()
            .map(|token| HashedToken {
                hash: token.hash,
                original_index: usize::try_from(token.original_index).unwrap_or(usize::MAX),
            })
            .collect();
        let suppressions = self
            .suppressions
            .iter()
            .map(|suppression| {
                let target = if suppression.kind == 0 {
                    None
                } else if suppression.kind == IssueKind::PolicyViolation.to_discriminant()
                    && !suppression.policy_pack.is_empty()
                    && !suppression.policy_rule_id.is_empty()
                {
                    Some(SuppressionTarget::PolicyRule(PolicyRuleSuppression::new(
                        suppression.policy_pack.clone(),
                        suppression.policy_rule_id.clone(),
                    )))
                } else {
                    IssueKind::from_discriminant(suppression.kind).map(SuppressionTarget::Issue)
                };
                Suppression {
                    line: suppression.line,
                    comment_line: suppression.comment_line,
                    target,
                    reason: None,
                }
            })
            .collect();
        TokenCacheEntry {
            hashed_tokens,
            file_tokens,
            suppressions,
        }
    }
}

/// Convert an oxc [`Span`] into its cache-serializable form.
const fn cached_span(span: Span) -> CachedSpan {
    CachedSpan {
        start: span.start,
        end: span.end,
    }
}

/// Convert a [`Suppression`] into its cache-serializable form, flattening the
/// target into a discriminant plus optional policy pack/rule strings.
fn cached_suppression(suppression: &Suppression) -> CachedSuppression {
    let (kind, policy_pack, policy_rule_id) = match &suppression.target {
        None => (0, String::new(), String::new()),
        Some(SuppressionTarget::Issue(kind)) => {
            (kind.to_discriminant(), String::new(), String::new())
        }
        Some(SuppressionTarget::PolicyRule(target)) => (
            IssueKind::PolicyViolation.to_discriminant(),
            target.pack.clone(),
            target.rule_id.clone(),
        ),
    };
    CachedSuppression {
        line: suppression.line,
        comment_line: suppression.comment_line,
        kind,
        policy_pack,
        policy_rule_id,
    }
}

fn cache_key(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use fallow_config::DetectionMode;

    fn mode() -> TokenCacheMode {
        TokenCacheMode::new(
            ResolvedNormalization::resolve(
                DetectionMode::Mild,
                &fallow_config::NormalizationConfig::default(),
            ),
            false,
            false,
        )
    }

    fn entry(source: &str) -> TokenCacheEntry {
        TokenCacheEntry {
            hashed_tokens: vec![HashedToken {
                hash: 42,
                original_index: 0,
            }],
            file_tokens: FileTokens {
                tokens: vec![SourceToken {
                    kind: TokenKind::Identifier("value".to_string()),
                    span: Span::new(0, 5),
                }],
                function_spans: vec![Span::new(0, 5)],
                atomic_invocation_spans: Vec::new(),
                source: source.to_owned(),
                line_count: 1,
            },
            suppressions: vec![Suppression::issue(2, 1, IssueKind::CodeDuplication)],
        }
    }

    fn insert_entry(
        cache: &mut TokenCache,
        file: &Path,
        metadata: &std::fs::Metadata,
        mode: TokenCacheMode,
        entry: &TokenCacheEntry,
    ) {
        cache.insert(
            file,
            metadata,
            u64::MAX,
            mode,
            &TokenPayload {
                hashed_tokens: &entry.hashed_tokens,
                file_tokens: &entry.file_tokens,
                suppressions: &entry.suppressions,
            },
        );
    }

    #[test]
    fn token_cache_roundtrips_hit() {
        let dir = tempfile::tempdir().expect("temp dir");
        let file = dir.path().join("src.ts");
        std::fs::write(&file, "const value = 1;\n").expect("write source");
        let metadata = std::fs::metadata(&file).expect("metadata");

        let mut cache = TokenCache::load(dir.path());
        let entry = entry("const value = 1;\n");
        insert_entry(&mut cache, &file, &metadata, mode(), &entry);
        cache.save().expect("save cache");

        let loaded = TokenCache::load(dir.path());
        let hit = loaded
            .get(&file, &metadata, mode())
            .expect("cache should hit");
        assert_eq!(hit.hashed_tokens[0].hash, 42);
        assert_eq!(hit.file_tokens.source, "const value = 1;\n");
        assert_eq!(hit.file_tokens.tokens[0].span.start, 0);
        assert_eq!(hit.file_tokens.function_spans, vec![Span::new(0, 5)]);
        assert!(matches!(
            &hit.file_tokens.tokens[0].kind,
            TokenKind::Identifier(name) if name == "value"
        ));
        assert_eq!(hit.suppressions.len(), 1);
        assert_eq!(hit.suppressions[0].line, 2);
        assert_eq!(hit.suppressions[0].comment_line, 1);
        assert_eq!(
            hit.suppressions[0].issue_kind_target(),
            Some(IssueKind::CodeDuplication)
        );
    }

    #[test]
    fn token_cache_save_writes_gitignore() {
        let dir = tempfile::tempdir().expect("temp dir");
        let cache = TokenCache::load(dir.path());
        cache.save().expect("save cache");

        let gitignore = dir
            .path()
            .join("cache")
            .join(format!("dupes-tokens-v{DUPES_CACHE_VERSION}"))
            .join(".gitignore");
        assert_eq!(
            std::fs::read_to_string(gitignore).expect("read gitignore"),
            "*\n"
        );
    }

    #[test]
    fn token_cache_misses_when_metadata_changes() {
        let dir = tempfile::tempdir().expect("temp dir");
        let file = dir.path().join("src.ts");
        std::fs::write(&file, "const value = 1;\n").expect("write source");
        let metadata = std::fs::metadata(&file).expect("metadata");

        let mut cache = TokenCache::load(dir.path());
        let entry = entry("const value = 1;\n");
        insert_entry(&mut cache, &file, &metadata, mode(), &entry);
        cache.save().expect("save cache");

        std::fs::write(&file, "const value = 12345;\n").expect("rewrite source");
        let changed_metadata = std::fs::metadata(&file).expect("metadata");
        let loaded = TokenCache::load(dir.path());
        assert!(loaded.get(&file, &changed_metadata, mode()).is_none());
    }

    #[test]
    fn token_cache_misses_when_normalization_changes() {
        let dir = tempfile::tempdir().expect("temp dir");
        let file = dir.path().join("src.ts");
        std::fs::write(&file, "const value = 1;\n").expect("write source");
        let metadata = std::fs::metadata(&file).expect("metadata");

        let mut cache = TokenCache::load(dir.path());
        let entry = entry("const value = 1;\n");
        insert_entry(&mut cache, &file, &metadata, mode(), &entry);
        cache.save().expect("save cache");

        let changed_mode = TokenCacheMode::new(
            ResolvedNormalization::resolve(
                DetectionMode::Semantic,
                &fallow_config::NormalizationConfig::default(),
            ),
            false,
            false,
        );
        let loaded = TokenCache::load(dir.path());
        assert!(loaded.get(&file, &metadata, changed_mode).is_none());
    }

    #[test]
    fn token_cache_ignores_wrong_version() {
        let dir = tempfile::tempdir().expect("temp dir");
        let file = dir.path().join("src.ts");
        std::fs::write(&file, "const value = 1;\n").expect("write source");
        let metadata = std::fs::metadata(&file).expect("metadata");

        let cache_dir = dir
            .path()
            .join("cache")
            .join(format!("dupes-tokens-v{DUPES_CACHE_VERSION}"));
        std::fs::create_dir_all(&cache_dir).expect("cache dir");
        let mut store = CacheStore::new();
        store.version = DUPES_CACHE_VERSION + 1;
        let entry = entry("const value = 1;\n");
        store.entries.insert(
            cache_key(&file),
            CachedTokenFile::from_tokens(
                SourceFingerprint::from_metadata(&metadata),
                mode().hash,
                &entry.hashed_tokens,
                &entry.file_tokens,
                &entry.suppressions,
            ),
        );
        std::fs::write(cache_dir.join("cache.bin"), bitcode::encode(&store)).expect("write cache");

        let loaded = TokenCache::load(dir.path());
        assert!(loaded.get(&file, &metadata, mode()).is_none());
    }

    #[test]
    fn token_cache_still_hits_when_only_the_timestamps_moved() {
        let dir = tempfile::tempdir().expect("temp dir");
        let file = dir.path().join("src.ts");
        std::fs::write(&file, "const value = 1;\n").expect("write source");
        let metadata = std::fs::metadata(&file).expect("metadata");

        let mut cache = TokenCache::load(dir.path());
        let entry = entry("const value = 1;\n");
        insert_entry(&mut cache, &file, &metadata, mode(), &entry);
        let cached = cache
            .store
            .entries
            .get_mut(&cache_key(&file))
            .expect("cached token entry");
        cached.mtime_ns = cached.mtime_ns.saturating_add(1);

        assert!(
            cache.get(&file, &metadata, mode()).is_some(),
            "a touch rewrites the timestamp without changing a byte, so the tokens are still good"
        );
    }

    #[test]
    fn token_cache_misses_when_the_timestamps_moved_and_the_content_did_too() {
        let dir = tempfile::tempdir().expect("temp dir");
        let file = dir.path().join("src.ts");
        std::fs::write(&file, "const value = 1;\n").expect("write source");
        let metadata = std::fs::metadata(&file).expect("metadata");

        let mut cache = TokenCache::load(dir.path());
        let entry = entry("const value = 1;\n");
        insert_entry(&mut cache, &file, &metadata, mode(), &entry);
        let cached = cache
            .store
            .entries
            .get_mut(&cache_key(&file))
            .expect("cached token entry");
        cached.mtime_ns = cached.mtime_ns.saturating_add(1);

        std::fs::write(&file, "const value = 2;\n").expect("rewrite source");

        assert!(
            cache.get(&file, &metadata, mode()).is_none(),
            "content is the authority, so a real edit misses however the timestamps look"
        );
    }

    /// Restore the mtime of a rewritten file, then wait until its ctime differs
    /// from `ctime_before`. A filesystem with a coarse clock can give the rewrite
    /// the same ctime as the first write. The test is about a moved ctime, so it
    /// applies the timestamps again until the clock ticks. Windows reports no
    /// ctime, so the loop stops at once there.
    fn restore_mtime_once_ctime_moves(
        path: &std::path::Path,
        accessed: std::time::SystemTime,
        modified: std::time::SystemTime,
        ctime_before: u64,
    ) {
        for _ in 0..200 {
            std::fs::OpenOptions::new()
                .write(true)
                .open(path)
                .expect("open source for timestamp restore")
                .set_times(
                    std::fs::FileTimes::new()
                        .set_accessed(accessed)
                        .set_modified(modified),
                )
                .expect("restore source timestamps");
            let metadata = std::fs::metadata(path).expect("source metadata after restore");
            let ctime = SourceFingerprint::from_metadata(&metadata).ctime_ns;
            if ctime == 0 || ctime != ctime_before {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        panic!("the ctime of {} did not move", path.display());
    }

    /// A rewrite that keeps the byte length and restores the mtime is invisible
    /// to `(mtime, size)`, so the ctime half of the fingerprint is what stops
    /// the cache from replaying the previous file's token stream.
    #[test]
    fn token_cache_misses_when_only_the_ctime_moved() {
        let dir = tempfile::tempdir().expect("temp dir");
        let file = dir.path().join("src.ts");
        std::fs::write(&file, "const value = 1;\n").expect("write source");
        let metadata = std::fs::metadata(&file).expect("metadata");
        let modified = metadata.modified().expect("mtime");
        let accessed = metadata.accessed().unwrap_or(modified);
        let ctime_before = SourceFingerprint::from_metadata(&metadata).ctime_ns;

        let mut cache = TokenCache::load(dir.path());
        let entry = entry("const value = 1;\n");
        insert_entry(&mut cache, &file, &metadata, mode(), &entry);

        std::fs::write(&file, "const other = 1;\n").expect("rewrite source");
        restore_mtime_once_ctime_moves(&file, accessed, modified, ctime_before);
        let rewritten = std::fs::metadata(&file).expect("metadata after rewrite");
        assert_eq!(rewritten.len(), metadata.len());
        assert_eq!(rewritten.modified().expect("mtime after rewrite"), modified);

        assert!(cache.get(&file, &rewritten, mode()).is_none());
    }

    /// A fingerprint with no known mtime is untrustworthy, so the lookup
    /// falls through to comparing real file content instead of refusing
    /// outright. When that content genuinely changed, it must still miss.
    #[test]
    fn token_cache_misses_when_mtime_is_unknown_and_content_changed() {
        let dir = tempfile::tempdir().expect("temp dir");
        let file = dir.path().join("src.ts");
        std::fs::write(&file, "const value = 1;\n").expect("write source");
        let metadata = std::fs::metadata(&file).expect("metadata");

        let mut cache = TokenCache::load(dir.path());
        let entry = entry("const value = 1;\n");
        insert_entry(&mut cache, &file, &metadata, mode(), &entry);

        std::fs::write(&file, "const value = 12345;\n").expect("rewrite source");

        let unknown_mtime = SourceFingerprint::new(0, metadata.len());
        assert!(
            cache
                .get_by_fingerprint(&file, unknown_mtime, mode())
                .is_none()
        );
    }

    /// Windows never reports ctime, so `is_trustworthy_without_content()` is
    /// permanently false there and every lookup must fall through to the
    /// content-hash comparison. Building the fingerprints directly (instead
    /// of relying on the host's own metadata) exercises that exact code path
    /// on any platform, including one where ctime is normally available.
    #[test]
    fn token_cache_hits_via_content_when_ctime_is_absent() {
        let dir = tempfile::tempdir().expect("temp dir");
        let file = dir.path().join("src.ts");
        std::fs::write(&file, "const value = 1;\n").expect("write source");
        let metadata = std::fs::metadata(&file).expect("metadata");

        let mut cache = TokenCache::load(dir.path());
        let entry = entry("const value = 1;\n");
        let stale_fingerprint = SourceFingerprint::new(1, metadata.len());
        cache.store.entries.insert(
            cache_key(&file),
            CachedTokenFile::from_tokens(
                stale_fingerprint,
                mode().hash,
                &entry.hashed_tokens,
                &entry.file_tokens,
                &entry.suppressions,
            ),
        );

        // A different mtime than what was cached models the metadata-only
        // change (e.g. an unrelated `touch`); ctime stays absent on both
        // sides and the file's real content never changed.
        let live_fingerprint = SourceFingerprint::new(2, metadata.len());
        let hit = cache
            .get_by_fingerprint(&file, live_fingerprint, mode())
            .expect(
                "content fallback should hit when ctime is unavailable and content is unchanged",
            );
        assert_eq!(hit.hashed_tokens[0].hash, 42);
        assert_eq!(hit.file_tokens.source, "const value = 1;\n");
    }

    /// The invariant the ctime work exists to protect: a same-size rewrite
    /// with a restored mtime must still miss, even when ctime is absent on
    /// every platform and the content-hash fallback is the only thing left
    /// to catch it.
    #[test]
    fn token_cache_misses_via_content_when_ctime_is_absent_and_content_changed() {
        let dir = tempfile::tempdir().expect("temp dir");
        let file = dir.path().join("src.ts");
        std::fs::write(&file, "const value = 1;\n").expect("write source");
        let metadata = std::fs::metadata(&file).expect("metadata");
        let modified = metadata.modified().expect("mtime");
        let accessed = metadata.accessed().unwrap_or(modified);
        let real_mtime_ns = SourceFingerprint::from_metadata(&metadata).mtime_ns;

        let mut cache = TokenCache::load(dir.path());
        let entry = entry("const value = 1;\n");
        let no_ctime_fingerprint = SourceFingerprint::new(real_mtime_ns, metadata.len());
        cache.store.entries.insert(
            cache_key(&file),
            CachedTokenFile::from_tokens(
                no_ctime_fingerprint,
                mode().hash,
                &entry.hashed_tokens,
                &entry.file_tokens,
                &entry.suppressions,
            ),
        );

        std::fs::write(&file, "const other = 1;\n").expect("rewrite source, same byte length");
        let handle = std::fs::OpenOptions::new()
            .write(true)
            .open(&file)
            .expect("open source for timestamp restore");
        handle
            .set_times(
                std::fs::FileTimes::new()
                    .set_accessed(accessed)
                    .set_modified(modified),
            )
            .expect("restore source timestamps");
        let rewritten = std::fs::metadata(&file).expect("metadata after rewrite");
        assert_eq!(rewritten.len(), metadata.len());

        assert!(
            cache
                .get_by_fingerprint(&file, no_ctime_fingerprint, mode())
                .is_none(),
            "a size-preserving content change with a restored mtime must miss even without ctime"
        );
    }

    /// A same-length write in the same timestamp tick as the cached read keeps
    /// mtime, ctime and size. The fingerprint that the run stores must then
    /// not stand in for the content, or the next run replays the old tokens.
    #[test]
    fn a_same_tick_rewrite_misses_the_token_cache() {
        let dir = tempfile::tempdir().expect("temp dir");
        let file = dir.path().join("src.ts");
        let before = "const alpha = 1;\n";
        let after = "const bravo = 1;\n";
        let read_started_ns = fallow_types::source_fingerprint::now_ns();
        std::fs::write(&file, before).expect("write source");
        let entry = entry(before);
        std::fs::write(&file, after).expect("rewrite source with equal-length content");
        // The metadata of the rewrite stands in for the metadata of a write in
        // the same tick as the read: all three values match the live file.
        let same_tick = std::fs::metadata(&file).expect("metadata after rewrite");

        let mut cache = TokenCache::load(dir.path());
        cache.insert(
            &file,
            &same_tick,
            read_started_ns,
            mode(),
            &TokenPayload {
                hashed_tokens: &entry.hashed_tokens,
                file_tokens: &entry.file_tokens,
                suppressions: &entry.suppressions,
            },
        );

        assert!(
            cache.get(&file, &same_tick, mode()).is_none(),
            "a fingerprint from inside the settle window must not replay the old tokens"
        );
    }

    #[test]
    fn a_content_hit_stores_the_settled_fingerprint() {
        let dir = tempfile::tempdir().expect("temp dir");
        let file = dir.path().join("src.ts");
        std::fs::write(&file, "const value = 1;\n").expect("write source");
        let metadata = std::fs::metadata(&file).expect("metadata");
        let mut cache = TokenCache::load(dir.path());
        let entry = entry("const value = 1;\n");
        cache.insert(
            &file,
            &metadata,
            0,
            mode(),
            &TokenPayload {
                hashed_tokens: &entry.hashed_tokens,
                file_tokens: &entry.file_tokens,
                suppressions: &entry.suppressions,
            },
        );
        let stored = |cache: &TokenCache| {
            cache
                .store
                .entries
                .get(&cache_key(&file))
                .expect("cached token entry")
                .source_fingerprint()
        };
        assert!(!stored(&cache).is_trustworthy_without_content());

        cache.refresh_fingerprint(&file, &metadata, u64::MAX);

        assert_eq!(stored(&cache), SourceFingerprint::from_metadata(&metadata));
    }
}
