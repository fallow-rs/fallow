use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::{Duration, Instant};

use fallow_config::{AuditGate, OutputFormat};
use fallow_core::git_env::clear_ambient_git_env;
use rustc_hash::FxHashSet;
use xxhash_rust::xxh3::xxh3_64;

use crate::base_worktree::{
    BaseWorktree, git_rev_parse, git_toplevel, resolve_cache_max_age, sweep_old_reusable_caches,
};
use crate::check::{CheckOptions, CheckResult, IssueFilters, TraceOptions};
use crate::dupes::{DupesMode, DupesOptions, DupesResult};
use crate::error::emit_error;
use crate::health::{HealthOptions, HealthResult, SortBy};

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
    /// Absolute paths of the files this run re-analyzed. Threaded into the
    /// Fallow Impact per-finding attribution so the frontier diff knows which
    /// files were authoritative this run.
    pub changed_files: Vec<PathBuf>,
    pub base_ref: String,
    /// Human-readable provenance of `base_ref` for the scope line, e.g.
    /// `merge-base with origin/main`. `None` for an explicit `--base` (the ref
    /// the user typed is already self-describing). Not serialized; the JSON
    /// envelope carries the resolved `base_ref` directly.
    pub base_description: Option<String>,
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
    pub cache_dir: &'a std::path::Path,
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
}

/// A base ref resolved by auto-detection: the git ref to diff against plus a
/// human-readable provenance string for the scope line.
struct DetectedBase {
    /// The ref the audit diffs against: a `git merge-base` SHA (the fork
    /// point), a remote-tracking ref, or a local branch name.
    git_ref: String,
    /// How the ref was resolved, e.g. `merge-base with origin/main`. Shown on
    /// the human audit scope line so the comparison target is checkable.
    description: String,
}

