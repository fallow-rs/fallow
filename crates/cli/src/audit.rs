use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::{Duration, Instant, SystemTime};

use colored::Colorize;
use fallow_config::{AuditConfig, AuditGate, OutputFormat};
use fallow_core::git_env::clear_ambient_git_env;
use rustc_hash::FxHashSet;
use xxhash_rust::xxh3::xxh3_64;

use crate::check::{CheckOptions, CheckResult, IssueFilters, TraceOptions};
use crate::dupes::{DupesMode, DupesOptions, DupesResult};
use crate::error::emit_error;
use crate::health::{HealthOptions, HealthResult, SortBy};
use crate::report;
use crate::report::plural;

// ── Types ────────────────────────────────────────────────────────

const AUDIT_BASE_SNAPSHOT_CACHE_VERSION: u8 = 2;
const MAX_AUDIT_BASE_SNAPSHOT_CACHE_SIZE: usize = 16 * 1024 * 1024;

/// Verdict for the audit command.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
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
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct AuditSummary {
    pub dead_code_issues: usize,
    pub dead_code_has_errors: bool,
    pub complexity_findings: usize,
    pub max_cyclomatic: Option<u16>,
    pub duplication_clone_groups: usize,
}

/// New-vs-inherited issue counts for audit.
#[derive(Debug, Default, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
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
    /// Istanbul coverage input for accurate CRAP scoring in the health sub-pass.
    pub coverage: Option<&'a std::path::Path>,
    /// Prefix to strip from Istanbul source paths before rebasing to `root`.
    pub coverage_root: Option<&'a std::path::Path>,
    pub gate: AuditGate,
    /// Report unused exports in entry files (forwarded to the dead-code sub-pass).
    pub include_entry_exports: bool,
    /// Paid runtime-coverage sidecar input (V8 directory, V8 JSON, or
    /// Istanbul coverage map). Forwarded into the embedded health pass so
    /// audit surfaces the `hot-path-touched` verdict alongside dead-code
    /// and complexity findings without requiring a second `fallow health`
    /// invocation in CI.
    pub runtime_coverage: Option<&'a std::path::Path>,
    /// Threshold for hot-path classification, forwarded to the sidecar.
    pub min_invocations_hot: u64,
    // `diff_file` was removed from this struct: audit now sources the
    // parsed diff index from the process-wide cache in
    // `crate::report::ci::diff_filter::shared_diff_index()`, populated
    // by `main()`. The cache covers `--diff-file PATH`, `--diff-file -`,
    // `--diff-stdin`, and the `$FALLOW_DIFF_FILE` env var.
}

// ── Auto-detect base branch ──────────────────────────────────────

/// Try to determine the default branch for the repository.
/// Priority: `git symbolic-ref refs/remotes/origin/HEAD` → `main` → `master`.
/// Returns `None` if none of these exist.
fn auto_detect_base_branch(root: &std::path::Path) -> Option<String> {
    // Try symbolic-ref first (works when origin HEAD is set)
    let mut symbolic_ref = std::process::Command::new("git");
    symbolic_ref
        .args(["symbolic-ref", "refs/remotes/origin/HEAD"])
        .current_dir(root);
    clear_ambient_git_env(&mut symbolic_ref);
    if let Ok(output) = symbolic_ref.output()
        && output.status.success()
    {
        let full_ref = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if let Some(branch) = full_ref.strip_prefix("refs/remotes/origin/") {
            return Some(branch.to_string());
        }
    }

    // Try main
    let mut verify_main = std::process::Command::new("git");
    verify_main
        .args(["rev-parse", "--verify", "main"])
        .current_dir(root);
    clear_ambient_git_env(&mut verify_main);
    if let Ok(output) = verify_main.output()
        && output.status.success()
    {
        return Some("main".to_string());
    }

    // Try master
    let mut verify_master = std::process::Command::new("git");
    verify_master
        .args(["rev-parse", "--verify", "master"])
        .current_dir(root);
    clear_ambient_git_env(&mut verify_master);
    if let Ok(output) = verify_master.output()
        && output.status.success()
    {
        return Some("master".to_string());
    }

    None
}

/// Get the short SHA of HEAD for the scope display line.
fn get_head_sha(root: &std::path::Path) -> Option<String> {
    let mut command = std::process::Command::new("git");
    command
        .args(["rev-parse", "--short", "HEAD"])
        .current_dir(root);
    clear_ambient_git_env(&mut command);
    let output = command.output().ok()?;
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
    let mut command = Command::new("git");
    command.args(["rev-parse", rev]).current_dir(root);
    clear_ambient_git_env(&mut command);
    let output = command.output().ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// If fallow's process inherited any ambient git repo-state env vars (typical
/// when invoked from a `pre-commit` / `pre-push` hook or a tool wrapping git),
/// surface the most likely culprit so a user hitting an unexpected worktree
/// failure can short-circuit the diagnosis. Returns `None` otherwise.
fn ambient_git_env_hint() -> Option<String> {
    use fallow_core::git_env::AMBIENT_GIT_ENV_VARS;
    for var in AMBIENT_GIT_ENV_VARS {
        if let Ok(value) = std::env::var(var)
            && !value.is_empty()
        {
            return Some(format!(
                "{var}={value} is set in the environment; if fallow is being \
invoked from a git hook this can interfere with worktree operations. Re-run \
with `env -u {var} fallow audit` to confirm."
            ));
        }
    }
    None
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

fn coverage_file_fingerprint(path: &Path, project_root: &Path) -> serde_json::Value {
    let resolved = crate::health::scoring::resolve_relative_to_root(path, Some(project_root));
    let file_path = if resolved.is_dir() {
        resolved.join("coverage-final.json")
    } else {
        resolved
    };
    match std::fs::read(&file_path) {
        Ok(bytes) => serde_json::json!({
            "path": path.to_string_lossy(),
            "resolved_path": file_path.to_string_lossy(),
            "content_hash": format!("{:016x}", xxh3_64(&bytes)),
            "len": bytes.len(),
        }),
        Err(err) => serde_json::json!({
            "path": path.to_string_lossy(),
            "resolved_path": file_path.to_string_lossy(),
            "error": err.kind().to_string(),
        }),
    }
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
    let coverage_file = opts
        .coverage
        .map(|p| coverage_file_fingerprint(p, opts.root));
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
        "coverage": coverage_file,
        "coverage_root": opts.coverage_root.map(|p| p.to_string_lossy().to_string()),
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
        use std::fmt::Write as _;
        let mut message =
            format!("could not create a temporary worktree for base ref '{base_ref}'");
        if let Some(hint) = ambient_git_env_hint() {
            let _ = write!(message, "\n  hint: {hint}");
        }
        return Err(emit_error(&message, 2, opts.output));
    };
    let base_root = base_analysis_root(opts.root, worktree.path());
    let current_config_path = opts
        .config_path
        .clone()
        .or_else(|| fallow_config::FallowConfig::find_config_path(opts.root));
    let base_opts = AuditOptions {
        root: &base_root,
        config_path: &current_config_path,
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
        coverage: opts.coverage,
        coverage_root: opts.coverage_root,
        gate: AuditGate::All,
        include_entry_exports: opts.include_entry_exports,
        // Base-snapshot pass intentionally does NOT spawn the sidecar
        // again or apply hot-path filtering: hot-path-touched is a
        // PR-vs-HEAD signal, and the recursive base run is HEAD's
        // baseline, so it has nothing to compare against. Suppressing
        // here also avoids a duplicate license check + sidecar download
        // cost on every audit run.
        runtime_coverage: None,
        min_invocations_hot: opts.min_invocations_hot,
    };

    let base_changed_files = remap_focus_files(changed_files, opts.root, &base_root);
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

fn base_analysis_root(current_root: &Path, base_worktree_root: &Path) -> PathBuf {
    let Some(git_root) = git_toplevel(current_root) else {
        return base_worktree_root.to_path_buf();
    };
    // `dunce::canonicalize` strips Windows `\\?\` verbatim prefix so this
    // current_root matches `git_root` (also dunce-canonicalised above) when
    // `strip_prefix` walks the component graph.
    let current_root =
        dunce::canonicalize(current_root).unwrap_or_else(|_| current_root.to_path_buf());
    match current_root.strip_prefix(&git_root) {
        Ok(relative) => base_worktree_root.join(relative),
        Err(err) => {
            tracing::warn!(
                current_root = %current_root.display(),
                git_root = %git_root.display(),
                error = %err,
                "Could not remap audit base root into the base worktree; falling back to worktree root"
            );
            base_worktree_root.to_path_buf()
        }
    }
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
    // `dunce::canonicalize` strips Windows `\\?\` verbatim prefix so the
    // `starts_with` checks below compare against a shape that matches the
    // changed_files paths (which also flow through dunce-canonicalised
    // `resolve_git_toplevel`). On POSIX dunce is identical to std.
    let canonical_cache_dir = dunce::canonicalize(&cache_dir).ok();
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
    let mut command = Command::new("git");
    command
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(root);
    clear_ambient_git_env(&mut command);
    let output = command.output().ok()?;
    if !output.status.success() {
        return None;
    }
    let path = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
    // Mirror `fallow_core::changed_files::resolve_git_toplevel`: use
    // `dunce::canonicalize` to strip Windows `\\?\` verbatim prefix so this
    // canonical form matches the shape `opts.root` and finding paths use
    // downstream. `std::fs::canonicalize` would diverge on Windows.
    Some(dunce::canonicalize(&path).unwrap_or(path))
}

fn git_show_file(root: &Path, base_ref: &str, relative: &Path) -> Option<String> {
    let spec = format!(
        "{}:{}",
        base_ref,
        relative.to_string_lossy().replace('\\', "/")
    );
    let mut command = Command::new("git");
    command
        .args(["show", "--end-of-options", &spec])
        .current_dir(root);
    clear_ambient_git_env(&mut command);
    let output = command.output().ok()?;
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
        let mut guard = WorktreeCleanupGuard::new(repo_root, &path);
        let mut command = Command::new("git");
        command
            .args([
                "worktree",
                "add",
                "--detach",
                "--quiet",
                guard.path().to_str()?,
                base_ref,
            ])
            .current_dir(repo_root);
        clear_ambient_git_env(&mut command);
        let output = crate::signal::scoped_child::output(&mut command).ok()?;
        if !output.status.success() {
            return None;
        }
        guard.defuse();
        drop(guard);
        let worktree = Self {
            repo_root: repo_root.to_path_buf(),
            path,
            persistent: false,
        };
        materialize_base_dependency_context(repo_root, worktree.path());
        Some(worktree)
    }

    fn reuse_or_create(repo_root: &Path, base_sha: &str) -> Option<Self> {
        let path = reusable_audit_worktree_path(repo_root, base_sha);
        // Serialise concurrent audits against the same base_sha. On contention,
        // fall through to the non-reusable PID-named path so the loser does not
        // block; matrix CI then gets at most one slow rebuild rather than racing
        // git worktree add against the same directory. The lock is released
        // automatically when `_lock` drops.
        let _lock = ReusableWorktreeLock::try_acquire(&path)?;

        if reusable_audit_worktree_is_ready(repo_root, &path, base_sha) {
            let worktree = Self {
                repo_root: repo_root.to_path_buf(),
                path,
                persistent: true,
            };
            materialize_base_dependency_context(repo_root, worktree.path());
            // Update the staleness signal so the age-based GC sweep does
            // not nuke a frequently-reused cache.
            touch_last_used(worktree.path());
            return Some(worktree);
        }

        remove_audit_worktree(repo_root, &path);
        let _ = std::fs::remove_dir_all(&path);
        let mut guard = WorktreeCleanupGuard::new(repo_root, &path);
        let mut command = Command::new("git");
        command
            .args([
                "worktree",
                "add",
                "--detach",
                "--quiet",
                guard.path().to_string_lossy().as_ref(),
                base_sha,
            ])
            .current_dir(repo_root);
        clear_ambient_git_env(&mut command);
        let output = crate::signal::scoped_child::output(&mut command).ok()?;
        if !output.status.success() {
            return None;
        }
        guard.defuse();
        drop(guard);

        let worktree = Self {
            repo_root: repo_root.to_path_buf(),
            path,
            persistent: true,
        };
        materialize_base_dependency_context(repo_root, worktree.path());
        // Stamp the sidecar at fresh-create time so the cache's age is
        // measured from "first existence" rather than "first reuse". The
        // sweep's sidecar-absent branch (`touch + skip`) is still
        // load-bearing for pre-upgrade caches created before this
        // feature shipped.
        touch_last_used(worktree.path());
        Some(worktree)
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

/// RAII cleanup guard for a freshly-created git worktree directory.
///
/// Armed before the `git worktree add` subprocess runs. If the holder returns
/// early (`?`) between subprocess success and the `BaseWorktree` struct binding,
/// `Drop` rolls back BOTH git's `.git/worktrees/<name>` registration AND the
/// on-disk directory. The owner calls `defuse()` once `BaseWorktree` is bound
/// and takes over cleanup via its own `Drop`.
///
/// With `panic = "abort"` on the release profile, this does not provide
/// panic-recovery cleanup (no unwind runs), but it is still load-bearing for
/// every early-return path between subprocess success and struct construction.
struct WorktreeCleanupGuard<'a> {
    repo_root: PathBuf,
    path: &'a Path,
    armed: bool,
}

impl<'a> WorktreeCleanupGuard<'a> {
    fn new(repo_root: &Path, path: &'a Path) -> Self {
        Self {
            repo_root: repo_root.to_path_buf(),
            path,
            armed: true,
        }
    }

    fn path(&self) -> &Path {
        self.path
    }

    /// Disarm in place. Idempotent; calling twice is harmless. Drop becomes a
    /// no-op after this returns.
    fn defuse(&mut self) {
        self.armed = false;
    }
}

impl Drop for WorktreeCleanupGuard<'_> {
    fn drop(&mut self) {
        if self.armed {
            remove_audit_worktree(&self.repo_root, self.path);
            let _ = std::fs::remove_dir_all(self.path);
        }
    }
}

/// Kernel-level advisory lock around the reusable-cache `reuse_or_create`
/// critical section, backed by `std::fs::File::try_lock` (stable since Rust
/// 1.89), which wraps `flock(2)` on Unix and `LockFileEx` on Windows.
/// Concurrent acquirers either fall through (`None`) or observe a
/// freshly-prepared cache after the holder releases.
struct ReusableWorktreeLock {
    // Drop on `File` calls the kernel's unlock automatically; we never call
    // `unlock_exclusive` explicitly.
    _file: std::fs::File,
}

impl ReusableWorktreeLock {
    fn try_acquire(reusable_path: &Path) -> Option<Self> {
        let lock_path = reusable_worktree_lock_path(reusable_path);
        // We never read the lock file's bytes, only its kernel-level lock
        // state, so set `truncate(false)` explicitly. Combining `O_TRUNC` with
        // `flock(2)` produced flaky `WouldBlock` returns under concurrent
        // acquire/release on macOS APFS during local tests.
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&lock_path)
            .ok()?;
        match file.try_lock() {
            Ok(()) => Some(Self { _file: file }),
            Err(std::fs::TryLockError::WouldBlock) => {
                tracing::debug!(
                    path = %lock_path.display(),
                    "reusable audit worktree lock contended; falling back to non-reusable worktree",
                );
                None
            }
            Err(std::fs::TryLockError::Error(err)) => {
                tracing::debug!(
                    path = %lock_path.display(),
                    error = %err,
                    "could not acquire reusable audit worktree lock; falling back to non-reusable worktree",
                );
                None
            }
        }
    }
}

