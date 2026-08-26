//! Bounded persistent cache for source-derived local embeddings.

use std::fs::{File, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use fallow_engine::source::similar_code::SimilarCodeSourceDigest;
use rustc_hash::{FxHashMap, FxHashSet};
use sha2::{Digest, Sha256};

use super::protocol::{
    EMBEDDING_SEMANTICS_VERSION, EXTRACTION_SEMANTICS_VERSION, MODEL_DIMENSIONS, MODEL_ID,
    MODEL_MAX_TOKENS, MODEL_NORMALIZATION, MODEL_REVISION, WIRE_PROTOCOL_VERSION,
};

const MAGIC: &[u8; 8] = b"FSCVEC03";
const TOKEN_TRUNCATED_FLAG: u8 = 1;
const MAX_CACHE_BYTES: usize = 64 * 1024 * 1024;
const HEADER_BYTES: usize = 8 + 4 + 4 + 4 + 4 + 32 + 4;
const MAX_TEMP_FILE_ATTEMPTS: usize = 32;

static CACHE_TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CacheLoadState {
    Disabled,
    Missing,
    Hit,
    Corrupt,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct CachedVector {
    pub(super) values: Vec<f32>,
    pub(super) token_truncated: bool,
}

#[derive(Debug, Clone)]
struct CacheLocation {
    root: PathBuf,
    path: PathBuf,
    lock_path: PathBuf,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct CacheSaveOutcome {
    pub(super) durable_writes: usize,
    pub(super) problem: Option<String>,
}

pub(super) struct VectorCache {
    location: Option<CacheLocation>,
    entries: FxHashMap<SimilarCodeSourceDigest, CachedVector>,
    active: FxHashSet<SimilarCodeSourceDigest>,
    pending: FxHashSet<SimilarCodeSourceDigest>,
    dirty: bool,
    disabled_problem: Option<String>,
    pub(super) load_state: CacheLoadState,
}

impl VectorCache {
    fn disabled(problem: Option<String>) -> Self {
        Self {
            location: None,
            entries: FxHashMap::default(),
            active: FxHashSet::default(),
            pending: FxHashSet::default(),
            dirty: false,
            disabled_problem: problem,
            load_state: CacheLoadState::Disabled,
        }
    }

    fn empty(location: CacheLocation, load_state: CacheLoadState) -> Self {
        Self {
            location: Some(location),
            entries: FxHashMap::default(),
            active: FxHashSet::default(),
            pending: FxHashSet::default(),
            dirty: load_state == CacheLoadState::Corrupt,
            disabled_problem: None,
            load_state,
        }
    }

    pub(super) fn load(provider_cache_dir: &Path, project_root: &Path, disabled: bool) -> Self {
        if disabled {
            return Self::disabled(None);
        }
        let location = match trusted_cache_location(provider_cache_dir, project_root) {
            Ok(location) => location,
            Err(problem) => return Self::disabled(Some(problem)),
        };
        match read_cache(&location.path) {
            CacheRead::Missing => Self::empty(location, CacheLoadState::Missing),
            CacheRead::Corrupt => Self::empty(location, CacheLoadState::Corrupt),
            CacheRead::Unsafe(problem) => Self::disabled(Some(problem)),
            CacheRead::Hit(entries) => Self {
                location: Some(location),
                entries,
                active: FxHashSet::default(),
                pending: FxHashSet::default(),
                dirty: false,
                disabled_problem: None,
                load_state: CacheLoadState::Hit,
            },
        }
    }

    pub(super) fn get(&mut self, digest: &SimilarCodeSourceDigest) -> Option<&CachedVector> {
        self.active.insert(*digest);
        self.entries.get(digest)
    }

    pub(super) fn insert(
        &mut self,
        digest: SimilarCodeSourceDigest,
        values: Vec<f32>,
        token_truncated: bool,
    ) -> bool {
        self.active.insert(digest);
        if self.location.is_none()
            || values.len() != MODEL_DIMENSIONS
            || values.iter().any(|value| !value.is_finite())
            || values.iter().all(|value| *value == 0.0)
        {
            return false;
        }
        let entry = CachedVector {
            values,
            token_truncated,
        };
        if self.entries.get(&digest) == Some(&entry) {
            return false;
        }
        self.entries.insert(digest, entry);
        self.pending.insert(digest);
        self.dirty = true;
        true
    }

    pub(super) fn save(&mut self) -> CacheSaveOutcome {
        let Some(location) = self.location.clone() else {
            return CacheSaveOutcome {
                durable_writes: 0,
                problem: self.disabled_problem.clone(),
            };
        };
        if !self.dirty {
            return CacheSaveOutcome::default();
        }
        if let Err(problem) = ensure_secure_parent(&location) {
            return CacheSaveOutcome {
                durable_writes: 0,
                problem: Some(problem),
            };
        }
        let _lock = match try_cache_lock(&location.lock_path) {
            Ok(Some(lock)) => lock,
            Ok(None) => {
                return CacheSaveOutcome {
                    durable_writes: 0,
                    problem: Some(
                        "similar-code vector cache lock is contended; persistence was skipped"
                            .to_owned(),
                    ),
                };
            }
            Err(problem) => {
                return CacheSaveOutcome {
                    durable_writes: 0,
                    problem: Some(problem),
                };
            }
        };

        let disk_entries = match read_cache(&location.path) {
            CacheRead::Hit(entries) => entries,
            CacheRead::Missing | CacheRead::Corrupt => FxHashMap::default(),
            CacheRead::Unsafe(problem) => {
                return CacheSaveOutcome {
                    durable_writes: 0,
                    problem: Some(problem),
                };
            }
        };
        let mut merged = disk_entries.clone();
        for digest in &self.active {
            if let Some(entry) = self.entries.get(digest) {
                merged.insert(*digest, entry.clone());
            }
        }
        let ordered = select_entries(&merged, &self.active, max_records());
        let durable_writes = ordered
            .iter()
            .filter(|(digest, entry)| {
                self.pending.contains(digest) && disk_entries.get(digest) != Some(entry)
            })
            .count();
        let bytes = encode_ordered(&ordered);
        if let Err(error) = atomic_replace_cache_no_follow(&location.path, &bytes) {
            return CacheSaveOutcome {
                durable_writes: 0,
                problem: Some(format!(
                    "failed to publish similar-code vector cache {}: {error}",
                    location.path.display()
                )),
            };
        }

        self.entries = ordered.into_iter().collect();
        self.pending.clear();
        self.dirty = false;
        CacheSaveOutcome {
            durable_writes,
            problem: None,
        }
    }
}

#[expect(
    clippy::filetype_is_file,
    reason = "cache mutation accepts only regular files and rejects symlinks and special files"
)]
pub(super) fn clear(provider_cache_dir: &Path, project_root: &Path) -> Result<bool, String> {
    let location = trusted_cache_location(provider_cache_dir, project_root)?;
    let Some(parent) = location.path.parent() else {
        return Err("similar-code vector cache has no parent directory".to_owned());
    };
    if !parent.exists() {
        return Ok(false);
    }
    ensure_secure_parent(&location)?;
    let Some(_lock) = try_cache_lock(&location.lock_path)? else {
        return Err("similar-code vector cache lock is contended; retry cache clear".to_owned());
    };
    let metadata = match std::fs::symlink_metadata(&location.path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return Err(format!(
                "failed to inspect similar-code vector cache {}: {error}",
                location.path.display()
            ));
        }
    };
    if !metadata.file_type().is_file() {
        return Err(format!(
            "similar-code vector cache {} is not a regular file",
            location.path.display()
        ));
    }
    std::fs::remove_file(&location.path).map_err(|error| {
        format!(
            "failed to remove similar-code vector cache {}: {error}",
            location.path.display()
        )
    })?;
    Ok(true)
}

