use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::{Duration, Instant};

use colored::Colorize;
use fallow_config::{AuditGate, OutputFormat};
use rustc_hash::FxHashSet;
use xxhash_rust::xxh3::xxh3_64;

use crate::check::{CheckOptions, CheckResult, IssueFilters, TraceOptions};
use crate::dupes::{DupesMode, DupesOptions, DupesResult};
use crate::error::emit_error;
use crate::health::{HealthOptions, HealthResult, SortBy};
use crate::report;
use crate::report::plural;

// ── Types ────────────────────────────────────────────────────────

const AUDIT_BASE_SNAPSHOT_CACHE_VERSION: u8 = 1;
const MAX_AUDIT_BASE_SNAPSHOT_CACHE_SIZE: usize = 16 * 1024 * 1024;

/// Verdict for the audit command.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditVerdict {
    /// No issues in changed files.
    Pass,
    /// Issues found, but all are warn-severity.
    Warn,
    /// Error-severity issues found in changed files.
    Fail,
}

/// Per-category summary counts for the audit result.
#[derive(Debug, serde::Serialize)]
pub struct AuditSummary {
    pub dead_code_issues: usize,
    pub dead_code_has_errors: bool,
    pub complexity_findings: usize,
    pub max_cyclomatic: Option<u16>,
    pub duplication_clone_groups: usize,
}

/// New-vs-inherited issue counts for audit.
#[derive(Debug, Default, serde::Serialize)]
pub struct AuditAttribution {
    pub gate: AuditGate,
    pub dead_code_introduced: usize,
    pub dead_code_inherited: usize,
    pub complexity_introduced: usize,
    pub complexity_inherited: usize,
    pub duplication_introduced: usize,
    pub duplication_inherited: usize,
}

/// Full audit result containing verdict, summary, and sub-results.
pub struct AuditResult {
    pub verdict: AuditVerdict,
    pub summary: AuditSummary,
    pub attribution: AuditAttribution,
    base_snapshot: Option<AuditKeySnapshot>,
    pub base_snapshot_skipped: bool,
    pub changed_files_count: usize,
    pub base_ref: String,
    pub head_sha: Option<String>,
    pub output: OutputFormat,
    pub performance: bool,
    pub check: Option<CheckResult>,
    pub dupes: Option<DupesResult>,
    pub health: Option<HealthResult>,
    pub elapsed: Duration,
}

pub struct AuditOptions<'a> {
    pub root: &'a std::path::Path,
    pub config_path: &'a Option<std::path::PathBuf>,
    pub output: OutputFormat,
    pub no_cache: bool,
    pub threads: usize,
    pub quiet: bool,
    pub changed_since: Option<&'a str>,
    pub production: bool,
    pub production_dead_code: Option<bool>,
    pub production_health: Option<bool>,
    pub production_dupes: Option<bool>,
    pub workspace: Option<&'a [String]>,
    pub changed_workspaces: Option<&'a str>,
    pub explain: bool,
    pub explain_skipped: bool,
    pub performance: bool,
    pub group_by: Option<crate::GroupBy>,
    /// Baseline file for dead-code analysis (as produced by `fallow dead-code --save-baseline`).
    pub dead_code_baseline: Option<&'a std::path::Path>,
    /// Baseline file for health analysis (as produced by `fallow health --save-baseline`).
    pub health_baseline: Option<&'a std::path::Path>,
    /// Baseline file for duplication analysis (as produced by `fallow dupes --save-baseline`).
    pub dupes_baseline: Option<&'a std::path::Path>,
    /// Maximum CRAP score threshold (overrides `health.maxCrap` from config).
    /// Functions meeting or exceeding this score cause audit to fail.
    pub max_crap: Option<f64>,
    pub gate: AuditGate,
    /// Report unused exports in entry files (forwarded to the dead-code sub-pass).
    pub include_entry_exports: bool,
}

// ── Auto-detect base branch ──────────────────────────────────────

/// Try to determine the default branch for the repository.
/// Priority: `git symbolic-ref refs/remotes/origin/HEAD` → `main` → `master`.
/// Returns `None` if none of these exist.
fn auto_detect_base_branch(root: &std::path::Path) -> Option<String> {
    // Try symbolic-ref first (works when origin HEAD is set)
    if let Ok(output) = std::process::Command::new("git")
        .args(["symbolic-ref", "refs/remotes/origin/HEAD"])
        .current_dir(root)
        .output()
        && output.status.success()
    {
        let full_ref = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if let Some(branch) = full_ref.strip_prefix("refs/remotes/origin/") {
            return Some(branch.to_string());
        }
    }

    // Try main
    if let Ok(output) = std::process::Command::new("git")
        .args(["rev-parse", "--verify", "main"])
        .current_dir(root)
        .output()
        && output.status.success()
    {
        return Some("main".to_string());
    }

    // Try master
    if let Ok(output) = std::process::Command::new("git")
        .args(["rev-parse", "--verify", "master"])
        .current_dir(root)
        .output()
        && output.status.success()
    {
        return Some("master".to_string());
    }

    None
}

/// Get the short SHA of HEAD for the scope display line.
fn get_head_sha(root: &std::path::Path) -> Option<String> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .current_dir(root)
        .output()
        .ok()?;
    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        None
    }
}

// ── Verdict computation ──────────────────────────────────────────

fn compute_verdict(
    check: Option<&CheckResult>,
    dupes: Option<&DupesResult>,
    health: Option<&HealthResult>,
) -> AuditVerdict {
    let mut has_errors = false;
    let mut has_warnings = false;

    // Dead code: use rules severity
    if let Some(result) = check {
        if crate::check::has_error_severity_issues(
            &result.results,
            &result.config.rules,
            Some(&result.config),
        ) {
            has_errors = true;
        } else if result.results.total_issues() > 0 {
            has_warnings = true;
        }
    }

    // Complexity: findings that exceeded configured thresholds are always errors.
    // Health rules don't have a warn-severity concept — any finding above the
    // threshold is a quality gate failure, matching `fallow health` exit code semantics.
    if let Some(result) = health
        && !result.report.findings.is_empty()
    {
        has_errors = true;
    }

    // Duplication: clone groups are warnings (unless threshold exceeded)
    if let Some(result) = dupes
        && !result.report.clone_groups.is_empty()
    {
        if result.threshold > 0.0 && result.report.stats.duplication_percentage > result.threshold {
            has_errors = true;
        } else {
            has_warnings = true;
        }
    }

    if has_errors {
        AuditVerdict::Fail
    } else if has_warnings {
        AuditVerdict::Warn
    } else {
        AuditVerdict::Pass
    }
}

fn build_summary(
    check: Option<&CheckResult>,
    dupes: Option<&DupesResult>,
    health: Option<&HealthResult>,
) -> AuditSummary {
    let dead_code_issues = check.map_or(0, |r| r.results.total_issues());
    let dead_code_has_errors = check.is_some_and(|r| {
        crate::check::has_error_severity_issues(&r.results, &r.config.rules, Some(&r.config))
    });
    let complexity_findings = health.map_or(0, |r| r.report.findings.len());
    let max_cyclomatic = health.and_then(|r| r.report.findings.iter().map(|f| f.cyclomatic).max());
    let duplication_clone_groups = dupes.map_or(0, |r| r.report.clone_groups.len());

    AuditSummary {
        dead_code_issues,
        dead_code_has_errors,
        complexity_findings,
        max_cyclomatic,
        duplication_clone_groups,
    }
}

fn compute_audit_attribution(
    check: Option<&CheckResult>,
    dupes: Option<&DupesResult>,
    health: Option<&HealthResult>,
    base: Option<&AuditKeySnapshot>,
    gate: AuditGate,
) -> AuditAttribution {
    let dead_code = check
        .map(|r| {
            count_introduced(
                &dead_code_keys(&r.results, &r.config.root),
                base.map(|b| &b.dead_code),
            )
        })
        .unwrap_or_default();
    let complexity = health
        .map(|r| {
            count_introduced(
                &health_keys(&r.report, &r.config.root),
                base.map(|b| &b.health),
            )
        })
        .unwrap_or_default();
    let duplication = dupes
        .map(|r| {
            count_introduced(
                &dupes_keys(&r.report, &r.config.root),
                base.map(|b| &b.dupes),
            )
        })
        .unwrap_or_default();

    AuditAttribution {
        gate,
        dead_code_introduced: dead_code.0,
        dead_code_inherited: dead_code.1,
        complexity_introduced: complexity.0,
        complexity_inherited: complexity.1,
        duplication_introduced: duplication.0,
        duplication_inherited: duplication.1,
    }
}

fn compute_introduced_verdict(
    check: Option<&CheckResult>,
    dupes: Option<&DupesResult>,
    health: Option<&HealthResult>,
    base: Option<&AuditKeySnapshot>,
) -> AuditVerdict {
    let mut has_errors = false;
    let mut has_warnings = false;

    if let Some(result) = check {
        let base_keys = base.map(|b| &b.dead_code);
        let mut introduced = result.results.clone();
        retain_introduced_dead_code(&mut introduced, &result.config.root, base_keys);
        if crate::check::has_error_severity_issues(
            &introduced,
            &result.config.rules,
            Some(&result.config),
        ) {
            has_errors = true;
        } else if introduced.total_issues() > 0 {
            has_warnings = true;
        }
    }

    if let Some(result) = health {
        let base_keys = base.map(|b| &b.health);
        let introduced = result
            .report
            .findings
            .iter()
            .filter(|finding| {
                !base_keys.is_some_and(|keys| {
                    keys.contains(&health_finding_key(finding, &result.config.root))
                })
            })
            .count();
        if introduced > 0 {
            has_errors = true;
        }
    }

    if let Some(result) = dupes {
        let base_keys = base.map(|b| &b.dupes);
        let introduced = result
            .report
            .clone_groups
            .iter()
            .filter(|group| {
                !base_keys
                    .is_some_and(|keys| keys.contains(&dupe_group_key(group, &result.config.root)))
            })
            .count();
        if introduced > 0 {
            if result.threshold > 0.0
                && result.report.stats.duplication_percentage > result.threshold
            {
                has_errors = true;
            } else {
                has_warnings = true;
            }
        }
    }

    if has_errors {
        AuditVerdict::Fail
    } else if has_warnings {
        AuditVerdict::Warn
    } else {
        AuditVerdict::Pass
    }
}

struct AuditKeySnapshot {
    dead_code: FxHashSet<String>,
    health: FxHashSet<String>,
    dupes: FxHashSet<String>,
}

struct AuditBaseSnapshotCacheKey {
    hash: u64,
    base_sha: String,
}

#[derive(bitcode::Encode, bitcode::Decode)]
struct CachedAuditKeySnapshot {
    version: u8,
    cli_version: String,
    key_hash: u64,
    base_sha: String,
    dead_code: Vec<String>,
    health: Vec<String>,
    dupes: Vec<String>,
}

fn count_introduced(keys: &FxHashSet<String>, base: Option<&FxHashSet<String>>) -> (usize, usize) {
    let Some(base) = base else {
        return (0, 0);
    };
    keys.iter().fold((0, 0), |(introduced, inherited), key| {
        if base.contains(key) {
            (introduced, inherited + 1)
        } else {
            (introduced + 1, inherited)
        }
    })
}

fn sorted_keys(keys: &FxHashSet<String>) -> Vec<String> {
    let mut keys: Vec<String> = keys.iter().cloned().collect();
    keys.sort_unstable();
    keys
}

fn snapshot_from_cached(cached: CachedAuditKeySnapshot) -> AuditKeySnapshot {
    AuditKeySnapshot {
        dead_code: cached.dead_code.into_iter().collect(),
        health: cached.health.into_iter().collect(),
        dupes: cached.dupes.into_iter().collect(),
    }
}

fn cached_from_snapshot(
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
    }
}

fn audit_base_snapshot_cache_dir(root: &Path) -> PathBuf {
    root.join(".fallow")
        .join("cache")
        .join(format!("audit-base-v{AUDIT_BASE_SNAPSHOT_CACHE_VERSION}"))
}

fn audit_base_snapshot_cache_file(root: &Path, key: &AuditBaseSnapshotCacheKey) -> PathBuf {
    audit_base_snapshot_cache_dir(root).join(format!("{:016x}.bin", key.hash))
}

