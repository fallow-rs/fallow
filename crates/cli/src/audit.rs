use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::{Duration, Instant};

use fallow_config::{AuditGate, OutputFormat};
use fallow_engine::changed_files::clear_ambient_git_env;
use rustc_hash::{FxHashMap, FxHashSet};
use xxhash_rust::xxh3::xxh3_64;

pub use fallow_api::{AuditAttribution, AuditSummary, AuditVerdict};

#[cfg(test)]
use crate::base_worktree::git_rev_parse;
use crate::base_worktree::{
    BaseWorktree, git_toplevel, resolve_cache_max_age, sweep_old_reusable_caches,
};
use crate::check::{CheckOptions, CheckResult, IssueFilters, TraceOptions};
use crate::dupes::{DupesMode, DupesOptions, DupesResult};
use crate::error::emit_error;
use crate::health::{HealthOptions, HealthResult};

/// Full audit result containing verdict, summary, and sub-results.
pub struct AuditResult {
    pub verdict: AuditVerdict,
    pub summary: AuditSummary,
    pub attribution: AuditAttribution,
    /// Key snapshot of the base ref for new-vs-inherited attribution. `None`
    /// when the base pass was skipped (`--gate all`) or unavailable. Exposed at
    /// crate scope so test fixtures in sibling modules can construct an
    /// `AuditResult` with `base_snapshot: None`.
    pub base_snapshot: Option<AuditKeySnapshot>,
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
    /// Review-brief data, populated only on the brief path. The deltas are
    /// computed from the head sets vs the base snapshot; weakening + routing are
    /// computed from git over the changed files. `None` off the brief path.
    pub review_deltas: Option<crate::audit_brief::ReviewDeltas>,
    pub weakening_signals: Vec<weakening::WeakeningSignal>,
    pub routing: Option<routing::RoutingFacts>,
    /// Decision surface (the apex): the ranked, capped, signal_id-anchored set
    /// of consequential structural decisions, each framed as a judgment question.
    /// Populated only on the brief path; `None` otherwise.
    pub decision_surface: Option<crate::audit_decision_surface::DecisionSurface>,
    /// Deterministic graph-snapshot hash: a stable hash of the relevant HEAD
    /// graph + diff state (the six key sets plus the resolved base ref + head
    /// sha). Pinned into the walkthrough guide digest so a stale agent JSON
    /// (whose echoed hash != this) is REFUSED on reentry. The verifier is the
    /// graph: a mutated tree changes a key set, changes this hash, refuses the
    /// stale payload. Populated only on the brief path; `None` otherwise.
    pub graph_snapshot_hash: Option<String>,
    /// Per-hunk change anchors derived from the diff: one stable, content-
    /// addressed id per changed region. Emitted in the walkthrough guide so an
    /// agent can anchor a trade-off about a changed region with no graph finding
    /// (and have it post-validated). Also folded into `graph_snapshot_hash` so a
    /// moved region refuses a stale payload. Populated only on the brief path.
    pub change_anchors: Vec<crate::audit_walkthrough::ChangeAnchor>,
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
    /// Run styling analytics (CSS + CSS-in-JS) in the health sub-pass so styling
    /// signals surface in the audit output. Default on; `--no-css` disables.
    /// Descriptive + verdict-neutral (never affects the audit verdict / exit code).
    pub css: bool,
    /// Run the project-wide CSS pass and narrow cross-file findings back to
    /// changed anchors. Default on for audit; `--no-css-deep` disables.
    pub css_deep: bool,
    /// Paid runtime-coverage sidecar input (V8 directory, V8 JSON, or
    /// Istanbul coverage map). Forwarded into the embedded health pass so
    /// audit surfaces the `hot-path-touched` verdict alongside dead-code
    /// and complexity findings without requiring a second `fallow health`
    /// invocation in CI.
    pub runtime_coverage: Option<&'a std::path::Path>,
    /// Threshold for hot-path classification, forwarded to the sidecar.
    pub min_invocations_hot: u64,
    /// Render the deterministic, always-exit-0 review brief (`fallow audit
    /// --brief` / `fallow review`) instead of the gating audit report. The
    /// audit analysis still runs and the verdict is still computed and carried
    /// informationally; it just never drives the exit code on this path.
    pub brief: bool,
    /// Decision-surface cap (the working-memory limit). Default 4; clamped to
    /// [3, 5] (4 plus or minus 1) by the extractor. Only consulted on the brief
    /// path.
    pub max_decisions: usize,
    /// Emit the agent-contract walkthrough GUIDE (digest + schema + graph-
    /// snapshot pin) instead of the brief body. Implies `brief`. Always exit 0.
    pub walkthrough_guide: bool,
    /// Render the existing walkthrough guide as a staged human or markdown tour.
    /// Implies `brief`. Always exit 0.
    pub walkthrough: bool,
    /// Changed files to record as viewed in the local walkthrough state ledger
    /// before rendering the tour. Empty off the walkthrough path.
    pub mark_viewed: &'a [std::path::PathBuf],
    /// Expand the Cleared panel in the human or markdown walkthrough tour.
    pub show_cleared: bool,
    /// Path to an agent's judgment JSON to POST-VALIDATE against the live
    /// graph. Implies `brief`. Always exit 0. `None` off the walkthrough path.
    pub walkthrough_file: Option<&'a std::path::Path>,
    /// Expand the de-prioritized units in the human focus map ("show me what
    /// you de-prioritized"). The `deprioritized` list is ALWAYS in the JSON
    /// regardless; this only re-expands the human render (collapse-by-default).
    /// Only consulted on the brief path.
    pub show_deprioritized: bool,
}

#[path = "audit_base_ref.rs"]
mod base_ref;
#[path = "audit_cache.rs"]
mod cache;

#[cfg(test)]
use base_ref::{auto_detect_base_ref, parse_audit_base_override};
use base_ref::{get_head_sha, resolve_base_ref};
#[cfg(test)]
use cache::{
    AUDIT_BASE_SNAPSHOT_CACHE_VERSION, CachedAuditKeySnapshot, audit_base_snapshot_cache_dir,
    audit_base_snapshot_cache_file, cached_from_snapshot, config_file_fingerprint,
    ensure_audit_base_snapshot_cache_dir, snapshot_from_cached,
};
use cache::{
    AuditBaseSnapshotCacheKey, audit_base_snapshot_cache_key, load_cached_base_snapshot,
    save_cached_base_snapshot, sorted_keys,
};