pub(super) fn parameter_digest() -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(MODEL_ID.as_bytes());
    hasher.update([0]);
    hasher.update(MODEL_REVISION.as_bytes());
    hasher.update([0]);
    hasher.update(MODEL_NORMALIZATION.as_bytes());
    hasher.update((MODEL_DIMENSIONS as u64).to_le_bytes());
    hasher.update((MODEL_MAX_TOKENS as u64).to_le_bytes());
    hasher.update(WIRE_PROTOCOL_VERSION.to_le_bytes());
    hasher.update(EXTRACTION_SEMANTICS_VERSION.to_le_bytes());
    hasher.update(EMBEDDING_SEMANTICS_VERSION.to_le_bytes());
    hasher.finalize().into()
}

fn trusted_cache_location(
    provider_cache_dir: &Path,
    project_root: &Path,
) -> Result<CacheLocation, String> {
    if !provider_cache_dir.is_absolute() {
        return Err("similar-code vector persistence requires an absolute user cache".to_owned());
    }
    let cache_root = provider_cache_dir
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| "similar-code provider returned an invalid user cache path".to_owned())?;
    let metadata = std::fs::symlink_metadata(cache_root).map_err(|error| {
        format!(
            "similar-code user cache {} is unavailable: {error}",
            cache_root.display()
        )
    })?;
    if !metadata.file_type().is_dir() {
        return Err(format!(
            "similar-code user cache {} is not a trusted directory",
            cache_root.display()
        ));
    }
    let canonical_cache_root = dunce::canonicalize(cache_root).map_err(|error| {
        format!(
            "failed to resolve similar-code user cache {}: {error}",
            cache_root.display()
        )
    })?;
    let canonical_project = dunce::canonicalize(project_root).map_err(|error| {
        format!(
            "failed to resolve similar-code project root {}: {error}",
            project_root.display()
        )
    })?;
    if canonical_cache_root.starts_with(&canonical_project)
        || canonical_project.starts_with(&canonical_cache_root)
    {
        return Err(
            "similar-code vector persistence is disabled because the user cache overlaps the project"
                .to_owned(),
        );
    }

    let namespace = project_namespace(&canonical_project);
    let directory = canonical_cache_root
        .join("vectors")
        .join(format!("v{WIRE_PROTOCOL_VERSION}"))
        .join(namespace)
        .join(MODEL_REVISION);
    Ok(CacheLocation {
        root: canonical_cache_root,
        path: directory.join("vectors.bin"),
        lock_path: directory.join("vectors.lock"),
    })
}

