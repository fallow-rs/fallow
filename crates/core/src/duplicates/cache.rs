//! Persistent token cache for duplication analysis.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use bitcode::{Decode, Encode};
use fallow_config::ResolvedNormalization;
use fallow_types::suppress::{PolicyRuleSuppression, SuppressionTarget};
use oxc_span::Span;
use rustc_hash::FxHashMap;
use tempfile::NamedTempFile;
use xxhash_rust::xxh3::xxh3_64;

use super::normalize::HashedToken;
use super::tokenize::{FileTokens, SourceToken, TokenKind};
use crate::cache::DUPES_CACHE_VERSION;
use crate::suppress::{IssueKind, Suppression};

const MAX_DUPES_CACHE_SIZE: usize = 512 * 1024 * 1024;

#[derive(Debug, Encode, Decode)]
struct CacheStore {
    version: u32,
    entries: FxHashMap<String, CachedTokenFile>,
}

#[derive(Debug, Clone, Encode, Decode)]
struct CachedTokenFile {
    mtime_ns: u64,
    file_size: u64,
    normalization_hash: u64,
    hashed_tokens: Vec<CachedHashedToken>,
    token_kinds: Vec<TokenKind>,
    token_spans: Vec<CachedSpan>,
    atomic_invocation_spans: Vec<CachedSpan>,
    source: String,
    line_count: u64,
    suppressions: Vec<CachedSuppression>,
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
        let entry = self.store.entries.get(&cache_key(path))?;
        let (mtime_ns, file_size) = metadata_key(metadata);
        if entry.mtime_ns != mtime_ns
            || entry.file_size != file_size
            || entry.normalization_hash != mode.hash
        {
            return None;
        }
        Some(entry.to_entry())
    }

    pub(super) fn insert(
        &mut self,
        path: &Path,
        metadata: &std::fs::Metadata,
        mode: TokenCacheMode,
        hashed_tokens: &[HashedToken],
        file_tokens: &FileTokens,
        suppressions: &[Suppression],
    ) {
        let (mtime_ns, file_size) = metadata_key(metadata);
        self.store.entries.insert(
            cache_key(path),
            CachedTokenFile::from_tokens(
                mtime_ns,
                file_size,
                mode.hash,
                hashed_tokens,
                file_tokens,
                suppressions,
            ),
        );
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
    pub(super) fn save(&self) -> Result<(), String> {
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
        mtime_ns: u64,
        file_size: u64,
        normalization_hash: u64,
        hashed_tokens: &[HashedToken],
        file_tokens: &FileTokens,
        suppressions: &[Suppression],
    ) -> Self {
        Self {
            mtime_ns,
            file_size,
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
                .map(|token| CachedSpan {
                    start: token.span.start,
                    end: token.span.end,
                })
                .collect(),
            atomic_invocation_spans: file_tokens
                .atomic_invocation_spans
                .iter()
                .map(|span| CachedSpan {
                    start: span.start,
                    end: span.end,
                })
                .collect(),
            source: file_tokens.source.clone(),
            line_count: file_tokens.line_count as u64,
            suppressions: suppressions
                .iter()
                .map(|suppression| {
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
                })
                .collect(),
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

fn cache_key(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "filesystem mtimes used for cache invalidation fit in u64 nanoseconds for supported dates"
)]
fn metadata_key(metadata: &std::fs::Metadata) -> (u64, u64) {
    let mtime_ns = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map_or(0, |duration| duration.as_nanos() as u64);
    (mtime_ns, metadata.len())
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
            mode,
            &entry.hashed_tokens,
            &entry.file_tokens,
            &entry.suppressions,
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
                metadata_key(&metadata).0,
                metadata.len(),
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
}
