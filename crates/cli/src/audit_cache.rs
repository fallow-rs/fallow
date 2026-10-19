use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, SystemTime};

use fallow_types::audit_cache::{
    AuditCacheKeyBuilder, AuditConfigFingerprint, AuditCoverageFingerprint,
};
use fallow_types::source_fingerprint::SourceFingerprint;
use rustc_hash::{FxHashMap, FxHashSet};
use xxhash_rust::xxh3::xxh3_64;

use super::keys::{ComplexityBaseline, ComplexityMetrics};
use super::{AuditKeySnapshot, AuditOptions};
use crate::base_worktree::{
    SECONDS_PER_DAY, SweepDecision, SweepDisposition, SweepEntry, SweepMode, SweepPass,
    git_rev_parse, git_toplevel,
};
use crate::error::emit_error;

/// Version 11: complexity keys hold path and function name only, and each
/// complexity entry carries its metric values (#3277). A version-10 snapshot
/// holds keys with the exceeded category and no metric values. Version 10:
/// dead-code keys are line-free canonical keys with an occurrence suffix on
/// repeated keys. A version-9 snapshot holds the old keys, some of
/// which carry lines, so the head run must not compare against it. Version 9
/// gave the per-file branching payload a field, version 8 introduced that
/// payload, and version 7 rebased Istanbul coverage paths onto the base
/// worktree (#2347), so snapshots computed by the coverage-blind base pass must
/// not be reused. Version 12 added the clone-group instance locations that
/// the `diverged-clone-sibling` rule reads. Version 13 added `--brief` to the
/// key: only a brief base pass computes the public-export set, so a version-12
/// snapshot from a run without `--brief` holds an empty set that a brief run
/// would read as "every head export is new". Version 14 added the line count
/// of each complexity entry, which the worsened-finding check compares.
pub(super) const AUDIT_BASE_SNAPSHOT_CACHE_VERSION: u8 = 14;
const MAX_AUDIT_BASE_SNAPSHOT_CACHE_SIZE: usize = 16 * 1024 * 1024;
/// Most snapshot files that one cache version directory keeps. Each key folds
/// in the base SHA and the changed files, so every new commit or edit adds a
/// file; the cap bounds the directory at this many files of at most
/// [`MAX_AUDIT_BASE_SNAPSHOT_CACHE_SIZE`] each.
pub(super) const MAX_AUDIT_BASE_SNAPSHOT_FILES: usize = 32;
const SNAPSHOT_FILE_SUFFIX: &str = ".bin";

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
    /// Base complexity findings, sorted by key and line.
    pub(super) health: Vec<CachedComplexityFinding>,
    pub(super) styling: Vec<String>,
    pub(super) dupes: Vec<String>,
    pub(super) boundary_edges: Vec<String>,
    pub(super) cycles: Vec<String>,
    pub(super) public_api: Vec<String>,
    /// Branching totals per root-relative path, sorted by path so the encoded
    /// bytes are stable for identical input.
    pub(super) branching: Vec<CachedFileBranching>,
    /// Instance locations of each base clone group, in snapshot order.
    pub(super) dupe_groups: Vec<Vec<CachedCloneSpan>>,
}

/// One clone instance location in the cached snapshot.
#[derive(bitcode::Encode, bitcode::Decode)]
pub(super) struct CachedCloneSpan {
    pub(super) path: String,
    pub(super) start_line: u32,
    pub(super) end_line: u32,
}

/// One base complexity finding in the cached snapshot.
#[derive(bitcode::Encode, bitcode::Decode)]
pub(super) struct CachedComplexityFinding {
    pub(super) key: String,
    pub(super) line: u32,
    pub(super) cyclomatic: u16,
    pub(super) cognitive: u16,
    pub(super) crap: Option<f64>,
    pub(super) line_count: u32,
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
        health: complexity_from_cached(cached.health),
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
        dupe_groups: cached
            .dupe_groups
            .into_iter()
            .map(|instances| fallow_api::diff_risk::clones::BaseCloneGroup {
                instances: instances
                    .into_iter()
                    .map(|span| fallow_api::diff_risk::clones::CloneSpan {
                        path: span.path,
                        start_line: span.start_line,
                        end_line: span.end_line,
                    })
                    .collect(),
            })
            .collect(),
        inherited_health: Vec::new(),
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
        health: sorted_complexity(&snapshot.health),
        styling: sorted_keys(&snapshot.styling),
        dupes: sorted_keys(&snapshot.dupes),
        boundary_edges: sorted_keys(&snapshot.boundary_edges),
        cycles: sorted_keys(&snapshot.cycles),
        public_api: sorted_keys(&snapshot.public_api),
        branching: sorted_branching(&snapshot.branching),
        dupe_groups: snapshot
            .dupe_groups
            .iter()
            .map(|group| {
                group
                    .instances
                    .iter()
                    .map(|span| CachedCloneSpan {
                        path: span.path.clone(),
                        start_line: span.start_line,
                        end_line: span.end_line,
                    })
                    .collect()
            })
            .collect(),
    })
}