fn project_namespace(project_root: &Path) -> String {
    let digest = Sha256::digest(project_root.as_os_str().as_encoded_bytes());
    hex(&digest)
}

fn ensure_secure_parent(location: &CacheLocation) -> Result<(), String> {
    let parent = location
        .path
        .parent()
        .ok_or_else(|| "similar-code vector cache has no parent directory".to_owned())?;
    let relative = parent
        .strip_prefix(&location.root)
        .map_err(|_| "similar-code vector cache escaped the verified user cache root".to_owned())?;
    let mut current = location.root.clone();
    for component in relative.components() {
        current.push(component);
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_dir() => {}
            Ok(_) => {
                return Err(format!(
                    "similar-code vector cache directory {} is not trusted",
                    current.display()
                ));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                std::fs::create_dir(&current).map_err(|error| {
                    format!(
                        "failed to create similar-code vector cache {}: {error}",
                        current.display()
                    )
                })?;
            }
            Err(error) => {
                return Err(format!(
                    "failed to inspect similar-code vector cache {}: {error}",
                    current.display()
                ));
            }
        }
    }
    Ok(())
}

struct CacheLock(File);

impl Drop for CacheLock {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}

#[expect(
    clippy::filetype_is_file,
    reason = "the advisory lock must be a regular file, never a symlink or special file"
)]
fn try_cache_lock(path: &Path) -> Result<Option<CacheLock>, String> {
    if let Ok(metadata) = std::fs::symlink_metadata(path)
        && !metadata.file_type().is_file()
    {
        return Err(format!(
            "similar-code vector cache lock {} is not a regular file",
            path.display()
        ));
    }
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)
        .map_err(|error| {
            format!(
                "failed to open similar-code vector cache lock {}: {error}",
                path.display()
            )
        })?;
    match file.try_lock() {
        Ok(()) => Ok(Some(CacheLock(file))),
        Err(std::fs::TryLockError::WouldBlock) => Ok(None),
        Err(std::fs::TryLockError::Error(error)) => Err(format!(
            "failed to lock similar-code vector cache {}: {error}",
            path.display()
        )),
    }
}

