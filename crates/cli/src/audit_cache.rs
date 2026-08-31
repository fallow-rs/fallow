use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use fallow_types::audit_cache::{
    AuditCacheKeyBuilder, AuditConfigFingerprint, AuditCoverageFingerprint,
};
use fallow_types::source_fingerprint::SourceFingerprint;
use rustc_hash::{FxHashMap, FxHashSet};
use xxhash_rust::xxh3::xxh3_64;

use super::{AuditKeySnapshot, AuditOptions};
use crate::base_worktree::{git_rev_parse, git_toplevel};
use crate::error::emit_error;

/// Version 8: the snapshot carries per-file branching totals, so a version-7
/// payload cannot answer the head-versus-base branching comparison. Version 7
/// rebased Istanbul coverage paths onto the base worktree (#2347), so snapshots
/// computed by the coverage-blind base pass must not be reused.
pub(super) const AUDIT_BASE_SNAPSHOT_CACHE_VERSION: u8 = 8;
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
    /// JSON-serialized `SemanticAnalysisIdentity` of the base pass, `None`
    /// when the base ran without type-aware analysis. Persisted so a warm
    /// cache hit still runs the identity compatibility check
    /// (`incompatible_fields`, not raw equality; see #2102) against the head
    /// identity: a genuinely incompatible cached base must keep degrading the
    /// new-only gate to syntactic attribution, and an identity-less cached
    /// base must not be mistaken for one that made semantic claims.
    pub(super) type_aware_identity: Option<String>,
    pub(super) type_aware_gap_signature: Vec<String>,
    /// Pre-refinement syntactic dead-code keys of the base pass, `None` when
    /// the base ran without type-aware analysis (then `dead_code` is already
    /// syntactic). Persisted so the degraded attribution path compares true
    /// syntactic sets on both sides even on a warm cache.
    pub(super) syntactic_dead_code: Option<Vec<String>>,
    pub(super) dead_code: Vec<String>,
    pub(super) health: Vec<String>,
    pub(super) styling: Vec<String>,
    pub(super) dupes: Vec<String>,
    pub(super) boundary_edges: Vec<String>,
    pub(super) cycles: Vec<String>,
    pub(super) public_api: Vec<String>,
    /// Branching totals per root-relative path, sorted by path so the encoded
    /// bytes are stable for identical input.
    pub(super) branching: Vec<CachedFileBranching>,
}

/// One file's branching totals in the cached snapshot. A named struct rather
/// than a tuple so a later field addition stays readable in the decode path.
#[derive(bitcode::Encode, bitcode::Decode)]
pub(super) struct CachedFileBranching {
    pub(super) path: String,
    pub(super) totals: fallow_types::extract::FileBranching,
}

pub(super) fn sorted_keys(keys: &FxHashSet<String>) -> Vec<String> {
    let mut keys: Vec<String> = keys.iter().cloned().collect();
    keys.sort_unstable();
    keys
}

/// Rehydrate an in-memory snapshot from its cached form. Returns `None` when
/// the persisted type-aware identity no longer deserializes (treated as a
/// cache miss so a stale payload can never masquerade as an identity-less
/// base).
pub(super) fn snapshot_from_cached(cached: CachedAuditKeySnapshot) -> Option<AuditKeySnapshot> {
    let type_aware_identity = match cached.type_aware_identity {
        Some(json) => Some(serde_json::from_str(&json).ok()?),
        None => None,
    };
    Some(AuditKeySnapshot {
        type_aware_identity,
        type_aware_gap_signature: cached.type_aware_gap_signature,
        syntactic_dead_code: cached
            .syntactic_dead_code
            .map(|keys| keys.into_iter().collect()),
        dead_code: cached.dead_code.into_iter().collect(),
        health: cached.health.into_iter().collect(),
        styling: cached.styling.into_iter().collect(),
        dupes: cached.dupes.into_iter().collect(),
        boundary_edges: cached.boundary_edges.into_iter().collect(),
        cycles: cached.cycles.into_iter().collect(),
        public_api: cached.public_api.into_iter().collect(),
        branching: cached
            .branching
            .into_iter()
            .map(|entry| (entry.path, entry.totals))
            .collect(),
    })
}