fn ensure_audit_base_snapshot_cache_dir(dir: &Path) -> Result<(), std::io::Error> {
    std::fs::create_dir_all(dir)?;
    let gitignore = dir.join(".gitignore");
    if std::fs::read_to_string(&gitignore).ok().as_deref() != Some("*\n") {
        std::fs::write(gitignore, "*\n")?;
    }
    Ok(())
}

fn load_cached_base_snapshot(
    opts: &AuditOptions<'_>,
    key: &AuditBaseSnapshotCacheKey,
) -> Option<AuditKeySnapshot> {
    let path = audit_base_snapshot_cache_file(opts.root, key);
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

fn save_cached_base_snapshot(
    opts: &AuditOptions<'_>,
    key: &AuditBaseSnapshotCacheKey,
    snapshot: &AuditKeySnapshot,
) {
    let dir = audit_base_snapshot_cache_dir(opts.root);
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
    let _ = tmp.persist(audit_base_snapshot_cache_file(opts.root, key));
}

fn git_rev_parse(root: &Path, rev: &str) -> Option<String> {
    let output = Command::new("git")
        .args(["rev-parse", rev])
        .current_dir(root)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
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

fn config_file_fingerprint(opts: &AuditOptions<'_>) -> Result<serde_json::Value, ExitCode> {
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
        return Ok(serde_json::json!({
            "path": null,
            "resolved_hash": null,
        }));
    };
    let bytes = serde_json::to_vec(&config).map_err(|e| {
        emit_error(
            &format!("failed to serialize resolved config for audit cache key: {e}"),
            2,
            opts.output,
        )
    })?;
    Ok(serde_json::json!({
        "path": path.to_string_lossy(),
        "resolved_hash": format!("{:016x}", xxh3_64(&bytes)),
    }))
}

fn audit_base_snapshot_cache_key(
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
    let payload = serde_json::json!({
        "cache_version": AUDIT_BASE_SNAPSHOT_CACHE_VERSION,
        "cli_version": env!("CARGO_PKG_VERSION"),
        "base_sha": base_sha,
        "config_file": config_file,
        "changed_files": normalized_changed_files(opts.root, changed_files),
        "production": opts.production,
        "production_dead_code": opts.production_dead_code,
        "production_health": opts.production_health,
        "production_dupes": opts.production_dupes,
        "workspace": opts.workspace,
        "changed_workspaces": opts.changed_workspaces,
        "group_by": opts.group_by.map(|g| format!("{g:?}")),
        "include_entry_exports": opts.include_entry_exports,
        "max_crap": opts.max_crap,
        "dead_code_baseline": opts.dead_code_baseline.map(|p| p.to_string_lossy().to_string()),
        "health_baseline": opts.health_baseline.map(|p| p.to_string_lossy().to_string()),
        "dupes_baseline": opts.dupes_baseline.map(|p| p.to_string_lossy().to_string()),
    });
    let bytes = serde_json::to_vec(&payload).map_err(|e| {
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

fn compute_base_snapshot(
    opts: &AuditOptions<'_>,
    base_ref: &str,
    changed_files: &FxHashSet<PathBuf>,
    base_sha: Option<&str>,
) -> Result<AuditKeySnapshot, ExitCode> {
    let Some(worktree) = BaseWorktree::create(opts.root, base_ref, base_sha) else {
        return Err(emit_error(
            &format!("could not create a temporary worktree for base ref '{base_ref}'"),
            2,
            opts.output,
        ));
    };
    let base_config_path = opts
        .config_path
        .as_ref()
        .filter(|path| path.is_relative())
        .map(|path| worktree.path().join(path));
    let config_path = if base_config_path.is_some() {
        &base_config_path
    } else {
        opts.config_path
    };
    let base_opts = AuditOptions {
        root: worktree.path(),
        config_path,
        output: opts.output,
        no_cache: opts.no_cache,
        threads: opts.threads,
        quiet: true,
        changed_since: None,
        production: opts.production,
        production_dead_code: opts.production_dead_code,
        production_health: opts.production_health,
        production_dupes: opts.production_dupes,
        workspace: opts.workspace,
        changed_workspaces: None,
        explain: false,
        explain_skipped: false,
        performance: false,
        group_by: opts.group_by,
        dead_code_baseline: None,
        health_baseline: None,
        dupes_baseline: None,
        max_crap: opts.max_crap,
        gate: AuditGate::All,
        include_entry_exports: opts.include_entry_exports,
    };

    let base_changed_files = remap_focus_files(changed_files, opts.root, worktree.path());
    let check_production = opts.production_dead_code.unwrap_or(opts.production);
    let health_production = opts.production_health.unwrap_or(opts.production);
    let share_dead_code_parse_with_health = check_production == health_production;

    // Base-snapshot check and dupes share no mutable state. Running them
    // concurrently keeps the expensive duplication pass overlapped with
    // dead-code analysis; health then consumes check's retained parse when the
    // production modes match, mirroring the HEAD-side audit pipeline.
    let (check_res, dupes_res) = rayon::join(
        || run_audit_check(&base_opts, None, share_dead_code_parse_with_health),
        || run_audit_dupes(&base_opts, None, base_changed_files.as_ref(), None),
    );
    let mut check = check_res?;
    let dupes = dupes_res?;
    let shared_parse = if share_dead_code_parse_with_health {
        check.as_mut().and_then(|r| r.shared_parse.take())
    } else {
        None
    };
    let health = run_audit_health(&base_opts, None, shared_parse)?;
    if let Some(ref mut check) = check {
        check.shared_parse = None;
    }

    Ok(AuditKeySnapshot {
        dead_code: check.as_ref().map_or_else(FxHashSet::default, |r| {
            dead_code_keys(&r.results, &r.config.root)
        }),
        health: health.as_ref().map_or_else(FxHashSet::default, |r| {
            health_keys(&r.report, &r.config.root)
        }),
        dupes: dupes.as_ref().map_or_else(FxHashSet::default, |r| {
            dupes_keys(&r.report, &r.config.root)
        }),
    })
}

fn current_keys_as_base_keys(
    check: Option<&CheckResult>,
    dupes: Option<&DupesResult>,
    health: Option<&HealthResult>,
) -> AuditKeySnapshot {
    AuditKeySnapshot {
        dead_code: check.as_ref().map_or_else(FxHashSet::default, |r| {
            dead_code_keys(&r.results, &r.config.root)
        }),
        health: health.as_ref().map_or_else(FxHashSet::default, |r| {
            health_keys(&r.report, &r.config.root)
        }),
        dupes: dupes.as_ref().map_or_else(FxHashSet::default, |r| {
            dupes_keys(&r.report, &r.config.root)
        }),
    }
}

fn can_reuse_current_as_base(
    opts: &AuditOptions<'_>,
    base_ref: &str,
    changed_files: &FxHashSet<PathBuf>,
) -> bool {
    let Some(git_root) = git_toplevel(opts.root) else {
        return false;
    };
    // `try_get_changed_files` joins the canonical git toplevel onto each
    // relative diff entry, so changed-file paths land canonical even when
    // `opts.root` itself was passed un-canonical (typical in tests). Match
    // against both forms so the cache-artifact check works in either case.
    let cache_dir = opts.root.join(".fallow");
    let canonical_cache_dir = cache_dir.canonicalize().ok();
    changed_files.iter().all(|path| {
        if is_fallow_cache_artifact(path, &cache_dir, canonical_cache_dir.as_deref()) {
            return true;
        }
        if !is_analysis_input(path) {
            return is_non_behavioral_doc(path);
        }
        let Ok(current) = std::fs::read_to_string(path) else {
            return false;
        };
        let Some(relative) = path.strip_prefix(&git_root).ok() else {
            return false;
        };
        let Some(base) = git_show_file(opts.root, base_ref, relative) else {
            return false;
        };
        if current == base {
            return true;
        }
        js_ts_tokens_equivalent(path, &current, &base)
    })
}

// `cache_dir` is the project-local cache root (`<opts.root>/.fallow`).
// Anything under it is a fallow internal artifact (token cache, parse cache,
// gitignore stubs) with no semantic effect on analysis, so a "changed" entry
// inside it must not block the audit-gate base-snapshot fast path. We accept
// both the as-given and the canonicalized cache_dir because changed-file
// paths from `try_get_changed_files` are joined onto the canonical git
// toplevel while `opts.root` may be un-canonical in tests.
fn is_fallow_cache_artifact(
    path: &Path,
    cache_dir: &Path,
    canonical_cache_dir: Option<&Path>,
) -> bool {
    path.starts_with(cache_dir)
        || canonical_cache_dir.is_some_and(|canonical| path.starts_with(canonical))
}

fn git_toplevel(root: &Path) -> Option<PathBuf> {
    let output = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(root)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let path = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
    Some(path.canonicalize().unwrap_or(path))
}

fn git_show_file(root: &Path, base_ref: &str, relative: &Path) -> Option<String> {
    let spec = format!(
        "{}:{}",
        base_ref,
        relative.to_string_lossy().replace('\\', "/")
    );
    let output = Command::new("git")
        .args(["show", "--end-of-options", &spec])
        .current_dir(root)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

fn is_analysis_input(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|ext| ext.to_str()),
        Some(
            "js" | "jsx"
                | "ts"
                | "tsx"
                | "mjs"
                | "mts"
                | "cjs"
                | "cts"
                | "vue"
                | "svelte"
                | "astro"
                | "mdx"
                | "css"
                | "scss"
        )
    )
}

fn is_non_behavioral_doc(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|ext| ext.to_str()),
        Some("md" | "markdown" | "txt" | "rst" | "adoc")
    )
}

fn js_ts_tokens_equivalent(path: &Path, current: &str, base: &str) -> bool {
    if current.contains("fallow-ignore") || base.contains("fallow-ignore") {
        return false;
    }
    if !matches!(
        path.extension().and_then(|ext| ext.to_str()),
        Some("js" | "jsx" | "ts" | "tsx" | "mjs" | "mts" | "cjs" | "cts")
    ) {
        return false;
    }
    let current_tokens = fallow_core::duplicates::tokenize::tokenize_file(path, current, false);
    let base_tokens = fallow_core::duplicates::tokenize::tokenize_file(path, base, false);
    current_tokens
        .tokens
        .iter()
        .map(|token| &token.kind)
        .eq(base_tokens.tokens.iter().map(|token| &token.kind))
}

// Remap focused-file paths from the current working tree into the base
// worktree, used so the duplication detector can scope clone-group
// extraction at base to the same files we focus on at HEAD.
//
// Path matching at base must align with `discover_files`, which walks
// `config.root` un-canonicalized and emits paths under that exact prefix.
// Canonicalizing here would silently shift the prefix on systems where the
// tempdir path traverses a symlink (`/tmp` → `/private/tmp`, `/var` →
// `/private/var` on macOS); the focus set would then miss every discovered
// file at base and disable the optimization. Use the prefixes as-is.
//
// `opts.root` is already canonical (from `validate_root`), and
// `changed_files` was joined onto the canonical git toplevel, so
// `strip_prefix(from_root)` succeeds for paths inside `opts.root`. Files
// outside `opts.root` (e.g., a sibling workspace touched in the same
// commit) are skipped rather than collapsing the whole set, so the focus
// optimization stays active for the in-scope subset.
fn remap_focus_files(
    files: &FxHashSet<PathBuf>,
    from_root: &Path,
    to_root: &Path,
) -> Option<FxHashSet<PathBuf>> {
    let mut remapped = FxHashSet::default();
    for file in files {
        if let Ok(relative) = file.strip_prefix(from_root) {
            remapped.insert(to_root.join(relative));
        }
    }
    if remapped.is_empty() {
        return None;
    }
    Some(remapped)
}

struct BaseWorktree {
    repo_root: PathBuf,
    path: PathBuf,
    persistent: bool,
}

impl BaseWorktree {
    fn create(repo_root: &Path, base_ref: &str, base_sha: Option<&str>) -> Option<Self> {
        sweep_orphan_audit_worktrees(repo_root);
        if let Some(base_sha) = base_sha
            && let Some(worktree) = Self::reuse_or_create(repo_root, base_sha)
        {
            return Some(worktree);
        }
        let path = std::env::temp_dir().join(format!(
            "fallow-audit-base-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .ok()?
                .as_nanos()
        ));
        let output = Command::new("git")
            .args([
                "worktree",
                "add",
                "--detach",
                "--quiet",
                path.to_str()?,
                base_ref,
            ])
            .current_dir(repo_root)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .output()
            .ok()?;
        if !output.status.success() {
            let _ = std::fs::remove_dir_all(&path);
            return None;
        }
        Some(Self {
            repo_root: repo_root.to_path_buf(),
            path,
            persistent: false,
        })
    }

    fn reuse_or_create(repo_root: &Path, base_sha: &str) -> Option<Self> {
        let path = reusable_audit_worktree_path(repo_root, base_sha);
        if reusable_audit_worktree_is_ready(repo_root, &path, base_sha) {
            return Some(Self {
                repo_root: repo_root.to_path_buf(),
                path,
                persistent: true,
            });
        }

        remove_audit_worktree(repo_root, &path);
        let _ = std::fs::remove_dir_all(&path);
        let output = Command::new("git")
            .args([
                "worktree",
                "add",
                "--detach",
                "--quiet",
                path.to_string_lossy().as_ref(),
                base_sha,
            ])
            .current_dir(repo_root)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .output()
            .ok()?;
        if !output.status.success() {
            let _ = std::fs::remove_dir_all(&path);
            return None;
        }

        Some(Self {
            repo_root: repo_root.to_path_buf(),
            path,
            persistent: true,
        })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

fn reusable_audit_worktree_path(repo_root: &Path, base_sha: &str) -> PathBuf {
    let repo_root = git_toplevel(repo_root).unwrap_or_else(|| repo_root.to_path_buf());
    let repo_root = repo_root.canonicalize().unwrap_or(repo_root);
    let repo_hash = xxh3_64(repo_root.to_string_lossy().as_bytes());
    let sha_prefix = base_sha.get(..16).unwrap_or(base_sha);
    std::env::temp_dir().join(format!(
        "fallow-audit-base-cache-{repo_hash:016x}-{sha_prefix}"
    ))
}

fn reusable_audit_worktree_is_ready(repo_root: &Path, path: &Path, base_sha: &str) -> bool {
    if !path.exists() || !audit_worktree_is_registered(repo_root, path) {
        return false;
    }
    git_rev_parse(path, "HEAD").is_some_and(|head| head == base_sha)
}

fn audit_worktree_is_registered(repo_root: &Path, path: &Path) -> bool {
    let Some(worktrees) = list_audit_worktrees(repo_root) else {
        return false;
    };
    worktrees.iter().any(|worktree| paths_equal(worktree, path))
}

fn paths_equal(left: &Path, right: &Path) -> bool {
    if left == right {
        return true;
    }
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

fn remove_audit_worktree(repo_root: &Path, path: &Path) {
    let _ = Command::new("git")
        .args([
            "worktree",
            "remove",
            "--force",
            path.to_string_lossy().as_ref(),
        ])
        .current_dir(repo_root)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .output();
}

fn sweep_orphan_audit_worktrees(repo_root: &Path) {
    let Some(worktrees) = list_audit_worktrees(repo_root) else {
        return;
    };
    let mut removed_any = false;
    for path in worktrees {
        if !is_fallow_audit_worktree_path(&path)
            || is_reusable_audit_worktree_path(&path)
            || audit_worktree_process_is_alive(&path)
        {
            continue;
        }
        remove_audit_worktree(repo_root, &path);
        let _ = std::fs::remove_dir_all(&path);
        removed_any = true;
    }
    if removed_any {
        let _ = Command::new("git")
            .args(["worktree", "prune", "--expire=now"])
            .current_dir(repo_root)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .output();
    }
}

fn list_audit_worktrees(repo_root: &Path) -> Option<Vec<PathBuf>> {
    let output = Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(repo_root)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(parse_worktree_list(&String::from_utf8_lossy(
        &output.stdout,
    )))
}

fn parse_worktree_list(output: &str) -> Vec<PathBuf> {
    output
        .lines()
        .filter_map(|line| line.strip_prefix("worktree "))
        .map(PathBuf::from)
        .filter(|path| is_fallow_audit_worktree_path(path))
        .collect()
}

fn is_fallow_audit_worktree_path(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    name.starts_with("fallow-audit-base-") && path_is_inside_temp_dir(path)
}

fn is_reusable_audit_worktree_path(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with("fallow-audit-base-cache-"))
}

fn path_is_inside_temp_dir(path: &Path) -> bool {
    let temp = std::env::temp_dir();
    if path.starts_with(&temp) {
        return true;
    }
    let Ok(canonical_temp) = temp.canonicalize() else {
        return false;
    };
    path.starts_with(&canonical_temp)
        || path
            .canonicalize()
            .is_ok_and(|canonical_path| canonical_path.starts_with(canonical_temp))
}

fn audit_worktree_process_is_alive(path: &Path) -> bool {
    let Some(pid) = path
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(audit_worktree_pid)
    else {
        return false;
    };
    process_is_alive(pid)
}

fn audit_worktree_pid(name: &str) -> Option<u32> {
    name.strip_prefix("fallow-audit-base-")?
        .split('-')
        .next()?
        .parse()
        .ok()
}

#[cfg(unix)]
fn process_is_alive(pid: u32) -> bool {
    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .output()
        .is_ok_and(|output| output.status.success())
}

#[cfg(not(unix))]
fn process_is_alive(_pid: u32) -> bool {
    true
}

impl Drop for BaseWorktree {
    fn drop(&mut self) {
        if self.persistent {
            return;
        }
        remove_audit_worktree(&self.repo_root, &self.path);
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn relative_key_path(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn dependency_location_key(location: &fallow_core::results::DependencyLocation) -> &'static str {
    match location {
        fallow_core::results::DependencyLocation::Dependencies => "unused-dependency",
        fallow_core::results::DependencyLocation::DevDependencies => "unused-dev-dependency",
        fallow_core::results::DependencyLocation::OptionalDependencies => {
            "unused-optional-dependency"
        }
    }
}

fn unused_dependency_key(item: &fallow_core::results::UnusedDependency, root: &Path) -> String {
    format!(
        "{}:{}:{}",
        dependency_location_key(&item.location),
        relative_key_path(&item.path, root),
        item.package_name
    )
}

fn unlisted_dependency_key(item: &fallow_core::results::UnlistedDependency, root: &Path) -> String {
    let mut sites = item
        .imported_from
        .iter()
        .map(|site| {
            format!(
                "{}:{}:{}",
                relative_key_path(&site.path, root),
                site.line,
                site.col
            )
        })
        .collect::<Vec<_>>();
    sites.sort();
    sites.dedup();
    format!(
        "unlisted-dependency:{}:{}",
        item.package_name,
        sites.join("|")
    )
}

fn unused_member_key(
    rule_id: &str,
    item: &fallow_core::results::UnusedMember,
    root: &Path,
) -> String {
    format!(
        "{}:{}:{}:{}",
        rule_id,
        relative_key_path(&item.path, root),
        item.parent_name,
        item.member_name
    )
}

fn dead_code_keys(
    results: &fallow_core::results::AnalysisResults,
    root: &Path,
) -> FxHashSet<String> {
    let mut keys = FxHashSet::default();
    for item in &results.unused_files {
        keys.insert(format!(
            "unused-file:{}",
            relative_key_path(&item.path, root)
        ));
    }
    for item in &results.unused_exports {
        keys.insert(format!(
            "unused-export:{}:{}",
            relative_key_path(&item.path, root),
            item.export_name
        ));
    }
    for item in &results.unused_types {
        keys.insert(format!(
            "unused-type:{}:{}",
            relative_key_path(&item.path, root),
            item.export_name
        ));
    }
    for item in &results.private_type_leaks {
        keys.insert(format!(
            "private-type-leak:{}:{}:{}",
            relative_key_path(&item.path, root),
            item.export_name,
            item.type_name
        ));
    }
    for item in results
        .unused_dependencies
        .iter()
        .chain(results.unused_dev_dependencies.iter())
        .chain(results.unused_optional_dependencies.iter())
    {
        keys.insert(unused_dependency_key(item, root));
    }
    for item in &results.unused_enum_members {
        keys.insert(unused_member_key("unused-enum-member", item, root));
    }
    for item in &results.unused_class_members {
        keys.insert(unused_member_key("unused-class-member", item, root));
    }
    for item in &results.unresolved_imports {
        keys.insert(format!(
            "unresolved-import:{}:{}",
            relative_key_path(&item.path, root),
            item.specifier
        ));
    }
    for item in &results.unlisted_dependencies {
        keys.insert(unlisted_dependency_key(item, root));
    }
    for item in &results.duplicate_exports {
        let mut locations: Vec<String> = item
            .locations
            .iter()
            .map(|loc| relative_key_path(&loc.path, root))
            .collect();
        locations.sort();
        locations.dedup();
        keys.insert(format!(
            "duplicate-export:{}:{}",
            item.export_name,
            locations.join("|")
        ));
    }
    for item in &results.type_only_dependencies {
        keys.insert(format!(
            "type-only-dependency:{}:{}",
            relative_key_path(&item.path, root),
            item.package_name
        ));
    }
    for item in &results.test_only_dependencies {
        keys.insert(format!(
            "test-only-dependency:{}:{}",
            relative_key_path(&item.path, root),
            item.package_name
        ));
    }
    for item in &results.circular_dependencies {
        let mut files: Vec<String> = item
            .files
            .iter()
            .map(|path| relative_key_path(path, root))
            .collect();
        files.sort();
        keys.insert(format!("circular-dependency:{}", files.join("|")));
    }
    for item in &results.boundary_violations {
        keys.insert(format!(
            "boundary-violation:{}:{}:{}",
            relative_key_path(&item.from_path, root),
            relative_key_path(&item.to_path, root),
            item.import_specifier
        ));
    }
    for item in &results.stale_suppressions {
        keys.insert(format!(
            "stale-suppression:{}:{}",
            relative_key_path(&item.path, root),
            item.description()
        ));
    }
    keys
}

fn retain_introduced_dead_code(
    results: &mut fallow_core::results::AnalysisResults,
    root: &Path,
    base: Option<&FxHashSet<String>>,
) {
    let Some(base) = base else {
        return;
    };
    results.unused_files.retain(|item| {
        !base.contains(&format!(
            "unused-file:{}",
            relative_key_path(&item.path, root)
        ))
    });
    results.unused_exports.retain(|item| {
        !base.contains(&format!(
            "unused-export:{}:{}",
            relative_key_path(&item.path, root),
            item.export_name
        ))
    });
    results.unused_types.retain(|item| {
        !base.contains(&format!(
            "unused-type:{}:{}",
            relative_key_path(&item.path, root),
            item.export_name
        ))
    });
    // The verdict path only needs correct issue counts and severities. For the
    // less common categories, rebuild the full key set and retain by membership.
    let introduced = dead_code_keys(results, root)
        .into_iter()
        .filter(|key| !base.contains(key))
        .collect::<FxHashSet<_>>();
    let keep = |key: String| introduced.contains(&key);
    results.private_type_leaks.retain(|item| {
        keep(format!(
            "private-type-leak:{}:{}:{}",
            relative_key_path(&item.path, root),
            item.export_name,
            item.type_name
        ))
    });
    results
        .unused_dependencies
        .retain(|item| keep(unused_dependency_key(item, root)));
    results
        .unused_dev_dependencies
        .retain(|item| keep(unused_dependency_key(item, root)));
    results
        .unused_optional_dependencies
        .retain(|item| keep(unused_dependency_key(item, root)));
    results
        .unused_enum_members
        .retain(|item| keep(unused_member_key("unused-enum-member", item, root)));
    results
        .unused_class_members
        .retain(|item| keep(unused_member_key("unused-class-member", item, root)));
    results.unresolved_imports.retain(|item| {
        keep(format!(
            "unresolved-import:{}:{}",
            relative_key_path(&item.path, root),
            item.specifier
        ))
    });
    results
        .unlisted_dependencies
        .retain(|item| keep(unlisted_dependency_key(item, root)));
    results.duplicate_exports.retain(|item| {
        let mut locations: Vec<String> = item
            .locations
            .iter()
            .map(|loc| relative_key_path(&loc.path, root))
            .collect();
        locations.sort();
        locations.dedup();
        keep(format!(
            "duplicate-export:{}:{}",
            item.export_name,
            locations.join("|")
        ))
    });
    results.type_only_dependencies.retain(|item| {
        keep(format!(
            "type-only-dependency:{}:{}",
            relative_key_path(&item.path, root),
            item.package_name
        ))
    });
    results.test_only_dependencies.retain(|item| {
        keep(format!(
            "test-only-dependency:{}:{}",
            relative_key_path(&item.path, root),
            item.package_name
        ))
    });
    results.circular_dependencies.retain(|item| {
        let mut files: Vec<String> = item
            .files
            .iter()
            .map(|path| relative_key_path(path, root))
            .collect();
        files.sort();
        keep(format!("circular-dependency:{}", files.join("|")))
    });
    results.boundary_violations.retain(|item| {
        keep(format!(
            "boundary-violation:{}:{}:{}",
            relative_key_path(&item.from_path, root),
            relative_key_path(&item.to_path, root),
            item.import_specifier
        ))
    });
    results.stale_suppressions.retain(|item| {
        keep(format!(
            "stale-suppression:{}:{}",
            relative_key_path(&item.path, root),
            item.description()
        ))
    });
}

fn issue_was_introduced(key: &str, base: &FxHashSet<String>) -> bool {
    !base.contains(key)
}

fn annotate_issue_array<I>(json: &mut serde_json::Value, key: &str, introduced: I)
where
    I: IntoIterator<Item = bool>,
{
    let Some(items) = json.get_mut(key).and_then(serde_json::Value::as_array_mut) else {
        return;
    };
    for (item, introduced) in items.iter_mut().zip(introduced) {
        if let serde_json::Value::Object(map) = item {
            map.insert("introduced".to_string(), serde_json::json!(introduced));
        }
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "keeps audit attribution keys adjacent to the JSON arrays they annotate"
)]
fn annotate_dead_code_json(
    json: &mut serde_json::Value,
    results: &fallow_core::results::AnalysisResults,
    root: &Path,
    base: &FxHashSet<String>,
) {
    annotate_issue_array(
        json,
        "unused_files",
        results.unused_files.iter().map(|item| {
            issue_was_introduced(
                &format!("unused-file:{}", relative_key_path(&item.path, root)),
                base,
            )
        }),
    );
    annotate_issue_array(
        json,
        "unused_exports",
        results.unused_exports.iter().map(|item| {
            issue_was_introduced(
                &format!(
                    "unused-export:{}:{}",
                    relative_key_path(&item.path, root),
                    item.export_name
                ),
                base,
            )
        }),
    );
    annotate_issue_array(
        json,
        "unused_types",
        results.unused_types.iter().map(|item| {
            issue_was_introduced(
                &format!(
                    "unused-type:{}:{}",
                    relative_key_path(&item.path, root),
                    item.export_name
                ),
                base,
            )
        }),
    );
    annotate_issue_array(
        json,
        "private_type_leaks",
        results.private_type_leaks.iter().map(|item| {
            issue_was_introduced(
                &format!(
                    "private-type-leak:{}:{}:{}",
                    relative_key_path(&item.path, root),
                    item.export_name,
                    item.type_name
                ),
                base,
            )
        }),
    );
    annotate_issue_array(
        json,
        "unused_dependencies",
        results
            .unused_dependencies
            .iter()
            .map(|item| issue_was_introduced(&unused_dependency_key(item, root), base)),
    );
    annotate_issue_array(
        json,
        "unused_dev_dependencies",
        results
            .unused_dev_dependencies
            .iter()
            .map(|item| issue_was_introduced(&unused_dependency_key(item, root), base)),
    );
    annotate_issue_array(
        json,
        "unused_optional_dependencies",
        results
            .unused_optional_dependencies
            .iter()
            .map(|item| issue_was_introduced(&unused_dependency_key(item, root), base)),
    );
    annotate_issue_array(
        json,
        "unused_enum_members",
        results.unused_enum_members.iter().map(|item| {
            issue_was_introduced(&unused_member_key("unused-enum-member", item, root), base)
        }),
    );
    annotate_issue_array(
        json,
        "unused_class_members",
        results.unused_class_members.iter().map(|item| {
            issue_was_introduced(&unused_member_key("unused-class-member", item, root), base)
        }),
    );
    annotate_issue_array(
        json,
        "unresolved_imports",
        results.unresolved_imports.iter().map(|item| {
            issue_was_introduced(
                &format!(
                    "unresolved-import:{}:{}",
                    relative_key_path(&item.path, root),
                    item.specifier
                ),
                base,
            )
        }),
    );
    annotate_issue_array(
        json,
        "unlisted_dependencies",
        results
            .unlisted_dependencies
            .iter()
            .map(|item| issue_was_introduced(&unlisted_dependency_key(item, root), base)),
    );
    annotate_issue_array(
        json,
        "duplicate_exports",
        results.duplicate_exports.iter().map(|item| {
            let mut locations: Vec<String> = item
                .locations
                .iter()
                .map(|loc| relative_key_path(&loc.path, root))
                .collect();
            locations.sort();
            locations.dedup();
            issue_was_introduced(
                &format!(
                    "duplicate-export:{}:{}",
                    item.export_name,
                    locations.join("|")
                ),
                base,
            )
        }),
    );
    annotate_issue_array(
        json,
        "type_only_dependencies",
        results.type_only_dependencies.iter().map(|item| {
            issue_was_introduced(
                &format!(
                    "type-only-dependency:{}:{}",
                    relative_key_path(&item.path, root),
                    item.package_name
                ),
                base,
            )
        }),
    );
    annotate_issue_array(
        json,
        "test_only_dependencies",
        results.test_only_dependencies.iter().map(|item| {
            issue_was_introduced(
                &format!(
                    "test-only-dependency:{}:{}",
                    relative_key_path(&item.path, root),
                    item.package_name
                ),
                base,
            )
        }),
    );
    annotate_issue_array(
        json,
        "circular_dependencies",
        results.circular_dependencies.iter().map(|item| {
            let mut files: Vec<String> = item
                .files
                .iter()
                .map(|path| relative_key_path(path, root))
                .collect();
            files.sort();
            issue_was_introduced(&format!("circular-dependency:{}", files.join("|")), base)
        }),
    );
    annotate_issue_array(
        json,
        "boundary_violations",
        results.boundary_violations.iter().map(|item| {
            issue_was_introduced(
                &format!(
                    "boundary-violation:{}:{}:{}",
                    relative_key_path(&item.from_path, root),
                    relative_key_path(&item.to_path, root),
                    item.import_specifier
                ),
                base,
            )
        }),
    );
    annotate_issue_array(
        json,
        "stale_suppressions",
        results.stale_suppressions.iter().map(|item| {
            issue_was_introduced(
                &format!(
                    "stale-suppression:{}:{}",
                    relative_key_path(&item.path, root),
                    item.description()
                ),
                base,
            )
        }),
    );
}

fn annotate_health_json(
    json: &mut serde_json::Value,
    report: &crate::health_types::HealthReport,
    root: &Path,
    base: &FxHashSet<String>,
) {
    let Some(items) = json
        .get_mut("findings")
        .and_then(serde_json::Value::as_array_mut)
    else {
        return;
    };
    for (item, finding) in items.iter_mut().zip(&report.findings) {
        if let serde_json::Value::Object(map) = item {
            map.insert(
                "introduced".to_string(),
                serde_json::json!(issue_was_introduced(
                    &health_finding_key(finding, root),
                    base
                )),
            );
        }
    }
}

fn annotate_dupes_json(
    json: &mut serde_json::Value,
    report: &fallow_core::duplicates::DuplicationReport,
    root: &Path,
    base: &FxHashSet<String>,
) {
    let Some(items) = json
        .get_mut("clone_groups")
        .and_then(serde_json::Value::as_array_mut)
    else {
        return;
    };
    for (item, group) in items.iter_mut().zip(&report.clone_groups) {
        if let serde_json::Value::Object(map) = item {
            map.insert(
                "introduced".to_string(),
                serde_json::json!(issue_was_introduced(&dupe_group_key(group, root), base)),
            );
        }
    }
}

fn health_keys(report: &crate::health_types::HealthReport, root: &Path) -> FxHashSet<String> {
    report
        .findings
        .iter()
        .map(|finding| health_finding_key(finding, root))
        .collect()
}

fn health_finding_key(finding: &crate::health_types::HealthFinding, root: &Path) -> String {
    format!(
        "complexity:{}:{}:{:?}",
        relative_key_path(&finding.path, root),
        finding.name,
        finding.exceeded
    )
}

fn dupes_keys(
    report: &fallow_core::duplicates::DuplicationReport,
    root: &Path,
) -> FxHashSet<String> {
    report
        .clone_groups
        .iter()
        .map(|group| dupe_group_key(group, root))
        .collect()
}

fn dupe_group_key(group: &fallow_core::duplicates::CloneGroup, root: &Path) -> String {
    let mut files: Vec<String> = group
        .instances
        .iter()
        .map(|instance| relative_key_path(&instance.file, root))
        .collect();
    files.sort();
    files.dedup();
    let mut hasher = DefaultHasher::new();
    for instance in &group.instances {
        instance.fragment.hash(&mut hasher);
    }
    format!(
        "dupe:{}:{}:{}:{:x}",
        files.join("|"),
        group.token_count,
        group.line_count,
        hasher.finish()
    )
}

// ── Execute ──────────────────────────────────────────────────────

/// Bundle of HEAD-side analysis results returned from [`run_audit_head_analyses`].
///
/// Lets the call site move all three results out of the parallel branch in one
/// shot, instead of threading three tuple slots through `rayon::join`.
struct HeadAnalyses {
    check: Option<CheckResult>,
    dupes: Option<DupesResult>,
    health: Option<HealthResult>,
}

/// Run the three HEAD-side analyses with intra-pipeline sharing intact:
/// check first (so its parsed modules are available), then dupes (which can
/// reuse check's discovered file list when production settings match), then
/// health (which can reuse check's parsed modules when production settings
/// match). Designed to be called from inside `rayon::join` alongside
/// [`compute_base_snapshot`], which operates on an isolated worktree.
fn run_audit_head_analyses(
    opts: &AuditOptions<'_>,
    changed_since: Option<&str>,
    changed_files: &FxHashSet<PathBuf>,
) -> Result<HeadAnalyses, ExitCode> {
    let check_production = opts.production_dead_code.unwrap_or(opts.production);
    let health_production = opts.production_health.unwrap_or(opts.production);
    let dupes_production = opts.production_dupes.unwrap_or(opts.production);
    let share_dead_code_parse_with_health = check_production == health_production;
    let share_dead_code_files_with_dupes =
        share_dead_code_parse_with_health && check_production == dupes_production;

    let mut check = run_audit_check(opts, changed_since, share_dead_code_parse_with_health)?;
    let dupes_files = if share_dead_code_files_with_dupes {
        check
            .as_ref()
            .and_then(|r| r.shared_parse.as_ref().map(|sp| sp.files.clone()))
    } else {
        None
    };
    let dupes = run_audit_dupes(opts, changed_since, Some(changed_files), dupes_files)?;
    let shared_parse = if share_dead_code_parse_with_health {
        check.as_mut().and_then(|r| r.shared_parse.take())
    } else {
        None
    };
    let health = run_audit_health(opts, changed_since, shared_parse)?;
    Ok(HeadAnalyses {
        check,
        dupes,
        health,
    })
}

/// Run the audit pipeline: resolve base ref, run analyses, compute verdict.
pub fn execute_audit(opts: &AuditOptions<'_>) -> Result<AuditResult, ExitCode> {
    let start = Instant::now();

    let base_ref = resolve_base_ref(opts)?;

    // Get changed files (hard error if it fails, unlike combined mode)
    let Some(changed_files) = crate::check::get_changed_files(opts.root, &base_ref) else {
        return Err(emit_error(
            &format!(
                "could not determine changed files for base ref '{base_ref}'. Verify the ref exists in this git repository"
            ),
            2,
            opts.output,
        ));
    };
    let changed_files_count = changed_files.len();

    if changed_files.is_empty() {
        return Ok(empty_audit_result(base_ref, opts, start.elapsed()));
    }

    let changed_since = Some(base_ref.as_str());

    // The HEAD analyses (check + dupes + health) operate on the working tree;
    // the base snapshot operates on an isolated git worktree checked out at
    // `base_ref` (reused by SHA when possible). They share no mutable state, so
    // we can run them concurrently via `rayon::join`, halving wall-clock time
    // on `--gate new-only` (the default). Inside each branch we keep the
    // existing share-the-parse optimization between dead-code and health, since
    // check finishes before either of its dependants run.
    let needs_real_base_snapshot = matches!(opts.gate, AuditGate::NewOnly)
        && !can_reuse_current_as_base(opts, &base_ref, &changed_files);
    let base_cache_key = if needs_real_base_snapshot {
        audit_base_snapshot_cache_key(opts, &base_ref, &changed_files)?
    } else {
        None
    };
    let cached_base_snapshot = base_cache_key
        .as_ref()
        .and_then(|key| load_cached_base_snapshot(opts, key));

    let (head_res, base_res) = if needs_real_base_snapshot && cached_base_snapshot.is_none() {
        let base_sha = base_cache_key.as_ref().map(|key| key.base_sha.as_str());
        let (h, b) = rayon::join(
            || run_audit_head_analyses(opts, changed_since, &changed_files),
            || compute_base_snapshot(opts, &base_ref, &changed_files, base_sha),
        );
        (h, Some(b))
    } else {
        (
            run_audit_head_analyses(opts, changed_since, &changed_files),
            None,
        )
    };

    let head = head_res?;
    let mut check_result = head.check;
    let dupes_result = head.dupes;
    let health_result = head.health;

    let (base_snapshot, base_snapshot_skipped) = if matches!(opts.gate, AuditGate::NewOnly) {
        if let Some(snapshot) = cached_base_snapshot {
            (Some(snapshot), false)
        } else if let Some(base_res) = base_res {
            let snapshot = base_res?;
            if let Some(ref key) = base_cache_key {
                save_cached_base_snapshot(opts, key, &snapshot);
            }
            (Some(snapshot), false)
        } else {
            (
                Some(current_keys_as_base_keys(
                    check_result.as_ref(),
                    dupes_result.as_ref(),
                    health_result.as_ref(),
                )),
                true,
            )
        }
    } else {
        (None, false)
    };
    // Drop shared parse data (no longer needed after base snapshot completed).
    if let Some(ref mut check) = check_result {
        check.shared_parse = None;
    }
    let attribution = compute_audit_attribution(
        check_result.as_ref(),
        dupes_result.as_ref(),
        health_result.as_ref(),
        base_snapshot.as_ref(),
        opts.gate,
    );
    let verdict = if matches!(opts.gate, AuditGate::NewOnly) {
        compute_introduced_verdict(
            check_result.as_ref(),
            dupes_result.as_ref(),
            health_result.as_ref(),
            base_snapshot.as_ref(),
        )
    } else {
        compute_verdict(
            check_result.as_ref(),
            dupes_result.as_ref(),
            health_result.as_ref(),
        )
    };
    let summary = build_summary(
        check_result.as_ref(),
        dupes_result.as_ref(),
        health_result.as_ref(),
    );

    Ok(AuditResult {
        verdict,
        summary,
        attribution,
        base_snapshot,
        base_snapshot_skipped,
        changed_files_count,
        base_ref,
        head_sha: get_head_sha(opts.root),
        output: opts.output,
        performance: opts.performance,
        check: check_result,
        dupes: dupes_result,
        health: health_result,
        elapsed: start.elapsed(),
    })
}

/// Resolve the base ref: explicit --changed-since / --base, or auto-detect.
fn resolve_base_ref(opts: &AuditOptions<'_>) -> Result<String, ExitCode> {
    if let Some(ref_str) = opts.changed_since {
        return Ok(ref_str.to_string());
    }
    let Some(branch) = auto_detect_base_branch(opts.root) else {
        return Err(emit_error(
            "could not detect base branch. Use --base <ref> to specify the comparison target (e.g., --base main)",
            2,
            opts.output,
        ));
    };
    // Validate auto-detected branch name (explicit --changed-since is validated in main.rs)
    if let Err(e) = crate::validate::validate_git_ref(&branch) {
        return Err(emit_error(
            &format!("auto-detected base branch '{branch}' is not a valid git ref: {e}"),
            2,
            opts.output,
        ));
    }
    Ok(branch)
}

/// Build an empty pass result when no files have changed.
fn empty_audit_result(base_ref: String, opts: &AuditOptions<'_>, elapsed: Duration) -> AuditResult {
    AuditResult {
        verdict: AuditVerdict::Pass,
        summary: AuditSummary {
            dead_code_issues: 0,
            dead_code_has_errors: false,
            complexity_findings: 0,
            max_cyclomatic: None,
            duplication_clone_groups: 0,
        },
        attribution: AuditAttribution {
            gate: opts.gate,
            ..AuditAttribution::default()
        },
        base_snapshot: None,
        base_snapshot_skipped: false,
        changed_files_count: 0,
        base_ref,
        head_sha: get_head_sha(opts.root),
        output: opts.output,
        performance: opts.performance,
        check: None,
        dupes: None,
        health: None,
        elapsed,
    }
}

/// Run dead code analysis for the audit pipeline.
fn run_audit_check<'a>(
    opts: &'a AuditOptions<'a>,
    changed_since: Option<&'a str>,
    retain_modules_for_health: bool,
) -> Result<Option<CheckResult>, ExitCode> {
    let filters = IssueFilters::default();
    let trace_opts = TraceOptions {
        trace_export: None,
        trace_file: None,
        trace_dependency: None,
        performance: opts.performance,
    };
    match crate::check::execute_check(&CheckOptions {
        root: opts.root,
        config_path: opts.config_path,
        output: opts.output,
        no_cache: opts.no_cache,
        threads: opts.threads,
        quiet: opts.quiet,
        fail_on_issues: false,
        filters: &filters,
        changed_since,
        baseline: opts.dead_code_baseline,
        save_baseline: None,
        sarif_file: None,
        production: opts.production_dead_code.unwrap_or(opts.production),
        production_override: opts.production_dead_code,
        workspace: opts.workspace,
        changed_workspaces: opts.changed_workspaces,
        group_by: opts.group_by,
        include_dupes: false,
        trace_opts: &trace_opts,
        explain: opts.explain,
        top: None,
        file: &[],
        include_entry_exports: opts.include_entry_exports,
        summary: false,
        regression_opts: crate::regression::RegressionOpts {
            fail_on_regression: false,
            tolerance: crate::regression::Tolerance::Absolute(0),
            regression_baseline_file: None,
            save_target: crate::regression::SaveRegressionTarget::None,
            scoped: true,
            quiet: opts.quiet,
        },
        retain_modules_for_health,
        defer_performance: false,
    }) {
        Ok(r) => Ok(Some(r)),
        Err(code) => Err(code),
    }
}

/// Run duplication analysis for the audit pipeline.
///
/// Reads duplication settings from the project config file so that user
/// options like `ignoreImports`, `crossLanguage`, and `skipLocal` are
/// respected (same as combined mode).
fn run_audit_dupes<'a>(
    opts: &'a AuditOptions<'a>,
    changed_since: Option<&'a str>,
    changed_files: Option<&'a FxHashSet<PathBuf>>,
    pre_discovered: Option<Vec<fallow_types::discover::DiscoveredFile>>,
) -> Result<Option<DupesResult>, ExitCode> {
    let dupes_cfg = match crate::load_config_for_analysis(
        opts.root,
        opts.config_path,
        opts.output,
        opts.no_cache,
        opts.threads,
        opts.production_dupes
            .or_else(|| opts.production.then_some(true)),
        opts.quiet,
        fallow_config::ProductionAnalysis::Dupes,
    ) {
        Ok(c) => c.duplicates,
        Err(code) => return Err(code),
    };
    let dupes_opts = DupesOptions {
        root: opts.root,
        config_path: opts.config_path,
        output: opts.output,
        no_cache: opts.no_cache,
        threads: opts.threads,
        quiet: opts.quiet,
        // The audit pipeline has already merged config + global flags into
        // `dupes_cfg`; pass them as explicit overrides so `build_dupes_config`
        // doesn't re-merge with stale toml values.
        mode: Some(DupesMode::from(dupes_cfg.mode)),
        min_tokens: Some(dupes_cfg.min_tokens),
        min_lines: Some(dupes_cfg.min_lines),
        threshold: Some(dupes_cfg.threshold),
        skip_local: dupes_cfg.skip_local,
        cross_language: dupes_cfg.cross_language,
        ignore_imports: dupes_cfg.ignore_imports,
        top: None,
        baseline_path: opts.dupes_baseline,
        save_baseline_path: None,
        production: opts.production_dupes.unwrap_or(opts.production),
        production_override: opts.production_dupes,
        trace: None,
        changed_since,
        changed_files,
        workspace: opts.workspace,
        changed_workspaces: opts.changed_workspaces,
        explain: opts.explain,
        explain_skipped: opts.explain_skipped,
        summary: false,
        group_by: opts.group_by,
        // Audit emits its own performance breakdown via the audit JSON / human
        // formatter; the standalone dupes panel would be redundant noise here.
        performance: false,
    };
    let dupes_run = if let Some(files) = pre_discovered {
        crate::dupes::execute_dupes_with_files(&dupes_opts, files)
    } else {
        crate::dupes::execute_dupes(&dupes_opts)
    };
    match dupes_run {
        Ok(r) => Ok(Some(r)),
        Err(code) => Err(code),
    }
}

/// Run complexity analysis for the audit pipeline (findings only, no scores/hotspots/targets).
fn run_audit_health<'a>(
    opts: &'a AuditOptions<'a>,
    changed_since: Option<&'a str>,
    shared_parse: Option<crate::health::SharedParseData>,
) -> Result<Option<HealthResult>, ExitCode> {
    let health_opts = HealthOptions {
        root: opts.root,
        config_path: opts.config_path,
        output: opts.output,
        no_cache: opts.no_cache,
        threads: opts.threads,
        quiet: opts.quiet,
        max_cyclomatic: None,
        max_cognitive: None,
        max_crap: opts.max_crap,
        top: None,
        sort: SortBy::Cyclomatic,
        production: opts.production_health.unwrap_or(opts.production),
        production_override: opts.production_health,
        changed_since,
        workspace: opts.workspace,
        changed_workspaces: opts.changed_workspaces,
        baseline: opts.health_baseline,
        save_baseline: None,
        complexity: true,
        file_scores: false,
        coverage_gaps: false,
        config_activates_coverage_gaps: false,
        hotspots: false,
        ownership: false,
        ownership_emails: None,
        targets: false,
        force_full: false,
        score_only_output: false,
        enforce_coverage_gap_gate: false,
        effort: None,
        score: false,
        min_score: None,
        since: None,
        min_commits: None,
        explain: opts.explain,
        summary: false,
        save_snapshot: None,
        trend: false,
        group_by: opts.group_by,
        coverage: None,
        coverage_root: None,
        performance: opts.performance,
        min_severity: None,
        runtime_coverage: None,
    };
    let health_run = if let Some(shared) = shared_parse {
        crate::health::execute_health_with_shared_parse(&health_opts, shared)
    } else {
        crate::health::execute_health(&health_opts)
    };
    match health_run {
        Ok(r) => Ok(Some(r)),
        Err(code) => Err(code),
    }
}

// ── Print ────────────────────────────────────────────────────────

/// Print audit results and return the appropriate exit code.
#[must_use]
pub fn print_audit_result(result: &AuditResult, quiet: bool, explain: bool) -> ExitCode {
    let output = result.output;

    let format_exit = match output {
        OutputFormat::Json => print_audit_json(result),
        OutputFormat::Human | OutputFormat::Compact | OutputFormat::Markdown => {
            print_audit_human(result, quiet, explain, output);
            ExitCode::SUCCESS
        }
        OutputFormat::Sarif => print_audit_sarif(result),
        OutputFormat::CodeClimate => print_audit_codeclimate(result),
        OutputFormat::Badge => {
            eprintln!("Error: badge format is not supported for the audit command");
            return ExitCode::from(2);
        }
    };

    if format_exit != ExitCode::SUCCESS {
        return format_exit;
    }

    match result.verdict {
        AuditVerdict::Fail => ExitCode::from(1),
        AuditVerdict::Pass | AuditVerdict::Warn => ExitCode::SUCCESS,
    }
}

// ── Human format ─────────────────────────────────────────────────

fn print_audit_human(result: &AuditResult, quiet: bool, explain: bool, output: OutputFormat) {
    let show_headers = matches!(output, OutputFormat::Human) && !quiet;

    // Scope line (stderr)
    if !quiet {
        let scope = format_scope_line(result);
        eprintln!();
        eprintln!("{scope}");
    }

    let has_check_issues = result.summary.dead_code_issues > 0;
    let has_health_findings = result.summary.complexity_findings > 0;
    let has_dupe_groups = result.summary.duplication_clone_groups > 0;
    let has_any_findings = has_check_issues || has_health_findings || has_dupe_groups;

    // On fail/warn with findings: show detail sections (reuse existing renderers)
    if has_any_findings {
        if show_headers && std::io::stdout().is_terminal() {
            println!(
                "{}",
                "Tip: run `fallow explain <issue-type>` for any finding below.".dimmed()
            );
            println!();
        }

        // Vital signs summary line (stdout) — only when verdict is pass/warn
        if result.verdict != AuditVerdict::Fail && !quiet {
            print_audit_vital_signs(result);
        }

        if has_check_issues && let Some(ref check) = result.check {
            if show_headers {
                eprintln!();
                eprintln!("── Dead Code ──────────────────────────────────────");
            }
            crate::check::print_check_result(
                check,
                crate::check::PrintCheckOptions {
                    quiet,
                    explain,
                    regression_json: false,
                    group_by: None,
                    top: None,
                    summary: false,
                    show_explain_tip: false,
                },
            );
        }

        if has_dupe_groups && let Some(ref dupes) = result.dupes {
            if show_headers {
                eprintln!();
                eprintln!("── Duplication ────────────────────────────────────");
            }
            crate::dupes::print_dupes_result(dupes, quiet, explain, false, false);
        }

        if has_health_findings && let Some(ref health) = result.health {
            if show_headers {
                eprintln!();
                eprintln!("── Complexity ─────────────────────────────────────");
            }
            crate::health::print_health_result(health, quiet, explain, None, None, false, false);
        }
    }

    if !has_dupe_groups && let Some(ref dupes) = result.dupes {
        crate::dupes::print_default_ignore_note(dupes, quiet);
    }

    // Status line (stderr) — always last
    if !quiet {
        print_audit_status_line(result);
    }
}

/// Format the scope context line.
fn format_scope_line(result: &AuditResult) -> String {
    let sha_suffix = result
        .head_sha
        .as_ref()
        .map_or(String::new(), |sha| format!(" ({sha}..HEAD)"));
    format!(
        "Audit scope: {} changed file{} vs {}{}",
        result.changed_files_count,
        plural(result.changed_files_count),
        result.base_ref,
        sha_suffix
    )
}

/// Print a dimmed vital-signs line summarizing warn-only findings.
fn print_audit_vital_signs(result: &AuditResult) {
    let mut parts = Vec::new();
    parts.push(format!("dead code {}", result.summary.dead_code_issues));
    if let Some(max) = result.summary.max_cyclomatic {
        parts.push(format!(
            "complexity {} (warn, max cyclomatic: {max})",
            result.summary.complexity_findings
        ));
    } else {
        parts.push(format!("complexity {}", result.summary.complexity_findings));
    }
    parts.push(format!(
        "duplication {}",
        result.summary.duplication_clone_groups
    ));

    let line = parts.join(" \u{00b7} ");
    println!(
        "{} {} {}",
        "\u{25a0}".dimmed(),
        "Metrics:".dimmed(),
        line.dimmed()
    );
}

/// Build summary parts for the status line (shared between warn and fail).
fn build_status_parts(summary: &AuditSummary) -> Vec<String> {
    let mut parts = Vec::new();
    if summary.dead_code_issues > 0 {
        let n = summary.dead_code_issues;
        parts.push(format!("dead code: {n} issue{}", plural(n)));
    }
    if summary.complexity_findings > 0 {
        let n = summary.complexity_findings;
        parts.push(format!("complexity: {n} finding{}", plural(n)));
    }
    if summary.duplication_clone_groups > 0 {
        let n = summary.duplication_clone_groups;
        parts.push(format!("duplication: {n} clone group{}", plural(n)));
    }
    parts
}

/// Print the final status line on stderr.
fn print_audit_status_line(result: &AuditResult) {
    let elapsed_str = format!("{:.2}s", result.elapsed.as_secs_f64());
    let n = result.changed_files_count;
    let files_str = format!("{n} changed file{}", plural(n));

    match result.verdict {
        AuditVerdict::Pass => {
            eprintln!(
                "{}",
                format!("\u{2713} No issues in {files_str} ({elapsed_str})")
                    .green()
                    .bold()
            );
        }
        AuditVerdict::Warn => {
            let summary = build_status_parts(&result.summary).join(" \u{00b7} ");
            eprintln!(
                "{}",
                format!("\u{2713} {summary} (warn) \u{00b7} {files_str} ({elapsed_str})")
                    .green()
                    .bold()
            );
        }
        AuditVerdict::Fail => {
            let summary = build_status_parts(&result.summary).join(" \u{00b7} ");
            eprintln!(
                "{}",
                format!("\u{2717} {summary} \u{00b7} {files_str} ({elapsed_str})")
                    .red()
                    .bold()
            );
        }
    }

    if !matches!(result.attribution.gate, AuditGate::All) {
        let inherited = result.attribution.dead_code_inherited
            + result.attribution.complexity_inherited
            + result.attribution.duplication_inherited;
        if inherited > 0 {
            eprintln!(
                "  {}",
                format!(
                    "audit gate excluded {inherited} inherited finding{} (run with --gate all to enforce)",
                    plural(inherited)
                )
                .dimmed()
            );
        }
    }
    if result.performance {
        eprintln!(
            "  {}",
            format!("base_snapshot_skipped: {}", result.base_snapshot_skipped).dimmed()
        );
    }
}

// ── JSON format ──────────────────────────────────────────────────

#[expect(
    clippy::cast_possible_truncation,
    reason = "elapsed milliseconds won't exceed u64::MAX"
)]
fn print_audit_json(result: &AuditResult) -> ExitCode {
    let mut obj = serde_json::Map::new();
    obj.insert("schema_version".into(), serde_json::Value::Number(3.into()));
    obj.insert(
        "version".into(),
        serde_json::Value::String(env!("CARGO_PKG_VERSION").to_string()),
    );
    obj.insert(
        "command".into(),
        serde_json::Value::String("audit".to_string()),
    );
    obj.insert(
        "verdict".into(),
        serde_json::to_value(result.verdict).unwrap_or(serde_json::Value::Null),
    );
    obj.insert(
        "changed_files_count".into(),
        serde_json::Value::Number(result.changed_files_count.into()),
    );
    obj.insert(
        "base_ref".into(),
        serde_json::Value::String(result.base_ref.clone()),
    );
    if let Some(ref sha) = result.head_sha {
        obj.insert("head_sha".into(), serde_json::Value::String(sha.clone()));
    }
    obj.insert(
        "elapsed_ms".into(),
        serde_json::Value::Number(serde_json::Number::from(result.elapsed.as_millis() as u64)),
    );
    if result.performance {
        obj.insert(
            "base_snapshot_skipped".into(),
            serde_json::Value::Bool(result.base_snapshot_skipped),
        );
    }

    // Summary
    if let Ok(summary_val) = serde_json::to_value(&result.summary) {
        obj.insert("summary".into(), summary_val);
    }
    if let Ok(attribution_val) = serde_json::to_value(&result.attribution) {
        obj.insert("attribution".into(), attribution_val);
    }

    // Full sub-results
    if let Some(ref check) = result.check {
        match report::build_json(&check.results, &check.config.root, check.elapsed) {
            Ok(mut json) => {
                if let Some(ref base) = result.base_snapshot {
                    annotate_dead_code_json(
                        &mut json,
                        &check.results,
                        &check.config.root,
                        &base.dead_code,
                    );
                }
                obj.insert("dead_code".into(), json);
            }
            Err(e) => {
                return emit_error(
                    &format!("JSON serialization error: {e}"),
                    2,
                    OutputFormat::Json,
                );
            }
        }
    }

    if let Some(ref dupes) = result.dupes {
        match serde_json::to_value(&dupes.report) {
            Ok(mut json) => {
                let root_prefix = format!("{}/", dupes.config.root.display());
                report::strip_root_prefix(&mut json, &root_prefix);
                report::inject_dupes_actions(&mut json);
                if let Some(ref base) = result.base_snapshot {
                    annotate_dupes_json(&mut json, &dupes.report, &dupes.config.root, &base.dupes);
                }
                obj.insert("duplication".into(), json);
            }
            Err(e) => {
                return emit_error(
                    &format!("JSON serialization error: {e}"),
                    2,
                    OutputFormat::Json,
                );
            }
        }
    }

    if let Some(ref health) = result.health {
        match serde_json::to_value(&health.report) {
            Ok(mut json) => {
                let root_prefix = format!("{}/", health.config.root.display());
                report::strip_root_prefix(&mut json, &root_prefix);
                report::inject_health_actions(&mut json, crate::health::health_action_opts(health));
                if let Some(ref base) = result.base_snapshot {
                    annotate_health_json(
                        &mut json,
                        &health.report,
                        &health.config.root,
                        &base.health,
                    );
                }
                obj.insert("complexity".into(), json);
            }
            Err(e) => {
                return emit_error(
                    &format!("JSON serialization error: {e}"),
                    2,
                    OutputFormat::Json,
                );
            }
        }
    }

    report::emit_json(&serde_json::Value::Object(obj), "audit")
}

