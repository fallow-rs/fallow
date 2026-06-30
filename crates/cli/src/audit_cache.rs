use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use fallow_types::audit_cache::{
    AuditCacheKeyBuilder, AuditConfigFingerprint, AuditCoverageFingerprint,
};
use fallow_types::source_fingerprint::SourceFingerprint;
use rustc_hash::FxHashSet;
use xxhash_rust::xxh3::xxh3_64;

use super::{AuditKeySnapshot, AuditOptions};
use crate::base_worktree::{git_rev_parse, git_toplevel};
use crate::error::emit_error;

pub(super) const AUDIT_BASE_SNAPSHOT_CACHE_VERSION: u8 = 3;
const MAX_AUDIT_BASE_SNAPSHOT_CACHE_SIZE: usize = 16 * 1024 * 1024;

pub(super) struct AuditBaseSnapshotCacheKey {
    pub(super) hash: u64,
    pub(super) base_sha: String,
}

#[derive(bitcode::Encode, bitcode::Decode)]
pub(super) struct CachedAuditKeySnapshot {
    pub(super) version: u8,
    pub(super) cli_version: String,
    pub(super) key_hash: u64,
    pub(super) base_sha: String,
    pub(super) dead_code: Vec<String>,
    pub(super) health: Vec<String>,
    pub(super) dupes: Vec<String>,
    pub(super) boundary_edges: Vec<String>,
    pub(super) cycles: Vec<String>,
    pub(super) public_api: Vec<String>,
}

pub(super) fn sorted_keys(keys: &FxHashSet<String>) -> Vec<String> {
    let mut keys: Vec<String> = keys.iter().cloned().collect();
    keys.sort_unstable();
    keys
}

pub(super) fn snapshot_from_cached(cached: CachedAuditKeySnapshot) -> AuditKeySnapshot {
    AuditKeySnapshot {
        dead_code: cached.dead_code.into_iter().collect(),
        health: cached.health.into_iter().collect(),
        dupes: cached.dupes.into_iter().collect(),
        boundary_edges: cached.boundary_edges.into_iter().collect(),
        cycles: cached.cycles.into_iter().collect(),
        public_api: cached.public_api.into_iter().collect(),
    }
}

pub(super) fn cached_from_snapshot(
    key: &AuditBaseSnapshotCacheKey,
    snapshot: &AuditKeySnapshot,
) -> CachedAuditKeySnapshot {
    CachedAuditKeySnapshot {
        version: AUDIT_BASE_SNAPSHOT_CACHE_VERSION,
        cli_version: env!("CARGO_PKG_VERSION").to_string(),
        key_hash: key.hash,
        base_sha: key.base_sha.clone(),
        dead_code: sorted_keys(&snapshot.dead_code),
        health: sorted_keys(&snapshot.health),
        dupes: sorted_keys(&snapshot.dupes),
        boundary_edges: sorted_keys(&snapshot.boundary_edges),
        cycles: sorted_keys(&snapshot.cycles),
        public_api: sorted_keys(&snapshot.public_api),
    }
}

pub(super) fn audit_base_snapshot_cache_dir(cache_dir: &Path) -> PathBuf {
    cache_dir
        .join("cache")
        .join(format!("audit-base-v{AUDIT_BASE_SNAPSHOT_CACHE_VERSION}"))
}

pub(super) fn audit_base_snapshot_cache_file(
    cache_dir: &Path,
    key: &AuditBaseSnapshotCacheKey,
) -> PathBuf {
    audit_base_snapshot_cache_dir(cache_dir).join(format!("{:016x}.bin", key.hash))
}

pub(super) fn ensure_audit_base_snapshot_cache_dir(dir: &Path) -> Result<(), std::io::Error> {
    std::fs::create_dir_all(dir)?;
    let gitignore = dir.join(".gitignore");
    if std::fs::read_to_string(&gitignore).ok().as_deref() != Some("*\n") {
        std::fs::write(gitignore, "*\n")?;
    }
    Ok(())
}

pub(super) fn load_cached_base_snapshot(
    opts: &AuditOptions<'_>,
    key: &AuditBaseSnapshotCacheKey,
) -> Option<AuditKeySnapshot> {
    let path = audit_base_snapshot_cache_file(opts.cache_dir, key);
    let data = std::fs::read(path).ok()?;
    if data.len() > MAX_AUDIT_BASE_SNAPSHOT_CACHE_SIZE {
        return None;
    }
    let cached: CachedAuditKeySnapshot = bitcode::decode(&data).ok()?;
    if cached.version != AUDIT_BASE_SNAPSHOT_CACHE_VERSION
        || cached.cli_version != env!("CARGO_PKG_VERSION")
        || cached.key_hash != key.hash
        || cached.base_sha != key.base_sha
    {
        return None;
    }
    Some(snapshot_from_cached(cached))
}