fn sorted_complexity(baseline: &ComplexityBaseline) -> Vec<CachedComplexityFinding> {
    let mut rows: Vec<CachedComplexityFinding> = baseline
        .iter()
        .flat_map(|(key, findings)| {
            findings.iter().map(|metrics| CachedComplexityFinding {
                key: key.clone(),
                line: metrics.line,
                cyclomatic: metrics.cyclomatic,
                cognitive: metrics.cognitive,
                crap: metrics.crap,
                line_count: metrics.line_count,
            })
        })
        .collect();
    rows.sort_unstable_by(|a, b| a.key.cmp(&b.key).then(a.line.cmp(&b.line)));
    rows
}

fn complexity_from_cached(rows: Vec<CachedComplexityFinding>) -> ComplexityBaseline {
    let mut baseline = ComplexityBaseline::default();
    for row in rows {
        baseline
            .entry(row.key)
            .or_default()
            .push(ComplexityMetrics {
                line: row.line,
                cyclomatic: row.cyclomatic,
                cognitive: row.cognitive,
                crap: row.crap,
                line_count: row.line_count,
            });
    }
    baseline
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
    audit_base_snapshot_cache_dir(cache_dir)
        .join(format!("{:016x}{SNAPSHOT_FILE_SUFFIX}", key.hash))
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
    let data = std::fs::read(&path).ok()?;
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
    let snapshot = snapshot_from_cached(cached)?;
    touch_snapshot_file(&path);
    Some(snapshot)
}

/// Mark a snapshot file as used now, so the age-based sweep measures the age
/// from the last reuse and not from the first write. Best effort.
fn touch_snapshot_file(path: &Path) {
    if let Ok(file) = std::fs::File::options().write(true).open(path) {
        let _ = file.set_modified(SystemTime::now());
    }
}

/// One snapshot file of the current cache version directory, with the
/// metadata the sweep sorts and reports on.
struct SnapshotFile {
    path: PathBuf,
    modified: SystemTime,
    len: u64,
}