// ── SARIF format ─────────────────────────────────────────────────

fn print_audit_sarif(result: &AuditResult) -> ExitCode {
    let mut all_runs = Vec::new();

    if let Some(ref check) = result.check {
        let sarif = report::build_sarif(&check.results, &check.config.root, &check.config.rules);
        if let Some(runs) = sarif.get("runs").and_then(|r| r.as_array()) {
            all_runs.extend(runs.iter().cloned());
        }
    }

    if let Some(ref dupes) = result.dupes
        && !dupes.report.clone_groups.is_empty()
    {
        let run = serde_json::json!({
            "tool": {
                "driver": {
                    "name": "fallow",
                    "version": env!("CARGO_PKG_VERSION"),
                    "informationUri": "https://github.com/fallow-rs/fallow",
                }
            },
            "automationDetails": { "id": "fallow/audit/dupes" },
            "results": dupes.report.clone_groups.iter().enumerate().map(|(i, g)| {
                serde_json::json!({
                    "ruleId": "fallow/code-duplication",
                    "level": "warning",
                    "message": { "text": format!("Clone group {} ({} lines, {} instances)", i + 1, g.line_count, g.instances.len()) },
                })
            }).collect::<Vec<_>>()
        });
        all_runs.push(run);
    }

    if let Some(ref health) = result.health {
        let sarif = report::build_health_sarif(&health.report, &health.config.root);
        if let Some(runs) = sarif.get("runs").and_then(|r| r.as_array()) {
            all_runs.extend(runs.iter().cloned());
        }
    }

    let combined = serde_json::json!({
        "$schema": "https://json.schemastore.org/sarif-2.1.0.json",
        "version": "2.1.0",
        "runs": all_runs,
    });

    report::emit_json(&combined, "SARIF audit")
}