/// Build the cached form of a base snapshot. Returns `None` when the
/// type-aware identity cannot be serialized; the caller then skips the save so
/// a warm cache can never resurrect an identity-less base for a type-aware
/// run.
pub(super) fn cached_from_snapshot(
    key: &AuditBaseSnapshotCacheKey,
    snapshot: &AuditKeySnapshot,
) -> Option<CachedAuditKeySnapshot> {
    let type_aware_identity = match snapshot.type_aware_identity.as_ref() {
        Some(identity) => Some(serde_json::to_string(identity).ok()?),
        None => None,
    };
    Some(CachedAuditKeySnapshot {
        version: AUDIT_BASE_SNAPSHOT_CACHE_VERSION,
        cli_version: env!("CARGO_PKG_VERSION").to_string(),
        key_hash: key.hash,
        base_sha: key.base_sha.clone(),
        type_aware_identity,
        type_aware_gap_signature: snapshot.type_aware_gap_signature.clone(),
        syntactic_dead_code: snapshot.syntactic_dead_code.as_ref().map(sorted_keys),
        dead_code: sorted_keys(&snapshot.dead_code),
        health: sorted_keys(&snapshot.health),
        styling: sorted_keys(&snapshot.styling),
        dupes: sorted_keys(&snapshot.dupes),
        boundary_edges: sorted_keys(&snapshot.boundary_edges),
        cycles: sorted_keys(&snapshot.cycles),
        public_api: sorted_keys(&snapshot.public_api),
        branching: sorted_branching(&snapshot.branching),
    })
}

fn sorted_branching(
    branching: &FxHashMap<String, fallow_types::extract::FileBranching>,
) -> Vec<CachedFileBranching> {
    let mut rows: Vec<CachedFileBranching> = branching
        .iter()
        .map(|(path, totals)| CachedFileBranching {
            path: path.clone(),
            totals: *totals,
        })
        .collect();
    rows.sort_unstable_by(|a, b| a.path.cmp(&b.path));
    rows
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
    sweep_stale_snapshot_cache_versions(dir);
    let gitignore = dir.join(".gitignore");
    if std::fs::read_to_string(&gitignore).ok().as_deref() != Some("*\n") {
        std::fs::write(gitignore, "*\n")?;
    }
    Ok(())
}

/// Best-effort removal of lower-versioned `audit-base-v*` sibling directories.
/// A cache version bump would otherwise strand every previous payload
/// permanently: the age-based sweep governed by `audit.cacheMaxAgeDays`
/// targets the reusable worktrees, not stale snapshot cache versions.
fn sweep_stale_snapshot_cache_versions(current_dir: &Path) {
    let Some(parent) = current_dir.parent() else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(parent) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(version) = name
            .to_str()
            .and_then(|n| n.strip_prefix("audit-base-v"))
            .and_then(|v| v.parse::<u8>().ok())
        else {
            continue;
        };
        if version < AUDIT_BASE_SNAPSHOT_CACHE_VERSION {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
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
    snapshot_from_cached(cached)
}

pub(super) fn save_cached_base_snapshot(
    opts: &AuditOptions<'_>,
    key: &AuditBaseSnapshotCacheKey,
    snapshot: &AuditKeySnapshot,
) {
    let Some(cached) = cached_from_snapshot(key, snapshot) else {
        return;
    };
    let dir = audit_base_snapshot_cache_dir(opts.cache_dir);
    if ensure_audit_base_snapshot_cache_dir(&dir).is_err() {
        return;
    }
    let data = bitcode::encode(&cached);
    if data.len() > MAX_AUDIT_BASE_SNAPSHOT_CACHE_SIZE {
        // The loader rejects anything over the cap, so writing it would cost a
        // cold base pass on every later run instead of just this one.
        return;
    }
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
    let load_options = fallow_config::ConfigLoadOptions {
        allow_remote_extends: opts.allow_remote_extends,
    };
    let loaded = if let Some(path) = opts.config_path {
        let config =
            fallow_config::FallowConfig::load_with_options(path, load_options).map_err(|e| {
                emit_error(
                    &format!("failed to load config '{}': {e}", path.display()),
                    2,
                    opts.output,
                )
            })?;
        Some((config, path.clone()))
    } else {
        fallow_config::FallowConfig::find_and_load_with_options(opts.root, load_options)
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
    // Auto-detected coverage feeds the base pass too (#2347), so its content
    // must invalidate cached base snapshots exactly like explicit `--coverage`.
    let coverage_file = opts
        .coverage
        .map(Path::to_path_buf)
        .or_else(|| fallow_engine::health::scoring::auto_detect_coverage(opts.root))
        .map(|p| coverage_file_fingerprint(&p, opts.root));
    let materialized_context =
        fallow_engine::repo_refs::audit_materialized_context_fingerprint(opts.root);
    let bytes = AuditCacheKeyBuilder::new(
        AUDIT_BASE_SNAPSHOT_CACHE_VERSION,
        env!("CARGO_PKG_VERSION"),
        base_sha.clone(),
        config_file,
        normalized_changed_files(opts.root, changed_files),
        materialized_context,
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
    .styling(opts.css, opts.css_deep)
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