pub(super) fn save_cached_base_snapshot(
    opts: &AuditOptions<'_>,
    key: &AuditBaseSnapshotCacheKey,
    snapshot: &AuditKeySnapshot,
) {
    let dir = audit_base_snapshot_cache_dir(opts.cache_dir);
    if ensure_audit_base_snapshot_cache_dir(&dir).is_err() {
        return;
    }
    let data = bitcode::encode(&cached_from_snapshot(key, snapshot));
    let Ok(mut tmp) = tempfile::NamedTempFile::new_in(&dir) else {
        return;
    };
    if tmp.write_all(&data).is_err() {
        return;
    }
    let _ = tmp.persist(audit_base_snapshot_cache_file(opts.cache_dir, key));
}

fn normalized_changed_files(root: &Path, changed_files: &FxHashSet<PathBuf>) -> Vec<String> {
    let git_root = git_toplevel(root);
    let mut files: Vec<String> = changed_files
        .iter()
        .map(|path| {
            git_root
                .as_ref()
                .and_then(|root| path.strip_prefix(root).ok())
                .unwrap_or(path)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();
    files.sort_unstable();
    files
}

pub(super) fn config_file_fingerprint(
    opts: &AuditOptions<'_>,
) -> Result<AuditConfigFingerprint, ExitCode> {
    let loaded = if let Some(path) = opts.config_path {
        let config = fallow_config::FallowConfig::load(path).map_err(|e| {
            emit_error(
                &format!("failed to load config '{}': {e}", path.display()),
                2,
                opts.output,
            )
        })?;
        Some((config, path.clone()))
    } else {
        fallow_config::FallowConfig::find_and_load(opts.root)
            .map_err(|e| emit_error(&e, 2, opts.output))?
    };

    let Some((config, path)) = loaded else {
        return Ok(AuditConfigFingerprint {
            path: None,
            resolved_hash: None,
        });
    };
    let bytes = serde_json::to_vec(&config).map_err(|e| {
        emit_error(
            &format!("failed to serialize resolved config for audit cache key: {e}"),
            2,
            opts.output,
        )
    })?;
    Ok(AuditConfigFingerprint {
        path: Some(path.to_string_lossy().to_string()),
        resolved_hash: Some(format!("{:016x}", xxh3_64(&bytes))),
    })
}

fn coverage_file_fingerprint(path: &Path, project_root: &Path) -> AuditCoverageFingerprint {
    let resolved = crate::health::scoring::resolve_relative_to_root(path, Some(project_root));
    let file_path = if resolved.is_dir() {
        resolved.join("coverage-final.json")
    } else {
        resolved
    };
    match std::fs::read(&file_path) {
        Ok(bytes) => {
            let source = std::fs::metadata(&file_path)
                .ok()
                .map(|metadata| SourceFingerprint::from_metadata(&metadata));
            AuditCoverageFingerprint {
                path: path.to_string_lossy().to_string(),
                resolved_path: file_path.to_string_lossy().to_string(),
                source,
                content_hash: Some(format!("{:016x}", xxh3_64(&bytes))),
                len: Some(bytes.len()),
                error: None,
            }
        }
        Err(err) => AuditCoverageFingerprint {
            path: path.to_string_lossy().to_string(),
            resolved_path: file_path.to_string_lossy().to_string(),
            source: None,
            content_hash: None,
            len: None,
            error: Some(err.kind().to_string()),
        },
    }
}

pub(super) fn audit_base_snapshot_cache_key(
    opts: &AuditOptions<'_>,
    base_ref: &str,
    changed_files: &FxHashSet<PathBuf>,
) -> Result<Option<AuditBaseSnapshotCacheKey>, ExitCode> {
    if opts.no_cache {
        return Ok(None);
    }
    let Some(base_sha) = git_rev_parse(opts.root, base_ref) else {
        return Ok(None);
    };
    let config_file = config_file_fingerprint(opts)?;
    let coverage_file = opts
        .coverage
        .map(|p| coverage_file_fingerprint(p, opts.root));
    let bytes = AuditCacheKeyBuilder::new(
        AUDIT_BASE_SNAPSHOT_CACHE_VERSION,
        env!("CARGO_PKG_VERSION"),
        base_sha.clone(),
        config_file,
        normalized_changed_files(opts.root, changed_files),
    )
    .production(
        opts.production,
        opts.production_dead_code,
        opts.production_health,
        opts.production_dupes,
    )
    .scope(
        opts.workspace.map(<[String]>::to_vec),
        opts.changed_workspaces.map(str::to_string),
        opts.group_by.map(|g| format!("{g:?}")),
        opts.include_entry_exports,
    )
    .health(
        opts.max_crap,
        coverage_file,
        opts.coverage_root.map(|p| p.to_string_lossy().to_string()),
    )
    .baselines(
        opts.dead_code_baseline
            .map(|p| p.to_string_lossy().to_string()),
        opts.health_baseline
            .map(|p| p.to_string_lossy().to_string()),
        opts.dupes_baseline.map(|p| p.to_string_lossy().to_string()),
    )
    .to_json_bytes()
    .map_err(|e| {
        emit_error(
            &format!("failed to build audit cache key: {e}"),
            2,
            opts.output,
        )
    })?;
    Ok(Some(AuditBaseSnapshotCacheKey {
        hash: xxh3_64(&bytes),
        base_sha,
    }))
}