enum CacheRead {
    Missing,
    Corrupt,
    Unsafe(String),
    Hit(FxHashMap<SimilarCodeSourceDigest, CachedVector>),
}

#[expect(
    clippy::filetype_is_file,
    reason = "cache reads accept only regular files and reject symlinks and special files"
)]
fn read_cache(path: &Path) -> CacheRead {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return CacheRead::Missing,
        Err(_) => return CacheRead::Corrupt,
    };
    if !metadata.file_type().is_file() {
        return CacheRead::Unsafe(format!(
            "similar-code vector cache {} is not a regular file; persistence was disabled",
            path.display()
        ));
    }
    if metadata.len() > MAX_CACHE_BYTES as u64 {
        return CacheRead::Corrupt;
    }
    match std::fs::read(path) {
        Ok(bytes) if bytes.len() <= MAX_CACHE_BYTES => {
            decode_cache(&bytes).map_or(CacheRead::Corrupt, CacheRead::Hit)
        }
        Ok(_) | Err(_) => CacheRead::Corrupt,
    }
}

#[expect(
    clippy::filetype_is_file,
    reason = "cache publication accepts only regular destination files and rejects symlinks"
)]
fn atomic_replace_cache_no_follow(path: &Path, content: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "similar-code vector cache has no parent directory".to_owned())?;
    let mut temporary = None;
    for _ in 0..MAX_TEMP_FILE_ATTEMPTS {
        let sequence = CACHE_TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let candidate = parent.join(format!(
            ".vectors.bin.{}.{}.tmp",
            std::process::id(),
            sequence
        ));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(file) => {
                temporary = Some((candidate, file));
                break;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => {
                return Err(format!(
                    "failed to create temporary similar-code vector cache in {}: {error}",
                    parent.display()
                ));
            }
        }
    }
    let Some((temporary_path, mut temporary_file)) = temporary else {
        return Err(format!(
            "failed to reserve a temporary similar-code vector cache in {}",
            parent.display()
        ));
    };

    let publish = (|| {
        temporary_file.write_all(content).map_err(|error| {
            format!(
                "failed to write temporary similar-code vector cache {}: {error}",
                temporary_path.display()
            )
        })?;
        temporary_file.sync_all().map_err(|error| {
            format!(
                "failed to sync temporary similar-code vector cache {}: {error}",
                temporary_path.display()
            )
        })?;
        drop(temporary_file);

        match std::fs::symlink_metadata(path) {
            Ok(metadata) if metadata.file_type().is_file() => {
                std::fs::set_permissions(&temporary_path, metadata.permissions()).map_err(
                    |error| {
                        format!(
                            "failed to preserve similar-code vector cache permissions {}: {error}",
                            path.display()
                        )
                    },
                )?;
            }
            Ok(_) => {
                return Err(format!(
                    "similar-code vector cache {} is not a regular file; persistence was disabled",
                    path.display()
                ));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(format!(
                    "failed to inspect similar-code vector cache {} before publication: {error}",
                    path.display()
                ));
            }
        }

        std::fs::rename(&temporary_path, path).map_err(|error| {
            format!(
                "failed to atomically replace similar-code vector cache {}: {error}",
                path.display()
            )
        })?;
        sync_cache_directory(parent)?;
        Ok(())
    })();

    if publish.is_err() {
        let _ = std::fs::remove_file(&temporary_path);
    }
    publish
}