/// Whether a styling finding's per-rule severity escalates to `error` (and thus
/// gates the verdict). Styling is verdict-NEUTRAL by default (rule `warn`); each
/// family maps its kebab `code` to its `RulesConfig` rule. Add a match arm per
/// graduating family.
fn styling_finding_gates(rules: &fallow_config::RulesConfig, code: &str) -> bool {
    let severity = match code {
        "css-token-drift" => rules.css_token_drift,
        "css-duplicate-block" => rules.css_duplicate_block,
        "css-selector-complexity" => rules.css_selector_complexity,
        "css-dead-surface" => rules.css_dead_surface,
        "css-broken-reference" => rules.css_broken_reference,
        _ => fallow_config::Severity::Warn,
    };
    severity == fallow_config::Severity::Error
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

    // Styling findings are verdict-NEUTRAL by default (the rule defaults to
    // `warn`): a warn styling finding never flips the verdict, Pass stays Pass.
    // Only a per-rule `error` escalation gates. Sole family today is
    // css-token-drift; add sibling rule checks as families graduate.
    if let Some(result) = health
        && result
            .report
            .styling_findings
            .iter()
            .any(|finding| styling_finding_gates(&result.config.rules, &finding.code))
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
        let (errors, warnings) = introduced_check_verdict(result, base);
        has_errors |= errors;
        has_warnings |= warnings;
    }
    if let Some(result) = health {
        has_errors |= introduced_health_has_errors(result, base);
    }
    if let Some(result) = health
        && result
            .report
            .styling_findings
            .iter()
            .any(|finding| introduced_styling_finding_gates(result, base, finding))
    {
        has_errors = true;
    }
    if let Some(result) = dupes {
        let (errors, warnings) = introduced_dupes_verdict(result, base);
        has_errors |= errors;
        has_warnings |= warnings;
    }

    if has_errors {
        AuditVerdict::Fail
    } else if has_warnings {
        AuditVerdict::Warn
    } else {
        AuditVerdict::Pass
    }
}

/// Error/warning contribution from dead-code issues introduced since the base.
fn introduced_check_verdict(result: &CheckResult, base: Option<&AuditKeySnapshot>) -> (bool, bool) {
    let base_keys = base.map(|b| &b.dead_code);
    let mut introduced = result.results.clone();
    retain_introduced_dead_code(&mut introduced, &result.config.root, base_keys);
    if crate::check::has_error_severity_issues(
        &introduced,
        &result.config.rules,
        Some(&result.config),
    ) {
        (true, false)
    } else if introduced.total_issues() > 0 {
        (false, true)
    } else {
        (false, false)
    }
}

/// True when complexity findings were introduced since the base (always an error).
fn introduced_health_has_errors(result: &HealthResult, base: Option<&AuditKeySnapshot>) -> bool {
    let base_keys = base.map(|b| &b.health);
    result.report.findings.iter().any(|finding| {
        !base_keys
            .is_some_and(|keys| keys.contains(&health_finding_key(finding, &result.config.root)))
    })
}

fn introduced_styling_finding_gates(
    result: &HealthResult,
    base: Option<&AuditKeySnapshot>,
    finding: &fallow_output::StylingFinding,
) -> bool {
    styling_finding_gates(&result.config.rules, &finding.code)
        && !base.is_some_and(|snapshot| {
            snapshot
                .styling
                .contains(&styling_finding_key(finding, &result.config.root))
        })
}

/// Error/warning contribution from clone groups introduced since the base.
fn introduced_dupes_verdict(result: &DupesResult, base: Option<&AuditKeySnapshot>) -> (bool, bool) {
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
    if introduced == 0 {
        return (false, false);
    }
    if result.threshold > 0.0 && result.report.stats.duplication_percentage > result.threshold {
        (true, false)
    } else {
        (false, true)
    }
}

pub struct AuditKeySnapshot {
    dead_code: FxHashSet<String>,
    health: FxHashSet<String>,
    styling: FxHashSet<String>,
    dupes: FxHashSet<String>,
    /// Review-brief delta substrate (populated only on the brief path; empty
    /// otherwise). Cross-zone boundary EDGE keys (`<from_zone>->-<to_zone>`),
    /// one per distinct zone pair (R2 first-edge-only framing).
    boundary_edges: FxHashSet<String>,
    /// Canonical circular-dependency keys (rotation-independent file set).
    cycles: FxHashSet<String>,
    /// Exports-aware public-export keys (`<rel_path>::<name>`), the surface
    /// reachable through `package.json` `exports` + re-export reachability.
    public_api: FxHashSet<String>,
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

/// If fallow's process inherited any ambient git repo-state env vars (typical
/// when invoked from a `pre-commit` / `pre-push` hook or a tool wrapping git),
/// surface the most likely culprit so a user hitting an unexpected worktree
/// failure can short-circuit the diagnosis. Returns `None` otherwise.
fn ambient_git_env_hint() -> Option<String> {
    use fallow_engine::changed_files::AMBIENT_GIT_ENV_VARS;
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
    let base_opts =
        build_base_audit_options(opts, &base_root, &current_config_path, &base_cache_dir);

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
    // Compute the exports-aware public-export set against the BASE graph while it
    // is still retained on the check result, BEFORE health consumes it. The
    // public_api delta is brief-only, so this only runs on the brief path.
    let base_public_api = if opts.brief {
        public_api_keys_from_check(check.as_ref(), &base_root)
    } else {
        FxHashSet::default()
    };
    let shared_parse = if share_dead_code_parse_with_health {
        check.as_mut().and_then(|r| r.shared_parse.take())
    } else {
        None
    };
    let health = run_audit_health(&base_opts, None, shared_parse)?;
    if let Some(ref mut check) = check {
        check.shared_parse = None;
    }

    Ok(snapshot_from_results(
        check.as_ref(),
        dupes.as_ref(),
        health.as_ref(),
        base_public_api,
    ))
}

/// Build an `AuditKeySnapshot` of dead-code/health/dupes keys from analysis
/// results. `public_api` is the exports-aware public-export key set, computed by
/// the caller from the retained graph BEFORE it is dropped (empty off the brief
/// path). Boundary-edge and cycle delta keys are derived directly from the
/// dead-code results, so they are always available.
fn snapshot_from_results(
    check: Option<&CheckResult>,
    dupes: Option<&DupesResult>,
    health: Option<&HealthResult>,
    public_api: FxHashSet<String>,
) -> AuditKeySnapshot {
    let (boundary_edges, cycles) = check.map_or_else(
        || (FxHashSet::default(), FxHashSet::default()),
        |r| {
            (
                review_deltas::boundary_edge_keys(&r.results.boundary_violations),
                review_deltas::cycle_keys(&r.results.circular_dependencies, &r.config.root),
            )
        },
    );
    AuditKeySnapshot {
        dead_code: check.map_or_else(FxHashSet::default, |r| {
            dead_code_keys(&r.results, &r.config.root)
        }),
        health: health.map_or_else(FxHashSet::default, |r| {
            health_keys(&r.report, &r.config.root)
        }),
        styling: health.map_or_else(FxHashSet::default, |r| {
            styling_keys(&r.report, &r.config.root)
        }),
        dupes: dupes.map_or_else(FxHashSet::default, |r| {
            dupes_keys(&r.report, &r.config.root)
        }),
        boundary_edges,
        cycles,
        public_api,
    }
}

/// Compute the exports-aware public-export key set from a check result's retained
/// graph. Returns an empty set when the graph was not retained (off the brief
/// path) so non-brief base snapshots stay cheap. Reuses the check session's
/// workspaces so the exports-aware entry resolution (R4) does not rescan.
fn public_api_keys_from_check(check: Option<&CheckResult>, root: &Path) -> FxHashSet<String> {
    let Some(check) = check else {
        return FxHashSet::default();
    };
    let Some(graph) = check
        .shared_parse
        .as_ref()
        .and_then(|sp| sp.analysis_output.as_ref())
        .and_then(|out| out.graph.as_ref())
    else {
        return FxHashSet::default();
    };
    let root_pkg = fallow_config::PackageJson::load(&check.config.root.join("package.json")).ok();
    review_deltas::public_export_keys_for(
        graph,
        &check.config,
        root_pkg.as_ref(),
        &check.workspaces,
        root,
    )
}

/// Build the `AuditOptions` for the isolated base-worktree analysis pass.
#[expect(
    clippy::ref_option,
    reason = "AuditOptions.config_path is &Option<PathBuf>; the borrow is stored into the returned struct"
)]
fn build_base_audit_options<'a>(
    opts: &AuditOptions<'a>,
    base_root: &'a Path,
    current_config_path: &'a Option<PathBuf>,
    base_cache_dir: &'a Path,
) -> AuditOptions<'a> {
    AuditOptions {
        root: base_root,
        config_path: current_config_path,
        cache_dir: base_cache_dir,
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
        // Base styling keys keep opt-in `rules.css-* = error` gated on
        // introduced findings only; the base snapshot is cached.
        css: opts.css,
        css_deep: opts.css_deep,
        runtime_coverage: None,
        min_invocations_hot: opts.min_invocations_hot,
        brief: false,
        max_decisions: 4,
        walkthrough_guide: false,
        walkthrough: false,
        mark_viewed: &[],
        show_cleared: false,
        walkthrough_file: None,
        show_deprioritized: false,
    }
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
    // Reuse path (no behavioral change vs base): head IS base, so the delta
    // sets are the head's own keys, which makes every head-minus-base delta
    // empty. `public_api_keys` is the head set already computed on the brief
    // path; the boundary/cycle keys come from the head results.
    let public_api = check
        .and_then(|r| r.public_api_keys.clone())
        .unwrap_or_default();
    snapshot_from_results(check, dupes, health, public_api)
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
    fallow_engine::duplicates::source_token_kinds_equivalent(path, current, base, false)
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