/// Run `git <args>` in `root` with ambient git env cleared and return trimmed
/// stdout, or `None` on non-zero exit / empty output.
fn git_stdout(root: &std::path::Path, args: &[&str]) -> Option<String> {
    let mut command = std::process::Command::new("git");
    command.args(args).current_dir(root);
    clear_ambient_git_env(&mut command);
    let output = command.output().ok()?;
    if !output.status.success() {
        return None;
    }
    let trimmed = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

/// Whether `git_ref` resolves to a commit in this repository.
fn git_ref_exists(root: &std::path::Path, git_ref: &str) -> bool {
    git_stdout(root, &["rev-parse", "--verify", "--quiet", git_ref]).is_some()
}

/// The current branch's configured upstream (`@{upstream}`), e.g. `origin/main`,
/// or `None` when no tracking branch is set (detached HEAD, fresh worktree).
fn git_upstream_ref(root: &std::path::Path) -> Option<String> {
    git_stdout(
        root,
        &[
            "rev-parse",
            "--abbrev-ref",
            "--symbolic-full-name",
            "@{upstream}",
        ],
    )
}

/// The merge-base (fork point) SHA of `a` and `b`, or `None` when there is no
/// common ancestor (shallow clone, unrelated history).
fn git_merge_base(root: &std::path::Path, a: &str, b: &str) -> Option<String> {
    git_stdout(root, &["merge-base", a, b])
}

/// The remote default branch as a remote-tracking ref (`origin/<branch>`).
/// Priority: `origin/HEAD` symbolic ref, then `origin/main`, then
/// `origin/master`. Returns `None` when there is no `origin` remote at all.
fn detect_remote_default_ref(root: &std::path::Path) -> Option<String> {
    if let Some(full_ref) = git_stdout(root, &["symbolic-ref", "refs/remotes/origin/HEAD"])
        && let Some(branch) = full_ref.strip_prefix("refs/remotes/origin/")
    {
        return Some(format!("origin/{branch}"));
    }
    for candidate in ["origin/main", "origin/master"] {
        if git_ref_exists(root, candidate) {
            return Some(candidate.to_string());
        }
    }
    None
}

/// Auto-detect the base ref for `fallow audit` when no `--base` / env override
/// is set.
///
/// The base is the `git merge-base` (fork point) against the branch's upstream
/// or the remote default, mirroring the `fallow hooks install --target git`
/// pre-commit hook (issue #242). Resolving to the merge-base SHA, rather than a
/// bare branch name, fixes the long-standing bug where the default branch was
/// discovered via `origin/HEAD` but returned as the bare name `main` (issue
/// #1168): git resolves a bare `main` to the LOCAL `refs/heads/main`, which is
/// stale on worktree checkouts cut from `origin/main`, so the audit diffed
/// every branch against an ancient base and false-failed the gate.
///
/// Resolution order:
/// 1. `@{upstream}` merge-base, so a branch forked off a non-default
///    integration branch compares against where it actually forked.
/// 2. Remote default (`origin/HEAD` -> `origin/main` -> `origin/master`)
///    merge-base. The remote-tracking ref refreshes on fetch, unlike a
///    long-stale local branch; the merge-base is also immune to an unfetched
///    `origin/main` in the false-fail direction.
/// 3. Local `main` / `master` when there is no `origin` remote, preserving the
///    historical behavior for air-gapped / local-only repos.
fn auto_detect_base_ref(root: &std::path::Path) -> Option<DetectedBase> {
    if let Some(upstream) = git_upstream_ref(root) {
        if let Some(sha) = git_merge_base(root, &upstream, "HEAD") {
            return Some(DetectedBase {
                git_ref: sha,
                description: format!("merge-base with {upstream}"),
            });
        }
        // No common ancestor (shallow clone / unrelated history): fall back to
        // the upstream tip rather than failing the detection outright.
        return Some(DetectedBase {
            description: format!("{upstream} (tip)"),
            git_ref: upstream,
        });
    }

    if let Some(remote_ref) = detect_remote_default_ref(root) {
        if let Some(sha) = git_merge_base(root, &remote_ref, "HEAD") {
            return Some(DetectedBase {
                git_ref: sha,
                description: format!("merge-base with {remote_ref}"),
            });
        }
        return Some(DetectedBase {
            description: format!("{remote_ref} (tip)"),
            git_ref: remote_ref,
        });
    }

    for candidate in ["main", "master"] {
        if git_ref_exists(root, candidate) {
            return Some(DetectedBase {
                git_ref: candidate.to_string(),
                description: format!("local {candidate}"),
            });
        }
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

fn compute_verdict(
    check: Option<&CheckResult>,
    dupes: Option<&DupesResult>,
    health: Option<&HealthResult>,
) -> AuditVerdict {
    let mut has_errors = false;
    let mut has_warnings = false;

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

    if let Some(result) = health
        && !result.report.findings.is_empty()
    {
        has_errors = true;
    }

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

fn audit_base_snapshot_cache_dir(cache_dir: &Path) -> PathBuf {
    cache_dir
        .join("cache")
        .join(format!("audit-base-v{AUDIT_BASE_SNAPSHOT_CACHE_VERSION}"))
}

fn audit_base_snapshot_cache_file(cache_dir: &Path, key: &AuditBaseSnapshotCacheKey) -> PathBuf {
    audit_base_snapshot_cache_dir(cache_dir).join(format!("{:016x}.bin", key.hash))
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

fn save_cached_base_snapshot(
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
    let base_cache_dir = remap_cache_dir_for_base_worktree(opts.root, &base_root, opts.cache_dir);
    let current_config_path = opts
        .config_path
        .clone()
        .or_else(|| fallow_config::FallowConfig::find_config_path(opts.root));
    let base_opts = AuditOptions {
        root: &base_root,
        config_path: &current_config_path,
        cache_dir: &base_cache_dir,
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
        runtime_coverage: None,
        min_invocations_hot: opts.min_invocations_hot,
    };

    let base_changed_files = remap_focus_files(changed_files, opts.root, &base_root);
    let check_production = opts.production_dead_code.unwrap_or(opts.production);
    let health_production = opts.production_health.unwrap_or(opts.production);
    let share_dead_code_parse_with_health = check_production == health_production;

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
    let cache_dir = opts.cache_dir.to_path_buf();
    let canonical_cache_dir = dunce::canonicalize(&cache_dir).ok();
    // Spawn the batched base-file reader lazily: a changeset of only cache
    // artifacts or docs never touches git, so it spawns zero processes.
    let mut reader: Option<BaseFileReader> = None;
    for path in changed_files {
        if is_fallow_cache_artifact(path, &cache_dir, canonical_cache_dir.as_deref()) {
            continue;
        }
        if !is_analysis_input(path) {
            if is_non_behavioral_doc(path) {
                continue;
            }
            return false;
        }
        let Ok(current) = std::fs::read_to_string(path) else {
            return false;
        };
        let Ok(relative) = path.strip_prefix(&git_root) else {
            return false;
        };
        let reader = match reader.as_mut() {
            Some(reader) => reader,
            None => {
                let Some(spawned) = BaseFileReader::spawn(opts.root) else {
                    return false;
                };
                reader.insert(spawned)
            }
        };
        let Some(base) = reader.read(base_ref, relative) else {
            return false;
        };
        if current == base {
            continue;
        }
        if !js_ts_tokens_equivalent(path, &current, &base) {
            return false;
        }
    }
    true
}

/// A long-lived `git cat-file --batch` child process used to read the base
/// version of changed files without spawning one `git show` per file.
///
/// Requests and responses are strictly lockstep (one request line, one
/// response) to avoid pipe-buffer deadlock. Per-file comparison semantics are
/// byte-identical to the previous `git show` path: a missing object yields
/// `None` (treated as not reusable), and content is read with lossy UTF-8
/// conversion to match `String::from_utf8_lossy`.
///
/// The child is owned through a [`ScopedChild`](crate::signal::ScopedChild) so
/// an interrupt (SIGINT/SIGTERM) during a large reuse loop kills the long-lived
/// `cat-file` process via the signal registry instead of orphaning it.
struct BaseFileReader {
    /// The registered `cat-file --batch` child. Wrapped in `Option` so `Drop`
    /// can `take()` it and call the consuming `ScopedChild::wait` after closing
    /// stdin, reaping the child and deregistering its PID.
    child: Option<crate::signal::ScopedChild>,
    /// Wrapped in `Option` so `Drop` can `take()` and drop it explicitly,
    /// closing the pipe before the blocking wait (which would otherwise block).
    stdin: Option<std::process::ChildStdin>,
    stdout: std::io::BufReader<std::process::ChildStdout>,
}

impl BaseFileReader {
    /// Spawn a single `git cat-file --batch` process rooted at `root`.
    ///
    /// Returns `None` on spawn failure or if the child's stdio pipes are
    /// unavailable; the caller then degrades to "not reusable" (returns
    /// `false`), mirroring the previous per-file `git show` failure behavior.
    fn spawn(root: &Path) -> Option<Self> {
        let mut command = Command::new("git");
        command
            .args(["cat-file", "--batch"])
            .current_dir(root)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null());
        clear_ambient_git_env(&mut command);
        let mut child = crate::signal::ScopedChild::spawn(&mut command).ok()?;
        let stdin = child.take_stdin()?;
        let stdout = child.take_stdout()?;
        Some(Self {
            child: Some(child),
            stdin: Some(stdin),
            stdout: std::io::BufReader::new(stdout),
        })
    }

    /// Read the base version of `relative` at `base_ref`.
    ///
    /// Writes one `<base_ref>:<path>` request line (forward-slash separators)
    /// and reads exactly one response in lockstep. Returns `None` if the object
    /// is missing (the ` missing` header path), on any parse or IO error, or if
    /// the path contains a newline (which would corrupt the request stream).
    fn read(&mut self, base_ref: &str, relative: &Path) -> Option<String> {
        use std::io::{BufRead, Read};

        let relative = relative.to_string_lossy().replace('\\', "/");
        // A newline in the path cannot be expressed as a single batch request
        // line; treat it as not reusable rather than writing a corrupt request.
        if relative.contains('\n') {
            return None;
        }

        let stdin = self.stdin.as_mut()?;
        writeln!(stdin, "{base_ref}:{relative}").ok()?;
        stdin.flush().ok()?;

        let mut header = String::new();
        if self.stdout.read_line(&mut header).ok()? == 0 {
            return None;
        }
        // `git cat-file --batch` reports a missing object as `<spec> missing\n`.
        if header.trim_end().ends_with(" missing") {
            return None;
        }
        // Otherwise the header is `<oid> <type> <size>\n`; parse the size.
        let size: usize = header.trim_end().rsplit(' ').next()?.parse().ok()?;
        let mut buf = vec![0u8; size];
        self.stdout.read_exact(&mut buf).ok()?;
        // Consume the single trailing newline that follows the object content.
        // An off-by-one here corrupts every subsequent read in the batch.
        let mut newline = [0u8; 1];
        self.stdout.read_exact(&mut newline).ok()?;

        Some(String::from_utf8_lossy(&buf).into_owned())
    }
}

impl Drop for BaseFileReader {
    fn drop(&mut self) {
        // Close stdin so the child sees EOF and exits, then reap it through the
        // ScopedChild's blocking `wait` (which also deregisters the PID from the
        // signal registry). Dropping the `ChildStdin` closes the pipe; doing
        // this before the wait prevents it from blocking.
        self.stdin.take();
        if let Some(child) = self.child.take() {
            let _ = child.wait();
        }
    }
}

fn is_fallow_cache_artifact(
    path: &Path,
    cache_dir: &Path,
    canonical_cache_dir: Option<&Path>,
) -> bool {
    path.starts_with(cache_dir)
        || canonical_cache_dir.is_some_and(|canonical| path.starts_with(canonical))
}

fn remap_cache_dir_for_base_worktree(
    current_root: &Path,
    base_worktree_root: &Path,
    cache_dir: &Path,
) -> PathBuf {
    if cache_dir.is_absolute()
        && let Ok(relative) = cache_dir.strip_prefix(current_root)
    {
        return base_worktree_root.join(relative);
    }
    cache_dir.to_path_buf()
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

#[cfg(test)]
use std::time::SystemTime;

#[cfg(test)]
use crate::base_worktree::{
    ReusableWorktreeLock, WorktreeCleanupGuard, audit_worktree_pid, days_to_duration,
    is_fallow_audit_worktree_path, is_reusable_audit_worktree_path, list_audit_worktrees,
    materialize_base_dependency_context, parse_worktree_list, paths_equal, process_is_alive,
    remove_audit_worktree, reusable_worktree_last_used_path, reusable_worktree_lock_path,
    sweep_orphan_audit_worktrees, touch_last_used,
};

#[path = "audit_keys.rs"]
mod keys;

use keys::{
    dead_code_keys, dupe_group_key, dupes_keys, health_finding_key, health_keys,
    retain_introduced_dead_code,
};

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

    let (base_ref, base_description) = resolve_base_ref(opts)?;

    // Always sweep: prunable orphans (cache dir externally reaped, git admin
    // entry left behind) are reclaimed regardless of the age threshold, so the
    // sweep runs even when age-based GC is disabled (`max_age` is `None`).
    sweep_old_reusable_caches(
        opts.root,
        resolve_cache_max_age(opts.root, opts.config_path.as_ref()),
        opts.quiet,
    );

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
        return Ok(empty_audit_result(
            base_ref,
            base_description,
            opts,
            start.elapsed(),
        ));
    }

    let changed_since = Some(base_ref.as_str());

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
    crate::telemetry::note_final_result_count(
        summary.dead_code_issues + summary.complexity_findings + summary.duplication_clone_groups,
    );

    Ok(AuditResult {
        verdict,
        summary,
        attribution,
        base_snapshot,
        base_snapshot_skipped,
        changed_files_count,
        changed_files: changed_files.into_iter().collect(),
        base_ref,
        base_description,
        head_sha: get_head_sha(opts.root),
        output: opts.output,
        performance: opts.performance,
        check: check_result,
        dupes: dupes_result,
        health: health_result,
        elapsed: start.elapsed(),
    })
}

/// Parse a raw `FALLOW_AUDIT_BASE` value: trim, treat empty / whitespace-only as
/// unset. Pure helper so the trimming logic is testable without mutating env.
fn parse_audit_base_override(raw: Option<String>) -> Option<String> {
    let trimmed = raw?.trim().to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

/// The `FALLOW_AUDIT_BASE` override (trimmed), or `None` when unset / empty.
/// Lets a downstream consumer pin the base without editing the generated agent
/// gate script (issue #1168), e.g. `FALLOW_AUDIT_BASE=upstream/main` on a fork.
fn audit_base_env_override() -> Option<String> {
    parse_audit_base_override(std::env::var("FALLOW_AUDIT_BASE").ok())
}

/// Resolve the base ref and an optional human-readable provenance for the scope
/// line. Precedence: explicit `--changed-since` / `--base` flag, then the
/// `FALLOW_AUDIT_BASE` env override, then auto-detection.
fn resolve_base_ref(opts: &AuditOptions<'_>) -> Result<(String, Option<String>), ExitCode> {
    if let Some(ref_str) = opts.changed_since {
        return Ok((ref_str.to_string(), None));
    }
    if let Some(env_ref) = audit_base_env_override() {
        if let Err(e) = crate::validate::validate_git_ref(&env_ref) {
            return Err(emit_error(
                &format!("FALLOW_AUDIT_BASE='{env_ref}' is not a valid git ref: {e}"),
                2,
                opts.output,
            ));
        }
        let description = format!("FALLOW_AUDIT_BASE={env_ref}");
        return Ok((env_ref, Some(description)));
    }
    let Some(detected) = auto_detect_base_ref(opts.root) else {
        return Err(emit_error(
            "could not detect base branch. Use --base <ref> to specify the comparison target (e.g., --base main)",
            2,
            opts.output,
        ));
    };
    if let Err(e) = crate::validate::validate_git_ref(&detected.git_ref) {
        return Err(emit_error(
            &format!(
                "auto-detected base ref '{}' is not a valid git ref: {e}",
                detected.git_ref
            ),
            2,
            opts.output,
        ));
    }
    Ok((detected.git_ref, Some(detected.description)))
}

/// Build an empty pass result when no files have changed.
fn empty_audit_result(
    base_ref: String,
    base_description: Option<String>,
    opts: &AuditOptions<'_>,
    elapsed: Duration,
) -> AuditResult {
    crate::telemetry::note_final_result_count(0);

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
        changed_files: Vec::new(),
        base_ref,
        base_description,
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
        complexity_breakdown: false,
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
        report_only: false,
        runtime_coverage,
        // audit runs no hotspot/ownership pass; --churn-file is health-only.
        churn_file: None,
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

#[path = "audit_output.rs"]
mod output;

pub use output::print_audit_result;

/// Run the full audit command: execute analyses, print results, return exit code.
/// Run audit, optionally tagged with a gate marker (e.g. `"pre-commit"`) so
/// Fallow Impact can record a containment event when the gate blocks then
/// clears. The marker only affects the local Impact store; it never changes
/// the verdict, exit code, or output.
pub fn run_audit(opts: &AuditOptions<'_>, gate_marker: Option<&str>) -> ExitCode {
    if let Err(e) = crate::health::scoring::validate_coverage_root_absolute(opts.coverage_root) {
        return emit_error(&e, 2, opts.output);
    }
    let coverage_resolved = opts
        .coverage
        .map(|p| crate::health::scoring::resolve_relative_to_root(p, Some(opts.root)));
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
            let mut findings = result
                .check
                .as_ref()
                .map(|c| crate::impact::collect_dead_code_findings(&c.results))
                .unwrap_or_default();
            if let Some(health) = result.health.as_ref() {
                findings.extend(crate::impact::collect_complexity_findings(&health.report));
            }
            let clones = result
                .dupes
                .as_ref()
                .map(|d| crate::impact::collect_clone_findings(&d.report))
                .unwrap_or_default();
            let empty_supps: Vec<fallow_core::results::ActiveSuppression> = Vec::new();
            let suppressions = result.check.as_ref().map_or(empty_supps.as_slice(), |c| {
                c.results.active_suppressions.as_slice()
            });
            let attribution = crate::impact::AttributionInput {
                root: opts.root,
                scope: crate::impact::Scope::ChangedFiles(&result.changed_files),
                findings,
                clones,
                suppressions,
            };
            crate::impact::record_audit_run(
                opts.root,
                &result.summary,
                &crate::impact::AuditRunRecord {
                    verdict: result.verdict,
                    gate: gate_marker.is_some(),
                    git_sha: result.head_sha.as_deref(),
                    version: env!("CARGO_PKG_VERSION"),
                    timestamp: &crate::vital_signs::chrono_timestamp(),
                    attribution: Some(&attribution),
                },
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

    /// Add a tracked file and commit it; return the new HEAD SHA.
    fn commit_file(repo: &std::path::Path, name: &str, body: &str) -> String {
        fs::write(repo.join(name), body).expect("file should be written");
        git(repo, &["add", "."]);
        git(repo, &["-c", "commit.gpgsign=false", "commit", "-m", name]);
        git_rev_parse(repo, "HEAD").expect("HEAD should resolve")
    }

    #[test]
    fn auto_detect_base_ref_resolves_origin_default_to_merge_base() {
        let tmp = tempfile::TempDir::new().expect("temp dir should be created");
        let repo = init_throwaway_repo(tmp.path(), "repo");
        let head = git_rev_parse(&repo, "HEAD").expect("HEAD should resolve");
        git(&repo, &["branch", "trunk"]);
        git(&repo, &["update-ref", "refs/remotes/origin/trunk", "trunk"]);
        git(
            &repo,
            &[
                "symbolic-ref",
                "refs/remotes/origin/HEAD",
                "refs/remotes/origin/trunk",
            ],
        );

        let detected = auto_detect_base_ref(&repo).expect("base should be detected");
        // trunk == HEAD, so the merge-base is HEAD's own SHA (the bare branch
        // name `trunk` is no longer returned: it would resolve to a local ref).
        assert_eq!(detected.git_ref, head);
        assert_eq!(detected.description, "merge-base with origin/trunk");
    }

    /// Regression for issue #1168: a worktree checkout whose local `main` is
    /// stale relative to a fresh `origin/main`. The base must be the fork point
    /// (merge-base with `origin/main`), NOT the stale local-`main` commit that
    /// the old bare-name resolution diffed against.
    #[test]
    fn auto_detect_base_ref_ignores_stale_local_main() {
        let tmp = tempfile::TempDir::new().expect("temp dir should be created");
        let repo = init_throwaway_repo(tmp.path(), "repo");
        let stale = git_rev_parse(&repo, "HEAD").expect("HEAD should resolve");

        // origin/main starts at the first commit, then a teammate advances it.
        git(&repo, &["update-ref", "refs/remotes/origin/main", "main"]);
        git(
            &repo,
            &[
                "symbolic-ref",
                "refs/remotes/origin/HEAD",
                "refs/remotes/origin/main",
            ],
        );
        let fork_point = commit_file(&repo, "teammate.txt", "merged work\n");
        git(&repo, &["update-ref", "refs/remotes/origin/main", "main"]);

        // Cut a feature branch from the fresh origin tip using the raw SHA (no
        // upstream tracking), then leave local `main` behind at the stale commit.
        git(&repo, &["checkout", "-b", "feature", &fork_point]);
        commit_file(&repo, "feature.txt", "my change\n");
        git(&repo, &["branch", "-f", "main", &stale]);

        let detected = auto_detect_base_ref(&repo).expect("base should be detected");
        assert_eq!(
            detected.git_ref, fork_point,
            "base must be the fork point (origin/main), not stale local main"
        );
        assert_ne!(
            detected.git_ref, stale,
            "must not diff against stale local main"
        );
        assert_eq!(detected.description, "merge-base with origin/main");
    }

    #[test]
    fn auto_detect_base_ref_prefers_configured_upstream() {
        let tmp = tempfile::TempDir::new().expect("temp dir should be created");
        let repo = init_throwaway_repo(tmp.path(), "repo");
        let fork_point = git_rev_parse(&repo, "HEAD").expect("HEAD should resolve");
        // Configure `origin` so refs/remotes/origin/* are recognized as tracking
        // refs and `--set-upstream-to` is accepted.
        git(&repo, &["remote", "add", "origin", &repo.to_string_lossy()]);
        git(&repo, &["update-ref", "refs/remotes/origin/main", "main"]);

        git(&repo, &["checkout", "-b", "feature"]);
        git(
            &repo,
            &["branch", "--set-upstream-to=origin/main", "feature"],
        );
        commit_file(&repo, "feature.txt", "my change\n");

        let detected = auto_detect_base_ref(&repo).expect("base should be detected");
        assert_eq!(detected.git_ref, fork_point);
        assert_eq!(detected.description, "merge-base with origin/main");
    }

    #[test]
    fn auto_detect_base_ref_falls_back_to_local_main_without_remote() {
        let tmp = tempfile::TempDir::new().expect("temp dir should be created");
        let repo = init_throwaway_repo(tmp.path(), "repo");

        let detected = auto_detect_base_ref(&repo).expect("base should be detected");
        assert_eq!(detected.git_ref, "main");
        assert_eq!(detected.description, "local main");
    }

    #[test]
    fn auto_detect_base_ref_falls_back_to_local_master_without_remote() {
        let tmp = tempfile::TempDir::new().expect("temp dir should be created");
        let repo = tmp.path().join("repo");
        fs::create_dir_all(&repo).expect("repo root should be created");
        fs::write(repo.join("README.md"), "seed\n").expect("seed file should be written");
        git(&repo, &["init", "-b", "master"]);
        git(&repo, &["add", "."]);
        git(
            &repo,
            &["-c", "commit.gpgsign=false", "commit", "-m", "initial"],
        );

        let detected = auto_detect_base_ref(&repo).expect("base should be detected");
        assert_eq!(detected.git_ref, "master");
        assert_eq!(detected.description, "local master");
    }

    #[test]
    fn auto_detect_base_ref_returns_none_outside_git_repo() {
        let tmp = tempfile::TempDir::new().expect("temp dir should be created");

        assert!(auto_detect_base_ref(tmp.path()).is_none());
    }

    #[test]
    fn parse_audit_base_override_trims_and_rejects_empty() {
        assert_eq!(parse_audit_base_override(None), None);
        assert_eq!(parse_audit_base_override(Some(String::new())), None);
        assert_eq!(parse_audit_base_override(Some("   ".to_string())), None);
        assert_eq!(
            parse_audit_base_override(Some("  origin/main  ".to_string())),
            Some("origin/main".to_string())
        );
    }

    /// When the remote default shares no history with HEAD (the merge-base
    /// failure case a shallow clone also hits), auto-detect falls back to the
    /// remote-tracking ref tip rather than failing detection.
    #[test]
    fn auto_detect_base_ref_falls_back_to_remote_tip_without_common_ancestor() {
        let tmp = tempfile::TempDir::new().expect("temp dir should be created");
        let repo = init_throwaway_repo(tmp.path(), "repo");
        // Build an unrelated-history commit and point origin/main at it, so
        // merge-base(origin/main, HEAD) has no common ancestor.
        git(&repo, &["checkout", "--orphan", "unrelated"]);
        commit_file(&repo, "unrelated.txt", "no shared history\n");
        let unrelated = git_rev_parse(&repo, "HEAD").expect("HEAD should resolve");
        git(
            &repo,
            &["update-ref", "refs/remotes/origin/main", &unrelated],
        );
        git(
            &repo,
            &[
                "symbolic-ref",
                "refs/remotes/origin/HEAD",
                "refs/remotes/origin/main",
            ],
        );
        git(&repo, &["checkout", "main"]);

        let detected = auto_detect_base_ref(&repo).expect("base should be detected");
        assert_eq!(detected.git_ref, "origin/main");
        assert_eq!(detected.description, "origin/main (tip)");
    }

    #[test]
    fn get_head_sha_returns_short_head_for_git_repo() {
        let tmp = tempfile::TempDir::new().expect("temp dir should be created");
        let repo = init_throwaway_repo(tmp.path(), "repo");
        let output = Command::new("git")
            .args(["rev-parse", "--short", "HEAD"])
            .current_dir(&repo)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .output()
            .expect("git rev-parse should run");
        assert!(output.status.success());

        assert_eq!(
            get_head_sha(&repo),
            Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
        );
    }

    #[test]
    fn get_head_sha_returns_none_outside_git_repo() {
        let tmp = tempfile::TempDir::new().expect("temp dir should be created");

        assert_eq!(get_head_sha(tmp.path()), None);
    }

    fn worktree_is_registered_with_git(repo_root: &std::path::Path, worktree_path: &Path) -> bool {
        list_audit_worktrees(repo_root)
            .is_some_and(|paths| paths.iter().any(|p| paths_equal(p, worktree_path)))
    }

    /// True when `git worktree list --porcelain` still carries an admin entry
    /// whose path ends with `worktree_path`'s basename. Unlike
    /// `worktree_is_registered_with_git`, this matches by basename against the
    /// raw porcelain output, so it stays correct even when the directory has
    /// been deleted (a prunable orphan): `paths_equal` canonicalization cannot
    /// match a missing path across the macOS `/var` -> `/private/var` symlink,
    /// but the unique nanos-suffixed basename is stable.
    fn worktree_admin_entry_present(repo_root: &std::path::Path, worktree_path: &Path) -> bool {
        let basename = worktree_path
            .file_name()
            .and_then(|n| n.to_str())
            .expect("reusable worktree path has a utf-8 basename");
        let output = Command::new("git")
            .args(["worktree", "list", "--porcelain"])
            .current_dir(repo_root)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .output()
            .expect("git worktree list should run");
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|line| line.strip_prefix("worktree "))
            .any(|p| p.ends_with(basename))
    }

    #[test]
    fn worktree_cleanup_guard_runs_on_drop() {
        let tmp = tempfile::TempDir::new().expect("temp dir should be created");
        let repo = init_throwaway_repo(tmp.path(), "repo");
        let worktree_path = tmp.path().join("fallow-audit-base-1234-5678");

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

        remove_audit_worktree(&repo, &worktree_path);
        let _ = fs::remove_dir_all(&worktree_path);
    }

    #[test]
    fn audit_orphan_sweep_removes_dead_pid_worktree() {
        const DEAD_PID: u32 = 99_999_999;
        assert!(!process_is_alive(DEAD_PID));

        let tmp = tempfile::TempDir::new().expect("temp dir should be created");
        let repo = init_throwaway_repo(tmp.path(), "repo");

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

        sweep_old_reusable_caches(&repo, Some(Duration::from_hours(30 * 24)), true);

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
        cleanup_reusable_worktree(&repo, &worktree_path);
    }

    #[test]
    fn reusable_cache_gc_keeps_fresh_entry() {
        let tmp = tempfile::TempDir::new().expect("temp dir should be created");
        let repo = init_throwaway_repo(tmp.path(), "repo-gc-keep");
        let worktree_path = make_reusable_path("gc-keep");
        register_reusable_worktree(&repo, &worktree_path);
        write_sidecar_with_age(&worktree_path, Duration::from_mins(1));

        sweep_old_reusable_caches(&repo, Some(Duration::from_hours(30 * 24)), true);

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

        let lock = ReusableWorktreeLock::try_acquire(&worktree_path)
            .expect("test should acquire the lock first");

        sweep_old_reusable_caches(&repo, Some(Duration::from_hours(30 * 24)), true);

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
        let sidecar = reusable_worktree_last_used_path(&worktree_path);
        assert!(
            !sidecar.exists(),
            "test pre-condition: sidecar should not exist",
        );

        sweep_old_reusable_caches(&repo, Some(Duration::from_hours(30 * 24)), true);

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
    fn reusable_cache_gc_reclaims_prunable_orphan_when_dir_missing() {
        let tmp = tempfile::TempDir::new().expect("temp dir should be created");
        let repo = init_throwaway_repo(tmp.path(), "repo-gc-orphan");
        let worktree_path = make_reusable_path("gc-orphan");
        register_reusable_worktree(&repo, &worktree_path);
        // Fresh sidecar: the age branch alone would KEEP this entry, so a
        // successful reclaim proves the dir-missing branch drove it.
        write_sidecar_with_age(&worktree_path, Duration::from_mins(1));
        let sidecar = reusable_worktree_last_used_path(&worktree_path);

        // Simulate an external temp-reaper: delete only the worktree directory,
        // leaving git's admin entry and the sidecar behind.
        fs::remove_dir_all(&worktree_path).expect("test should remove the cache dir");
        assert!(
            !worktree_path.exists(),
            "test pre-condition: cache dir should be gone",
        );
        assert!(
            worktree_admin_entry_present(&repo, &worktree_path),
            "test pre-condition: git admin entry should still be registered (prunable)",
        );
        assert!(
            sidecar.exists(),
            "test pre-condition: sidecar survives a dir-only reaper",
        );

        sweep_old_reusable_caches(&repo, Some(Duration::from_hours(30 * 24)), true);

        assert!(
            !worktree_admin_entry_present(&repo, &worktree_path),
            "sweep should unregister a prunable orphan whose dir was externally removed",
        );
        assert!(
            !sidecar.exists(),
            "sweep should remove the stale sidecar for a reclaimed orphan",
        );
        cleanup_reusable_worktree(&repo, &worktree_path);
    }

    #[test]
    fn reusable_cache_gc_reclaims_prunable_orphan_even_when_age_gc_disabled() {
        let tmp = tempfile::TempDir::new().expect("temp dir should be created");
        let repo = init_throwaway_repo(tmp.path(), "repo-gc-orphan-nogc");
        let worktree_path = make_reusable_path("gc-orphan-nogc");
        register_reusable_worktree(&repo, &worktree_path);
        write_sidecar_with_age(&worktree_path, Duration::from_mins(1));
        let sidecar = reusable_worktree_last_used_path(&worktree_path);
        fs::remove_dir_all(&worktree_path).expect("test should remove the cache dir");
        assert!(
            worktree_admin_entry_present(&repo, &worktree_path),
            "test pre-condition: git admin entry should still be registered (prunable)",
        );
        assert!(
            sidecar.exists(),
            "test pre-condition: sidecar survives a dir-only reaper",
        );

        // `None` = age-based GC disabled (`cacheMaxAgeDays = 0`). Orphan reclaim
        // must still run so dead admin entries do not accumulate forever.
        sweep_old_reusable_caches(&repo, None, true);

        assert!(
            !worktree_admin_entry_present(&repo, &worktree_path),
            "orphan reclaim must run even when age-based GC is disabled",
        );
        assert!(
            !sidecar.exists(),
            "sweep should remove the stale sidecar even when age-based GC is disabled",
        );
        cleanup_reusable_worktree(&repo, &worktree_path);
    }

    #[test]
    fn reusable_cache_gc_preserves_lock_file_after_removal() {
        let tmp = tempfile::TempDir::new().expect("temp dir should be created");
        let repo = init_throwaway_repo(tmp.path(), "repo-gc-lockfile");
        let worktree_path = make_reusable_path("gc-lockfile");
        register_reusable_worktree(&repo, &worktree_path);
        write_sidecar_with_age(&worktree_path, Duration::from_hours(31 * 24));
        let lock_path = reusable_worktree_lock_path(&worktree_path);
        drop(
            ReusableWorktreeLock::try_acquire(&worktree_path)
                .expect("test should acquire the lock"),
        );
        assert!(
            lock_path.exists(),
            "test pre-condition: lock file should exist before sweep",
        );

        sweep_old_reusable_caches(&repo, Some(Duration::from_hours(30 * 24)), true);

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
    fn reuse_or_create_stamps_sidecar_on_fresh_create() {
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

        drop(worktree);
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
        let reusable = tmp.path().join("fallow-audit-base-cache-deadbeef-0000");
        let lock_path = reusable_worktree_lock_path(&reusable);

        let first = ReusableWorktreeLock::try_acquire(&reusable)
            .expect("first acquire on a fresh path should succeed");
        assert!(
            ReusableWorktreeLock::try_acquire(&reusable).is_none(),
            "second acquire must fail while the first is held",
        );
        drop(first);
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
    fn audit_base_snapshot_cache_dir_writes_gitignore() {
        let tmp = tempfile::TempDir::new().expect("temp dir should be created");
        let cache_root = tmp.path().join(".custom-fallow-cache");
        let cache_dir = audit_base_snapshot_cache_dir(&cache_root);

        ensure_audit_base_snapshot_cache_dir(&cache_dir).expect("cache dir should be created");

        assert_eq!(
            fs::read_to_string(cache_dir.join(".gitignore")).expect("gitignore should read"),
            "*\n"
        );
    }

    #[test]
    fn audit_base_snapshot_cache_roundtrips_from_disk() {
        let tmp = tempfile::TempDir::new().expect("temp dir should be created");
        let config_path = None;
        let cache_root = tmp.path().join(".custom-fallow-cache");
        let opts = AuditOptions {
            root: tmp.path(),
            cache_dir: &cache_root,
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
        let key = AuditBaseSnapshotCacheKey {
            hash: 0xfeed,
            base_sha: "abc123".to_string(),
        };
        let snapshot = AuditKeySnapshot {
            dead_code: std::iter::once("dead:a".to_string()).collect(),
            health: std::iter::once("health:a".to_string()).collect(),
            dupes: std::iter::once("dupe:a".to_string()).collect(),
        };

        save_cached_base_snapshot(&opts, &key, &snapshot);
        assert!(
            audit_base_snapshot_cache_file(&cache_root, &key).exists(),
            "snapshot should be saved below the configured cache directory"
        );
        let loaded = load_cached_base_snapshot(&opts, &key).expect("snapshot should load");

        assert_eq!(loaded.dead_code, snapshot.dead_code);
        assert_eq!(loaded.health, snapshot.health);
        assert_eq!(loaded.dupes, snapshot.dupes);
    }

    #[test]
    fn audit_base_snapshot_cache_rejects_mismatched_key() {
        let tmp = tempfile::TempDir::new().expect("temp dir should be created");
        let config_path = None;
        let cache_root = tmp.path().join(".custom-fallow-cache");
        let opts = AuditOptions {
            root: tmp.path(),
            cache_dir: &cache_root,
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
        let key = AuditBaseSnapshotCacheKey {
            hash: 0xbeef,
            base_sha: "head".to_string(),
        };
        let cached = CachedAuditKeySnapshot {
            version: AUDIT_BASE_SNAPSHOT_CACHE_VERSION,
            cli_version: env!("CARGO_PKG_VERSION").to_string(),
            key_hash: key.hash,
            base_sha: "other".to_string(),
            dead_code: vec!["dead:a".to_string()],
            health: vec![],
            dupes: vec![],
        };
        let cache_dir = audit_base_snapshot_cache_dir(&cache_root);
        ensure_audit_base_snapshot_cache_dir(&cache_dir).expect("cache dir should be created");
        fs::write(
            audit_base_snapshot_cache_file(&cache_root, &key),
            bitcode::encode(&cached),
        )
        .expect("cache file should be written");

        assert!(load_cached_base_snapshot(&opts, &key).is_none());
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
        let cache_root = root.join(".fallow");
        let opts = AuditOptions {
            root,
            cache_dir: &cache_root,
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
        let cache_root = root.join(".fallow");
        let opts = AuditOptions {
            root,
            cache_dir: &cache_root,
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
        let cache_root = root.join(".fallow");
        let opts = AuditOptions {
            root,
            cache_dir: &cache_root,
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
        let cache_root = root.join(".fallow");
        let opts = AuditOptions {
            root,
            cache_dir: &cache_root,
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
        assert!(
            result.dupes.is_some(),
            "dupes should run when changed files exist"
        );
    }

    #[test]
    fn audit_dupes_falls_back_to_own_discovery_when_health_off() {
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
        let cache_root = root.join(".fallow");
        let opts = AuditOptions {
            root,
            cache_dir: &cache_root,
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
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let real = tmp.path().join("real");
        let link = tmp.path().join("link");
        fs::create_dir_all(&real).expect("real dir");
        std::os::unix::fs::symlink(&real, &link).expect("symlink");
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
    fn remap_cache_dir_moves_project_local_cache_to_base_worktree() {
        let tmp = tempfile::TempDir::new().expect("temp dir should be created");
        let current_root = tmp.path().join("repo");
        let base_root = tmp.path().join("fallow-base");
        let cache_dir = current_root.join(".cache").join("fallow");

        let remapped = remap_cache_dir_for_base_worktree(&current_root, &base_root, &cache_dir);

        assert_eq!(remapped, base_root.join(".cache").join("fallow"));
    }

    #[test]
    fn remap_cache_dir_keeps_external_absolute_cache_shared() {
        let tmp = tempfile::TempDir::new().expect("temp dir should be created");
        let current_root = tmp.path().join("repo");
        let base_root = tmp.path().join("fallow-base");
        let cache_dir = tmp.path().join("shared").join("fallow-cache");

        let remapped = remap_cache_dir_for_base_worktree(&current_root, &base_root, &cache_dir);

        assert_eq!(remapped, cache_dir);
    }

    #[test]
    fn audit_gate_new_only_inherits_pre_existing_duplicates_in_focused_files() {
        let tmp = tempfile::TempDir::new().expect("temp dir should be created");
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
        let cache_root = root.join(".fallow");
        let opts = AuditOptions {
            root,
            cache_dir: &cache_root,
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
        let cache_root = root.join(".fallow");
        let opts = AuditOptions {
            root,
            cache_dir: &cache_root,
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
        let cache_root = root.join(".fallow");
        let opts = AuditOptions {
            root: &root,
            cache_dir: &cache_root,
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
        let cache_root = root.join(".fallow");
        let opts = AuditOptions {
            root,
            cache_dir: &cache_root,
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
        let cache_root = root.join(".fallow");
        let opts = AuditOptions {
            root,
            cache_dir: &cache_root,
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
        let cache_root = root.join(".fallow");
        let opts = AuditOptions {
            root,
            cache_dir: &cache_root,
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
        let cache_root = root.join(".fallow");
        let opts = AuditOptions {
            root,
            cache_dir: &cache_root,
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

    // ── Unit tests for js_ts_tokens_equivalent, is_analysis_input, is_non_behavioral_doc ──

    #[test]
    fn tokens_equivalent_whitespace_only() {
        // Reformatting (indentation, blank lines) must not change token identity.
        let a = "export const x = 1;\nexport const y = 2;\n";
        let b = "export const x = 1;\n\n\nexport const y = 2;\n";
        assert!(
            js_ts_tokens_equivalent(Path::new("a.ts"), a, b),
            "whitespace-only change must be treated as equivalent"
        );
    }

    #[test]
    fn tokens_equivalent_comment_only_change() {
        // Comments do not produce tokens; adding or removing a comment should be
        // treated as equivalent by the tokenizer.
        let a = "export const x = 1;\n";
        let b = "// note\nexport const x = 1;\n";
        assert!(
            js_ts_tokens_equivalent(Path::new("a.ts"), a, b),
            "comment-only change must be treated as equivalent (comments emit no tokens)"
        );
    }

    #[test]
    fn tokens_equivalent_identifier_rename_is_not_equivalent() {
        // Identifier carries its text payload; a rename must not be reusable.
        let a = "export const a = 1;\n";
        let b = "export const b = 1;\n";
        assert!(
            !js_ts_tokens_equivalent(Path::new("a.ts"), a, b),
            "identifier rename must be treated as non-equivalent"
        );
    }

    #[test]
    fn tokens_equivalent_string_literal_change_is_not_equivalent() {
        // StringLiteral carries its text payload; a changed import path must not be reusable.
        let a = r#"import x from "./a";"#;
        let b = r#"import x from "./b";"#;
        assert!(
            !js_ts_tokens_equivalent(Path::new("a.ts"), a, b),
            "string-literal change must be treated as non-equivalent"
        );
    }

    #[test]
    fn tokens_equivalent_fallow_ignore_marker_forces_false() {
        // The guard fires before tokenization; even identical content containing the
        // marker must return false so suppression changes are never skipped.
        let code = "// fallow-ignore-next-line unused-exports\nexport const x = 1;\n";
        assert!(
            !js_ts_tokens_equivalent(Path::new("a.ts"), code, code),
            "fallow-ignore marker in either side must force false"
        );
    }

    #[test]
    fn tokens_equivalent_non_js_extension_is_false() {
        // The extension check fires before tokenization; CSS content cannot be reused.
        let a = ".foo { color: red; }\n";
        let b = ".foo {\n  color: red;\n}\n";
        assert!(
            !js_ts_tokens_equivalent(Path::new("styles.css"), a, b),
            "non-JS/TS extension must always return false"
        );
    }

    /// KNOWN SOUNDNESS GAP: `TokenKind::TemplateLiteral` carries no payload
    /// (see `crates/core/src/duplicates/token_types.rs`), so a change to the
    /// content of a template literal is invisible to the tokenizer and is
    /// treated as equivalent. This is safe for most template strings but
    /// unsound for dynamic `import(\`...\`)` patterns where the quasi prefix
    /// feeds module-resolution pattern edges. This test pins the current
    /// behavior. A follow-up fix should give `TemplateLiteral` a payload to
    /// close the gap.
    #[test]
    fn tokens_equivalent_template_literal_content_change_is_equivalent_known_gap() {
        let a = "const p = import(`./pages/${x}`);\n";
        let b = "const p = import(`./views/${x}`);\n";
        // KNOWN GAP: changing the quasi string of a template literal is NOT
        // detected as a behavioral change because TokenKind::TemplateLiteral
        // has no payload. Expected: true (equivalent), which is incorrect for
        // dynamic-import prefixes but documents the current reality.
        assert!(
            js_ts_tokens_equivalent(Path::new("a.ts"), a, b),
            "template-literal content change is CURRENTLY treated as equivalent (known gap)"
        );
    }

    /// Companion to the template-literal gap test: a regex-literal content
    /// change is also invisible to the tokenizer.
    #[test]
    fn tokens_equivalent_regex_literal_content_change_is_equivalent_known_gap() {
        let a = "const re = /^foo/;\n";
        let b = "const re = /^bar/;\n";
        // KNOWN GAP: TokenKind::RegExpLiteral has no payload.
        assert!(
            js_ts_tokens_equivalent(Path::new("a.ts"), a, b),
            "regex-literal content change is CURRENTLY treated as equivalent (known gap)"
        );
    }

    #[test]
    fn analysis_input_and_doc_classification() {
        // Analysis inputs: JS/TS variants and component formats are behavioral.
        assert!(is_analysis_input(Path::new("src/app.ts")));
        assert!(is_analysis_input(Path::new("src/app.tsx")));
        assert!(is_analysis_input(Path::new("src/app.js")));
        assert!(is_analysis_input(Path::new("src/app.jsx")));
        assert!(is_analysis_input(Path::new("src/app.mts")));
        assert!(is_analysis_input(Path::new("src/app.vue")));
        assert!(is_analysis_input(Path::new("src/styles.css")));

        // Non-analysis inputs.
        assert!(!is_analysis_input(Path::new("README.md")));
        assert!(!is_analysis_input(Path::new("package.json")));
        assert!(!is_analysis_input(Path::new("image.png")));

        // Non-behavioral docs.
        assert!(is_non_behavioral_doc(Path::new("README.md")));
        assert!(is_non_behavioral_doc(Path::new("CHANGELOG.txt")));
        assert!(is_non_behavioral_doc(Path::new("docs/guide.rst")));
        assert!(is_non_behavioral_doc(Path::new("docs/guide.adoc")));

        // .json is neither an analysis input nor a non-behavioral doc, so the
        // predicate treats it as behavioral (can_reuse returns false for it).
        assert!(!is_analysis_input(Path::new("package.json")));
        assert!(!is_non_behavioral_doc(Path::new("package.json")));
    }
}