#[cfg(unix)]
fn sync_cache_directory(path: &Path) -> Result<(), String> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| {
            format!(
                "failed to sync similar-code vector cache directory {}: {error}",
                path.display()
            )
        })
}

#[cfg(not(unix))]
#[expect(
    clippy::unnecessary_wraps,
    reason = "the no-op platform implementation preserves the fallible Unix contract"
)]
fn sync_cache_directory(_path: &Path) -> Result<(), String> {
    Ok(())
}

fn record_bytes() -> usize {
    32 + 1 + MODEL_DIMENSIONS * std::mem::size_of::<f32>()
}

fn max_records() -> usize {
    MAX_CACHE_BYTES
        .saturating_sub(HEADER_BYTES)
        .checked_div(record_bytes())
        .unwrap_or(0)
}

fn select_entries(
    entries: &FxHashMap<SimilarCodeSourceDigest, CachedVector>,
    active: &FxHashSet<SimilarCodeSourceDigest>,
    limit: usize,
) -> Vec<(SimilarCodeSourceDigest, CachedVector)> {
    let mut active_entries = entries
        .iter()
        .filter(|(digest, _)| active.contains(digest))
        .map(|(digest, entry)| (*digest, entry.clone()))
        .collect::<Vec<_>>();
    let mut inactive_entries = entries
        .iter()
        .filter(|(digest, _)| !active.contains(digest))
        .map(|(digest, entry)| (*digest, entry.clone()))
        .collect::<Vec<_>>();
    active_entries.sort_by_key(|(digest, _)| *digest);
    inactive_entries.sort_by_key(|(digest, _)| *digest);
    active_entries.extend(inactive_entries);
    active_entries.truncate(limit);
    active_entries
}

#[cfg(test)]
fn encode_cache(
    entries: &FxHashMap<SimilarCodeSourceDigest, CachedVector>,
    active: &FxHashSet<SimilarCodeSourceDigest>,
) -> Vec<u8> {
    encode_ordered(&select_entries(entries, active, max_records()))
}