pub use fallow_api::audit_keys as keys;

#[path = "audit_review_deltas.rs"]
pub mod review_deltas;

#[path = "audit_weakening.rs"]
pub mod weakening;

#[path = "audit_routing.rs"]
pub mod routing;

use keys::{
    dead_code_keys, dupe_group_key, dupes_keys, health_finding_key, health_keys,
    retain_introduced_dead_code, styling_finding_key, styling_keys,
};

struct HeadAnalyses {
    check: Option<CheckResult>,
    dupes: Option<DupesResult>,
    health: Option<HealthResult>,
}

/// HEAD analyses result paired with an optional freshly computed base snapshot
/// (present only when a real base worktree was run in parallel).
type HeadAndBaseResult = (
    Result<HeadAnalyses, ExitCode>,
    Option<Result<AuditKeySnapshot, ExitCode>>,
);

/// Run the HEAD analyses, optionally alongside a fresh base snapshot via
/// `rayon::join` when `run_base` is set. Mirrors the previous inline branch.
fn run_audit_head_and_base(
    opts: &AuditOptions<'_>,
    changed_since: Option<&str>,
    changed_files: &FxHashSet<PathBuf>,
    base_ref: &str,
    base_cache_key: Option<&AuditBaseSnapshotCacheKey>,
    run_base: bool,
) -> HeadAndBaseResult {
    if run_base {
        let base_sha = base_cache_key.map(|key| key.base_sha.as_str());
        let (h, b) = rayon::join(
            || run_audit_head_analyses(opts, changed_since, changed_files),
            || compute_base_snapshot(opts, base_ref, changed_files, base_sha),
        );
        (h, Some(b))
    } else {
        (
            run_audit_head_analyses(opts, changed_since, changed_files),
            None,
        )
    }
}