fn reusable_worktree_lock_path(reusable_path: &Path) -> PathBuf {
    let mut name = reusable_path
        .file_name()
        .map(std::ffi::OsString::from)
        .unwrap_or_default();
    name.push(".lock");
    reusable_path
        .parent()
        .map_or_else(|| PathBuf::from(&name), |parent| parent.join(&name))
}

/// Default GC threshold for persistent reusable base-snapshot caches.
const DEFAULT_AUDIT_CACHE_MAX_AGE_DAYS: u32 = 30;

/// Env var that overrides `audit.cacheMaxAgeDays` from the config.
const AUDIT_CACHE_MAX_AGE_ENV: &str = "FALLOW_AUDIT_CACHE_MAX_AGE_DAYS";

/// Sidecar filename suffix used to track last-use of a reusable worktree.
const REUSABLE_LAST_USED_SUFFIX: &str = ".last-used";

/// Sidecar path for the "last used" timestamp of a reusable cache entry.
///
/// Lives next to the cache directory (NOT inside it) so the sidecar is
/// untouched by `git worktree add/remove` on the cache directory itself.
fn reusable_worktree_last_used_path(reusable_path: &Path) -> PathBuf {
    let mut name = reusable_path
        .file_name()
        .map(std::ffi::OsString::from)
        .unwrap_or_default();
    name.push(REUSABLE_LAST_USED_SUFFIX);
    reusable_path
        .parent()
        .map_or_else(|| PathBuf::from(&name), |parent| parent.join(&name))
}

/// Stamp the sidecar `.last-used` file's mtime to now.
///
/// Called on every cache-hit reuse (and from the pre-upgrade-grace branch
/// of the GC sweep) so the staleness signal stays current even when the
/// cache directory itself is not mutated. Failures are surfaced at
/// `warn!` so a persistent ENOSPC / read-only-tmp condition is visible at
/// default `RUST_LOG=warn`; the caller does not abort the audit.
fn touch_last_used(reusable_path: &Path) {
    let last_used = reusable_worktree_last_used_path(reusable_path);
    let result = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&last_used)
        .and_then(|file| file.set_modified(SystemTime::now()));
    if let Err(err) = result {
        tracing::warn!(
            path = %last_used.display(),
            error = %err,
            "failed to touch reusable audit worktree sidecar; staleness signal may not update",
        );
    }
}

/// Resolve the GC threshold for persistent reusable caches.
///
/// Precedence: `FALLOW_AUDIT_CACHE_MAX_AGE_DAYS` env var > `audit.cacheMaxAgeDays`
/// config field > 30-day default. `0` from either source disables the sweep
/// entirely (returns `None`). Invalid env values (non-integer) silently fall
/// back to config / default; audits do not fail on a typo in a runner env var.
fn resolve_cache_max_age(opts: &AuditOptions<'_>) -> Option<Duration> {
    if let Ok(raw) = std::env::var(AUDIT_CACHE_MAX_AGE_ENV) {
        if let Ok(days) = raw.trim().parse::<u32>() {
            return days_to_duration(days);
        }
        tracing::debug!(
            value = %raw,
            "FALLOW_AUDIT_CACHE_MAX_AGE_DAYS is not a valid u32; falling back to config/default",
        );
    }
    if let Some(days) = load_audit_config(opts).and_then(|c| c.cache_max_age_days) {
        return days_to_duration(days);
    }
    days_to_duration(DEFAULT_AUDIT_CACHE_MAX_AGE_DAYS)
}

fn days_to_duration(days: u32) -> Option<Duration> {
    if days == 0 {
        return None;
    }
    Some(Duration::from_secs(u64::from(days) * 86_400))
}

/// Load `AuditConfig` from `opts.config_path` (or auto-discover from
/// `opts.root`) for GC-threshold resolution only. Errors silently fall
/// back to `None`; the caller defaults to a 30-day window.
fn load_audit_config(opts: &AuditOptions<'_>) -> Option<AuditConfig> {
    if let Some(path) = opts.config_path {
        return fallow_config::FallowConfig::load(path)
            .ok()
            .map(|config| config.audit);
    }
    fallow_config::FallowConfig::find_and_load(opts.root)
        .ok()
        .flatten()
        .map(|(config, _path)| config.audit)
}

/// Remove persistent reusable base-snapshot worktree caches whose sidecar
/// `.last-used` file is older than `max_age`.
///
/// Concurrency: each candidate is gated by [`ReusableWorktreeLock`] before
/// removal, so an in-flight `fallow audit` mid-rebuild against the same
/// cache entry will not be disturbed (the sweep skips on contention).
///
/// Pre-upgrade caches lacking a sidecar are NOT removed: instead the sweep
/// seeds a fresh sidecar so the next invocation can age them from real
/// last-use. Without this grace, the dir's own mtime (= creation date on
/// POSIX) would wipe every legitimately-warm pre-upgrade cache on the
/// first run after upgrade.
///
/// The `.lock` sidecar file is intentionally NOT deleted on removal: a
/// racing acquirer of an unlinked-but-still-flocked inode plus a sibling
/// `open(O_CREAT)` at the same path would produce two processes each
/// holding a kernel flock on different inodes. Lock files are tens of
/// bytes; leaking them is harmless.
fn sweep_old_reusable_caches(repo_root: &Path, max_age: Duration, quiet: bool) {
    let Some(worktrees) = list_audit_worktrees(repo_root) else {
        return;
    };
    let now = SystemTime::now();
    let mut removed: u32 = 0;
    for path in worktrees {
        if !is_reusable_audit_worktree_path(&path) {
            continue;
        }
        let sidecar = reusable_worktree_last_used_path(&path);
        let sidecar_mtime = std::fs::metadata(&sidecar)
            .ok()
            .and_then(|m| m.modified().ok());
        let Some(mtime) = sidecar_mtime else {
            touch_last_used(&path);
            continue;
        };
        let Ok(age) = now.duration_since(mtime) else {
            continue;
        };
        if age < max_age {
            continue;
        }
        let Some(_lock) = ReusableWorktreeLock::try_acquire(&path) else {
            continue;
        };
        remove_audit_worktree(repo_root, &path);
        let dir_removed = match std::fs::remove_dir_all(&path) {
            Ok(()) => true,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => true,
            Err(err) => {
                tracing::warn!(
                    path = %path.display(),
                    error = %err,
                    "failed to remove stale reusable audit worktree directory; entry may leak",
                );
                false
            }
        };
        let _ = std::fs::remove_file(&sidecar);
        if dir_removed {
            removed += 1;
        }
    }
    if removed == 0 {
        return;
    }
    let mut command = Command::new("git");
    command
        .args(["worktree", "prune", "--expire=now"])
        .current_dir(repo_root);
    clear_ambient_git_env(&mut command);
    let _ = command.output();
    tracing::info!(
        count = removed,
        "reclaimed stale audit base-snapshot caches",
    );
    if !quiet {
        let s = plural(removed as usize);
        let _ = writeln!(
            std::io::stderr(),
            "fallow: reclaimed {removed} stale base-snapshot cache{s}",
        );
    }
}