fn encode_ordered(entries: &[(SimilarCodeSourceDigest, CachedVector)]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(HEADER_BYTES + entries.len() * record_bytes());
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&WIRE_PROTOCOL_VERSION.to_le_bytes());
    bytes.extend_from_slice(&EXTRACTION_SEMANTICS_VERSION.to_le_bytes());
    bytes.extend_from_slice(&EMBEDDING_SEMANTICS_VERSION.to_le_bytes());
    bytes.extend_from_slice(&(MODEL_DIMENSIONS as u32).to_le_bytes());
    bytes.extend_from_slice(&parameter_digest());
    bytes.extend_from_slice(&(entries.len() as u32).to_le_bytes());
    for (digest, entry) in entries {
        bytes.extend_from_slice(digest.as_bytes());
        bytes.push(u8::from(entry.token_truncated) * TOKEN_TRUNCATED_FLAG);
        for value in &entry.values {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    bytes
}

fn decode_cache(bytes: &[u8]) -> Option<FxHashMap<SimilarCodeSourceDigest, CachedVector>> {
    if bytes.len() < HEADER_BYTES || bytes.get(..8)? != MAGIC {
        return None;
    }
    let mut cursor = 8usize;
    let protocol = take_u32(bytes, &mut cursor)?;
    let extraction = take_u32(bytes, &mut cursor)?;
    let embedding = take_u32(bytes, &mut cursor)?;
    let dimensions = take_u32(bytes, &mut cursor)? as usize;
    let parameters: [u8; 32] = bytes.get(cursor..cursor + 32)?.try_into().ok()?;
    cursor += 32;
    let count = take_u32(bytes, &mut cursor)? as usize;
    if protocol != WIRE_PROTOCOL_VERSION
        || extraction != EXTRACTION_SEMANTICS_VERSION
        || embedding != EMBEDDING_SEMANTICS_VERSION
        || dimensions != MODEL_DIMENSIONS
        || parameters != parameter_digest()
        || count > max_records()
        || bytes.len() != HEADER_BYTES.checked_add(count.checked_mul(record_bytes())?)?
    {
        return None;
    }
    let mut entries = FxHashMap::default();
    for _ in 0..count {
        let digest = SimilarCodeSourceDigest::new(bytes.get(cursor..cursor + 32)?.try_into().ok()?);
        cursor += 32;
        let flags = *bytes.get(cursor)?;
        cursor += 1;
        if flags & !TOKEN_TRUNCATED_FLAG != 0 {
            return None;
        }
        let mut values = Vec::with_capacity(MODEL_DIMENSIONS);
        for _ in 0..MODEL_DIMENSIONS {
            let value = f32::from_le_bytes(bytes.get(cursor..cursor + 4)?.try_into().ok()?);
            cursor += 4;
            if !value.is_finite() {
                return None;
            }
            values.push(value);
        }
        if values.iter().all(|value| *value == 0.0) {
            return None;
        }
        let entry = CachedVector {
            values,
            token_truncated: flags & TOKEN_TRUNCATED_FLAG != 0,
        };
        if entries.insert(digest, entry).is_some() {
            return None;
        }
    }
    Some(entries)
}

fn take_u32(bytes: &[u8], cursor: &mut usize) -> Option<u32> {
    let value = u32::from_le_bytes(bytes.get(*cursor..*cursor + 4)?.try_into().ok()?);
    *cursor += 4;
    Some(value)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(
        String::with_capacity(bytes.len().saturating_mul(2)),
        |mut output, byte| {
            use std::fmt::Write as _;
            let _ = write!(output, "{byte:02x}");
            output
        },
    )
}

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    reason = "test fixture construction must fail immediately"
)]
mod tests {
    use super::*;

    struct Fixture {
        _temp: tempfile::TempDir,
        provider_cache_dir: PathBuf,
        project_root: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let temp = tempfile::tempdir().unwrap();
            let cache_root = temp.path().join("user-cache");
            let provider_cache_dir = cache_root.join("models").join(MODEL_REVISION);
            let project_root = temp.path().join("project");
            std::fs::create_dir_all(&cache_root).unwrap();
            std::fs::create_dir_all(&project_root).unwrap();
            Self {
                _temp: temp,
                provider_cache_dir,
                project_root,
            }
        }

        fn load(&self, disabled: bool) -> VectorCache {
            VectorCache::load(&self.provider_cache_dir, &self.project_root, disabled)
        }