struct AuditResultParts {
    verdict: AuditVerdict,
    summary: AuditSummary,
    attribution: AuditAttribution,
    base_snapshot: Option<AuditKeySnapshot>,
    base_snapshot_skipped: bool,
    changed_files_count: usize,
    changed_files: FxHashSet<PathBuf>,
    base_ref: String,
    base_description: Option<String>,
    head_sha: Option<String>,
    output: OutputFormat,
    performance: bool,
    check: Option<CheckResult>,
    dupes: Option<DupesResult>,
    health: Option<HealthResult>,
    elapsed: Duration,
    review_deltas: Option<crate::audit_brief::ReviewDeltas>,
    weakening_signals: Vec<weakening::WeakeningSignal>,
    routing: Option<routing::RoutingFacts>,
    decision_surface: Option<crate::audit_decision_surface::DecisionSurface>,
    graph_snapshot_hash: Option<String>,
    change_anchors: Vec<crate::audit_walkthrough::ChangeAnchor>,
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
    // Compute the impact closure AND the exports-aware public-export key
    // set for the review brief BEFORE health consumes the shared parse (which
    // owns the retained graph). Both are stored on the check result so they
    // survive the graph drop.
    if opts.brief
        && let Some(ref mut check) = check
    {
        check.impact_closure = compute_brief_impact_closure(opts.root, check, changed_files);
        check.public_api_keys = Some(public_api_keys_from_check(Some(check), opts.root));
        check.partition_order = compute_brief_partition_order(opts.root, check, changed_files);
        check.focus_facts = compute_brief_focus_facts(opts.root, check, changed_files);
        check.export_lines = compute_brief_export_lines(opts.root, check, changed_files);
        check.internal_consumers =
            compute_brief_internal_consumers(opts.root, check, changed_files);
    }
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

/// Compute the impact closure for the review brief from the check result's
/// retained graph against the changed-file set.
///
/// Delegates changed-path resolution and graph traversal to the engine, then
/// returns `{ in_diff, affected_not_shown, coordination_gap }`. Returns `None`
/// when the graph was not retained (off the brief path) or no changed file maps
/// to a known module.
fn compute_brief_impact_closure(
    root: &std::path::Path,
    check: &CheckResult,
    changed_files: &FxHashSet<PathBuf>,
) -> Option<fallow_engine::module_graph::ImpactClosurePaths> {
    let graph = check
        .shared_parse
        .as_ref()
        .and_then(|sp| sp.analysis_output.as_ref())
        .and_then(|out| out.graph.as_ref())?;

    fallow_engine::module_graph::impact_closure_for_changed_paths(graph, root, changed_files)
}

/// Compute the partition + order for the review brief's stage 2 from the
/// check result's retained graph against the changed-file set.
///
/// Maps each changed absolute path to its graph `FileId`, groups the changed
/// files into by-module units, and computes a dependency-sensible review order
/// over those units. Returns `None` when the graph was not retained (off the
/// brief path) or no changed file maps to a known module.
fn compute_brief_partition_order(
    root: &std::path::Path,
    check: &CheckResult,
    changed_files: &FxHashSet<PathBuf>,
) -> Option<fallow_engine::module_graph::PartitionOrderPaths> {
    let graph = check
        .shared_parse
        .as_ref()
        .and_then(|sp| sp.analysis_output.as_ref())
        .and_then(|out| out.graph.as_ref())?;

    fallow_engine::module_graph::partition_order_for_changed_paths(graph, root, changed_files)
}

/// Precompute the per-changed-file `rel_path -> [(export-name, 1-based line)]` map
/// for the decision surface, from the retained graph's export spans + each file's
/// line offsets, BEFORE health drops the graph. Lets a coordination / public-API
/// decision anchor to the exact export line. `None` when the graph is not retained.
fn compute_brief_export_lines(
    root: &std::path::Path,
    check: &CheckResult,
    changed_files: &FxHashSet<PathBuf>,
) -> Option<FxHashMap<String, Vec<(String, u32)>>> {
    let graph = check
        .shared_parse
        .as_ref()
        .and_then(|sp| sp.analysis_output.as_ref())
        .and_then(|out| out.graph.as_ref())?;

    fallow_engine::module_graph::export_lines_for_changed_paths(graph, root, changed_files)
}

/// Precompute the per-anchor honest consumer count for the decision surface:
/// `rel_path -> count of distinct in-repo modules OUTSIDE the diff that directly
/// import the anchor file`, from the retained graph's reverse-deps BEFORE health
/// drops the graph (mirroring [`compute_brief_export_lines`]). This is the honest
/// per-decision DISPLAY number ("N in-repo modules already depend on this"),
/// distinct from the project-wide `affected_not_shown` ranking proxy. Importers
/// that are themselves part of the diff are excluded (they are the change, not a
/// pre-existing dependent). `None` when the graph is not retained.
fn compute_brief_internal_consumers(
    root: &std::path::Path,
    check: &CheckResult,
    changed_files: &FxHashSet<PathBuf>,
) -> Option<FxHashMap<String, u64>> {
    let graph = check
        .shared_parse
        .as_ref()
        .and_then(|sp| sp.analysis_output.as_ref())
        .and_then(|out| out.graph.as_ref())?;

    fallow_engine::module_graph::internal_consumers_for_changed_paths(graph, root, changed_files)
}

/// Compute the per-file focus graph facts (fan-in/out + the dynamic-dispatch /
/// re-export-indirection confidence-flag signals) for the review brief's stage 4
/// weighted focus map, from the check result's retained graph against the
/// changed-file set.
///
/// Maps each changed absolute path to its graph `FileId`, computes the per-file
/// blast + confidence signals, and path-resolves them. Returns `None` when the
/// graph was not retained (off the brief path) or no changed file maps to a known
/// module.
fn compute_brief_focus_facts(
    root: &std::path::Path,
    check: &CheckResult,
    changed_files: &FxHashSet<PathBuf>,
) -> Option<Vec<fallow_engine::module_graph::FocusFileFactsPaths>> {
    let graph = check
        .shared_parse
        .as_ref()
        .and_then(|sp| sp.analysis_output.as_ref())
        .and_then(|out| out.graph.as_ref())?;

    fallow_engine::module_graph::focus_facts_for_changed_paths(graph, root, changed_files)
}

/// Run the audit pipeline: resolve base ref, run analyses, compute verdict.
pub fn execute_audit(opts: &AuditOptions<'_>) -> Result<AuditResult, ExitCode> {
    let start = Instant::now();

    let (base_ref, base_description) = resolve_base_ref(opts)?;

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

    // Sweep only once audit will do real changed-code work. A clean tree never
    // creates or reuses a base worktree, so keeping the no-change fast path
    // free of worktree-listing IO is both safe and visibly cheaper.
    sweep_old_reusable_caches(
        opts.root,
        resolve_cache_max_age(opts.root, opts.config_path.as_ref()),
        opts.quiet,
    );

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

    let (head_res, base_res) = run_audit_head_and_base(
        opts,
        changed_since,
        &changed_files,
        &base_ref,
        base_cache_key.as_ref(),
        needs_real_base_snapshot && cached_base_snapshot.is_none(),
    );

    assemble_audit_result(AuditAssemblyInput {
        opts,
        head_res,
        base_res,
        cached_base_snapshot,
        base_cache_key,
        changed_files,
        changed_files_count,
        base_ref,
        base_description,
        start,
    })
}

/// Inputs threaded from the audit prelude into [`assemble_audit_result`].
struct AuditAssemblyInput<'a> {
    opts: &'a AuditOptions<'a>,
    head_res: Result<HeadAnalyses, ExitCode>,
    base_res: Option<Result<AuditKeySnapshot, ExitCode>>,
    cached_base_snapshot: Option<AuditKeySnapshot>,
    base_cache_key: Option<AuditBaseSnapshotCacheKey>,
    changed_files: FxHashSet<PathBuf>,
    changed_files_count: usize,
    base_ref: String,
    base_description: Option<String>,
    start: Instant,
}

/// Resolve the base snapshot, compute attribution/verdict/summary, and build the
/// final `AuditResult` from the HEAD-side analyses.
fn assemble_audit_result(input: AuditAssemblyInput<'_>) -> Result<AuditResult, ExitCode> {
    let opts = input.opts;
    let head = input.head_res?;
    let mut check_result = head.check;
    let dupes_result = head.dupes;
    let health_result = head.health;

    let (base_snapshot, base_snapshot_skipped) = resolve_base_snapshot(
        opts,
        input.cached_base_snapshot,
        input.base_res,
        input.base_cache_key.as_ref(),
        CurrentAnalysisRefs {
            check: check_result.as_ref(),
            dupes: dupes_result.as_ref(),
            health: health_result.as_ref(),
        },
    )?;
    if let Some(ref mut check) = check_result {
        check.shared_parse = None;
    }
    let (attribution, verdict, summary) = compute_audit_outcome(
        opts.gate,
        check_result.as_ref(),
        dupes_result.as_ref(),
        health_result.as_ref(),
        base_snapshot.as_ref(),
    );

    // Review-brief data: deltas (head-vs-base sets), weakening (base-vs-head
    // diff scan over the changed files), and ownership routing. Brief-path only.
    let (review_deltas, weakening_signals, routing) = if opts.brief {
        compute_brief_e3_data(
            opts,
            check_result.as_ref(),
            base_snapshot.as_ref(),
            &input.changed_files,
            &input.base_ref,
        )
    } else {
        (None, Vec::new(), None)
    };

    // Decision surface (the apex): classify the SOLID-3 candidates from the
    // deltas + coordination gaps, anchor each `signal_id`, rank, cap, and pair
    // the routed expert. Brief-path only.
    let decision_surface = if opts.brief {
        Some(compute_decision_surface(
            opts,
            check_result.as_ref(),
            review_deltas.as_ref(),
            routing.as_ref(),
        ))
    } else {
        None
    };

    // Per-hunk change anchors (the region-level anchor set): derived from the
    // SAME diff source the run used (an opt-in `--diff-stdin`/`--diff-file` diff,
    // else the committed `base...HEAD`), so emission and validation anchor to one
    // changed set. Brief-path only.
    let change_anchors = if opts.brief {
        compute_change_anchors(opts.root, &input.base_ref)
    } else {
        Vec::new()
    };

    // Graph-snapshot hash (the staleness pin): a deterministic hash of the
    // HEAD-side key sets plus the resolved base ref + head sha + the change-anchor
    // id set. Brief-path only. The verifier is the graph: a mutated tree changes a
    // key set OR a changed region, changes this hash, and refuses a stale agent
    // walkthrough on reentry (so a cited change_anchor that moved is refused too).
    let head_sha = get_head_sha(opts.root);
    let graph_snapshot_hash = if opts.brief {
        Some(compute_graph_snapshot_hash(
            check_result.as_ref(),
            dupes_result.as_ref(),
            health_result.as_ref(),
            &input.base_ref,
            head_sha.as_deref(),
            &change_anchors,
        ))
    } else {
        None
    };

    Ok(build_audit_result(AuditResultParts {
        verdict,
        summary,
        attribution,
        base_snapshot,
        base_snapshot_skipped,
        changed_files_count: input.changed_files_count,
        changed_files: input.changed_files,
        base_ref: input.base_ref,
        base_description: input.base_description,
        head_sha,
        output: opts.output,
        performance: opts.performance,
        check: check_result,
        dupes: dupes_result,
        health: health_result,
        elapsed: input.start.elapsed(),
        review_deltas,
        weakening_signals,
        routing,
        decision_surface,
        graph_snapshot_hash,
        change_anchors,
    }))
}