fn reusable_audit_worktree_path(repo_root: &Path, base_sha: &str) -> PathBuf {
    let repo_root = git_toplevel(repo_root).unwrap_or_else(|| repo_root.to_path_buf());
    // `dunce::canonicalize` keeps the hash deterministic across Windows
    // callers that pass verbatim-vs-non-verbatim shapes for the same repo.
    let repo_root = dunce::canonicalize(&repo_root).unwrap_or(repo_root);
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
    // `dunce::canonicalize` strips Windows `\\?\` verbatim prefix so two
    // paths that differ only in prefix shape compare equal.
    match (dunce::canonicalize(left), dunce::canonicalize(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

/// Directories the audit base worktree shares with the host checkout.
///
/// `node_modules` is the original case: bare `git worktree add` lacks the
/// installed dependencies. `.nuxt` / `.astro` extend the same idea to
/// meta-framework `prepare` / `sync` outputs that the project gitignores;
/// without them the base pass cannot resolve tsconfig `references` chains
/// pointing into the generated tsconfigs and falls back to resolver-less
/// resolution. The trade-off matches `node_modules`: the symlinked dir is
/// HEAD-shaped, not base-shaped, but the alias resolution accuracy recovered
/// far outweighs the residual drift.
///
/// The meta-framework entries must stay aligned with the set recognized by
/// `missing_meta_framework_prerequisites` in `fallow_core`'s plugin registry.
/// Adding a framework's prepare-dir warning there without extending this list
/// silently reintroduces the broken-tsconfig-chain bug on the base pass for
/// that framework.
const MATERIALIZED_CONTEXT_DIRS: &[&str] = &["node_modules", ".nuxt", ".astro"];

fn materialize_base_dependency_context(repo_root: &Path, worktree_path: &Path) {
    for &name in MATERIALIZED_CONTEXT_DIRS {
        let source = repo_root.join(name);
        if !source.is_dir() {
            continue;
        }

        let destination = worktree_path.join(name);
        if destination.is_dir() {
            continue;
        }
        if let Ok(metadata) = std::fs::symlink_metadata(&destination) {
            if !metadata.file_type().is_symlink() {
                continue;
            }
            let _ = std::fs::remove_file(&destination);
        }

        let _ = symlink_dependency_dir(&source, &destination);
    }
}

#[cfg(unix)]
fn symlink_dependency_dir(source: &Path, destination: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(source, destination)
}

#[cfg(windows)]
fn symlink_dependency_dir(source: &Path, destination: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_dir(source, destination)
}

fn remove_audit_worktree(repo_root: &Path, path: &Path) {
    let mut command = Command::new("git");
    command
        .args([
            "worktree",
            "remove",
            "--force",
            path.to_string_lossy().as_ref(),
        ])
        .current_dir(repo_root);
    clear_ambient_git_env(&mut command);
    match crate::signal::scoped_child::output(&mut command) {
        Ok(output) => {
            // Only warn when an observable leak survives: the on-disk path still
            // exists after a non-zero `git worktree remove --force`. A missing
            // registration with no surviving directory is the partial-create
            // cleanup case and not noteworthy.
            if !output.status.success() && path.exists() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                tracing::warn!(
                    path = %path.display(),
                    stderr = %stderr.trim(),
                    "git worktree remove failed; the directory remains and may leak",
                );
            }
        }
        Err(err) => {
            tracing::warn!(
                path = %path.display(),
                error = %err,
                "git worktree remove subprocess failed to spawn",
            );
        }
    }
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
        let mut command = Command::new("git");
        command
            .args(["worktree", "prune", "--expire=now"])
            .current_dir(repo_root);
        clear_ambient_git_env(&mut command);
        let _ = command.output();
    }
}

fn list_audit_worktrees(repo_root: &Path) -> Option<Vec<PathBuf>> {
    let mut command = Command::new("git");
    command
        .args(["worktree", "list", "--porcelain"])
        .current_dir(repo_root);
    clear_ambient_git_env(&mut command);
    let output = command.output().ok()?;
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
    // `dunce::simplified` strips Windows `\\?\` verbatim prefix WITHOUT any
    // filesystem I/O, so this handles both verbatim and non-verbatim inputs
    // (synthetic test paths, real canonical paths from std OR dunce) without
    // requiring the path to actually exist on disk. The earlier
    // `dunce::canonicalize` attempt failed for the synthetic test paths in
    // `audit_worktree_helpers_filter_to_fallow_temp_prefix` because the
    // worktree dirs are constructed in-memory and never written.
    let simple_path = dunce::simplified(path);
    let simple_temp = dunce::simplified(&temp);
    if simple_path.starts_with(simple_temp) {
        return true;
    }
    // Fallback for symlinked temp dirs: canonicalize via std::fs (POSIX
    // resolves the symlink target; on Windows this also matches when path
    // canonicalises to something under temp).
    let Ok(canonical_temp) = std::fs::canonicalize(&temp) else {
        return false;
    };
    let simple_canonical_temp = dunce::simplified(&canonical_temp);
    simple_path.starts_with(simple_canonical_temp)
        || std::fs::canonicalize(path).is_ok_and(|canonical_path| {
            dunce::simplified(&canonical_path).starts_with(simple_canonical_temp)
        })
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
pub fn process_is_alive(pid: u32) -> bool {
    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .output()
        .is_ok_and(|output| output.status.success())
}

#[cfg(windows)]
pub fn process_is_alive(pid: u32) -> bool {
    windows_process::is_alive(pid)
}

#[cfg(not(any(unix, windows)))]
pub fn process_is_alive(_pid: u32) -> bool {
    // Conservative default on unknown platforms: treat every PID as alive so the
    // orphan sweep never removes anything we can't prove is dead.
    true
}

#[cfg(windows)]
#[allow(
    unsafe_code,
    reason = "Win32 process-query API (OpenProcess / WaitForSingleObject / CloseHandle / GetLastError) requires unsafe FFI"
)]
mod windows_process {
    use windows_sys::Win32::Foundation::{
        CloseHandle, ERROR_ACCESS_DENIED, ERROR_INVALID_PARAMETER, GetLastError, HANDLE,
        WAIT_OBJECT_0,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, WaitForSingleObject,
    };

    /// RAII wrapper that calls `CloseHandle` on drop, mirroring `std::mem::drop`
    /// semantics for kernel handles. Used so every exit path through
    /// `is_alive` releases the handle without manual cleanup.
    struct ProcessHandle(HANDLE);

    impl Drop for ProcessHandle {
        fn drop(&mut self) {
            // SAFETY: `self.0` is a non-null handle obtained from a successful
            // `OpenProcess` call. We have unique ownership (the value is only
            // ever created inside `is_alive`), so this is the sole consumer.
            unsafe {
                CloseHandle(self.0);
            }
        }
    }

    /// Cross-platform PID liveness check for Windows.
    ///
    /// Mirrors `kill -0 $pid` semantics: returns `true` when the process is
    /// running OR when we cannot prove it dead (e.g., `ERROR_ACCESS_DENIED` on
    /// processes owned by another session). Returns `false` only when the PID
    /// definitively does not exist (`ERROR_INVALID_PARAMETER`) or the wait
    /// reports the process has exited.
    pub(super) fn is_alive(pid: u32) -> bool {
        // SAFETY: `OpenProcess` accepts any `u32` PID; it either returns a
        // non-null handle we own, or null on failure with `GetLastError`
        // describing why. No memory is borrowed across the FFI boundary.
        let raw = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if raw.is_null() {
            // SAFETY: `GetLastError` reads thread-local storage set by the
            // failing `OpenProcess` call. It has no preconditions.
            let err = unsafe { GetLastError() };
            // The named `ERROR_ACCESS_DENIED` arm and the `_` arm map to the
            // same conservative default; the named arm is kept solely to
            // document the protected-process / cross-session case. Collapsing
            // would lose that documentation.
            #[expect(
                clippy::match_same_arms,
                reason = "named arm documents the cross-session protected-process case; collapsing loses that intent"
            )]
            return match err {
                // PID never existed or has already been fully reaped.
                ERROR_INVALID_PARAMETER => false,
                // Process exists but is owned by another session / under
                // protected access. Conservative default: treat as alive so we
                // never sweep a worktree owned by a live process we can't see.
                ERROR_ACCESS_DENIED => true,
                // Anything else (transient, unknown): conservative default.
                _ => true,
            };
        }
        let handle = ProcessHandle(raw);
        // `WaitForSingleObject(handle, 0)` returns `WAIT_OBJECT_0` (0) when the
        // process has exited and its handle is signalled, `WAIT_TIMEOUT` (0x102)
        // when the process is still running, and `WAIT_FAILED` (0xFFFF_FFFF) on
        // unexpected errors. We compare against `WAIT_OBJECT_0` specifically so
        // every other return value (including `WAIT_FAILED`) follows the
        // conservative default: treat as alive when we cannot prove the
        // process is dead.
        //
        // This is preferred over `GetExitCodeProcess + STILL_ACTIVE` because
        // `STILL_ACTIVE` (259) is a valid u32 exit code: a process that
        // legitimately exits with 259 would otherwise be misreported as alive.
        //
        // SAFETY: `handle.0` is non-null (checked above) and owned by the
        // `ProcessHandle` RAII wrapper.
        let wait_result = unsafe { WaitForSingleObject(handle.0, 0) };
        wait_result != WAIT_OBJECT_0
    }
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
    // `dunce::simplified` strips the Windows `\\?\` verbatim prefix when present
    // without touching the filesystem, so a path that came back from
    // `std::fs::canonicalize` (verbatim form on Windows) compares equal to a
    // path that did not (e.g., the BASE worktree path built via
    // `std::env::temp_dir().join(...)`). On POSIX `dunce::simplified` is a
    // no-op. Without this, audit's BASE-vs-HEAD finding-key intersection on
    // Windows produced 0 matches because `config.root` and `finding.path`
    // disagreed on the prefix shape, so every BASE key landed as a full
    // absolute path while HEAD keys landed as relative; the intersection
    // was empty and every pre-existing issue surfaced as "introduced".
    let simple_path = dunce::simplified(path);
    let simple_root = dunce::simplified(root);
    simple_path
        .strip_prefix(simple_root)
        .unwrap_or(simple_path)
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

fn unused_catalog_entry_key(
    item: &fallow_core::results::UnusedCatalogEntry,
    root: &Path,
) -> String {
    format!(
        "unused-catalog-entry:{}:{}:{}:{}",
        relative_key_path(&item.path, root),
        item.line,
        item.catalog_name,
        item.entry_name
    )
}

fn empty_catalog_group_key(item: &fallow_core::results::EmptyCatalogGroup, root: &Path) -> String {
    format!(
        "empty-catalog-group:{}:{}:{}",
        relative_key_path(&item.path, root),
        item.line,
        item.catalog_name
    )
}

#[expect(
    clippy::too_many_lines,
    reason = "one key-builder block per issue type keeps the audit-attribution key shape local and easy to audit; the count grows linearly with new issue types"
)]
fn dead_code_keys(
    results: &fallow_core::results::AnalysisResults,
    root: &Path,
) -> FxHashSet<String> {
    let mut keys = FxHashSet::default();
    for item in &results.unused_files {
        keys.insert(format!(
            "unused-file:{}",
            relative_key_path(&item.file.path, root)
        ));
    }
    for item in &results.unused_exports {
        keys.insert(format!(
            "unused-export:{}:{}",
            relative_key_path(&item.export.path, root),
            item.export.export_name
        ));
    }
    for item in &results.unused_types {
        keys.insert(format!(
            "unused-type:{}:{}",
            relative_key_path(&item.export.path, root),
            item.export.export_name
        ));
    }
    for item in &results.private_type_leaks {
        keys.insert(format!(
            "private-type-leak:{}:{}:{}",
            relative_key_path(&item.leak.path, root),
            item.leak.export_name,
            item.leak.type_name
        ));
    }
    for item in results
        .unused_dependencies
        .iter()
        .map(|f| &f.dep)
        .chain(results.unused_dev_dependencies.iter().map(|f| &f.dep))
        .chain(results.unused_optional_dependencies.iter().map(|f| &f.dep))
    {
        keys.insert(unused_dependency_key(item, root));
    }
    for item in &results.unused_enum_members {
        keys.insert(unused_member_key("unused-enum-member", &item.member, root));
    }
    for item in &results.unused_class_members {
        keys.insert(unused_member_key("unused-class-member", &item.member, root));
    }
    for item in &results.unresolved_imports {
        keys.insert(format!(
            "unresolved-import:{}:{}",
            relative_key_path(&item.import.path, root),
            item.import.specifier
        ));
    }
    for item in results.unlisted_dependencies.iter().map(|f| &f.dep) {
        keys.insert(unlisted_dependency_key(item, root));
    }
    for item in &results.duplicate_exports {
        let mut locations: Vec<String> = item
            .export
            .locations
            .iter()
            .map(|loc| relative_key_path(&loc.path, root))
            .collect();
        locations.sort();
        locations.dedup();
        keys.insert(format!(
            "duplicate-export:{}:{}",
            item.export.export_name,
            locations.join("|")
        ));
    }
    for item in &results.type_only_dependencies {
        keys.insert(format!(
            "type-only-dependency:{}:{}",
            relative_key_path(&item.dep.path, root),
            item.dep.package_name
        ));
    }
    for item in &results.test_only_dependencies {
        keys.insert(format!(
            "test-only-dependency:{}:{}",
            relative_key_path(&item.dep.path, root),
            item.dep.package_name
        ));
    }
    for item in &results.circular_dependencies {
        let mut files: Vec<String> = item
            .cycle
            .files
            .iter()
            .map(|path| relative_key_path(path, root))
            .collect();
        files.sort();
        keys.insert(format!("circular-dependency:{}", files.join("|")));
    }
    for item in &results.re_export_cycles {
        // Prefix the audit-gate key with the kind discriminator so self-loops
        // cannot keyspace-collide with future single-file multi-node shapes
        // (panel catch #7; same rationale as `baseline.rs::re_export_cycle_key`).
        let kind = match item.cycle.kind {
            fallow_core::results::ReExportCycleKind::MultiNode => "multi-node",
            fallow_core::results::ReExportCycleKind::SelfLoop => "self-loop",
        };
        let mut files: Vec<String> = item
            .cycle
            .files
            .iter()
            .map(|path| relative_key_path(path, root))
            .collect();
        files.sort();
        keys.insert(format!("re-export-cycle:{kind}:{}", files.join("|")));
    }
    for item in &results.boundary_violations {
        keys.insert(format!(
            "boundary-violation:{}:{}:{}",
            relative_key_path(&item.violation.from_path, root),
            relative_key_path(&item.violation.to_path, root),
            item.violation.import_specifier
        ));
    }
    for item in &results.stale_suppressions {
        keys.insert(format!(
            "stale-suppression:{}:{}",
            relative_key_path(&item.path, root),
            item.description()
        ));
    }
    for item in &results.unresolved_catalog_references {
        keys.insert(format!(
            "unresolved-catalog-reference:{}:{}:{}:{}",
            relative_key_path(&item.reference.path, root),
            item.reference.line,
            item.reference.catalog_name,
            item.reference.entry_name
        ));
    }
    for item in &results.unused_catalog_entries {
        keys.insert(unused_catalog_entry_key(&item.entry, root));
    }
    for item in &results.empty_catalog_groups {
        keys.insert(empty_catalog_group_key(&item.group, root));
    }
    for item in &results.unused_dependency_overrides {
        keys.insert(format!(
            "unused-dependency-override:{}:{}:{}",
            relative_key_path(&item.entry.path, root),
            item.entry.line,
            item.entry.raw_key
        ));
    }
    for item in &results.misconfigured_dependency_overrides {
        keys.insert(format!(
            "misconfigured-dependency-override:{}:{}:{}",
            relative_key_path(&item.entry.path, root),
            item.entry.line,
            item.entry.raw_key
        ));
    }
    keys
}