/// Whether `name` has the exact shape that [`audit_base_snapshot_cache_file`]
/// writes: 16 lowercase hex digits and the snapshot suffix.
///
/// The sweep deletes files, so it matches only names that it wrote itself.
fn is_snapshot_file_name(name: &str) -> bool {
    name.strip_suffix(SNAPSHOT_FILE_SUFFIX).is_some_and(|stem| {
        stem.len() == 16
            && stem
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

fn list_snapshot_files(dir: &Path) -> Vec<SnapshotFile> {
    // A symlinked version directory can point outside the cache. The sweep
    // deletes files, so it does not follow the link.
    if std::fs::symlink_metadata(dir).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Vec::new();
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<SnapshotFile> = entries
        .flatten()
        .filter(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(is_snapshot_file_name)
        })
        .filter_map(|entry| {
            let metadata = entry.metadata().ok()?;
            if !metadata.is_file() {
                return None;
            }
            Some(SnapshotFile {
                path: entry.path(),
                modified: metadata.modified().ok()?,
                len: metadata.len(),
            })
        })
        .collect();
    // Newest first. The path breaks ties so the order is stable.
    files.sort_by(|a, b| b.modified.cmp(&a.modified).then(a.path.cmp(&b.path)));
    files
}

fn snapshot_disposition(rank: usize, age: Duration, max_age: Option<Duration>) -> SweepDisposition {
    if max_age.is_some_and(|max_age| age >= max_age) {
        return SweepDisposition::ReclaimedAged;
    }
    if rank >= MAX_AUDIT_BASE_SNAPSHOT_FILES {
        return SweepDisposition::ReclaimedOverCapacity;
    }
    if max_age.is_none() {
        SweepDisposition::KeptAgeGcDisabled
    } else {
        SweepDisposition::KeptFresh
    }
}

/// Bound the base-snapshot files of the current cache version.
///
/// A file is removed when it is older than `max_age` (the
/// `audit.cacheMaxAgeDays` threshold, measured from the last write or reuse)
/// or when it is not among the newest [`MAX_AUDIT_BASE_SNAPSHOT_FILES`]
/// files. `fallow audit` runs this in [`SweepMode::Apply`] after each save,
/// and `fallow audit-cache prune` runs it in both modes and reports the
/// entries.
pub fn sweep_base_snapshot_cache(
    cache_dir: &Path,
    max_age: Option<Duration>,
    mode: SweepMode,
    now: SystemTime,
) -> Vec<SweepEntry> {
    let dir = audit_base_snapshot_cache_dir(cache_dir);
    list_snapshot_files(&dir)
        .into_iter()
        .enumerate()
        .map(|(rank, file)| {
            let age = now.duration_since(file.modified).unwrap_or_default();
            let mut disposition = snapshot_disposition(rank, age, max_age);
            if mode == SweepMode::Apply
                && disposition.decision() == SweepDecision::Removed
                && let Err(error) = std::fs::remove_file(&file.path)
                && error.kind() != std::io::ErrorKind::NotFound
            {
                disposition = SweepDisposition::RemoveFailed;
            }
            SweepEntry {
                path: file.path,
                pass: SweepPass::Snapshot,
                disposition,
                age_days: Some(age.as_secs() / SECONDS_PER_DAY),
                owner_root: None,
                size_bytes: Some(file.len),
            }
        })
        .collect()
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
        // The loader rejects anything over the cap, so the file could only ever
        // be written and never read. Skipping the write avoids leaving an
        // unreadable payload on disk; the cold base pass happens either way.
        return;
    }
    let Ok(mut tmp) = tempfile::NamedTempFile::new_in(&dir) else {
        return;
    };
    if tmp.write_all(&data).is_err() {
        return;
    }
    if tmp
        .persist(audit_base_snapshot_cache_file(opts.cache_dir, key))
        .is_err()
    {
        return;
    }
    let max_age = crate::base_worktree::resolve_cache_max_age_with_options(
        opts.root,
        opts.config_path.as_ref(),
        opts.allow_remote_extends,
    );
    sweep_base_snapshot_cache(opts.cache_dir, max_age, SweepMode::Apply, SystemTime::now());
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

/// The `workspaces.patterns` of the audit config.
///
/// A config that does not load gives no patterns. The head analysis loads the
/// same config and reports the error, so this function stays silent.
pub(super) fn audit_workspace_patterns(opts: &AuditOptions<'_>) -> Vec<String> {
    let load_options = fallow_config::ConfigLoadOptions {
        allow_remote_extends: opts.allow_remote_extends,
    };
    let config = match opts.config_path {
        Some(path) => fallow_config::FallowConfig::load_with_options(path, load_options).ok(),
        None => fallow_config::FallowConfig::find_and_load_with_options(opts.root, load_options)
            .ok()
            .flatten()
            .map(|(config, _)| config),
    };
    config
        .and_then(|config| config.workspaces)
        .map(|workspaces| workspaces.patterns)
        .unwrap_or_default()
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

/// `workspace_patterns` must be the patterns that the base checkout gets, so
/// the key covers each context directory that the checkout links.
pub(super) fn audit_base_snapshot_cache_key(
    opts: &AuditOptions<'_>,
    base_ref: &str,
    changed_files: &FxHashSet<PathBuf>,
    workspace_patterns: &[String],
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
    let materialized_context = fallow_engine::repo_refs::audit_materialized_context_fingerprint(
        opts.root,
        workspace_patterns,
    );
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
    .show_cascade(opts.show_cascade)
    // Only a brief base pass computes the public-export set of the snapshot,
    // so a non-brief snapshot must never serve a brief run.
    .brief(opts.brief)
    .health(
        opts.max_crap,
        coverage_file,
        opts.coverage_root.map(|p| p.to_string_lossy().to_string()),
    )
    .analyses(
        opts.analyses.dead_code,
        opts.analyses.health,
        opts.analyses.dupes,
        opts.analyses.architecture_label(),
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