/// Compute the deterministic graph-snapshot hash from the HEAD-side analysis
/// results plus the resolved base ref + head sha. Reuses [`snapshot_from_results`]
/// for the six key sets (dead_code / health / dupes / boundary_edges / cycles /
/// public_api), each sorted, then folds in the base ref and head sha so the same
/// tree compared against the same base always yields the same hash.
///
/// The verifier is the graph: any structural change (a new finding, a new edge,
/// a new export) shifts a key set and changes this hash, so a stale agent
/// walkthrough whose echoed hash no longer matches is REFUSED on reentry.
fn compute_graph_snapshot_hash(
    check: Option<&CheckResult>,
    dupes: Option<&DupesResult>,
    health: Option<&HealthResult>,
    base_ref: &str,
    head_sha: Option<&str>,
    change_anchors: &[crate::audit_walkthrough::ChangeAnchor],
) -> String {
    // The HEAD public-export set was computed on the brief path and retained on
    // the check result (`public_api_keys`); reuse it so the hash is exports-aware
    // without re-walking the graph.
    let public_api = check
        .and_then(|c| c.public_api_keys.clone())
        .unwrap_or_default();
    let snapshot = snapshot_from_results(check, dupes, health, public_api);
    let mut bytes: Vec<u8> = Vec::new();
    // Sorted key sets, each length-prefixed, so the byte stream is unambiguous.
    for set in [
        &snapshot.dead_code,
        &snapshot.health,
        &snapshot.dupes,
        &snapshot.boundary_edges,
        &snapshot.cycles,
        &snapshot.public_api,
    ] {
        for key in sorted_keys(set) {
            bytes.extend_from_slice(key.as_bytes());
            bytes.push(0);
        }
        bytes.push(1);
    }
    // Seventh key set: the SORTED change-anchor id set, so a moved/added/removed
    // changed region shifts this hash and a cited change_anchor that moved is
    // refused as stale (the finding key sets are line-independent and would not
    // otherwise cover the region-level anchors).
    let mut anchor_ids: Vec<&str> = change_anchors
        .iter()
        .map(|a| a.change_anchor.as_str())
        .collect();
    anchor_ids.sort_unstable();
    for id in anchor_ids {
        bytes.extend_from_slice(id.as_bytes());
        bytes.push(0);
    }
    bytes.push(1);
    bytes.extend_from_slice(base_ref.as_bytes());
    bytes.push(0);
    bytes.extend_from_slice(head_sha.unwrap_or("").as_bytes());
    format!("graph:{:016x}", xxh3_64(&bytes))
}

/// Derive the per-hunk change anchors for this run from the SAME diff source the
/// run used: the opt-in shared diff (`--diff-stdin` / `--diff-file`) when present,
/// else the committed merge-base diff `base_ref...HEAD`. Using one source for both
/// the guide emission and the `--walkthrough-file` validation keeps the emitted
/// anchor set equal to the set validated on reentry (a committed-vs-staged
/// mismatch would otherwise reject every staged region's anchor). A diff that
/// cannot be computed yields an empty set (no anchors, never a panic).
fn compute_change_anchors(
    root: &std::path::Path,
    base_ref: &str,
) -> Vec<crate::audit_walkthrough::ChangeAnchor> {
    if let Some(raw) = crate::report::ci::diff_filter::shared_diff_raw() {
        return crate::audit_walkthrough::parse_change_anchors(raw);
    }
    fallow_engine::changed_files::try_get_changed_diff(root, base_ref)
        .map(|diff| crate::audit_walkthrough::parse_change_anchors(&diff))
        .unwrap_or_default()
}