#[expect(
    clippy::too_many_lines,
    reason = "one retain block per issue type keeps the gate-filter local and grep-friendly; the count grows linearly with new issue types and parallels dead_code_keys"
)]
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
            relative_key_path(&item.file.path, root)
        ))
    });
    results.unused_exports.retain(|item| {
        !base.contains(&format!(
            "unused-export:{}:{}",
            relative_key_path(&item.export.path, root),
            item.export.export_name
        ))
    });
    results.unused_types.retain(|item| {
        !base.contains(&format!(
            "unused-type:{}:{}",
            relative_key_path(&item.export.path, root),
            item.export.export_name
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
            relative_key_path(&item.leak.path, root),
            item.leak.export_name,
            item.leak.type_name
        ))
    });
    results
        .unused_dependencies
        .retain(|item| keep(unused_dependency_key(&item.dep, root)));
    results
        .unused_dev_dependencies
        .retain(|item| keep(unused_dependency_key(&item.dep, root)));
    results
        .unused_optional_dependencies
        .retain(|item| keep(unused_dependency_key(&item.dep, root)));
    results
        .unused_enum_members
        .retain(|item| keep(unused_member_key("unused-enum-member", &item.member, root)));
    results
        .unused_class_members
        .retain(|item| keep(unused_member_key("unused-class-member", &item.member, root)));
    results.unresolved_imports.retain(|item| {
        keep(format!(
            "unresolved-import:{}:{}",
            relative_key_path(&item.import.path, root),
            item.import.specifier
        ))
    });
    results
        .unlisted_dependencies
        .retain(|item| keep(unlisted_dependency_key(&item.dep, root)));
    results.duplicate_exports.retain(|item| {
        let mut locations: Vec<String> = item
            .export
            .locations
            .iter()
            .map(|loc| relative_key_path(&loc.path, root))
            .collect();
        locations.sort();
        locations.dedup();
        keep(format!(
            "duplicate-export:{}:{}",
            item.export.export_name,
            locations.join("|")
        ))
    });
    results.type_only_dependencies.retain(|item| {
        keep(format!(
            "type-only-dependency:{}:{}",
            relative_key_path(&item.dep.path, root),
            item.dep.package_name
        ))
    });
    results.test_only_dependencies.retain(|item| {
        keep(format!(
            "test-only-dependency:{}:{}",
            relative_key_path(&item.dep.path, root),
            item.dep.package_name
        ))
    });
    results.circular_dependencies.retain(|item| {
        let mut files: Vec<String> = item
            .cycle
            .files
            .iter()
            .map(|path| relative_key_path(path, root))
            .collect();
        files.sort();
        keep(format!("circular-dependency:{}", files.join("|")))
    });
    results.re_export_cycles.retain(|item| {
        let kind = match item.cycle.kind {
            fallow_core::results::ReExportCycleKind::MultiNode => "multi-node",
            fallow_core::results::ReExportCycleKind::SelfLoop => "self-loop",
        };
        let mut files: Vec<String> = item
            .cycle
            .files
            .iter()
            .map(|path| relative_key_path(path, root))
            .collect();
        files.sort();
        keep(format!("re-export-cycle:{kind}:{}", files.join("|")))
    });
    results.boundary_violations.retain(|item| {
        keep(format!(
            "boundary-violation:{}:{}:{}",
            relative_key_path(&item.violation.from_path, root),
            relative_key_path(&item.violation.to_path, root),
            item.violation.import_specifier
        ))
    });
    results.stale_suppressions.retain(|item| {
        keep(format!(
            "stale-suppression:{}:{}",
            relative_key_path(&item.path, root),
            item.description()
        ))
    });
    results.unresolved_catalog_references.retain(|item| {
        keep(format!(
            "unresolved-catalog-reference:{}:{}:{}:{}",
            relative_key_path(&item.reference.path, root),
            item.reference.line,
            item.reference.catalog_name,
            item.reference.entry_name
        ))
    });
    results
        .unused_catalog_entries
        .retain(|item| keep(unused_catalog_entry_key(&item.entry, root)));
    results
        .empty_catalog_groups
        .retain(|item| keep(empty_catalog_group_key(&item.group, root)));
    results.unused_dependency_overrides.retain(|item| {
        keep(format!(
            "unused-dependency-override:{}:{}:{}",
            relative_key_path(&item.entry.path, root),
            item.entry.line,
            item.entry.raw_key
        ))
    });
    results.misconfigured_dependency_overrides.retain(|item| {
        keep(format!(
            "misconfigured-dependency-override:{}:{}:{}",
            relative_key_path(&item.entry.path, root),
            item.entry.line,
            item.entry.raw_key
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
                &format!("unused-file:{}", relative_key_path(&item.file.path, root)),
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
                    relative_key_path(&item.export.path, root),
                    item.export.export_name
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
                    relative_key_path(&item.export.path, root),
                    item.export.export_name
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
                    relative_key_path(&item.leak.path, root),
                    item.leak.export_name,
                    item.leak.type_name
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
            .map(|item| issue_was_introduced(&unused_dependency_key(&item.dep, root), base)),
    );
    annotate_issue_array(
        json,
        "unused_dev_dependencies",
        results
            .unused_dev_dependencies
            .iter()
            .map(|item| issue_was_introduced(&unused_dependency_key(&item.dep, root), base)),
    );
    annotate_issue_array(
        json,
        "unused_optional_dependencies",
        results
            .unused_optional_dependencies
            .iter()
            .map(|item| issue_was_introduced(&unused_dependency_key(&item.dep, root), base)),
    );
    annotate_issue_array(
        json,
        "unused_enum_members",
        results.unused_enum_members.iter().map(|item| {
            issue_was_introduced(
                &unused_member_key("unused-enum-member", &item.member, root),
                base,
            )
        }),
    );
    annotate_issue_array(
        json,
        "unused_class_members",
        results.unused_class_members.iter().map(|item| {
            issue_was_introduced(
                &unused_member_key("unused-class-member", &item.member, root),
                base,
            )
        }),
    );
    annotate_issue_array(
        json,
        "unresolved_imports",
        results.unresolved_imports.iter().map(|item| {
            issue_was_introduced(
                &format!(
                    "unresolved-import:{}:{}",
                    relative_key_path(&item.import.path, root),
                    item.import.specifier
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
            .map(|item| issue_was_introduced(&unlisted_dependency_key(&item.dep, root), base)),
    );
    annotate_issue_array(
        json,
        "duplicate_exports",
        results.duplicate_exports.iter().map(|item| {
            let mut locations: Vec<String> = item
                .export
                .locations
                .iter()
                .map(|loc| relative_key_path(&loc.path, root))
                .collect();
            locations.sort();
            locations.dedup();
            issue_was_introduced(
                &format!(
                    "duplicate-export:{}:{}",
                    item.export.export_name,
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
                    relative_key_path(&item.dep.path, root),
                    item.dep.package_name
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
                    relative_key_path(&item.dep.path, root),
                    item.dep.package_name
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
                .cycle
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
        "re_export_cycles",
        results.re_export_cycles.iter().map(|item| {
            let kind = match item.cycle.kind {
                fallow_core::results::ReExportCycleKind::MultiNode => "multi-node",
                fallow_core::results::ReExportCycleKind::SelfLoop => "self-loop",
            };
            let mut files: Vec<String> = item
                .cycle
                .files
                .iter()
                .map(|path| relative_key_path(path, root))
                .collect();
            files.sort();
            issue_was_introduced(&format!("re-export-cycle:{kind}:{}", files.join("|")), base)
        }),
    );
    annotate_issue_array(
        json,
        "boundary_violations",
        results.boundary_violations.iter().map(|item| {
            issue_was_introduced(
                &format!(
                    "boundary-violation:{}:{}:{}",
                    relative_key_path(&item.violation.from_path, root),
                    relative_key_path(&item.violation.to_path, root),
                    item.violation.import_specifier
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
    annotate_issue_array(
        json,
        "unresolved_catalog_references",
        results.unresolved_catalog_references.iter().map(|item| {
            issue_was_introduced(
                &format!(
                    "unresolved-catalog-reference:{}:{}:{}:{}",
                    relative_key_path(&item.reference.path, root),
                    item.reference.line,
                    item.reference.catalog_name,
                    item.reference.entry_name
                ),
                base,
            )
        }),
    );
    annotate_issue_array(
        json,
        "unused_catalog_entries",
        results
            .unused_catalog_entries
            .iter()
            .map(|item| issue_was_introduced(&unused_catalog_entry_key(&item.entry, root), base)),
    );
    annotate_issue_array(
        json,
        "empty_catalog_groups",
        results
            .empty_catalog_groups
            .iter()
            .map(|item| issue_was_introduced(&empty_catalog_group_key(&item.group, root), base)),
    );
    annotate_issue_array(
        json,
        "unused_dependency_overrides",
        results.unused_dependency_overrides.iter().map(|item| {
            issue_was_introduced(
                &format!(
                    "unused-dependency-override:{}:{}:{}",
                    relative_key_path(&item.entry.path, root),
                    item.entry.line,
                    item.entry.raw_key
                ),
                base,
            )
        }),
    );
    annotate_issue_array(
        json,
        "misconfigured_dependency_overrides",
        results
            .misconfigured_dependency_overrides
            .iter()
            .map(|item| {
                issue_was_introduced(
                    &format!(
                        "misconfigured-dependency-override:{}:{}:{}",
                        relative_key_path(&item.entry.path, root),
                        item.entry.line,
                        item.entry.raw_key
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

fn health_finding_key(finding: &crate::health_types::ComplexityViolation, root: &Path) -> String {
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

    // Age-based GC of persistent reusable base-snapshot caches. Runs on
    // every invocation (not gated on whether this audit needs a real
    // base snapshot) so disk-reclaim happens even when this run is fully
    // cache-warm. Skipped entirely when the user sets
    // `FALLOW_AUDIT_CACHE_MAX_AGE_DAYS=0` or `audit.cacheMaxAgeDays = 0`.
    if let Some(max_age) = resolve_cache_max_age(opts) {
        sweep_old_reusable_caches(opts.root, max_age, opts.quiet);
    }

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
        diff_index: None,
        use_shared_diff_index: true,
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
            output: opts.output,
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
        min_occurrences: Some(dupes_cfg.min_occurrences),
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
        diff_index: None,
        use_shared_diff_index: true,
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
    // Build runtime-coverage sidecar options when --runtime-coverage was
    // supplied. License JWT loading + 7/30/hard-fail grace evaluation
    // happen inside prepare_options; an exit here means the user is past
    // the hard-fail line and audit cannot proceed.
    let runtime_coverage = match opts.runtime_coverage {
        Some(path) => match crate::health::coverage::prepare_options(
            path,
            opts.min_invocations_hot,
            None,
            None,
            opts.output,
        ) {
            Ok(options) => Some(options),
            Err(code) => return Err(code),
        },
        None => None,
    };

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
        diff_index: None,
        use_shared_diff_index: true,
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
        coverage: opts.coverage,
        coverage_root: opts.coverage_root,
        performance: opts.performance,
        min_severity: None,
        runtime_coverage,
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
        OutputFormat::PrCommentGithub => {
            let value = build_audit_codeclimate(result);
            report::ci::pr_comment::print_pr_comment(
                "audit",
                report::ci::pr_comment::Provider::Github,
                &value,
            )
        }
        OutputFormat::PrCommentGitlab => {
            let value = build_audit_codeclimate(result);
            report::ci::pr_comment::print_pr_comment(
                "audit",
                report::ci::pr_comment::Provider::Gitlab,
                &value,
            )
        }
        OutputFormat::ReviewGithub => {
            let value = build_audit_codeclimate(result);
            report::ci::review::print_review_envelope(
                "audit",
                report::ci::pr_comment::Provider::Github,
                &value,
            )
        }
        OutputFormat::ReviewGitlab => {
            let value = build_audit_codeclimate(result);
            report::ci::review::print_review_envelope(
                "audit",
                report::ci::pr_comment::Provider::Gitlab,
                &value,
            )
        }
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
                "Tip: run `fallow explain <issue label>`; spaces and hyphens both work, e.g. `fallow explain unused files`."
                    .dimmed()
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
                    summary_heading: true,
                    show_explain_tip: false,
                },
            );
        }

        if has_dupe_groups && let Some(ref dupes) = result.dupes {
            if show_headers {
                eprintln!();
                eprintln!("── Duplication ────────────────────────────────────");
            }
            crate::dupes::print_dupes_result(dupes, quiet, explain, false, true, false);
        }

        if has_health_findings && let Some(ref health) = result.health {
            if show_headers {
                eprintln!();
                eprintln!("── Complexity ─────────────────────────────────────");
            }
            // `fallow audit` does not surface the health score / trend block
            // (no orientation header), so let the standalone health renderer
            // emit it inline like `fallow health`.
            crate::health::print_health_result(
                health, quiet, explain, None, None, false, true, false, false,
            );
        }
    }

    if !has_dupe_groups && let Some(ref dupes) = result.dupes {
        crate::dupes::print_default_ignore_note(dupes, quiet);
        crate::dupes::print_min_occurrences_note(dupes, quiet);
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
    obj.insert(
        "schema_version".into(),
        serde_json::Value::Number(crate::report::SCHEMA_VERSION.into()),
    );
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
        match report::build_json_with_config_fixable(
            &check.results,
            &check.config.root,
            check.elapsed,
            check.config_fixable,
        ) {
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
        let payload = crate::output_dupes::DupesReportPayload::from_report(&dupes.report);
        match serde_json::to_value(&payload) {
            Ok(mut json) => {
                let root_prefix = format!("{}/", dupes.config.root.display());
                report::strip_root_prefix(&mut json, &root_prefix);
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

    let mut output = serde_json::Value::Object(obj);
    report::harmonize_multi_kind_suppress_line_actions(&mut output);
    report::emit_json(&output, "audit")
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
    let value = build_audit_codeclimate(result);
    report::emit_json(&value, "CodeClimate audit")
}

fn build_audit_codeclimate(result: &AuditResult) -> serde_json::Value {
    let mut all_issues: Vec<crate::output_envelope::CodeClimateIssue> = Vec::new();

    if let Some(ref check) = result.check {
        all_issues.extend(report::build_codeclimate(
            &check.results,
            &check.config.root,
            &check.config.rules,
        ));
    }

    if let Some(ref dupes) = result.dupes {
        all_issues.extend(report::build_duplication_codeclimate(
            &dupes.report,
            &dupes.config.root,
        ));
    }

    if let Some(ref health) = result.health {
        all_issues.extend(report::build_health_codeclimate(
            &health.report,
            &health.config.root,
        ));
    }

    serde_json::to_value(&all_issues).expect("CodeClimateIssue serializes infallibly")
}

// ── Entry point ──────────────────────────────────────────────────

/// Run the full audit command: execute analyses, print results, return exit code.
/// Run audit, optionally tagged with a gate marker (e.g. `"pre-commit"`) so
/// Fallow Impact can record a containment event when the gate blocks then
/// clears. The marker only affects the local Impact store; it never changes
/// the verdict, exit code, or output.
pub fn run_audit(opts: &AuditOptions<'_>, gate_marker: Option<&str>) -> ExitCode {
    if let Err(e) = crate::health::scoring::validate_coverage_root_absolute(opts.coverage_root) {
        return emit_error(&e, 2, opts.output);
    }
    // Resolve the coverage input path to absolute UP FRONT, against the user's
    // original `--root`. The base-snapshot recursion in `compute_base_snapshot`
    // swaps `--root` to a temp worktree directory, so a relative path that
    // worked at the entry would re-resolve against the worktree (which doesn't
    // contain the coverage file) on the recursive pass. Resolving once at the
    // top means downstream `resolve_relative_to_root` calls become no-ops on
    // an already-absolute path, regardless of which `--root` is in effect.
    let coverage_resolved = opts
        .coverage
        .map(|p| crate::health::scoring::resolve_relative_to_root(p, Some(opts.root)));
    // Absolutize runtime_coverage at the public entry for the same
    // reason coverage is absolutized: `compute_base_snapshot` swaps
    // `opts.root` to a temp worktree directory, and any relative path
    // would re-resolve against that worktree on the recursive base
    // pass. The diff source is resolved separately by `main()` into
    // the process-wide shared-diff cache before audit even runs, so
    // it does not need entry-point absolutization here.
    let runtime_coverage_resolved = opts
        .runtime_coverage
        .map(|p| crate::health::scoring::resolve_relative_to_root(p, Some(opts.root)));
    let resolved_opts = AuditOptions {
        coverage: coverage_resolved.as_deref(),
        runtime_coverage: runtime_coverage_resolved.as_deref(),
        ..*opts
    };
    match execute_audit(&resolved_opts) {
        Ok(result) => {
            // Best-effort: record this run into the local Impact store. No-op
            // when Impact tracking is disabled; never affects exit/output.
            crate::impact::record_audit_run(
                opts.root,
                &result.summary,
                result.verdict,
                gate_marker.is_some(),
                result.head_sha.as_deref(),
                env!("CARGO_PKG_VERSION"),
                &crate::vital_signs::chrono_timestamp(),
            );
            print_audit_result(&result, opts.quiet, opts.explain)
        }
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

    /// Initialize a throwaway git repo with a single commit and return its root.
    /// Used by the worktree-lifecycle tests below as a parent repo that can host
    /// `git worktree add` invocations.
    fn init_throwaway_repo(parent: &std::path::Path, name: &str) -> PathBuf {
        let root = parent.join(name);
        fs::create_dir_all(&root).expect("repo root should be created");
        fs::write(root.join("README.md"), "seed\n").expect("seed file should be written");
        git(&root, &["init", "-b", "main"]);
        git(&root, &["add", "."]);
        git(
            &root,
            &["-c", "commit.gpgsign=false", "commit", "-m", "initial"],
        );
        root
    }

    fn worktree_is_registered_with_git(repo_root: &std::path::Path, worktree_path: &Path) -> bool {
        list_audit_worktrees(repo_root)
            .is_some_and(|paths| paths.iter().any(|p| paths_equal(p, worktree_path)))
    }

    #[test]
    fn worktree_cleanup_guard_runs_on_drop() {
        let tmp = tempfile::TempDir::new().expect("temp dir should be created");
        let repo = init_throwaway_repo(tmp.path(), "repo");
        let worktree_path = tmp.path().join("fallow-audit-base-1234-5678");

        // Register a real worktree with git so the guard's `git worktree remove`
        // has something concrete to roll back.
        git(
            &repo,
            &[
                "worktree",
                "add",
                "--detach",
                "--quiet",
                worktree_path.to_str().expect("path is utf-8"),
                "HEAD",
            ],
        );
        assert!(worktree_path.is_dir());
        assert!(worktree_is_registered_with_git(&repo, &worktree_path));

        {
            let _guard = WorktreeCleanupGuard::new(&repo, &worktree_path);
            // Guard drops at end of scope without `defuse()`.
        }

        assert!(
            !worktree_path.exists(),
            "guard Drop should remove the worktree directory",
        );
        assert!(
            !worktree_is_registered_with_git(&repo, &worktree_path),
            "guard Drop should remove the git worktree registration",
        );
    }

    #[test]
    fn worktree_cleanup_guard_defused_skips_drop() {
        let tmp = tempfile::TempDir::new().expect("temp dir should be created");
        let repo = init_throwaway_repo(tmp.path(), "repo");
        let worktree_path = tmp.path().join("fallow-audit-base-1234-5679");

        git(
            &repo,
            &[
                "worktree",
                "add",
                "--detach",
                "--quiet",
                worktree_path.to_str().expect("path is utf-8"),
                "HEAD",
            ],
        );
        assert!(worktree_path.is_dir());

        {
            let mut guard = WorktreeCleanupGuard::new(&repo, &worktree_path);
            guard.defuse();
            // Idempotent: a second defuse must not panic.
            guard.defuse();
        }

        assert!(
            worktree_path.is_dir(),
            "defused guard must not remove the worktree on drop",
        );
        assert!(
            worktree_is_registered_with_git(&repo, &worktree_path),
            "defused guard must not unregister the worktree from git",
        );

        // Clean up manually so the tempdir teardown does not race git's lock files.
        remove_audit_worktree(&repo, &worktree_path);
        let _ = fs::remove_dir_all(&worktree_path);
    }

    #[test]
    fn audit_orphan_sweep_removes_dead_pid_worktree() {
        // Use a PID well above all platforms' typical and maximum ranges:
        //   - Linux:  pid_max defaults to 32 768, max cap 4 194 304 (2^22)
        //   - macOS:  kern.maxproc defaults to 99 998
        //   - Windows: PIDs are multiples of 4; 99 999 999 mod 4 == 3, so it
        //     cannot be a valid Windows PID either.
        // 99 999 999 exceeds all three.
        const DEAD_PID: u32 = 99_999_999;
        assert!(!process_is_alive(DEAD_PID));

        let tmp = tempfile::TempDir::new().expect("temp dir should be created");
        let repo = init_throwaway_repo(tmp.path(), "repo");

        // The sweep only considers worktrees whose parent is the system temp dir.
        // Mirror that here so the test exercises the real filter path.
        let worktree_path = std::env::temp_dir().join(format!(
            "fallow-audit-base-{}-{}",
            DEAD_PID,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock should be after epoch")
                .as_nanos()
        ));
        git(
            &repo,
            &[
                "worktree",
                "add",
                "--detach",
                "--quiet",
                worktree_path.to_str().expect("path is utf-8"),
                "HEAD",
            ],
        );
        assert!(worktree_path.is_dir());
        assert!(worktree_is_registered_with_git(&repo, &worktree_path));

        sweep_orphan_audit_worktrees(&repo);

        assert!(
            !worktree_path.exists(),
            "sweep should remove worktree owned by a dead PID",
        );
        assert!(
            !worktree_is_registered_with_git(&repo, &worktree_path),
            "sweep should unregister worktree owned by a dead PID",
        );
    }

    #[test]
    fn audit_orphan_sweep_keeps_live_pid_worktree() {
        let live_pid = std::process::id();
        assert!(process_is_alive(live_pid));

        let tmp = tempfile::TempDir::new().expect("temp dir should be created");
        let repo = init_throwaway_repo(tmp.path(), "repo");

        let worktree_path = std::env::temp_dir().join(format!(
            "fallow-audit-base-{}-{}",
            live_pid,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock should be after epoch")
                .as_nanos()
        ));
        git(
            &repo,
            &[
                "worktree",
                "add",
                "--detach",
                "--quiet",
                worktree_path.to_str().expect("path is utf-8"),
                "HEAD",
            ],
        );

        sweep_orphan_audit_worktrees(&repo);

        assert!(
            worktree_path.is_dir(),
            "sweep must not remove worktree owned by a live PID",
        );
        assert!(
            worktree_is_registered_with_git(&repo, &worktree_path),
            "sweep must not unregister worktree owned by a live PID",
        );

        // Tear down the live-PID worktree so it does not leak across tests.
        remove_audit_worktree(&repo, &worktree_path);
        let _ = fs::remove_dir_all(&worktree_path);
    }

    /// Build a reusable-shaped worktree path inside the system tempdir
    /// (so `is_reusable_audit_worktree_path` and `path_is_inside_temp_dir`
    /// both match), uniquified by nanos so parallel tests do not collide.
    fn make_reusable_path(label: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock should be after epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("fallow-audit-base-cache-{label}-{nanos:032x}"))
    }

    /// Register a worktree with the parent repo at `path` checked out at HEAD.
    /// Mirrors what `BaseWorktree::reuse_or_create` does for the fresh-create
    /// path so the GC sweep tests can build real cache entries.
    fn register_reusable_worktree(repo: &Path, path: &Path) {
        git(
            repo,
            &[
                "worktree",
                "add",
                "--detach",
                "--quiet",
                path.to_str().expect("path is utf-8"),
                "HEAD",
            ],
        );
    }

    fn write_sidecar_with_age(path: &Path, age: Duration) {
        let sidecar = reusable_worktree_last_used_path(path);
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&sidecar)
            .expect("sidecar should open");
        let when = SystemTime::now()
            .checked_sub(age)
            .expect("backdated time should fit in SystemTime");
        file.set_modified(when)
            .expect("set_modified should succeed");
    }

    /// Tear down a reusable worktree (git registration + dir + sidecar + lock)
    /// regardless of which of those the test created. Idempotent.
    fn cleanup_reusable_worktree(repo: &Path, path: &Path) {
        remove_audit_worktree(repo, path);
        let _ = fs::remove_dir_all(path);
        let _ = fs::remove_file(reusable_worktree_last_used_path(path));
        let _ = fs::remove_file(reusable_worktree_lock_path(path));
    }

    #[test]
    fn reusable_cache_gc_removes_old_entry_with_backdated_sidecar() {
        let tmp = tempfile::TempDir::new().expect("temp dir should be created");
        let repo = init_throwaway_repo(tmp.path(), "repo-gc-remove");
        let worktree_path = make_reusable_path("gc-remove");
        register_reusable_worktree(&repo, &worktree_path);
        write_sidecar_with_age(&worktree_path, Duration::from_hours(31 * 24));

        sweep_old_reusable_caches(&repo, Duration::from_hours(30 * 24), true);

        assert!(
            !worktree_path.exists(),
            "sweep should remove worktree dir whose sidecar is older than the threshold",
        );
        assert!(
            !worktree_is_registered_with_git(&repo, &worktree_path),
            "sweep should unregister the worktree from git",
        );
        assert!(
            !reusable_worktree_last_used_path(&worktree_path).exists(),
            "sweep should remove the sidecar `.last-used` file alongside the worktree",
        );
        // Lock file may or may not exist; it is created only when
        // `try_acquire` is called. We do NOT assert on it here.
        cleanup_reusable_worktree(&repo, &worktree_path);
    }

    #[test]
    fn reusable_cache_gc_keeps_fresh_entry() {
        let tmp = tempfile::TempDir::new().expect("temp dir should be created");
        let repo = init_throwaway_repo(tmp.path(), "repo-gc-keep");
        let worktree_path = make_reusable_path("gc-keep");
        register_reusable_worktree(&repo, &worktree_path);
        write_sidecar_with_age(&worktree_path, Duration::from_mins(1));

        sweep_old_reusable_caches(&repo, Duration::from_hours(30 * 24), true);

        assert!(
            worktree_path.is_dir(),
            "sweep must not remove a worktree whose sidecar is fresher than the threshold",
        );
        assert!(
            worktree_is_registered_with_git(&repo, &worktree_path),
            "sweep must not unregister a fresh worktree",
        );
        cleanup_reusable_worktree(&repo, &worktree_path);
    }

    #[test]
    fn reusable_cache_gc_skips_locked_entry() {
        let tmp = tempfile::TempDir::new().expect("temp dir should be created");
        let repo = init_throwaway_repo(tmp.path(), "repo-gc-locked");
        let worktree_path = make_reusable_path("gc-locked");
        register_reusable_worktree(&repo, &worktree_path);
        write_sidecar_with_age(&worktree_path, Duration::from_hours(31 * 24));

        // Hold the lock from this thread so the sweep's `try_acquire`
        // observes contention and skips the entry. Drop after the sweep.
        let lock = ReusableWorktreeLock::try_acquire(&worktree_path)
            .expect("test should acquire the lock first");

        sweep_old_reusable_caches(&repo, Duration::from_hours(30 * 24), true);

        assert!(
            worktree_path.is_dir(),
            "sweep must skip a locked entry even when its sidecar is stale",
        );
        assert!(
            worktree_is_registered_with_git(&repo, &worktree_path),
            "sweep must not unregister a locked entry",
        );
        drop(lock);
        cleanup_reusable_worktree(&repo, &worktree_path);
    }

    #[test]
    fn reusable_cache_gc_grace_when_sidecar_absent() {
        let tmp = tempfile::TempDir::new().expect("temp dir should be created");
        let repo = init_throwaway_repo(tmp.path(), "repo-gc-grace");
        let worktree_path = make_reusable_path("gc-grace");
        register_reusable_worktree(&repo, &worktree_path);
        // No sidecar written. Backdate the dir's own mtime so that "fall back
        // to dir mtime" would falsely trigger removal; the grace path must
        // NOT consult dir mtime.
        // (Skipping dir mtime backdate is fine: the implementation never
        // reads it, so the assertion is structural: sidecar absent => keep.)
        let sidecar = reusable_worktree_last_used_path(&worktree_path);
        assert!(
            !sidecar.exists(),
            "test pre-condition: sidecar should not exist",
        );

        sweep_old_reusable_caches(&repo, Duration::from_hours(30 * 24), true);

        assert!(
            worktree_path.is_dir(),
            "pre-upgrade grace: sidecar-absent entries must NOT be removed on first encounter",
        );
        assert!(
            sidecar.exists(),
            "pre-upgrade grace: sidecar must be seeded so the next run can age from real last-used",
        );
        let mtime = std::fs::metadata(&sidecar)
            .and_then(|m| m.modified())
            .expect("seeded sidecar should have a readable mtime");
        let age = SystemTime::now()
            .duration_since(mtime)
            .unwrap_or(Duration::ZERO);
        assert!(
            age < Duration::from_mins(1),
            "seeded sidecar mtime should be near `now()`, got age {age:?}",
        );
        cleanup_reusable_worktree(&repo, &worktree_path);
    }

    #[test]
    fn reusable_cache_gc_preserves_lock_file_after_removal() {
        // Lock-file lifecycle invariant: the sweep MUST NOT delete the
        // `.lock` file. If it did, a sibling acquirer holding a kernel
        // flock on the now-unlinked inode could race with a later
        // `open(O_CREAT)` that produces a fresh inode at the same path,
        // letting two processes hold "the lock" concurrently on
        // different inodes.
        let tmp = tempfile::TempDir::new().expect("temp dir should be created");
        let repo = init_throwaway_repo(tmp.path(), "repo-gc-lockfile");
        let worktree_path = make_reusable_path("gc-lockfile");
        register_reusable_worktree(&repo, &worktree_path);
        write_sidecar_with_age(&worktree_path, Duration::from_hours(31 * 24));
        // Create the lock file by attempting (and immediately dropping) a lock.
        // This mirrors the file shape `ReusableWorktreeLock::try_acquire`
        // leaves behind under normal usage.
        let lock_path = reusable_worktree_lock_path(&worktree_path);
        drop(
            ReusableWorktreeLock::try_acquire(&worktree_path)
                .expect("test should acquire the lock"),
        );
        assert!(
            lock_path.exists(),
            "test pre-condition: lock file should exist before sweep",
        );

        sweep_old_reusable_caches(&repo, Duration::from_hours(30 * 24), true);

        assert!(
            !worktree_path.exists(),
            "sweep should still remove the worktree directory",
        );
        assert!(
            lock_path.exists(),
            "sweep MUST NOT delete the `.lock` file (lock-lifecycle invariant)",
        );
        let _ = fs::remove_file(&lock_path);
        cleanup_reusable_worktree(&repo, &worktree_path);
    }

    #[test]
    fn reuse_or_create_stamps_sidecar_on_fresh_create_and_age_threshold_applies() {
        // Documented contract on `cache_max_age_days`: "Maximum age (in days
        // since last reuse or fresh create)". This test pins both halves:
        // (a) a fresh `reuse_or_create` writes the sidecar with a near-now
        //     mtime, AND
        // (b) backdating that sidecar past the threshold causes the next
        //     sweep to actually remove the entry. Without (a), one-off
        //     base SHAs would persist through the first sweep regardless
        //     of age, contradicting the contract.
        let tmp = tempfile::TempDir::new().expect("temp dir should be created");
        let repo = init_throwaway_repo(tmp.path(), "repo-fresh-create-stamp");
        let base_sha = git_rev_parse(&repo, "HEAD").expect("HEAD should resolve");

        let worktree = BaseWorktree::reuse_or_create(&repo, &base_sha)
            .expect("fresh reuse_or_create should succeed on a clean repo");
        let cache_path = worktree.path().to_path_buf();
        let sidecar = reusable_worktree_last_used_path(&cache_path);

        assert!(
            sidecar.exists(),
            "fresh-create must write the sidecar so age is measured from now",
        );
        let initial_age = std::fs::metadata(&sidecar)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|mtime| SystemTime::now().duration_since(mtime).ok())
            .expect("sidecar mtime should be readable and not in the future");
        assert!(
            initial_age < Duration::from_mins(1),
            "fresh-create sidecar mtime should be near now(), got age {initial_age:?}",
        );

        // Drop the worktree handle so the persistent cache survives but we
        // can mutate the sidecar.
        drop(worktree);

        // Backdate the sidecar past the threshold; sweep must now remove it.
        write_sidecar_with_age(&cache_path, Duration::from_hours(31 * 24));
        sweep_old_reusable_caches(&repo, Duration::from_hours(30 * 24), true);

        assert!(
            !cache_path.exists(),
            "after backdating, sweep must remove the fresh-created cache",
        );
        assert!(
            !sidecar.exists(),
            "sweep should remove the sidecar alongside the cache dir",
        );
        cleanup_reusable_worktree(&repo, &cache_path);
    }

    #[test]
    fn days_to_duration_zero_disables() {
        assert!(days_to_duration(0).is_none());
        assert_eq!(days_to_duration(1), Some(Duration::from_hours(24)));
        assert_eq!(days_to_duration(30), Some(Duration::from_hours(30 * 24)));
    }

    #[test]
    fn reusable_worktree_last_used_path_lives_next_to_cache_dir() {
        let cache_dir = std::env::temp_dir().join("fallow-audit-base-cache-abcd-1234");
        let sidecar = reusable_worktree_last_used_path(&cache_dir);
        assert_eq!(sidecar.parent(), cache_dir.parent());
        assert_eq!(
            sidecar.file_name().and_then(|s| s.to_str()),
            Some("fallow-audit-base-cache-abcd-1234.last-used"),
        );
    }

    #[test]
    fn touch_last_used_creates_sidecar_if_missing() {
        let tmp = tempfile::TempDir::new().expect("temp dir should be created");
        let cache_dir = tmp.path().join("fallow-audit-base-cache-touchtest-0000");
        fs::create_dir(&cache_dir).expect("cache dir should be created");
        let sidecar = reusable_worktree_last_used_path(&cache_dir);
        assert!(!sidecar.exists(), "sidecar should not exist before touch");

        touch_last_used(&cache_dir);

        assert!(sidecar.exists(), "touch should create the sidecar");
        let mtime = fs::metadata(&sidecar)
            .and_then(|m| m.modified())
            .expect("sidecar should have an mtime");
        let age = SystemTime::now()
            .duration_since(mtime)
            .unwrap_or(Duration::ZERO);
        assert!(
            age < Duration::from_mins(1),
            "touched sidecar should be near `now()`",
        );
    }

    #[test]
    fn reusable_worktree_lock_excludes_concurrent_acquires() {
        let tmp = tempfile::TempDir::new().expect("temp dir should be created");
        // Use a stable reusable-path-shaped value inside the tempdir so the
        // lock file lives somewhere we can clean up automatically.
        let reusable = tmp.path().join("fallow-audit-base-cache-deadbeef-0000");
        let lock_path = reusable_worktree_lock_path(&reusable);

        let first = ReusableWorktreeLock::try_acquire(&reusable)
            .expect("first acquire on a fresh path should succeed");
        assert!(
            ReusableWorktreeLock::try_acquire(&reusable).is_none(),
            "second acquire must fail while the first is held",
        );
        // Don't assert that a same-process re-acquire-after-drop succeeds:
        // macOS flock(2) can keep the lock visible to other open file
        // descriptions in the same process for a brief window after close,
        // and this test would flake under parallel `cargo test` execution.
        // The cross-process release path is exercised by every real `fallow
        // audit` invocation; the in-process exclusion above is the actual
        // invariant we need to guarantee here.
        drop(first);
        // The lock file inode persists after the holder drops; only the
        // kernel lock state is released. Anchor that so future maintainers
        // don't conflate "release" with "delete".
        assert!(
            lock_path.exists(),
            "lock file must persist after drop (only the kernel lock is released)",
        );
    }

    #[test]
    fn base_analysis_root_preserves_repo_subdirectory_roots() {
        let tmp = tempfile::TempDir::new().expect("temp dir should be created");
        let repo = tmp.path().join("repo");
        let app_root = repo.join("apps/mobile");
        let base_worktree = tmp.path().join("base-worktree");
        fs::create_dir_all(&app_root).expect("app root should be created");
        fs::create_dir_all(&base_worktree).expect("base worktree should be created");
        git(&repo, &["init", "-b", "main"]);

        assert_eq!(
            base_analysis_root(&app_root, &base_worktree),
            base_worktree.join("apps/mobile")
        );
    }

    #[test]
    fn audit_base_worktree_reuses_current_node_modules_context() {
        let tmp = tempfile::TempDir::new().expect("temp dir should be created");
        let root = tmp.path();
        fs::create_dir_all(root.join("src")).expect("src dir should be created");
        fs::write(root.join(".gitignore"), "node_modules\n.fallow\n")
            .expect("gitignore should be written");
        fs::write(
            root.join("package.json"),
            r#"{"name":"audit-rn-alias","main":"src/index.ts","dependencies":{"@react-native/typescript-config":"1.0.0"}}"#,
        )
        .expect("package.json should be written");
        fs::write(
            root.join("tsconfig.json"),
            r#"{"extends":"./node_modules/@react-native/typescript-config/tsconfig.json","compilerOptions":{"baseUrl":".","paths":{"@/*":["src/*"]}},"include":["src"]}"#,
        )
        .expect("tsconfig should be written");
        fs::write(
            root.join("src/index.ts"),
            "import { used } from '@/feature';\nconsole.log(used);\n",
        )
        .expect("index should be written");
        fs::write(root.join("src/feature.ts"), "export const used = 1;\n")
            .expect("feature should be written");

        git(root, &["init", "-b", "main"]);
        git(root, &["add", "."]);
        git(
            root,
            &["-c", "commit.gpgsign=false", "commit", "-m", "initial"],
        );

        let rn_config = root.join("node_modules/@react-native/typescript-config");
        fs::create_dir_all(&rn_config).expect("node_modules config dir should be created");
        fs::write(
            rn_config.join("tsconfig.json"),
            r#"{"compilerOptions":{"jsx":"react-native","moduleResolution":"bundler"}}"#,
        )
        .expect("node_modules tsconfig should be written");

        let worktree =
            BaseWorktree::create(root, "HEAD", None).expect("base worktree should be created");
        assert!(
            worktree.path().join("node_modules").is_dir(),
            "base worktree should reuse ignored node_modules from the current checkout"
        );
        assert!(
            worktree
                .path()
                .join("node_modules/@react-native/typescript-config/tsconfig.json")
                .is_file(),
            "base worktree should preserve tsconfig extends targets installed in node_modules"
        );
    }

    /// Confirms `materialize_base_dependency_context` symlinks the Nuxt
    /// `.nuxt/` generated dir from the host checkout into the audit base
    /// worktree. Without this, root `tsconfig.json` `references` entries
    /// pointing into `.nuxt/tsconfig.app.json` break in the base pass and
    /// emit "Nuxt project missing .nuxt/tsconfig.json" plus "Broken tsconfig
    /// chain" warnings. The function is exercised directly here rather than
    /// through `BaseWorktree::create` to avoid the `git worktree add`
    /// concurrency-flakiness the worktree-level integration tests already
    /// exhibit.
    #[test]
    fn materialize_base_dependency_context_symlinks_nuxt_generated_dir() {
        let host = tempfile::TempDir::new().expect("host tempdir should be created");
        let worktree = tempfile::TempDir::new().expect("worktree tempdir should be created");

        let dot_nuxt = host.path().join(".nuxt");
        fs::create_dir_all(&dot_nuxt).expect(".nuxt dir should be created");
        fs::write(dot_nuxt.join("tsconfig.json"), r#"{"compilerOptions":{}}"#)
            .expect(".nuxt/tsconfig.json should be written");
        fs::write(
            dot_nuxt.join("tsconfig.app.json"),
            r#"{"compilerOptions":{}}"#,
        )
        .expect(".nuxt/tsconfig.app.json should be written");

        materialize_base_dependency_context(host.path(), worktree.path());

        let mirrored = worktree.path().join(".nuxt");
        assert!(
            mirrored.is_dir(),
            "base worktree should reuse the ignored .nuxt dir from the host checkout"
        );
        let link_meta = fs::symlink_metadata(&mirrored)
            .expect(".nuxt entry should exist as a symlink in the worktree");
        assert!(
            link_meta.file_type().is_symlink(),
            "base worktree's .nuxt should be a symlink to the host checkout"
        );
        assert!(
            mirrored.join("tsconfig.json").is_file(),
            "base worktree should expose .nuxt/tsconfig.json so the Nuxt meta-framework \
             prerequisite check stays quiet"
        );
        assert!(
            mirrored.join("tsconfig.app.json").is_file(),
            "base worktree should expose .nuxt/tsconfig.app.json so root tsconfig references \
             resolve without falling back to resolver-less resolution"
        );
    }

    /// Confirms the same symlink treatment for Astro's `.astro/` generated
    /// types directory, which is gitignored by default and would otherwise
    /// trip the "Astro project missing .astro/" prerequisite check on the
    /// base pass.
    #[test]
    fn materialize_base_dependency_context_symlinks_astro_generated_dir() {
        let host = tempfile::TempDir::new().expect("host tempdir should be created");
        let worktree = tempfile::TempDir::new().expect("worktree tempdir should be created");

        let dot_astro = host.path().join(".astro");
        fs::create_dir_all(&dot_astro).expect(".astro dir should be created");
        fs::write(dot_astro.join("types.d.ts"), "// generated types\n")
            .expect(".astro/types.d.ts should be written");

        materialize_base_dependency_context(host.path(), worktree.path());

        let mirrored = worktree.path().join(".astro");
        assert!(
            mirrored.is_dir(),
            "base worktree should reuse the ignored .astro dir from the host checkout"
        );
        assert!(
            mirrored.join("types.d.ts").is_file(),
            "base worktree should expose generated Astro types so the Astro meta-framework \
             prerequisite check stays quiet"
        );
    }

    /// Confirms the symlink step is a no-op when the host checkout has no
    /// meta-framework output. We must not fabricate a dangling `.nuxt`
    /// symlink: the Nuxt prerequisite check would then pass on the base pass
    /// while the actual `.nuxt/tsconfig.json` still doesn't exist, hiding a
    /// real "run `nuxt prepare`" warning on the HEAD pass behind a
    /// process-wide dedupe key.
    #[test]
    fn materialize_base_dependency_context_skips_when_host_lacks_meta_framework_dir() {
        let host = tempfile::TempDir::new().expect("host tempdir should be created");
        let worktree = tempfile::TempDir::new().expect("worktree tempdir should be created");

        materialize_base_dependency_context(host.path(), worktree.path());

        assert!(
            !worktree.path().join(".nuxt").exists(),
            "base worktree should not fabricate a .nuxt symlink when the host has no .nuxt dir"
        );
        assert!(
            !worktree.path().join(".astro").exists(),
            "base worktree should not fabricate a .astro symlink when the host has no .astro dir"
        );
        assert!(
            !worktree.path().join("node_modules").exists(),
            "base worktree should not fabricate a node_modules symlink when the host has none"
        );
    }

    /// Confirms each entry in `MATERIALIZED_CONTEXT_DIRS` is independent: a
    /// missing host `.nuxt/` must not prevent `node_modules` from being
    /// symlinked when only one of the two is present on the host.
    #[test]
    fn materialize_base_dependency_context_handles_each_dir_independently() {
        let host = tempfile::TempDir::new().expect("host tempdir should be created");
        let worktree = tempfile::TempDir::new().expect("worktree tempdir should be created");

        fs::create_dir_all(host.path().join("node_modules"))
            .expect("host node_modules should be created");

        materialize_base_dependency_context(host.path(), worktree.path());

        assert!(
            worktree.path().join("node_modules").is_dir(),
            "node_modules should still be symlinked even when host has no .nuxt or .astro"
        );
        assert!(
            !worktree.path().join(".nuxt").exists(),
            "missing host .nuxt should leave the worktree slot empty"
        );
    }

    /// Confirms a real (non-symlink) generated dir already present in the base
    /// worktree is preserved, not clobbered by a host symlink. A base commit
    /// that genuinely tracks `.nuxt/` is base-shaped and authoritative; the
    /// host-symlink shortcut only fills the gap when the worktree slot is
    /// empty (or a stale dangling symlink), so the `destination.is_dir()`
    /// early-continue must keep the worktree's own contents.
    #[test]
    fn materialize_base_dependency_context_preserves_real_worktree_dir() {
        let host = tempfile::TempDir::new().expect("host tempdir should be created");
        let worktree = tempfile::TempDir::new().expect("worktree tempdir should be created");

        let host_nuxt = host.path().join(".nuxt");
        fs::create_dir_all(&host_nuxt).expect("host .nuxt dir should be created");
        fs::write(host_nuxt.join("tsconfig.json"), r#"{"_source":"host"}"#)
            .expect("host .nuxt/tsconfig.json should be written");

        let worktree_nuxt = worktree.path().join(".nuxt");
        fs::create_dir_all(&worktree_nuxt).expect("worktree .nuxt dir should be created");
        fs::write(worktree_nuxt.join("tsconfig.json"), r#"{"_source":"base"}"#)
            .expect("worktree .nuxt/tsconfig.json should be written");

        materialize_base_dependency_context(host.path(), worktree.path());

        let link_meta = fs::symlink_metadata(&worktree_nuxt)
            .expect(".nuxt entry should still exist in the worktree");
        assert!(
            !link_meta.file_type().is_symlink(),
            "a real base-tracked .nuxt dir must not be replaced by a host symlink"
        );
        let contents =
            fs::read_to_string(worktree_nuxt.join("tsconfig.json")).expect("tsconfig should read");
        assert!(
            contents.contains("base"),
            "base worktree's own .nuxt contents must survive, not be overwritten by the host's"
        );
    }

    #[test]
    fn audit_reusable_base_worktree_refreshes_current_node_modules_context() {
        let tmp = tempfile::TempDir::new().expect("temp dir should be created");
        let root = tmp.path();
        fs::write(root.join(".gitignore"), "node_modules\n.fallow\n")
            .expect("gitignore should be written");
        fs::write(root.join("package.json"), r#"{"name":"audit-reusable"}"#)
            .expect("package.json should be written");

        git(root, &["init", "-b", "main"]);
        git(root, &["add", "."]);
        git(
            root,
            &["-c", "commit.gpgsign=false", "commit", "-m", "initial"],
        );

        let rn_config = root.join("node_modules/@react-native/typescript-config");
        fs::create_dir_all(&rn_config).expect("node_modules config dir should be created");
        fs::write(rn_config.join("tsconfig.json"), "{}")
            .expect("node_modules tsconfig should be written");

        let base_sha = git_rev_parse(root, "HEAD").expect("HEAD should resolve");
        let first = BaseWorktree::create(root, "HEAD", Some(&base_sha))
            .expect("persistent base worktree should be created");
        let worktree_path = first.path().to_path_buf();
        assert!(
            worktree_path.join("node_modules").is_dir(),
            "initial persistent worktree should receive node_modules context"
        );
        remove_node_modules_context(&worktree_path);
        assert!(
            !worktree_path.join("node_modules").exists(),
            "test setup should remove the dependency context from the reusable worktree"
        );
        drop(first);

        let reused = BaseWorktree::create(root, "HEAD", Some(&base_sha))
            .expect("ready persistent base worktree should be reused");
        assert_eq!(reused.path(), worktree_path.as_path());
        assert!(
            reused.path().join("node_modules").is_dir(),
            "ready persistent worktree should refresh missing node_modules context"
        );

        remove_audit_worktree(root, reused.path());
        let _ = fs::remove_dir_all(reused.path());
    }

    fn remove_node_modules_context(worktree_path: &Path) {
        let path = worktree_path.join("node_modules");
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            return;
        };
        if metadata.file_type().is_symlink() {
            #[cfg(unix)]
            let _ = fs::remove_file(path);
            #[cfg(windows)]
            let _ = fs::remove_dir(&path).or_else(|_| fs::remove_file(&path));
        } else {
            let _ = fs::remove_dir_all(path);
        }
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
            coverage: None,
            coverage_root: None,
            gate: AuditGate::NewOnly,
            include_entry_exports: false,
            runtime_coverage: None,
            min_invocations_hot: 100,
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
            coverage: None,
            coverage_root: None,
            gate: AuditGate::All,
            include_entry_exports: false,
            runtime_coverage: None,
            min_invocations_hot: 100,
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
            coverage: None,
            coverage_root: None,
            gate: AuditGate::NewOnly,
            include_entry_exports: false,
            runtime_coverage: None,
            min_invocations_hot: 100,
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
            coverage: None,
            coverage_root: None,
            gate: AuditGate::NewOnly,
            include_entry_exports: false,
            runtime_coverage: None,
            min_invocations_hot: 100,
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
            coverage: None,
            coverage_root: None,
            gate: AuditGate::NewOnly,
            include_entry_exports: false,
            runtime_coverage: None,
            min_invocations_hot: 100,
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
            coverage: None,
            coverage_root: None,
            gate: AuditGate::NewOnly,
            include_entry_exports: false,
            runtime_coverage: None,
            min_invocations_hot: 100,
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
    fn audit_base_preserves_tsconfig_paths_when_extends_is_in_untracked_node_modules() {
        let tmp = tempfile::TempDir::new().expect("temp dir should be created");
        let root = tmp.path();
        fs::create_dir_all(root.join("src/screens")).expect("src dir should be created");
        fs::create_dir_all(root.join("node_modules/@react-native/typescript-config"))
            .expect("node_modules config dir should be created");
        fs::write(root.join(".gitignore"), "node_modules/\n").expect("gitignore should be written");
        fs::write(
            root.join("package.json"),
            r#"{
                "name": "audit-react-native-tsconfig-base",
                "private": true,
                "main": "src/App.tsx",
                "dependencies": {
                    "react-native": "0.80.0"
                }
            }"#,
        )
        .expect("package.json should be written");
        fs::write(
            root.join("tsconfig.json"),
            r#"{
                "extends": "./node_modules/@react-native/typescript-config/tsconfig.json",
                "compilerOptions": {
                    "baseUrl": ".",
                    "paths": {
                        "@/*": ["src/*"]
                    }
                },
                "include": ["src/**/*"]
            }"#,
        )
        .expect("tsconfig should be written");
        fs::write(
            root.join("node_modules/@react-native/typescript-config/tsconfig.json"),
            r#"{"compilerOptions":{"strict":true,"jsx":"react-jsx"}}"#,
        )
        .expect("react native tsconfig should be written");
        fs::write(
            root.join("src/App.tsx"),
            r#"import { homeTitle } from "@/screens/Home";

export function App() {
  return homeTitle;
}
"#,
        )
        .expect("app should be written");
        fs::write(
            root.join("src/screens/Home.ts"),
            r#"export const homeTitle = "home";
"#,
        )
        .expect("home should be written");

        git(root, &["init", "-b", "main"]);
        git(root, &["add", "."]);
        git(
            root,
            &["-c", "commit.gpgsign=false", "commit", "-m", "initial"],
        );
        fs::write(
            root.join("src/App.tsx"),
            r#"import { homeTitle } from "@/screens/Home";

export function App() {
  return homeTitle.toUpperCase();
}
"#,
        )
        .expect("app should be modified");

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
            coverage: None,
            coverage_root: None,
            gate: AuditGate::NewOnly,
            include_entry_exports: false,
            runtime_coverage: None,
            min_invocations_hot: 100,
        };

        let result = execute_audit(&opts).expect("audit should execute");
        assert!(
            !result.base_snapshot_skipped,
            "source diffs should run a real base snapshot"
        );
        let base = result
            .base_snapshot
            .as_ref()
            .expect("base snapshot should run");
        assert!(
            !base
                .dead_code
                .contains("unresolved-import:src/App.tsx:@/screens/Home"),
            "base audit must keep local @/* tsconfig aliases when extends points into ignored node_modules: {:?}",
            base.dead_code
        );
        assert!(
            !base.dead_code.contains("unused-file:src/screens/Home.ts"),
            "alias target should stay reachable in the base worktree: {:?}",
            base.dead_code
        );
        let check = result.check.as_ref().expect("dead-code audit should run");
        assert!(
            check.results.unresolved_imports.is_empty(),
            "HEAD audit should also resolve @/* aliases: {:?}",
            check.results.unresolved_imports
        );
    }

    #[test]
    fn audit_base_preserves_subdirectory_root_resolution() {
        let tmp = tempfile::TempDir::new().expect("temp dir should be created");
        let repo = tmp.path().join("repo");
        let root = repo.join("apps/mobile");
        fs::create_dir_all(root.join("src/screens")).expect("src dir should be created");
        fs::create_dir_all(root.join("node_modules/@react-native/typescript-config"))
            .expect("node_modules config dir should be created");
        fs::write(repo.join(".gitignore"), "apps/mobile/node_modules/\n")
            .expect("gitignore should be written");
        fs::write(
            root.join("package.json"),
            r#"{
                "name": "audit-subdir-react-native-tsconfig-base",
                "private": true,
                "main": "src/App.tsx",
                "dependencies": {
                    "react-native": "0.80.0"
                }
            }"#,
        )
        .expect("package.json should be written");
        fs::write(
            root.join("tsconfig.json"),
            r#"{
                "extends": "./node_modules/@react-native/typescript-config/tsconfig.json",
                "compilerOptions": {
                    "baseUrl": ".",
                    "paths": {
                        "@/*": ["src/*"]
                    }
                },
                "include": ["src/**/*"]
            }"#,
        )
        .expect("tsconfig should be written");
        fs::write(
            root.join("node_modules/@react-native/typescript-config/tsconfig.json"),
            r#"{"compilerOptions":{"strict":true,"jsx":"react-jsx"}}"#,
        )
        .expect("react native tsconfig should be written");
        fs::write(
            root.join("src/App.tsx"),
            r#"import { homeTitle } from "@/screens/Home";

export function App() {
  return homeTitle;
}
"#,
        )
        .expect("app should be written");
        fs::write(
            root.join("src/screens/Home.ts"),
            r#"export const homeTitle = "home";
"#,
        )
        .expect("home should be written");

        git(&repo, &["init", "-b", "main"]);
        git(&repo, &["add", "."]);
        git(
            &repo,
            &["-c", "commit.gpgsign=false", "commit", "-m", "initial"],
        );
        fs::write(
            root.join("src/App.tsx"),
            r#"import { homeTitle } from "@/screens/Home";

export function App() {
  return homeTitle.toUpperCase();
}
"#,
        )
        .expect("app should be modified");

        let config_path = None;
        let opts = AuditOptions {
            root: &root,
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
            coverage: None,
            coverage_root: None,
            gate: AuditGate::NewOnly,
            include_entry_exports: false,
            runtime_coverage: None,
            min_invocations_hot: 100,
        };

        let result = execute_audit(&opts).expect("audit should execute");
        assert!(
            !result.base_snapshot_skipped,
            "source diffs should run a real base snapshot"
        );
        let base = result
            .base_snapshot
            .as_ref()
            .expect("base snapshot should run");
        assert!(
            !base
                .dead_code
                .contains("unresolved-import:src/App.tsx:@/screens/Home"),
            "base audit should analyze from the app subdirectory, not the repo root: {:?}",
            base.dead_code
        );
        assert!(
            !base.dead_code.contains("unused-file:src/screens/Home.ts"),
            "subdirectory base audit should keep alias targets reachable: {:?}",
            base.dead_code
        );
    }

    #[test]
    fn audit_base_uses_new_explicit_config_without_hard_failure() {
        let tmp = tempfile::TempDir::new().expect("temp dir should be created");
        let root = tmp.path();
        fs::create_dir_all(root.join("src")).expect("src dir should be created");
        fs::write(
            root.join("package.json"),
            r#"{"name":"audit-new-config","main":"src/index.ts"}"#,
        )
        .expect("package.json should be written");
        fs::write(root.join("src/index.ts"), "export const used = 1;\n")
            .expect("index should be written");

        git(root, &["init", "-b", "main"]);
        git(root, &["add", "."]);
        git(
            root,
            &["-c", "commit.gpgsign=false", "commit", "-m", "initial"],
        );

        let explicit_config = root.join(".fallowrc.json");
        fs::write(&explicit_config, r#"{"rules":{"unused-files":"error"}}"#)
            .expect("new config should be written");
        fs::write(root.join("src/index.ts"), "export const used = 2;\n")
            .expect("index should be modified");

        let config_path = Some(explicit_config);
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
            coverage: None,
            coverage_root: None,
            gate: AuditGate::NewOnly,
            include_entry_exports: false,
            runtime_coverage: None,
            min_invocations_hot: 100,
        };

        let result = execute_audit(&opts).expect("audit should execute with a new explicit config");
        assert!(
            result.base_snapshot.is_some(),
            "base snapshot should use the current explicit config even when the base commit lacks it"
        );
    }

    #[test]
    fn audit_base_uses_current_discovered_config_for_attribution() {
        let tmp = tempfile::TempDir::new().expect("temp dir should be created");
        let root = tmp.path();
        fs::create_dir_all(root.join("src")).expect("src dir should be created");
        fs::write(
            root.join("package.json"),
            r#"{"name":"audit-current-config","main":"src/index.ts","dependencies":{"left-pad":"1.3.0"}}"#,
        )
        .expect("package.json should be written");
        fs::write(
            root.join(".fallowrc.json"),
            r#"{"rules":{"unused-dependencies":"off"}}"#,
        )
        .expect("base config should be written");
        fs::write(root.join("src/index.ts"), "export const used = 1;\n")
            .expect("index should be written");

        git(root, &["init", "-b", "main"]);
        git(root, &["add", "."]);
        git(
            root,
            &["-c", "commit.gpgsign=false", "commit", "-m", "initial"],
        );

        fs::write(
            root.join(".fallowrc.json"),
            r#"{"rules":{"unused-dependencies":"error"}}"#,
        )
        .expect("current config should be written");
        fs::write(
            root.join("package.json"),
            r#"{"name":"audit-current-config","main":"src/index.ts","dependencies":{"left-pad":"1.3.1"}}"#,
        )
        .expect("package.json should be touched");

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
            coverage: None,
            coverage_root: None,
            gate: AuditGate::NewOnly,
            include_entry_exports: false,
            runtime_coverage: None,
            min_invocations_hot: 100,
        };

        let result = execute_audit(&opts).expect("audit should execute");
        assert_eq!(
            result.attribution.dead_code_introduced, 0,
            "enabling a rule should not make pre-existing changed-file findings look introduced: {:?}",
            result.attribution
        );
        assert!(
            result.attribution.dead_code_inherited > 0,
            "pre-existing changed-file findings should be classified as inherited: {:?}",
            result.attribution
        );
    }

    #[test]
    fn audit_base_current_config_attribution_survives_cache_hit() {
        let tmp = tempfile::TempDir::new().expect("temp dir should be created");
        let root = tmp.path();
        fs::create_dir_all(root.join("src")).expect("src dir should be created");
        fs::write(
            root.join("package.json"),
            r#"{"name":"audit-current-config-cache","main":"src/index.ts","dependencies":{"left-pad":"1.3.0"}}"#,
        )
        .expect("package.json should be written");
        fs::write(
            root.join(".fallowrc.json"),
            r#"{"rules":{"unused-dependencies":"off"}}"#,
        )
        .expect("base config should be written");
        fs::write(root.join("src/index.ts"), "export const used = 1;\n")
            .expect("index should be written");

        git(root, &["init", "-b", "main"]);
        git(root, &["add", "."]);
        git(
            root,
            &["-c", "commit.gpgsign=false", "commit", "-m", "initial"],
        );

        fs::write(
            root.join(".fallowrc.json"),
            r#"{"rules":{"unused-dependencies":"error"}}"#,
        )
        .expect("current config should be written");
        fs::write(
            root.join("package.json"),
            r#"{"name":"audit-current-config-cache","main":"src/index.ts","dependencies":{"left-pad":"1.3.1"}}"#,
        )
        .expect("package.json should be touched");

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
            coverage: None,
            coverage_root: None,
            gate: AuditGate::NewOnly,
            include_entry_exports: false,
            runtime_coverage: None,
            min_invocations_hot: 100,
        };

        let first = execute_audit(&opts).expect("first audit should execute");
        assert_eq!(
            first.attribution.dead_code_introduced, 0,
            "first audit should classify pre-existing findings as inherited: {:?}",
            first.attribution
        );

        let changed_files =
            crate::check::get_changed_files(root, "HEAD").expect("changed files should resolve");
        let key = audit_base_snapshot_cache_key(&opts, "HEAD", &changed_files)
            .expect("cache key should compute")
            .expect("cache key should exist");
        assert!(
            load_cached_base_snapshot(&opts, &key).is_some(),
            "first audit should store a reusable base snapshot"
        );

        let second = execute_audit(&opts).expect("second audit should execute");
        assert_eq!(
            second.attribution.dead_code_introduced, 0,
            "cache hit should keep current-config attribution stable: {:?}",
            second.attribution
        );
        assert!(
            second.attribution.dead_code_inherited > 0,
            "cache hit should preserve inherited base findings: {:?}",
            second.attribution
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
            coverage: None,
            coverage_root: None,
            gate: AuditGate::All,
            include_entry_exports: false,
            runtime_coverage: None,
            min_invocations_hot: 100,
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