        fn location(&self) -> CacheLocation {
            trusted_cache_location(&self.provider_cache_dir, &self.project_root).unwrap()
        }
    }

    fn vector(value: f32) -> Vec<f32> {
        vec![value; MODEL_DIMENSIONS]
    }

    fn entry(value: f32, token_truncated: bool) -> CachedVector {
        CachedVector {
            values: vector(value),
            token_truncated,
        }
    }

    fn entries(rows: &[(u8, f32)]) -> FxHashMap<SimilarCodeSourceDigest, CachedVector> {
        rows.iter()
            .map(|(digest, value)| {
                (
                    SimilarCodeSourceDigest::new([*digest; 32]),
                    entry(*value, false),
                )
            })
            .collect()
    }

    #[test]
    fn round_trip_uses_full_digest_and_versioned_parameters() {
        let digest = SimilarCodeSourceDigest::new([7; 32]);
        let mut values = FxHashMap::default();
        values.insert(digest, entry(0.25, true));
        let decoded = decode_cache(&encode_cache(&values, &FxHashSet::default())).unwrap();
        assert_eq!(decoded[&digest], entry(0.25, true));

        let mut drifted = encode_cache(&values, &FxHashSet::default());
        drifted[16] ^= 1;
        assert!(decode_cache(&drifted).is_none());
    }

    #[test]
    fn zero_magnitude_cache_vectors_are_rejected() {
        let digest = SimilarCodeSourceDigest::new([8; 32]);
        let mut values = FxHashMap::default();
        values.insert(digest, entry(0.0, false));

        assert!(decode_cache(&encode_cache(&values, &FxHashSet::default())).is_none());
    }

    #[test]
    fn repository_local_cache_bytes_are_never_loaded() {
        let fixture = Fixture::new();
        let digest = SimilarCodeSourceDigest::new([9; 32]);
        let project_cache = fixture
            .project_root
            .join(".fallow/similar-code/v1/vectors.bin");
        std::fs::create_dir_all(project_cache.parent().unwrap()).unwrap();
        let mut values = FxHashMap::default();
        values.insert(digest, entry(0.5, false));
        std::fs::write(&project_cache, encode_cache(&values, &FxHashSet::default())).unwrap();

        let mut cache = fixture.load(false);
        assert_eq!(cache.load_state, CacheLoadState::Missing);
        assert!(cache.get(&digest).is_none());
        assert!(project_cache.exists());
    }

    #[test]
    fn project_overlapping_cache_disables_persistence_without_disabling_vectors() {
        let fixture = Fixture::new();
        let project_cache = fixture.project_root.join("cache");
        std::fs::create_dir_all(&project_cache).unwrap();
        let provider_cache_dir = project_cache.join("models").join(MODEL_REVISION);
        let mut cache = VectorCache::load(&provider_cache_dir, &fixture.project_root, false);
        assert_eq!(cache.load_state, CacheLoadState::Disabled);
        assert!(!cache.insert(SimilarCodeSourceDigest::new([1; 32]), vector(0.5), false));
        assert!(cache.save().problem.is_some());
    }

    #[cfg(unix)]
    #[test]
    fn project_namespace_preserves_non_utf8_path_identity() {
        use std::os::unix::ffi::OsStringExt as _;

        let left = PathBuf::from(std::ffi::OsString::from_vec(vec![b'/', 0xff]));
        let right = PathBuf::from(std::ffi::OsString::from_vec(vec![b'/', 0xfe]));
        assert_ne!(project_namespace(&left), project_namespace(&right));
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_user_cache_root_disables_persistence() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let actual = temp.path().join("actual-cache");
        let linked = temp.path().join("linked-cache");
        let project_root = temp.path().join("project");
        std::fs::create_dir_all(&actual).unwrap();
        std::fs::create_dir_all(&project_root).unwrap();
        symlink(&actual, &linked).unwrap();
        let provider_cache_dir = linked.join("models").join(MODEL_REVISION);

        let cache = VectorCache::load(&provider_cache_dir, &project_root, false);
        assert_eq!(cache.load_state, CacheLoadState::Disabled);
        assert!(
            cache
                .disabled_problem
                .unwrap()
                .contains("not a trusted directory")
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_cache_leaf_is_preserved_and_persistence_fails_closed() {
        use std::os::unix::fs::symlink;

        let fixture = Fixture::new();
        let location = fixture.location();
        ensure_secure_parent(&location).unwrap();
        let target = fixture.project_root.join("cache-symlink-target");
        std::fs::write(&target, b"must remain unchanged").unwrap();

        let mut cache = fixture.load(false);
        assert!(cache.insert(SimilarCodeSourceDigest::new([6; 32]), vector(0.5), false));
        symlink(&target, &location.path).unwrap();

        let outcome = cache.save();
        assert_eq!(outcome.durable_writes, 0);
        assert!(outcome.problem.unwrap().contains("not a regular file"));
        assert!(
            atomic_replace_cache_no_follow(&location.path, b"replacement")
                .unwrap_err()
                .contains("not a regular file")
        );
        assert_eq!(std::fs::read(&target).unwrap(), b"must remain unchanged");
        assert!(
            std::fs::symlink_metadata(&location.path)
                .unwrap()
                .file_type()
                .is_symlink()
        );

        let mut disabled = fixture.load(false);
        assert_eq!(disabled.load_state, CacheLoadState::Disabled);
        assert!(
            disabled
                .save()
                .problem
                .unwrap()
                .contains("persistence was disabled")
        );
    }

    #[test]
    fn save_rereads_and_merges_a_concurrent_writer() {
        let fixture = Fixture::new();
        let first = SimilarCodeSourceDigest::new([1; 32]);
        let second = SimilarCodeSourceDigest::new([2; 32]);
        let mut left = fixture.load(false);
        let mut right = fixture.load(false);
        assert!(left.insert(first, vector(0.25), true));
        assert!(right.insert(second, vector(0.5), false));
        assert_eq!(left.save().durable_writes, 1);
        assert_eq!(right.save().durable_writes, 1);

        let mut merged = fixture.load(false);
        assert_eq!(merged.get(&first), Some(&entry(0.25, true)));
        assert_eq!(merged.get(&second), Some(&entry(0.5, false)));
    }

    #[test]
    fn active_entries_win_deterministic_saturation() {
        let values = entries(&[(1, 0.1), (2, 0.2), (3, 0.3), (4, 0.4)]);
        let active = [
            SimilarCodeSourceDigest::new([3; 32]),
            SimilarCodeSourceDigest::new([4; 32]),
        ]
        .into_iter()
        .collect();
        let selected = select_entries(&values, &active, 3);
        let digests = selected
            .iter()
            .map(|(digest, _)| digest.as_bytes()[0])
            .collect::<Vec<_>>();
        assert_eq!(digests, vec![3, 4, 1]);
    }

    #[test]
    fn lock_contention_reports_no_durable_writes() {
        let fixture = Fixture::new();
        let location = fixture.location();
        ensure_secure_parent(&location).unwrap();
        let held = try_cache_lock(&location.lock_path).unwrap().unwrap();
        let mut cache = fixture.load(false);
        assert!(cache.insert(SimilarCodeSourceDigest::new([4; 32]), vector(0.25), false));
        let outcome = cache.save();
        assert_eq!(outcome.durable_writes, 0);
        assert!(outcome.problem.unwrap().contains("contended"));
        drop(held);
        assert_eq!(cache.save().durable_writes, 1);
    }

    #[test]
    fn corrupt_cache_is_rewritten_without_claiming_vector_writes() {
        let fixture = Fixture::new();
        let location = fixture.location();
        ensure_secure_parent(&location).unwrap();
        std::fs::write(&location.path, b"invalid cache").unwrap();

        let mut cache = fixture.load(false);
        assert_eq!(cache.load_state, CacheLoadState::Corrupt);
        assert_eq!(cache.save().durable_writes, 0);
        assert!(matches!(read_cache(&location.path), CacheRead::Hit(_)));
    }

    #[test]
    fn save_releases_the_advisory_lock_before_returning() {
        let fixture = Fixture::new();
        let mut cache = fixture.load(false);
        assert!(cache.insert(SimilarCodeSourceDigest::new([10; 32]), vector(0.5), false));
        assert_eq!(cache.save().durable_writes, 1);

        let location = fixture.location();
        let reacquired = try_cache_lock(&location.lock_path).unwrap().unwrap();
        drop(reacquired);
    }

    #[test]
    fn clear_is_idempotent_and_keeps_the_permanent_lock() {
        let fixture = Fixture::new();
        let digest = SimilarCodeSourceDigest::new([5; 32]);
        let mut cache = fixture.load(false);
        assert!(cache.insert(digest, vector(0.5), false));
        assert_eq!(cache.save().durable_writes, 1);
        let location = fixture.location();

        assert!(clear(&fixture.provider_cache_dir, &fixture.project_root).unwrap());
        assert!(location.lock_path.exists());
        assert!(!clear(&fixture.provider_cache_dir, &fixture.project_root).unwrap());
    }
}