/// Compute the decision surface from the assembled brief inputs: gather the
/// boundary anchors (one representative per introduced zone-pair), the
/// coordination gaps, and the impact-closure blast magnitude, then run the
/// extractor. The cap is taken from the audit options (clamped to [3, 5] by the
/// extractor). Returns an empty surface when no check result is available.
fn compute_decision_surface(
    opts: &AuditOptions<'_>,
    check: Option<&CheckResult>,
    review_deltas: Option<&crate::audit_brief::ReviewDeltas>,
    routing: Option<&routing::RoutingFacts>,
) -> crate::audit_decision_surface::DecisionSurface {
    use crate::audit_decision_surface::{
        BoundaryAnchor, CoordinationAnchor, DecisionInputs, extract_decision_surface,
    };

    let (Some(check), Some(deltas)) = (check, review_deltas) else {
        return crate::audit_decision_surface::DecisionSurface::default();
    };
    let root = &check.config.root;

    // One representative boundary anchor per introduced zone-pair: pick the first
    // violation whose zone-pair key matches an introduced edge, for the file +
    // line suppression/routing anchor.
    let mut boundary_anchors: Vec<BoundaryAnchor> = Vec::new();
    let mut seen_pairs: FxHashSet<String> = FxHashSet::default();
    for finding in &check.results.boundary_violations {
        let key = review_deltas::boundary_edge_key(finding);
        if !deltas.boundary_introduced.contains(&key) || !seen_pairs.insert(key.clone()) {
            continue;
        }
        boundary_anchors.push(BoundaryAnchor {
            zone_pair_key: key,
            from_file: keys::relative_key_path(&finding.violation.from_path, root),
            from_zone: finding.violation.from_zone.clone(),
            to_zone: finding.violation.to_zone.clone(),
            line: finding.violation.line,
        });
    }

    // Coordination gaps projected to the public-API/contract decision shape.
    // Aggregate per changed file: ONE contract decision per changed file (R1
    // batch-consolidate), counting its distinct non-diff consumers as the blast.
    let closure = check.impact_closure.as_ref();
    let mut coordination: Vec<CoordinationAnchor> = closure
        .map(|c| aggregate_coordination_gaps(&c.coordination_gap))
        .unwrap_or_default();
    let affected_not_shown = closure.map_or(0, |c| c.affected_not_shown.len() as u64);

    let empty_routing = routing::RoutingFacts::default();
    let routing = routing.unwrap_or(&empty_routing);

    // Head-source reader for suppression checks AND for resolving a contract
    // symbol's declaration line: read the on-disk (head) content of an anchor file
    // by its root-relative path. Best-effort; an unreadable file is not suppressed.
    let root_owned = root.clone();
    let head_source = move |rel: &str| std::fs::read_to_string(root_owned.join(rel)).ok();

    // Resolve a contract symbol's 1-based declaration line from the per-file
    // export-line map precomputed on the brief path (the graph is already dropped
    // by health here, so we cannot re-derive it now). Lets coordination /
    // public-API decisions deep-link to the exact export instead of the file head.
    let export_lines = check.export_lines.as_ref();
    let resolve_line = |rel: &str, symbols: &[String]| -> u32 {
        let Some(exports) = export_lines.and_then(|map| map.get(rel)) else {
            return 0;
        };
        exports
            .iter()
            .find(|(name, _)| symbols.iter().any(|s| name == s))
            .or_else(|| exports.first())
            .map_or(0, |(_, line)| *line)
    };
    for anchor in &mut coordination {
        anchor.line = resolve_line(&anchor.changed_file, &anchor.consumed_symbols);
    }
    let public_api_anchor_line = deltas.public_api_added.first().map_or(0, |key| {
        let mut parts = key.splitn(2, "::");
        let path = parts.next().unwrap_or_default();
        let name = parts.next().unwrap_or_default();
        resolve_line(path, &[name.to_string()])
    });

    // Rename resolver: a head (post-rename) root-relative path -> its pre-rename
    // path, from the diff's rename pairs. Best-effort (empty without a shared diff
    // or renames); lets each decision carry a rename-durable `previous_signal_id`.
    let rename_old_path = |rel: &str| -> Option<String> {
        crate::report::ci::diff_filter::shared_diff_index()
            .and_then(|idx| idx.old_path_for(rel))
            .map(str::to_string)
    };

    // Honest per-anchor consumer count, looked up from the map precomputed before
    // the graph drop. `0` for an anchor with no recorded importers (a new file).
    let internal_consumers_map = check.internal_consumers.as_ref();
    let internal_consumers = |rel: &str| -> u64 {
        internal_consumers_map
            .and_then(|map| map.get(rel))
            .copied()
            .unwrap_or(0)
    };

    extract_decision_surface(&DecisionInputs {
        deltas,
        boundary_anchors: &boundary_anchors,
        coordination: &coordination,
        public_api_anchor_line,
        affected_not_shown,
        routing,
        head_source: &head_source,
        rename_old_path: &rename_old_path,
        internal_consumers: &internal_consumers,
        cap: opts.max_decisions,
    })
}

/// Aggregate per-(changed, consumer) coordination gaps into ONE contract anchor
/// per changed file (R1 batch-consolidate), with the distinct-consumer count as
/// the blast and the union of consumed symbols as the contract. Sorted by changed
/// file for deterministic output.
fn aggregate_coordination_gaps(
    gaps: &[fallow_engine::module_graph::CoordinationGapPaths],
) -> Vec<crate::audit_decision_surface::CoordinationAnchor> {
    use crate::audit_decision_surface::CoordinationAnchor;
    let mut by_file: FxHashMap<String, (u64, FxHashSet<String>)> = FxHashMap::default();
    for gap in gaps {
        let entry = by_file
            .entry(gap.changed_file.clone())
            .or_insert_with(|| (0, FxHashSet::default()));
        entry.0 += 1;
        for symbol in &gap.consumed_symbols {
            entry.1.insert(symbol.clone());
        }
    }
    let mut anchors: Vec<CoordinationAnchor> = by_file
        .into_iter()
        .map(|(changed_file, (consumer_count, symbols))| {
            let mut consumed_symbols: Vec<String> = symbols.into_iter().collect();
            consumed_symbols.sort_unstable();
            CoordinationAnchor {
                changed_file,
                consumed_symbols,
                consumer_count,
                line: 0,
            }
        })
        .collect();
    anchors.sort_by(|a, b| a.changed_file.cmp(&b.changed_file));
    anchors
}

/// Compute the review-brief data: the diff-aware deltas (head sets vs base
/// snapshot), the weakening-signal pass (base-vs-head diff over the changed
/// files), and ownership routing. Pure-ish: weakening + routing shell out to git
/// (via [`BaseFileReader`] / churn), so this runs only on the brief path.
fn compute_brief_e3_data(
    opts: &AuditOptions<'_>,
    check: Option<&CheckResult>,
    base_snapshot: Option<&AuditKeySnapshot>,
    changed_files: &FxHashSet<PathBuf>,
    base_ref: &str,
) -> (
    Option<crate::audit_brief::ReviewDeltas>,
    Vec<weakening::WeakeningSignal>,
    Option<routing::RoutingFacts>,
) {
    let deltas = check.map(|check| {
        let head_boundary = review_deltas::boundary_edge_keys(&check.results.boundary_violations);
        let head_cycles =
            review_deltas::cycle_keys(&check.results.circular_dependencies, &check.config.root);
        let head_public_api = check.public_api_keys.clone().unwrap_or_default();
        let empty = FxHashSet::default();
        let (base_boundary, base_cycles, base_public_api) = base_snapshot
            .map_or((&empty, &empty, &empty), |b| {
                (&b.boundary_edges, &b.cycles, &b.public_api)
            });
        crate::audit_brief::build_review_deltas(
            &head_boundary,
            base_boundary,
            &head_cycles,
            base_cycles,
            &head_public_api,
            base_public_api,
        )
    });

    let weakening_signals = compute_weakening_signals(opts.root, base_ref, changed_files);

    let routing =
        check.map(|check| routing::compute_routing(opts.root, &check.config, changed_files));

    (deltas, weakening_signals, routing)
}