// ── CodeClimate format ───────────────────────────────────────────

fn print_audit_codeclimate(result: &AuditResult) -> ExitCode {
    let mut all_issues = Vec::new();

    if let Some(ref check) = result.check
        && let serde_json::Value::Array(items) =
            report::build_codeclimate(&check.results, &check.config.root, &check.config.rules)
    {
        all_issues.extend(items);
    }

    if let Some(ref dupes) = result.dupes
        && let serde_json::Value::Array(items) =
            report::build_duplication_codeclimate(&dupes.report, &dupes.config.root)
    {
        all_issues.extend(items);
    }

    if let Some(ref health) = result.health
        && let serde_json::Value::Array(items) =
            report::build_health_codeclimate(&health.report, &health.config.root)
    {
        all_issues.extend(items);
    }

    report::emit_json(&serde_json::Value::Array(all_issues), "CodeClimate audit")
}

// ── Entry point ──────────────────────────────────────────────────

/// Run the full audit command: execute analyses, print results, return exit code.
pub fn run_audit(opts: &AuditOptions<'_>) -> ExitCode {
    match execute_audit(opts) {
        Ok(result) => print_audit_result(&result, opts.quiet, opts.explain),
        Err(code) => code,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, process::Command};

    fn git(dir: &std::path::Path, args: &[&str]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(dir)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_AUTHOR_NAME", "test")
            .env("GIT_AUTHOR_EMAIL", "test@test.com")
            .env("GIT_COMMITTER_NAME", "test")
            .env("GIT_COMMITTER_EMAIL", "test@test.com")
            .output()
            .expect("git command failed");
        assert!(
            output.status.success(),
            "git {:?} failed\nstdout:\n{}\nstderr:\n{}",
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn audit_worktree_helpers_filter_to_fallow_temp_prefix() {
        let temp = std::env::temp_dir();
        let audit_path = temp.join("fallow-audit-base-123-456");
        let reusable_path = temp.join("fallow-audit-base-cache-abcd-1234");
        let canonical_audit_path = temp
            .canonicalize()
            .unwrap_or_else(|_| temp.clone())
            .join("fallow-audit-base-456-789");
        let unrelated_temp = temp.join("other-worktree");
        let output = format!(
            "worktree /repo\nHEAD abc\n\nworktree {}\nHEAD def\n\nworktree {}\nHEAD ghi\n\nworktree {}\nHEAD jkl\n",
            audit_path.display(),
            unrelated_temp.display(),
            reusable_path.display()
        );

        assert_eq!(
            parse_worktree_list(&output),
            vec![audit_path, reusable_path.clone()]
        );
        assert!(is_fallow_audit_worktree_path(&canonical_audit_path));
        assert!(is_reusable_audit_worktree_path(&reusable_path));
        assert_eq!(audit_worktree_pid("fallow-audit-base-123-456"), Some(123));
        assert_eq!(
            audit_worktree_pid("fallow-audit-base-cache-abcd-1234"),
            None
        );
        assert_eq!(audit_worktree_pid("not-fallow-audit-base-123"), None);
    }

    #[test]
    fn audit_base_snapshot_cache_payload_roundtrips_sets() {
        let key = AuditBaseSnapshotCacheKey {
            hash: 42,
            base_sha: "abc123".to_string(),
        };
        let snapshot = AuditKeySnapshot {
            dead_code: ["dead:a".to_string(), "dead:b".to_string()]
                .into_iter()
                .collect(),
            health: std::iter::once("health:a".to_string()).collect(),
            dupes: ["dupe:a".to_string(), "dupe:b".to_string()]
                .into_iter()
                .collect(),
        };

        let cached = cached_from_snapshot(&key, &snapshot);
        assert_eq!(cached.version, AUDIT_BASE_SNAPSHOT_CACHE_VERSION);
        assert_eq!(cached.key_hash, key.hash);
        assert_eq!(cached.base_sha, key.base_sha);
        assert_eq!(cached.dead_code, vec!["dead:a", "dead:b"]);

        let decoded = snapshot_from_cached(cached);
        assert_eq!(decoded.dead_code, snapshot.dead_code);
        assert_eq!(decoded.health, snapshot.health);
        assert_eq!(decoded.dupes, snapshot.dupes);
    }

    #[test]
    fn audit_base_snapshot_cache_key_includes_extended_config() {
        let tmp = tempfile::TempDir::new().expect("temp dir should be created");
        let root = tmp.path();
        fs::write(
            root.join(".fallowrc.json"),
            r#"{"extends":"base.json","entry":["src/index.ts"]}"#,
        )
        .expect("config should be written");
        fs::write(
            root.join("base.json"),
            r#"{"rules":{"unused-exports":"off"}}"#,
        )
        .expect("base config should be written");

        let config_path = None;
        let opts = AuditOptions {
            root,
            config_path: &config_path,
            output: OutputFormat::Json,
            no_cache: false,
            threads: 1,
            quiet: true,
            changed_since: Some("HEAD"),
            production: false,
            production_dead_code: None,
            production_health: None,
            production_dupes: None,
            workspace: None,
            changed_workspaces: None,
            explain: false,
            explain_skipped: false,
            performance: false,
            group_by: None,
            dead_code_baseline: None,
            health_baseline: None,
            dupes_baseline: None,
            max_crap: None,
            gate: AuditGate::NewOnly,
            include_entry_exports: false,
        };

        let first = config_file_fingerprint(&opts).expect("fingerprint should be computed");
        fs::write(
            root.join("base.json"),
            r#"{"rules":{"unused-exports":"error"}}"#,
        )
        .expect("base config should be updated");
        let second = config_file_fingerprint(&opts).expect("fingerprint should be recomputed");

        assert_ne!(
            first["resolved_hash"], second["resolved_hash"],
            "extended config changes must invalidate cached base snapshots"
        );
    }

    #[test]
    fn audit_gate_all_skips_base_snapshot() {
        let tmp = tempfile::TempDir::new().expect("temp dir should be created");
        let root = tmp.path();
        fs::create_dir_all(root.join("src")).expect("src dir should be created");
        fs::write(
            root.join("package.json"),
            r#"{"name":"audit-gate-all","main":"src/index.ts"}"#,
        )
        .expect("package.json should be written");
        fs::write(root.join("src/index.ts"), "export const legacy = 1;\n")
            .expect("index should be written");

        git(root, &["init", "-b", "main"]);
        git(root, &["add", "."]);
        git(
            root,
            &["-c", "commit.gpgsign=false", "commit", "-m", "initial"],
        );
        fs::write(
            root.join("src/index.ts"),
            "export const legacy = 1;\nexport const changed = 2;\n",
        )
        .expect("changed module should be written");

        let config_path = None;
        let opts = AuditOptions {
            root,
            config_path: &config_path,
            output: OutputFormat::Json,
            no_cache: true,
            threads: 1,
            quiet: true,
            changed_since: Some("HEAD"),
            production: false,
            production_dead_code: None,
            production_health: None,
            production_dupes: None,
            workspace: None,
            changed_workspaces: None,
            explain: false,
            explain_skipped: false,
            performance: false,
            group_by: None,
            dead_code_baseline: None,
            health_baseline: None,
            dupes_baseline: None,
            max_crap: None,
            gate: AuditGate::All,
            include_entry_exports: false,
        };

        let result = execute_audit(&opts).expect("audit should execute");
        assert!(result.base_snapshot.is_none());
        assert_eq!(result.attribution.gate, AuditGate::All);
        assert_eq!(result.attribution.dead_code_introduced, 0);
        assert_eq!(result.attribution.dead_code_inherited, 0);
    }

    #[test]
    fn audit_gate_new_only_skips_base_snapshot_for_docs_only_diff() {
        let tmp = tempfile::TempDir::new().expect("temp dir should be created");
        let root = tmp.path();
        fs::create_dir_all(root.join("src")).expect("src dir should be created");
        fs::write(
            root.join("package.json"),
            r#"{"name":"audit-docs-only","main":"src/index.ts"}"#,
        )
        .expect("package.json should be written");
        fs::write(
            root.join(".fallowrc.json"),
            r#"{"duplicates":{"minTokens":5,"minLines":2,"mode":"strict"}}"#,
        )
        .expect("config should be written");
        let duplicated = "export function same(input: number): number {\n  const doubled = input * 2;\n  const shifted = doubled + 1;\n  return shifted;\n}\n";
        fs::write(root.join("src/index.ts"), duplicated).expect("index should be written");
        fs::write(root.join("src/copy.ts"), duplicated).expect("copy should be written");
        fs::write(root.join("README.md"), "before\n").expect("readme should be written");

        git(root, &["init", "-b", "main"]);
        git(root, &["add", "."]);
        git(
            root,
            &["-c", "commit.gpgsign=false", "commit", "-m", "initial"],
        );
        fs::write(root.join("README.md"), "after\n").expect("readme should be modified");
        fs::create_dir_all(root.join(".fallow/cache/dupes-tokens-v2"))
            .expect("cache dir should be created");
        fs::write(
            root.join(".fallow/cache/dupes-tokens-v2/cache.bin"),
            b"cache",
        )
        .expect("cache artifact should be written");

        let before_worktrees = audit_worktree_names(root);

        let config_path = None;
        let opts = AuditOptions {
            root,
            config_path: &config_path,
            output: OutputFormat::Json,
            no_cache: true,
            threads: 1,
            quiet: true,
            changed_since: Some("HEAD"),
            production: false,
            production_dead_code: None,
            production_health: None,
            production_dupes: None,
            workspace: None,
            changed_workspaces: None,
            explain: false,
            explain_skipped: false,
            performance: true,
            group_by: None,
            dead_code_baseline: None,
            health_baseline: None,
            dupes_baseline: None,
            max_crap: None,
            gate: AuditGate::NewOnly,
            include_entry_exports: false,
        };

        let result = execute_audit(&opts).expect("audit should execute");
        assert_eq!(result.verdict, AuditVerdict::Pass);
        assert_eq!(result.changed_files_count, 2);
        assert!(result.base_snapshot_skipped);
        assert!(result.base_snapshot.is_some());

        let after_worktrees = audit_worktree_names(root);
        assert_eq!(
            before_worktrees, after_worktrees,
            "base snapshot skip must not create a temporary base worktree"
        );
    }

    fn audit_worktree_names(repo_root: &Path) -> Vec<String> {
        let mut names: Vec<String> = list_audit_worktrees(repo_root)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .map(str::to_owned)
            })
            .collect();
        names.sort();
        names
    }

    #[test]
    fn audit_reuses_dead_code_parse_for_health_when_production_matches() {
        let tmp = tempfile::TempDir::new().expect("temp dir should be created");
        let root = tmp.path();
        fs::create_dir_all(root.join("src")).expect("src dir should be created");
        fs::write(
            root.join("package.json"),
            r#"{"name":"audit-shared-parse","main":"src/index.ts"}"#,
        )
        .expect("package.json should be written");
        fs::write(
            root.join("src/index.ts"),
            "import { used } from './used';\nused();\n",
        )
        .expect("index should be written");
        fs::write(
            root.join("src/used.ts"),
            "export function used() {\n  return 1;\n}\n",
        )
        .expect("used module should be written");

        git(root, &["init", "-b", "main"]);
        git(root, &["add", "."]);
        git(
            root,
            &["-c", "commit.gpgsign=false", "commit", "-m", "initial"],
        );
        fs::write(
            root.join("src/used.ts"),
            "export function used() {\n  return 1;\n}\nexport function changed() {\n  return 2;\n}\n",
        )
        .expect("changed module should be written");

        let config_path = None;
        let opts = AuditOptions {
            root,
            config_path: &config_path,
            output: OutputFormat::Json,
            no_cache: true,
            threads: 1,
            quiet: true,
            changed_since: Some("HEAD"),
            production: false,
            production_dead_code: None,
            production_health: None,
            production_dupes: None,
            workspace: None,
            changed_workspaces: None,
            explain: false,
            explain_skipped: false,
            performance: true,
            group_by: None,
            dead_code_baseline: None,
            health_baseline: None,
            dupes_baseline: None,
            max_crap: None,
            gate: AuditGate::NewOnly,
            include_entry_exports: false,
        };

        let result = execute_audit(&opts).expect("audit should execute");
        let health = result.health.expect("health should run for changed files");
        let timings = health.timings.expect("performance timings should be kept");
        assert!(timings.discover_ms.abs() < f64::EPSILON);
        assert!(timings.parse_ms.abs() < f64::EPSILON);
        // Same production settings, so dupes should also have piggy-backed on
        // the dead-code file list (no separate verifiable signal in DupesResult,
        // but the run must still produce a non-None result).
        assert!(
            result.dupes.is_some(),
            "dupes should run when changed files exist"
        );
    }

    #[test]
    fn audit_dupes_falls_back_to_own_discovery_when_health_off() {
        // When health and dupes have different production settings, dupes must
        // not borrow files from dead-code (the file sets can differ). The two
        // execution paths should still produce a result.
        let tmp = tempfile::TempDir::new().expect("temp dir should be created");
        let root = tmp.path();
        fs::create_dir_all(root.join("src")).expect("src dir should be created");
        fs::write(
            root.join("package.json"),
            r#"{"name":"audit-dupes-fallback","main":"src/index.ts"}"#,
        )
        .expect("package.json should be written");
        fs::write(
            root.join("src/index.ts"),
            "import { used } from './used';\nused();\n",
        )
        .expect("index should be written");
        fs::write(
            root.join("src/used.ts"),
            "export function used() {\n  return 1;\n}\n",
        )
        .expect("used module should be written");

        git(root, &["init", "-b", "main"]);
        git(root, &["add", "."]);
        git(
            root,
            &["-c", "commit.gpgsign=false", "commit", "-m", "initial"],
        );
        fs::write(
            root.join("src/used.ts"),
            "export function used() {\n  return 1;\n}\nexport function changed() {\n  return 2;\n}\n",
        )
        .expect("changed module should be written");

        let config_path = None;
        let opts = AuditOptions {
            root,
            config_path: &config_path,
            output: OutputFormat::Json,
            no_cache: true,
            threads: 1,
            quiet: true,
            changed_since: Some("HEAD"),
            production: false,
            production_dead_code: Some(true),
            production_health: Some(false),
            production_dupes: Some(false),
            workspace: None,
            changed_workspaces: None,
            explain: false,
            explain_skipped: false,
            performance: true,
            group_by: None,
            dead_code_baseline: None,
            health_baseline: None,
            dupes_baseline: None,
            max_crap: None,
            gate: AuditGate::NewOnly,
            include_entry_exports: false,
        };

        let result = execute_audit(&opts).expect("audit should execute");
        assert!(result.dupes.is_some(), "dupes should still run");
    }

    #[cfg(unix)]
    #[test]
    fn remap_focus_files_does_not_canonicalize_through_symlinks() {
        // Function-level contract: `remap_focus_files` must NOT canonicalize
        // `to_root`. The base worktree path comes from `std::env::temp_dir()`
        // un-canonicalized, and `discover_files` walks the worktree using that
        // exact prefix; resolving symlinks here would silently shift the prefix
        // on systems where the tempdir traverses one (`/tmp` -> `/private/tmp`,
        // `/var` -> `/private/var` on macOS) and miss every discovered file at
        // base. Pin the contract via a synthetic `from_root` and a real
        // symlinked `to_root`; the matching end-to-end behavior is covered by
        // `audit_gate_new_only_inherits_pre_existing_duplicates_in_focused_files`.
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let real = tmp.path().join("real");
        let link = tmp.path().join("link");
        fs::create_dir_all(&real).expect("real dir");
        std::os::unix::fs::symlink(&real, &link).expect("symlink");
        // Sanity: `link` and `link.canonicalize()` differ. If the OS canonicalized
        // them to the same path, the test premise doesn't hold and the assertion
        // below is meaningless.
        let canonical = link.canonicalize().expect("canonicalize symlink");
        assert_ne!(link, canonical, "symlink should not equal its target");

        let from_root = PathBuf::from("/repo");
        let mut focus = FxHashSet::default();
        focus.insert(from_root.join("src/foo.ts"));

        let remapped = remap_focus_files(&focus, &from_root, &link)
            .expect("remap should succeed for in-prefix files");

        let expected = link.join("src/foo.ts");
        assert!(
            remapped.contains(&expected),
            "remapped paths must keep the un-canonical to_root prefix; got {remapped:?}, expected entry {expected:?}"
        );
    }

    #[test]
    fn remap_focus_files_skips_paths_outside_from_root() {
        // A file outside `from_root` (e.g., a sibling workspace touched in the
        // same diff) must not collapse the entire focus set. The optimization
        // should stay active for the in-scope subset.
        let from_root = PathBuf::from("/repo/apps/web");
        let to_root = PathBuf::from("/wt/apps/web");
        let mut focus = FxHashSet::default();
        focus.insert(PathBuf::from("/repo/apps/web/src/in.ts"));
        focus.insert(PathBuf::from("/repo/services/api/src/out.ts"));

        let remapped =
            remap_focus_files(&focus, &from_root, &to_root).expect("partial map should succeed");

        assert_eq!(remapped.len(), 1);
        assert!(remapped.contains(&PathBuf::from("/wt/apps/web/src/in.ts")));
    }

    #[test]
    fn remap_focus_files_returns_none_when_no_paths_map() {
        let from_root = PathBuf::from("/repo/apps/web");
        let to_root = PathBuf::from("/wt/apps/web");
        let mut focus = FxHashSet::default();
        focus.insert(PathBuf::from("/elsewhere/foo.ts"));

        let remapped = remap_focus_files(&focus, &from_root, &to_root);
        assert!(
            remapped.is_none(),
            "remap should return None when no paths can be mapped, falling caller back to full corpus"
        );
    }

    #[test]
    fn audit_gate_new_only_inherits_pre_existing_duplicates_in_focused_files() {
        // Regression test for the dupe-focus optimization: when changed files
        // contain duplicates that ALSO existed at base (HEAD~1), the audit gate
        // must classify them as `inherited`, not `introduced`. The original
        // implementation canonicalized `to_root` in `remap_focus_files`, which
        // on macOS shifted the prefix from `/var/folders/...` to
        // `/private/var/folders/...`. `discover_files` in the base worktree
        // walked the un-canonical path, so set membership at base missed every
        // remapped focus path. `find_duplicates_touching_files` returned 0
        // groups at base, base_keys was empty, and every current finding
        // misclassified as `introduced`.
        let tmp = tempfile::TempDir::new().expect("temp dir should be created");
        // Mirror production: `validate_root` canonicalizes user-supplied roots
        // before they reach `execute_audit`. This test exercises the *base
        // worktree* side of the bug, where the worktree path comes from
        // `std::env::temp_dir()` and is canonical-vs-un-canonical INDEPENDENT
        // of what `opts.root` looks like. On macOS, `std::env::temp_dir()`
        // returns `/var/folders/...` and `canonicalize` resolves it to
        // `/private/var/folders/...`, so a buggy remap loses every focus path
        // even when `opts.root` is already canonical.
        let root_buf = tmp
            .path()
            .canonicalize()
            .expect("temp root should canonicalize");
        let root = root_buf.as_path();
        fs::create_dir_all(root.join("src")).expect("src dir should be created");
        fs::write(
            root.join("package.json"),
            r#"{"name":"audit-newonly-inherit","main":"src/changed.ts"}"#,
        )
        .expect("package.json should be written");
        fs::write(
            root.join(".fallowrc.json"),
            r#"{"duplicates":{"minTokens":10,"minLines":3,"mode":"strict"}}"#,
        )
        .expect("config should be written");

        let dup_block = "export function processItems(input: number[]): number[] {\n  const doubled = input.map((value) => value * 2);\n  const filtered = doubled.filter((value) => value > 0);\n  const summed = filtered.reduce((acc, value) => acc + value, 0);\n  const shifted = summed + 10;\n  const scaled = shifted * 3;\n  const rounded = Math.round(scaled / 7);\n  return [rounded, scaled, summed];\n}\n";
        fs::write(root.join("src/changed.ts"), dup_block).expect("changed should be written");
        fs::write(root.join("src/peer.ts"), dup_block).expect("peer should be written");

        git(root, &["init", "-b", "main"]);
        git(root, &["add", "."]);
        git(
            root,
            &["-c", "commit.gpgsign=false", "commit", "-m", "initial"],
        );
        // Append a comment-only line so the file is "changed" without altering
        // the duplicated token sequence.
        fs::write(
            root.join("src/changed.ts"),
            format!("{dup_block}// touched\n"),
        )
        .expect("changed file should be modified");
        git(root, &["add", "."]);
        git(
            root,
            &["-c", "commit.gpgsign=false", "commit", "-m", "touch"],
        );

        let config_path = None;
        let opts = AuditOptions {
            root,
            config_path: &config_path,
            output: OutputFormat::Json,
            no_cache: true,
            threads: 1,
            quiet: true,
            changed_since: Some("HEAD~1"),
            production: false,
            production_dead_code: None,
            production_health: None,
            production_dupes: None,
            workspace: None,
            changed_workspaces: None,
            explain: false,
            explain_skipped: false,
            performance: false,
            group_by: None,
            dead_code_baseline: None,
            health_baseline: None,
            dupes_baseline: None,
            max_crap: None,
            gate: AuditGate::NewOnly,
            include_entry_exports: false,
        };

        let result = execute_audit(&opts).expect("audit should execute");
        assert!(
            result.base_snapshot_skipped,
            "comment-only JS/TS diffs should reuse current keys as the base snapshot"
        );
        let dupes_report = &result.dupes.as_ref().expect("dupes should run").report;
        assert!(
            !dupes_report.clone_groups.is_empty(),
            "current run should detect the pre-existing duplicate"
        );
        assert_eq!(
            result.attribution.duplication_introduced, 0,
            "pre-existing duplicate must not be classified as introduced; \
             attribution = {:?}",
            result.attribution
        );
        assert!(
            result.attribution.duplication_inherited > 0,
            "pre-existing duplicate must be classified as inherited; \
             attribution = {:?}",
            result.attribution
        );
    }

    #[test]
    fn audit_dupes_only_materializes_groups_touching_changed_files() {
        let tmp = tempfile::TempDir::new().expect("temp dir should be created");
        let root_path = tmp
            .path()
            .canonicalize()
            .expect("temp root should canonicalize");
        let root = root_path.as_path();
        fs::create_dir_all(root.join("src")).expect("src dir should be created");
        fs::write(
            root.join("package.json"),
            r#"{"name":"audit-dupes-focus","main":"src/changed.ts"}"#,
        )
        .expect("package.json should be written");
        fs::write(
            root.join(".fallowrc.json"),
            r#"{"duplicates":{"minTokens":5,"minLines":2,"mode":"strict"}}"#,
        )
        .expect("config should be written");

        let focused_code = "export function focused(input: number): number {\n  const doubled = input * 2;\n  const shifted = doubled + 10;\n  return shifted / 2;\n}\n";
        let untouched_code = "export function untouched(input: string): string {\n  const lowered = input.toLowerCase();\n  const padded = lowered.padStart(10, \"x\");\n  return padded.slice(0, 8);\n}\n";
        fs::write(root.join("src/changed.ts"), focused_code).expect("changed should be written");
        fs::write(root.join("src/focused-copy.ts"), focused_code)
            .expect("focused copy should be written");
        fs::write(root.join("src/untouched-a.ts"), untouched_code)
            .expect("untouched a should be written");
        fs::write(root.join("src/untouched-b.ts"), untouched_code)
            .expect("untouched b should be written");

        git(root, &["init", "-b", "main"]);
        git(root, &["add", "."]);
        git(
            root,
            &["-c", "commit.gpgsign=false", "commit", "-m", "initial"],
        );
        fs::write(
            root.join("src/changed.ts"),
            format!("{focused_code}export const changedMarker = true;\n"),
        )
        .expect("changed file should be modified");

        let config_path = None;
        let opts = AuditOptions {
            root,
            config_path: &config_path,
            output: OutputFormat::Json,
            no_cache: true,
            threads: 1,
            quiet: true,
            changed_since: Some("HEAD"),
            production: false,
            production_dead_code: None,
            production_health: None,
            production_dupes: None,
            workspace: None,
            changed_workspaces: None,
            explain: false,
            explain_skipped: false,
            performance: false,
            group_by: None,
            dead_code_baseline: None,
            health_baseline: None,
            dupes_baseline: None,
            max_crap: None,
            gate: AuditGate::All,
            include_entry_exports: false,
        };

        let result = execute_audit(&opts).expect("audit should execute");
        let dupes = result.dupes.expect("dupes should run");
        let changed_path = root.join("src/changed.ts");

        assert!(
            !dupes.report.clone_groups.is_empty(),
            "changed file should still match unchanged duplicate code"
        );
        assert!(dupes.report.clone_groups.iter().all(|group| {
            group
                .instances
                .iter()
                .any(|instance| instance.file == changed_path)
        }));
    }
}