/// Run the weakening-signal pass over the changed files: read each file's base
/// content via [`BaseFileReader`], diff it against the on-disk head content, and
/// emit a [`weakening::WeakeningSignal`] per detected weakening. Best-effort: a
/// file whose base or head cannot be read is skipped silently.
fn compute_weakening_signals(
    root: &Path,
    base_ref: &str,
    changed_files: &FxHashSet<PathBuf>,
) -> Vec<weakening::WeakeningSignal> {
    use weakening::{WeakeningKind, WeakeningSignal};

    let Some(git_root) = git_toplevel(root) else {
        return Vec::new();
    };
    let Some(mut reader) = BaseFileReader::spawn(root) else {
        return Vec::new();
    };

    let mut signals: Vec<WeakeningSignal> = Vec::new();
    // Sort the changed files for deterministic signal ordering.
    let mut files: Vec<&PathBuf> = changed_files.iter().collect();
    files.sort();

    for abs in files {
        let Ok(relative) = abs.strip_prefix(&git_root) else {
            continue;
        };
        let rel_str = relative.to_string_lossy().replace('\\', "/");
        let head = std::fs::read_to_string(abs).unwrap_or_default();
        let base = reader.read(base_ref, relative).unwrap_or_default();
        // A net-new file (no base) or a non-source file still gets the scan; the
        // detectors are no-ops on irrelevant content.

        if weakening::is_test_file(&rel_str) {
            for token in weakening::detect_test_weakening(&base, &head) {
                signals.push(WeakeningSignal {
                    kind: WeakeningKind::TestWeakened,
                    file: rel_str.clone(),
                    evidence: format!("{token} added"),
                });
            }
            for ev in weakening::detect_removed_tests(&base, &head) {
                signals.push(WeakeningSignal {
                    kind: WeakeningKind::TestWeakened,
                    file: rel_str.clone(),
                    evidence: ev,
                });
            }
        }
        for ev in weakening::detect_added_suppressions(&base, &head) {
            signals.push(WeakeningSignal {
                kind: WeakeningKind::SuppressionAdded,
                file: rel_str.clone(),
                evidence: ev,
            });
        }
        for ev in weakening::detect_lowered_thresholds(&base, &head) {
            signals.push(WeakeningSignal {
                kind: WeakeningKind::ThresholdLowered,
                file: rel_str.clone(),
                evidence: ev,
            });
        }
        if weakening::is_ci_file(&rel_str) {
            for ev in weakening::detect_removed_security_steps(&base, &head) {
                signals.push(WeakeningSignal {
                    kind: WeakeningKind::SecurityCheckRemoved,
                    file: rel_str.clone(),
                    evidence: ev,
                });
            }
        }
    }
    signals
}

/// Attribution, verdict, and summary computed together from the HEAD analyses and
/// base snapshot. Also records the final result count for telemetry.
fn compute_audit_outcome(
    gate: AuditGate,
    check: Option<&CheckResult>,
    dupes: Option<&DupesResult>,
    health: Option<&HealthResult>,
    base: Option<&AuditKeySnapshot>,
) -> (AuditAttribution, AuditVerdict, AuditSummary) {
    let attribution = compute_audit_attribution(check, dupes, health, base, gate);
    let verdict = compute_audit_verdict(gate, check, dupes, health, base);
    let summary = build_summary(check, dupes, health);
    crate::telemetry::note_final_result_count(
        summary.dead_code_issues + summary.complexity_findings + summary.duplication_clone_groups,
    );
    (attribution, verdict, summary)
}

/// Resolve the base key snapshot for the `new`-only gate: prefer the cache, then a
/// freshly computed base worktree (persisting it), else fall back to current keys
/// (marking the snapshot skipped). Returns `(None, false)` outside `new`-only mode.
/// The current-run analysis result references threaded together so the base
/// snapshot resolver can fall back to the current keys without a six-deep
/// argument list. Bundled refs of the optional check / dupes / health results.
#[derive(Clone, Copy)]
struct CurrentAnalysisRefs<'a> {
    check: Option<&'a CheckResult>,
    dupes: Option<&'a DupesResult>,
    health: Option<&'a HealthResult>,
}

fn resolve_base_snapshot(
    opts: &AuditOptions<'_>,
    cached_base_snapshot: Option<AuditKeySnapshot>,
    base_res: Option<Result<AuditKeySnapshot, ExitCode>>,
    base_cache_key: Option<&AuditBaseSnapshotCacheKey>,
    current: CurrentAnalysisRefs<'_>,
) -> Result<(Option<AuditKeySnapshot>, bool), ExitCode> {
    if !matches!(opts.gate, AuditGate::NewOnly) {
        return Ok((None, false));
    }
    if let Some(snapshot) = cached_base_snapshot {
        return Ok((Some(snapshot), false));
    }
    if let Some(base_res) = base_res {
        let snapshot = base_res?;
        if let Some(key) = base_cache_key {
            save_cached_base_snapshot(opts, key, &snapshot);
        }
        return Ok((Some(snapshot), false));
    }
    let CurrentAnalysisRefs {
        check,
        dupes,
        health,
    } = current;
    Ok((Some(current_keys_as_base_keys(check, dupes, health)), true))
}

/// Pick the audit verdict: the introduced-only gate in `new`-only mode, otherwise
/// the full backlog verdict.
fn compute_audit_verdict(
    gate: AuditGate,
    check: Option<&CheckResult>,
    dupes: Option<&DupesResult>,
    health: Option<&HealthResult>,
    base: Option<&AuditKeySnapshot>,
) -> AuditVerdict {
    if matches!(gate, AuditGate::NewOnly) {
        compute_introduced_verdict(check, dupes, health, base)
    } else {
        compute_verdict(check, dupes, health)
    }
}

fn build_audit_result(parts: AuditResultParts) -> AuditResult {
    AuditResult {
        verdict: parts.verdict,
        summary: parts.summary,
        attribution: parts.attribution,
        base_snapshot: parts.base_snapshot,
        base_snapshot_skipped: parts.base_snapshot_skipped,
        changed_files_count: parts.changed_files_count,
        changed_files: parts.changed_files.into_iter().collect(),
        base_ref: parts.base_ref,
        base_description: parts.base_description,
        head_sha: parts.head_sha,
        output: parts.output,
        performance: parts.performance,
        check: parts.check,
        dupes: parts.dupes,
        health: parts.health,
        elapsed: parts.elapsed,
        review_deltas: parts.review_deltas,
        weakening_signals: parts.weakening_signals,
        routing: parts.routing,
        decision_surface: parts.decision_surface,
        graph_snapshot_hash: parts.graph_snapshot_hash,
        change_anchors: parts.change_anchors,
    }
}

/// Build an empty pass result when no files have changed.
fn empty_audit_result(
    base_ref: String,
    base_description: Option<String>,
    opts: &AuditOptions<'_>,
    elapsed: Duration,
) -> AuditResult {
    crate::telemetry::note_final_result_count(0);

    let head_sha = get_head_sha(opts.root);
    // An empty changeset is a valid graph state: pin a hash on the brief path so
    // the walkthrough guide still carries a stable snapshot pin (no findings, so
    // the hash folds only the base ref + head sha).
    let graph_snapshot_hash = if opts.brief {
        // An empty changeset has no changed regions, so no change anchors.
        Some(compute_graph_snapshot_hash(
            None,
            None,
            None,
            &base_ref,
            head_sha.as_deref(),
            &[],
        ))
    } else {
        None
    };

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
        head_sha,
        output: opts.output,
        performance: opts.performance,
        check: None,
        dupes: None,
        health: None,
        elapsed,
        review_deltas: None,
        weakening_signals: Vec::new(),
        routing: None,
        decision_surface: None,
        graph_snapshot_hash,
        change_anchors: Vec::new(),
    }
}

/// Run dead code analysis for the audit pipeline.
fn run_audit_check<'a>(
    opts: &'a AuditOptions<'a>,
    changed_since: Option<&'a str>,
    retain_modules_for_health: bool,
) -> Result<Option<CheckResult>, ExitCode> {
    let filters = IssueFilters::default();
    // The review brief needs the module graph for the impact closure, which
    // rides the retained-modules path. Force retention on the brief path even
    // when health does not share the dead-code parse (mismatched production
    // modes), so the graph is available before health consumes the shared parse.
    let retain_modules_for_health = retain_modules_for_health || opts.brief;
    let trace_opts = TraceOptions {
        trace_export: None,
        trace_file: None,
        trace_dependency: None,
        impact_closure: None,
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
        crate::ConfigLoadOptions {
            output: opts.output,
            no_cache: opts.no_cache,
            threads: opts.threads,
            production_override: opts
                .production_dupes
                .or_else(|| opts.production.then_some(true)),
            quiet: opts.quiet,
        },
        fallow_config::ProductionAnalysis::Dupes,
    ) {
        Ok(c) => c.duplicates,
        Err(code) => return Err(code),
    };
    let dupes_opts = build_audit_dupes_options(opts, changed_since, changed_files, &dupes_cfg);
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

/// Build the `DupesOptions` for an audit run from project config + audit options.
fn build_audit_dupes_options<'a>(
    opts: &'a AuditOptions<'a>,
    changed_since: Option<&'a str>,
    changed_files: Option<&'a FxHashSet<PathBuf>>,
    dupes_cfg: &fallow_config::DuplicatesConfig,
) -> DupesOptions<'a> {
    DupesOptions {
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
        ignore_imports: Some(dupes_cfg.ignore_imports),
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
    }
}

/// Run complexity analysis for the audit pipeline (findings only, no scores/hotspots/targets).
fn run_audit_health<'a>(
    opts: &'a AuditOptions<'a>,
    changed_since: Option<&'a str>,
    shared_parse: Option<fallow_engine::health::HealthSharedParseData>,
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

    let health_opts = build_audit_health_options(opts, changed_since, runtime_coverage);
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

/// Build the findings-only `HealthOptions` for an audit run (no scores, hotspots,
/// ownership, or targets; `--churn-file` is health-only).
fn build_audit_health_options<'a>(
    opts: &'a AuditOptions<'a>,
    changed_since: Option<&'a str>,
    runtime_coverage: Option<fallow_engine::health::RuntimeCoverageOptions>,
) -> HealthOptions<'a> {
    HealthOptions {
        root: opts.root,
        config_path: opts.config_path,
        output: opts.output,
        no_cache: opts.no_cache,
        threads: opts.threads,
        quiet: opts.quiet,
        thresholds: fallow_engine::health::HealthThresholdOverrides {
            max_cyclomatic: None,
            max_cognitive: None,
            max_crap: opts.max_crap,
        },
        top: None,
        sort: fallow_engine::health::HealthSort::Cyclomatic,
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
        // Styling analytics surface in `fallow audit` so a coding agent gets
        // styling feedback in the same stream it already reads for dead-code +
        // complexity. Changed-file-scoped (cheap) + dep-gated; descriptive only
        // (verdict-neutral). See .plans/styling-findings-in-audit.md (Slice 1).
        css: opts.css,
        css_deep: opts.css_deep,
        force_full: false,
        score_only_output: false,
        enforce_coverage_gap_gate: false,
        effort: None,
        score: false,
        gates: fallow_engine::health::HealthGateOptions::default(),
        since: None,
        min_commits: None,
        explain: opts.explain,
        summary: false,
        save_snapshot: None,
        trend: false,
        coverage_inputs: fallow_engine::health::HealthCoverageInputs {
            coverage: opts.coverage,
            coverage_root: opts.coverage_root,
        },
        performance: opts.performance,
        runtime_coverage,
        churn_file: None,
        complexity_breakdown: false,
        group_by: opts.group_by.map(Into::into),
    }
}

#[path = "audit_output.rs"]
mod output;

pub use output::audit_json_header_input;
pub use output::{
    insert_audit_dead_code_json, insert_audit_duplication_json, insert_audit_health_json,
    print_audit_findings, print_audit_result,
};

/// Run the full audit command: execute analyses, print results, return exit code.
/// Run audit, optionally tagged with a gate marker (e.g. `"pre-commit"`) so
/// Fallow Impact can record a containment event when the gate blocks then
/// clears. The marker only affects the local Impact store; it never changes
/// the verdict, exit code, or output.
pub fn run_audit(opts: &AuditOptions<'_>, gate_marker: Option<&str>) -> ExitCode {
    if let Err(e) = fallow_engine::health::validate_coverage_root_absolute(opts.coverage_root) {
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
            let empty_supps: Vec<fallow_types::results::ActiveSuppression> = Vec::new();
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
            // Walkthrough surfaces take priority over the brief body when
            // requested. Both always exit 0.
            if opts.walkthrough_guide {
                crate::audit_brief::print_walkthrough_guide_result(&result)
            } else if opts.walkthrough {
                crate::audit_brief::print_walkthrough_human_result(
                    &result,
                    opts.root,
                    opts.cache_dir,
                    opts.mark_viewed,
                    opts.show_cleared,
                    opts.quiet,
                )
            } else if let Some(path) = opts.walkthrough_file {
                crate::audit_brief::print_walkthrough_file_result(&result, path)
            } else if opts.brief {
                // Exit-0 seam: the brief renders the same analysis but never
                // gates on the verdict. `print_brief_result` always returns
                // SUCCESS.
                crate::audit_brief::print_brief_result(
                    &result,
                    opts.quiet,
                    opts.explain,
                    opts.show_deprioritized,
                )
            } else {
                print_audit_result(&result, opts.quiet, opts.explain)
            }
        }
        Err(code) => code,
    }
}

/// Run the standalone `fallow decision-surface` command: the separable, cheap
/// apex. Executes the SAME changed-code analysis the review brief runs (it is
/// the brief path, NOT the full project pipeline), then emits ONLY the decision
/// surface envelope. Always exit 0 (the surface is advisory, never a gate).
///
/// The MCP `decision_surface` tool wraps this command. It is callable without the
/// full pipeline because it reuses `execute_audit` in brief mode (changed-code
/// scope), not bare `fallow`.
#[must_use]
pub fn run_decision_surface(opts: &AuditOptions<'_>) -> ExitCode {
    // Force brief mode: the decision surface is only computed on the brief path.
    let brief_opts = AuditOptions {
        brief: true,
        ..*opts
    };
    match execute_audit(&brief_opts) {
        Ok(result) => crate::audit_brief::print_decision_surface_result(&result, opts.quiet),
        Err(code) => code,
    }
}

#[cfg(test)]
#[path = "audit_tests.rs"]
mod tests;
