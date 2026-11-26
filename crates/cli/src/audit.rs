use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use fallow_config::{AuditGate, AuditWorsened, OutputFormat, PlannedAnalysis};
use fallow_engine::resolved_run::ResolvedRun;
use rustc_hash::{FxHashMap, FxHashSet};

pub use fallow_api::{AuditAttribution, AuditSummary, AuditVerdict};

#[cfg(test)]
use crate::base_worktree::git_rev_parse;
use crate::base_worktree::{BaseWorktree, git_toplevel, sweep_old_reusable_caches};
use crate::check::CheckResult;
use crate::dupes::{DupesMode, DupesOptions, DupesOverrides, DupesResult};
use crate::error::emit_input_error;
use crate::exit_codes::exit_invalid_input;
use crate::health::{HealthOptions, HealthResult};

pub use fallow_api::audit_run::{AuditKeySnapshot, DupeDemotionDiffSource, branching_keys};

/// Full audit result containing verdict, summary, and sub-results.
pub struct AuditResult {
    pub verdict: AuditVerdict,
    pub summary: AuditSummary,
    pub attribution: AuditAttribution,
    /// Which diff decided the new-only duplication demotion check; `None`
    /// when the check never ran.
    pub dupe_demotion_diff_source: Option<DupeDemotionDiffSource>,
    /// Key snapshot of the base ref for new-vs-inherited attribution. `None`
    /// when the base pass was skipped (`--gate all`) or unavailable. Exposed at
    /// crate scope so test fixtures in sibling modules can construct an
    /// `AuditResult` with `base_snapshot: None`.
    pub base_snapshot: Option<AuditKeySnapshot>,
    /// One-pass introduced-finding classification used by verdict and JSON.
    pub comparison: Option<keys::AuditComparison>,
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
    /// Abbreviated merge base of `base_ref` and `HEAD`, where the compared range
    /// starts. Not serialized; only the scope line prints it.
    pub base_sha: Option<String>,
    /// See [`base_ref::shallow_clone_base_is_head`].
    pub shallow_clone_without_base: bool,
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
    /// Diff-risk findings of the audit rules (`weakened-tests`,
    /// `added-suppression`, `test-only-branch`, `diverged-clone-sibling`,
    /// `added-dependency`). Computed on every audit, not only the brief path.
    pub diff_risk: fallow_output::DiffRiskReport,
    pub routing: Option<routing::RoutingFacts>,
    /// Owner-group reach of the change, computed from the CODEOWNERS file.
    /// Populated only on the brief path when a CODEOWNERS file exists.
    pub ownership: Option<fallow_output::OwnershipFacts>,
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
    /// Parsed metrics from the exact diff used by the brief path. Retained so
    /// rendering does not re-run git or consult process-global state.
    pub diff_index: Option<fallow_output::DiffIndex>,
}

pub struct AuditOptions<'a> {
    pub root: &'a std::path::Path,
    pub config_path: &'a Option<std::path::PathBuf>,
    pub cache_dir: &'a std::path::Path,
    pub output: OutputFormat,
    pub json_style: crate::json_style::JsonStyle,
    pub no_cache: bool,
    pub threads: usize,
    pub quiet: bool,
    pub allow_remote_extends: bool,
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
    /// How the health baseline is matched against current findings.
    pub health_baseline_mode: fallow_engine::baseline::HealthBaselineMode,
    /// Whether `--fail-on-stale-baseline` was passed.
    ///
    /// The gate itself cannot fire here: audit analyzes the files that changed
    /// against its base ref, so every sub-pass is change-scoped and a
    /// whole-project baseline would look stale for reasons that are not rot.
    /// Audit reads the flag only to say so once, in
    /// `notes::note_stale_baseline_gate_inert`, instead of accepting it silently.
    pub fail_on_stale_baseline: bool,
    /// Maximum CRAP score threshold (overrides `health.maxCrap` from config).
    /// Functions meeting or exceeding this score cause audit to fail.
    pub max_crap: Option<f64>,
    /// The file-length check (`health.maxFileLines`) is on in the config.
    pub file_lines_check: bool,
    /// Istanbul or raw V8 coverage input for accurate CRAP scoring in the health sub-pass.
    pub coverage: Option<&'a std::path::Path>,
    /// Prefix to strip from Istanbul source paths before rebasing to `root`.
    pub coverage_root: Option<&'a std::path::Path>,
    pub gate: AuditGate,
    /// How a worsened complexity finding affects the verdict
    /// (`--worsened` / `audit.worsened`).
    pub worsened: AuditWorsened,
    /// Report unused exports in entry files (forwarded to the dead-code sub-pass).
    pub include_entry_exports: bool,
    /// Keep the export and member findings of unused files (`--show-cascade`).
    pub show_cascade: bool,
    /// `--fail-on-parse-error`, forwarded to the dead-code and health
    /// sub-passes. The audit applies the `parse-error` gate once over both.
    pub fail_on_parse_error: bool,
    /// The sub-analyses to run on head and base, from `--only`, `--skip` and
    /// the `audit.*` config. The brief path always runs all of them.
    pub analyses: AuditAnalysisSelection,
    /// Run styling analytics (CSS + CSS-in-JS) in the health sub-pass so styling
    /// signals surface in the audit output. Default on; `--no-css` disables.
    /// Descriptive + verdict-neutral (never affects the audit verdict / exit code).
    pub css: bool,
    /// Run the project-wide CSS pass and narrow cross-file findings back to
    /// changed anchors. Default on for audit; `--no-css-deep` disables.
    pub css_deep: bool,
    /// Runtime coverage input (V8 directory, V8 JSON, or
    /// Istanbul coverage map). Forwarded into the embedded health pass so
    /// audit surfaces the `hot-path-touched` verdict alongside dead-code
    /// and complexity findings without requiring a second `fallow health`
    /// invocation in CI.
    pub runtime_coverage: Option<&'a std::path::Path>,
    /// Threshold for hot-path classification, forwarded to the sidecar.
    pub min_invocations_hot: u64,
    /// Parsed `--mutation-report` file for the brief. `None`: not measured.
    pub mutation_report: Option<&'a crate::audit_focus::mutation::AuditMutationReport>,
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
    /// Positional `[PATH]` scope: root-joined absolute file or directory inside
    /// the root. Narrows the changed-file universe before base focus, head
    /// analyses, attribution, and verdict, so the whole audit reads as the
    /// scoped slice. `None` means whole-project scope.
    pub scope: Option<std::path::PathBuf>,
}

#[path = "audit_selection.rs"]
mod selection;
pub use selection::AuditAnalysisSelection;

#[derive(Clone, Copy, Default)]
pub struct AuditTypeAwareOptions<'a> {
    /// CLI override: `Some(true)` for `--type-aware`, `Some(false)` for
    /// `--no-type-aware`, `None` when neither flag was passed.
    pub enabled: Option<bool>,
    /// `audit.typeAware` from config, applied below the CLI flags and the
    /// `FALLOW_TYPE_AWARE` environment variable but above `typeAware.enabled`.
    pub config_default: Option<bool>,
    pub projects: &'a [std::path::PathBuf],
    pub require: Option<fallow_config::TypeAwareRequire>,
}

impl AuditTypeAwareOptions<'_> {
    /// Whether `--type-aware` is on the command line. Such a run does not
    /// read or write the base-snapshot cache.
    const fn cli_enabled(&self) -> bool {
        matches!(self.enabled, Some(true))
    }
}

#[path = "audit_staged.rs"]
pub mod staged;
#[path = "audit_worktree_cache.rs"]
pub mod worktree_cache;

#[path = "audit_base_ref.rs"]
mod base_ref;
#[path = "audit_cache.rs"]
mod cache;
#[path = "audit_notes.rs"]
mod notes;
#[path = "audit_snapshot_hash.rs"]
mod snapshot_hash;
#[cfg(test)]
#[path = "audit_test_fixture.rs"]
pub mod test_fixture;

use base_ref::{audit_changed_files, resolve_base_ref};
pub use cache::sweep_base_snapshot_cache;
#[cfg(test)]
use cache::{
    AUDIT_BASE_SNAPSHOT_CACHE_VERSION, CachedAuditKeySnapshot, MAX_AUDIT_BASE_SNAPSHOT_FILES,
    audit_base_snapshot_cache_dir, audit_base_snapshot_cache_file, cached_from_snapshot,
    ensure_audit_base_snapshot_cache_dir, snapshot_from_cached,
};
use cache::{
    AuditBaseSnapshotCacheKey, audit_base_snapshot_cache_key, audit_workspace_patterns,
    load_cached_base_snapshot, save_cached_base_snapshot, sorted_keys,
};
use fallow_engine::repo_refs::short_head_sha;
use snapshot_hash::compute_graph_snapshot_hash;

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
    review_deltas::public_export_keys_for(graph, &check.config, &check.workspaces, root)
}

#[cfg(test)]
use std::time::SystemTime;

#[cfg(test)]
use crate::base_worktree::{
    ReusableWorktreeLock, WorktreeCleanupGuard, audit_worktree_pid, days_to_duration,
    is_fallow_audit_worktree_path, is_reusable_audit_worktree_path, list_audit_worktrees,
    materialize_base_dependency_context, parse_worktree_list, paths_equal, process_is_alive,
    record_last_used, remove_audit_worktree, reusable_audit_worktree_path,
    reusable_worktree_last_used_path, reusable_worktree_lock_path, reusable_worktree_sha_path,
    sweep_orphan_audit_worktrees_in, touch_last_used,
};

pub use fallow_api::audit_keys as keys;

#[path = "audit_review_deltas.rs"]
pub mod review_deltas;

#[path = "audit_weakening.rs"]
pub mod weakening;

#[path = "audit_security_scan.rs"]
mod security_scan;
use security_scan::compute_security_step_signals;

#[path = "audit_base_run.rs"]
mod base_run;
use base_run::BaseRunInputs;
#[path = "audit_dead_code.rs"]
mod dead_code;
use dead_code::{CheckPass, run_audit_check};
#[path = "audit_runs.rs"]
mod runs;

#[path = "audit_routing.rs"]
pub mod routing;

use fallow_api::audit_run::{
    AuditAnalyses, AuditAnalysesView, AuditBackend, AuditRun, AuditRunInput, BaseCheckout,
    BaseFileReader, BaseRead, DeadCodeView, HealthView, SharedDiff,
};

struct HeadAnalyses {
    check: Option<CheckResult>,
    dupes: Option<DupesResult>,
    health: Option<HealthResult>,
    /// The HEAD diff evidence of the brief pass, which the brief data reuses.
    brief_diff_evidence: Option<BriefDiffEvidence>,
    /// Unused-code keys of a base side under the config of the base commit.
    own_config_unused_code: Option<FxHashSet<String>>,
}

impl AuditAnalyses for HeadAnalyses {
    fn view(&self) -> AuditAnalysesView<'_> {
        let mut view = analyses_view(
            self.check.as_ref(),
            self.dupes.as_ref(),
            self.health.as_ref(),
        );
        if let Some(dead_code) = view.dead_code.as_mut() {
            dead_code.own_config_unused_code = self.own_config_unused_code.as_ref();
        }
        view
    }

    fn dead_code_results_mut(&mut self) -> Option<&mut fallow_types::results::AnalysisResults> {
        self.check.as_mut().map(|check| &mut check.results)
    }

    fn health_report_mut(&mut self) -> Option<&mut fallow_output::HealthReport> {
        self.health.as_mut().map(|health| &mut health.report)
    }

    fn record_type_aware_warning(&mut self, warning: &str) {
        if let Some(check) = self.check.as_mut() {
            check.type_aware_warnings.push(warning.to_owned());
            if let Some(meta) = check.type_aware_meta.as_mut() {
                meta.warnings.push(warning.to_owned());
                meta.warning_count = meta.warnings.len();
            }
        }
    }
}

/// The audit view of the CLI analysis results.
fn analyses_view<'a>(
    check: Option<&'a CheckResult>,
    dupes: Option<&'a DupesResult>,
    health: Option<&'a HealthResult>,
) -> AuditAnalysesView<'a> {
    AuditAnalysesView {
        dead_code: check.map(|check| DeadCodeView {
            results: &check.results,
            config: &check.config,
            root: &check.config.root,
            type_aware: check.type_aware_meta.as_ref(),
            syntactic_keys: check.syntactic_dead_code_keys.as_ref(),
            public_api: check.public_api_keys.as_ref(),
            own_config_unused_code: None,
        }),
        duplication: dupes.map(DupesResult::audit_view),
        health: health.map(|health| HealthView {
            report: &health.report,
            root: &health.config.root,
            rules: &health.config.rules,
            branching: Some(&health.branching_by_file),
        }),
    }
}

impl BaseCheckout for BaseWorktree {
    fn path(&self) -> &Path {
        Self::path(self)
    }
}

/// The CLI runners of `fallow audit`: `execute_check`, `execute_dupes` and
/// `execute_health_in`, with baselines, type-aware analysis, runtime coverage,
/// the reusable base worktree and the base-snapshot cache.
struct CliAuditBackend<'a> {
    opts: &'a AuditOptions<'a>,
    /// The resolved run of the head side.
    head: &'a ResolvedRun,
    /// The checkout that owns the base worktree and its reusable cache. It
    /// differs from `opts.root` for `--staged`, which analyzes a temporary
    /// worktree.
    host_root: &'a Path,
    type_aware: AuditTypeAwareOptions<'a>,
    base_ref: &'a str,
    /// Loaded on first use, so an audit without changes reads no config.
    workspace_patterns: OnceLock<Vec<String>>,
    /// Resolved on first use, so an audit without changes reads no config.
    base_run: OnceLock<BaseRunInputs>,
}

impl CliAuditBackend<'_> {
    /// The `workspaces.patterns` that the base checkout and its cache key use.
    fn workspace_patterns(&self) -> &[String] {
        self.workspace_patterns
            .get_or_init(|| audit_workspace_patterns(self.opts))
    }

    fn base_run(&self) -> &BaseRunInputs {
        self.base_run
            .get_or_init(|| BaseRunInputs::resolve(self.opts))
    }
}

impl AuditBackend for CliAuditBackend<'_> {
    type Analyses = HeadAnalyses;
    type Checkout = BaseWorktree;
    type CacheKey = AuditBaseSnapshotCacheKey;
    type Error = ExitCode;

    fn prepare(&self) {
        // Sweep only once audit does real changed-code work. A clean tree
        // never creates or reuses a base worktree, so the no-change path stays
        // free of worktree-listing IO.
        sweep_old_reusable_caches(
            self.host_root,
            crate::base_worktree::resolve_cache_max_age_with_options(
                self.host_root,
                self.opts.config_path.as_ref(),
                self.opts.allow_remote_extends,
            ),
            self.opts.quiet,
        );
    }

    fn run_head(&self, changed_files: &FxHashSet<PathBuf>) -> Result<HeadAnalyses, ExitCode> {
        run_audit_head_analyses(
            self.opts,
            self.head,
            self.type_aware,
            Some(self.base_ref),
            changed_files,
        )
    }

    fn create_base_checkout(
        &self,
        base_ref: &str,
        base_sha: Option<&str>,
    ) -> Result<BaseWorktree, ExitCode> {
        BaseWorktree::create(
            self.host_root,
            base_ref,
            base_sha,
            self.workspace_patterns(),
        )
        .ok_or_else(|| {
            use std::fmt::Write as _;
            let mut message =
                format!("could not create a temporary worktree for base ref '{base_ref}'");
            if let Some(hint) = ambient_git_env_hint() {
                let _ = write!(message, "\n  hint: {hint}");
            }
            emit_input_error(&message, self.opts.output)
        })
    }

    fn run_base(
        &self,
        base_root: &Path,
        focus: Option<&FxHashSet<PathBuf>>,
    ) -> Result<HeadAnalyses, ExitCode> {
        run_audit_base_analyses(
            self.opts,
            self.base_run(),
            self.head,
            self.type_aware,
            BaseAnalysisRoots {
                host: self.host_root,
                base: base_root,
            },
            focus,
        )
    }

    fn base_cache_key(
        &self,
        base_ref: &str,
        focus: &FxHashSet<PathBuf>,
    ) -> Result<Option<AuditBaseSnapshotCacheKey>, ExitCode> {
        audit_base_snapshot_cache_key(&self.base_key_request(base_ref, focus))
    }

    fn cached_base_sha<'k>(&self, key: &'k AuditBaseSnapshotCacheKey) -> Option<&'k str> {
        Some(key.base_sha.as_str())
    }

    fn load_cached_base(&self, key: &AuditBaseSnapshotCacheKey) -> Option<AuditKeySnapshot> {
        // A run with `--type-aware` computes its base afresh.
        if self.type_aware.cli_enabled() {
            return None;
        }
        load_cached_base_snapshot(self.opts, key)
    }

    fn save_cached_base(&self, key: &AuditBaseSnapshotCacheKey, snapshot: &AuditKeySnapshot) {
        if !self.type_aware.cli_enabled() {
            save_cached_base_snapshot(self.opts, self.host_root, key, snapshot);
        }
    }

    fn shared_diff(&self) -> Option<SharedDiff<'_>> {
        shared_diff()
    }
}

/// The opt-in shared diff (`--diff-file`, `--diff-stdin`, `$FALLOW_DIFF_FILE`)
/// of this run, when one is active.
fn shared_diff() -> Option<SharedDiff<'static>> {
    crate::report::ci::diff_filter::shared_diff_index().map(|index| SharedDiff {
        index,
        label: crate::report::ci::diff_filter::shared_diff_source_label().unwrap_or("shared diff"),
    })
}

/// The roots of one base pass.
#[derive(Clone, Copy)]
struct BaseAnalysisRoots<'a> {
    /// The checkout whose cache directory the base pass mirrors. It is
    /// `opts.root`, except for `--staged`.
    host: &'a Path,
    /// The analysis root inside the base worktree.
    base: &'a Path,
}

/// Run the base analyses in the base worktree. `focus` scopes dead code and
/// duplication to the changed files and the pre-rename paths; without it, the
/// results stay unscoped.
fn run_audit_base_analyses(
    opts: &AuditOptions<'_>,
    base_run: &BaseRunInputs,
    head: &ResolvedRun,
    type_aware: AuditTypeAwareOptions<'_>,
    roots: BaseAnalysisRoots<'_>,
    focus: Option<&FxHashSet<PathBuf>>,
) -> Result<HeadAnalyses, ExitCode> {
    let base_root = roots.base;
    // The head and base passes run at the same time, so the base pass gets
    // its own cache directory inside the base worktree.
    let base_cache_dir = worktree_cache::cache_dir(base_root);
    let _base_parse_cache = worktree_cache::lend_to_base(opts, roots.host, base_root);
    let base_opts = base_run.options(opts, base_root, &base_run.config_path, &base_cache_dir);
    let run = runs::worktree_run(head, base_root, opts, "--base")?;
    let share_dead_code_parse_with_health =
        opts.analyses.health && run.production_modes().dead_code_matches_health();
    let pass = CheckPass {
        run: &run,
        changed_since: None,
        changed_files: focus,
        retain_modules_for_health: share_dead_code_parse_with_health || opts.brief,
        snapshot: fallow_config::AnalysisSnapshot::Base,
    };
    let (check_res, dupes_res) = rayon::join(
        || run_audit_check(&base_opts, type_aware, pass),
        || run_audit_dupes(&base_opts, &run, None, focus, None),
    );
    let mut check = check_res?;
    let dupes = dupes_res?;
    // The public-export set of the base graph is brief-only. The pass keeps
    // the graph for it, as the base options turn the brief off.
    if opts.brief
        && let Some(check) = check.as_mut()
    {
        check.public_api_keys = Some(public_api_keys_from_check(Some(check), base_root));
    }
    let shared_parse = if share_dead_code_parse_with_health {
        check.as_mut().and_then(|r| r.shared_parse.take())
    } else {
        None
    };
    let health = run_audit_health(&base_opts, &run, None, shared_parse, true)?;
    if let Some(check) = check.as_mut() {
        check.shared_parse = None;
    }
    let own_config_unused_code =
        dead_code::base_own_config_unused_code(&dead_code::BaseOwnConfigInput {
            opts,
            head,
            current_config_path: base_run.config_path.as_deref(),
            base_root,
            base_cache_dir: &base_cache_dir,
            base_run,
            focus,
            base_check: check.as_ref(),
        });
    Ok(HeadAnalyses {
        check,
        dupes,
        health,
        brief_diff_evidence: None,
        own_config_unused_code,
    })
}

struct AuditResultParts {
    verdict: AuditVerdict,
    summary: AuditSummary,
    attribution: AuditAttribution,
    dupe_demotion_diff_source: Option<DupeDemotionDiffSource>,
    base_snapshot: Option<AuditKeySnapshot>,
    comparison: Option<keys::AuditComparison>,
    base_snapshot_skipped: bool,
    changed_files_count: usize,
    changed_files: FxHashSet<PathBuf>,
    base_ref: String,
    base_description: Option<String>,
    base_sha: Option<String>,
    head_sha: Option<String>,
    output: OutputFormat,
    performance: bool,
    check: Option<CheckResult>,
    dupes: Option<DupesResult>,
    health: Option<HealthResult>,
    elapsed: Duration,
    review_deltas: Option<crate::audit_brief::ReviewDeltas>,
    weakening_signals: Vec<weakening::WeakeningSignal>,
    diff_risk: fallow_output::DiffRiskReport,
    routing: Option<routing::RoutingFacts>,
    ownership: Option<fallow_output::OwnershipFacts>,
    decision_surface: Option<crate::audit_decision_surface::DecisionSurface>,
    graph_snapshot_hash: Option<String>,
    change_anchors: Vec<crate::audit_walkthrough::ChangeAnchor>,
    diff_index: Option<fallow_output::DiffIndex>,
}

#[derive(Default)]
struct AuditBriefData {
    review_deltas: Option<crate::audit_brief::ReviewDeltas>,
    weakening_signals: Vec<weakening::WeakeningSignal>,
    routing: Option<routing::RoutingFacts>,
    ownership: Option<fallow_output::OwnershipFacts>,
    decision_surface: Option<crate::audit_decision_surface::DecisionSurface>,
    graph_snapshot_hash: Option<String>,
    change_anchors: Vec<crate::audit_walkthrough::ChangeAnchor>,
    diff_index: Option<fallow_output::DiffIndex>,
}

struct AuditBriefDataInput<'a> {
    opts: &'a AuditOptions<'a>,
    check: Option<&'a CheckResult>,
    dupes: Option<&'a DupesResult>,
    health: Option<&'a HealthResult>,
    base_snapshot: Option<&'a AuditKeySnapshot>,
    changed_files: &'a FxHashSet<PathBuf>,
    base_ref: &'a str,
    head_sha: Option<&'a str>,
    /// The diff evidence that the HEAD analyses already read, if any.
    diff_evidence: Option<BriefDiffEvidence>,
    /// Diff-risk findings of the run. The brief weakening signals for tests,
    /// thresholds and suppressions come from this report.
    diff_risk: &'a fallow_output::DiffRiskReport,
}

/// Owned production-analysis inputs for the stable review-brief benchmark.
/// This is not a supported API.
#[doc(hidden)]
pub struct AuditReviewBenchmarkCorpus {
    root: PathBuf,
    state: Option<AuditReviewBenchmarkState>,
    head_sources: FxHashMap<String, String>,
}

struct AuditReviewBenchmarkState {
    head: HeadAnalyses,
    base_snapshot: AuditKeySnapshot,
    changed_files: FxHashSet<PathBuf>,
    external: AuditBriefExternalData,
}

#[derive(Debug, PartialEq, Eq)]
pub struct AuditReviewBenchmarkResult {
    pub introduced_count: usize,
    pub inherited_count: usize,
    pub public_api_added_count: usize,
    pub decision_count: usize,
    pub rendered_bytes: usize,
}

/// Run the three HEAD-side analyses with intra-pipeline sharing intact:
/// check first (so its parsed modules are available), then dupes (which can
/// reuse check's discovered file list when production settings match), then
/// health (which can reuse check's parsed modules when production settings
/// match). The audit runs it inside `rayon::join` alongside
/// [`run_audit_base_analyses`], which operates on an isolated worktree.
fn run_audit_head_analyses(
    opts: &AuditOptions<'_>,
    run: &ResolvedRun,
    type_aware: AuditTypeAwareOptions<'_>,
    changed_since: Option<&str>,
    changed_files: &FxHashSet<PathBuf>,
) -> Result<HeadAnalyses, ExitCode> {
    let modes = run.production_modes();
    let share_dead_code_parse_with_health =
        opts.analyses.health && modes.dead_code_matches_health();
    let share_dead_code_files_with_dupes = opts.analyses.dupes && modes.all_match();
    let pass = CheckPass {
        run,
        changed_since,
        changed_files: Some(changed_files),
        retain_modules_for_health: share_dead_code_parse_with_health,
        snapshot: fallow_config::AnalysisSnapshot::Current,
    };
    let mut check = run_audit_check(opts, type_aware, pass)?;
    let dupes_files = if share_dead_code_files_with_dupes {
        check
            .as_ref()
            .and_then(|r| r.shared_parse.as_ref().map(|sp| sp.files.clone()))
    } else {
        None
    };
    let dupes = run_audit_dupes(opts, run, changed_since, Some(changed_files), dupes_files)?;
    let mut brief_diff_evidence = None;
    // Compute the impact closure AND the exports-aware public-export key
    // set for the review brief BEFORE health consumes the shared parse (which
    // owns the retained graph). Both are stored on the check result so they
    // survive the graph drop.
    if opts.brief
        && let Some(ref mut check) = check
    {
        check.impact_closure = compute_brief_impact_closure(opts.root, check, changed_files);
        check.brief_churn = fallow_api::routing::audit_churn(opts.root, &check.config);
        check.co_change_gaps = compute_brief_co_change_gaps(opts, check, changed_files);
        check.public_api_keys = Some(public_api_keys_from_check(Some(check), opts.root));
        check.partition_order = compute_brief_partition_order(opts.root, check, changed_files);
        check.focus_facts = compute_brief_focus_facts(opts.root, check, changed_files);
        check.export_lines = compute_brief_export_lines(opts.root, check, changed_files);
        let diff_evidence = changed_since.map(|base_ref| {
            compute_brief_diff_evidence(opts.root, base_ref, opts.walkthrough_file)
        });
        let changed_exports =
            compute_brief_changed_exports(opts.root, check, changed_files).map(|exports| {
                diff_touched_exports(
                    exports,
                    diff_evidence
                        .as_ref()
                        .and_then(|evidence| evidence.diff_index.as_ref()),
                )
            });
        check.brief_mutation_survivors = opts.mutation_report.map(|report| {
            let diff_index = diff_evidence.as_ref().and_then(|e| e.diff_index.as_ref());
            report.brief_survivors(opts.root, opts.quiet, diff_index)
        });
        brief_diff_evidence = diff_evidence;
        check.search_gaps =
            compute_brief_search_gaps(opts.root, check, changed_files, changed_exports.as_deref());
        check.test_reach = compute_brief_test_reach(opts.root, check, changed_exports.as_deref());
        check.test_reach_base_exports = changed_since.and_then(|base_ref| {
            compute_brief_test_reach_base_exports(
                opts.root,
                base_ref,
                check.test_reach.as_ref()?,
                brief_diff_evidence
                    .as_ref()
                    .and_then(|evidence| evidence.diff_index.as_ref())?,
            )
        });
        check.internal_consumers =
            compute_brief_internal_consumers(opts.root, check, changed_files);
        check.test_adjacency = compute_brief_test_adjacency(opts.root, check, changed_files);
        check.package_importers = compute_brief_package_importers(check, changed_files);
    }
    let shared_parse = if share_dead_code_parse_with_health {
        check.as_mut().and_then(|r| r.shared_parse.take())
    } else {
        None
    };
    let health = run_audit_health(opts, run, changed_since, shared_parse, false)?;
    // The brief facts above hold what the review needs from the graph, so a
    // graph that health did not consume is released here.
    if let Some(check) = check.as_mut() {
        check.shared_parse = None;
    }
    Ok(HeadAnalyses {
        check,
        dupes,
        health,
        brief_diff_evidence,
        own_config_unused_code: None,
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

/// Scope note on every co-change gap fact.
const CO_CHANGE_GAP_NOTE: &str = "history evidence, not a proof that the partner must change";

/// Compute the co-change gaps for the review brief: changed files whose
/// hidden change-coupling partner is not in the diff.
///
/// Reads the churn pass on `check.brief_churn`, which routing also reads, and
/// the retained graph for the static edges. Applies `health.ignore` and the
/// workspace filter like the `change_coupling` health section. Empty off the
/// brief path, without a commit history, and on a shallow clone, whose
/// history cannot show coupling.
fn compute_brief_co_change_gaps(
    opts: &AuditOptions<'_>,
    check: &CheckResult,
    changed_files: &FxHashSet<PathBuf>,
) -> Vec<fallow_output::CoChangeGapFact> {
    use fallow_engine::change_coupling as coupling;

    let root = opts.root;
    let Some(graph) = check
        .shared_parse
        .as_ref()
        .and_then(|sp| sp.analysis_output.as_ref())
        .and_then(|out| out.graph.as_ref())
    else {
        return Vec::new();
    };
    let Some(churn) = check.brief_churn.as_ref() else {
        return Vec::new();
    };
    if churn.shallow_clone {
        return Vec::new();
    }
    let links = coupling::StaticLinkIndex::from_retained_graph(graph);
    let settings = coupling::ChangeCouplingSettings::from(&check.config.health.change_coupling);
    let mut analysis = coupling::analyze_change_history(&churn.commits, &links, &settings);
    // The check already reported an invalid workspace selection.
    let ws_roots = fallow_engine::workspace_scope::resolve_workspace_scope(
        root,
        opts.workspace,
        opts.changed_workspaces,
        &check.workspaces,
    )
    .ok()
    .flatten();
    let ignore_set = coupling::health_ignore_set(&check.config.health.ignore);
    let scope = coupling::PairScope {
        root,
        ignore_set: &ignore_set,
        ws_roots: ws_roots.as_ref(),
        diff_index: None,
    };
    analysis
        .hidden_pairs
        .retain(|pair| scope.contains(&pair.path_a, &pair.path_b));
    let relative = |path: &std::path::Path| {
        path.strip_prefix(root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/")
    };
    coupling::co_change_gaps(&analysis, changed_files)
        .into_iter()
        .map(|gap| fallow_output::CoChangeGapFact {
            changed_file: relative(&gap.changed),
            partner_file: relative(&gap.partner),
            shared_revisions: gap.shared_revisions,
            degree: gap.degree,
            note: CO_CHANGE_GAP_NOTE.to_owned(),
        })
        .collect()
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

/// Keep the changed exports whose declaration statement overlaps an added
/// diff line. The brief reports only these exports, so the search gap and the
/// test reach passes skip the other exports. Without a diff, keep all exports.
fn diff_touched_exports(
    mut exports: Vec<fallow_engine::module_graph::ChangedExport>,
    diff_index: Option<&fallow_output::DiffIndex>,
) -> Vec<fallow_engine::module_graph::ChangedExport> {
    let Some(diff_index) = diff_index else {
        return exports;
    };
    exports.retain(|export| {
        let key = diff_index.key_for_root_relative(&export.file);
        let (start, end) = export.declaration_lines;
        diff_index.range_overlaps_added(&key, u64::from(start), u64::from(end))
    });
    exports
}

/// List the exports of the changed files with their declaration lines, BEFORE
/// health drops the graph. The literal-search gap and the test reach passes
/// share this one parse. `None` when the graph is not retained.
fn compute_brief_changed_exports(
    root: &std::path::Path,
    check: &CheckResult,
    changed_files: &FxHashSet<PathBuf>,
) -> Option<Vec<fallow_engine::module_graph::ChangedExport>> {
    let graph = check
        .shared_parse
        .as_ref()
        .and_then(|sp| sp.analysis_output.as_ref())
        .and_then(|out| out.graph.as_ref())?;

    fallow_engine::module_graph::changed_exports_for_changed_paths(graph, root, changed_files)
}

/// Precompute the literal-search gaps of the changed exports for the brief,
/// BEFORE health drops the graph. The brief keeps only the exports whose
/// declaration the diff touches. `None` when the graph is not retained.
fn compute_brief_search_gaps(
    root: &std::path::Path,
    check: &CheckResult,
    changed_files: &FxHashSet<PathBuf>,
    changed_exports: Option<&[fallow_engine::module_graph::ChangedExport]>,
) -> Option<Vec<fallow_engine::module_graph::ExportSearchGapPaths>> {
    let graph = check
        .shared_parse
        .as_ref()
        .and_then(|sp| sp.analysis_output.as_ref())
        .and_then(|out| out.graph.as_ref())?;

    fallow_engine::module_graph::search_gaps_for_changed_paths(
        graph,
        root,
        changed_files,
        changed_exports?,
    )
}

/// Precompute how tests reach the value exports of the changed files, BEFORE
/// health drops the graph. The brief keeps only the exports whose declaration
/// the diff touches. Uses the same test-path classification as the weakening
/// scan. `None` when the graph is not retained.
fn compute_brief_test_reach(
    root: &std::path::Path,
    check: &CheckResult,
    changed_exports: Option<&[fallow_engine::module_graph::ChangedExport]>,
) -> Option<fallow_engine::module_graph::TestReachPaths> {
    let graph = check
        .shared_parse
        .as_ref()
        .and_then(|sp| sp.analysis_output.as_ref())
        .and_then(|out| out.graph.as_ref())?;

    fallow_engine::module_graph::test_reach_for_changed_paths(
        graph,
        root,
        changed_exports?,
        &|path| fallow_engine::test_paths::is_test_code_path_str(root, path),
    )
}

/// Read the export names of the test reach files at the base side of the
/// diff. The brief labels an export `new` only when its name is not among
/// them. A renamed file is read at its path before the rename. `None` when
/// the project has no tests, because the brief then lists no rows.
fn compute_brief_test_reach_base_exports(
    root: &std::path::Path,
    base_ref: &str,
    reach: &fallow_engine::module_graph::TestReachPaths,
    diff_index: &fallow_output::DiffIndex,
) -> Option<FxHashMap<String, FxHashSet<String>>> {
    if !reach.has_tests {
        return None;
    }
    let mut files: Vec<(String, String)> = Vec::new();
    for export in &reach.exports {
        // The rows keep the file order of the graph, so the rows of one file
        // are adjacent.
        if files.last().is_some_and(|(head, _)| *head == export.file) {
            continue;
        }
        let base = diff_index
            .old_path_for_root_relative(&export.file)
            .map_or_else(|| export.file.clone(), std::borrow::Cow::into_owned);
        files.push((export.file.clone(), base));
    }
    Some(fallow_api::audit_run::export_names_at_base(
        root, base_ref, &files,
    ))
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

/// Precompute the per-changed-source-file direct test adjacency for the review
/// direction from the retained graph, BEFORE health drops it. Uses the same
/// test-path classification as the weakening scan so the two surfaces agree.
/// `None` when the graph is not retained.
fn compute_brief_test_adjacency(
    root: &std::path::Path,
    check: &CheckResult,
    changed_files: &FxHashSet<PathBuf>,
) -> Option<FxHashMap<String, fallow_output::TestAdjacency>> {
    let graph = check
        .shared_parse
        .as_ref()
        .and_then(|sp| sp.analysis_output.as_ref())
        .and_then(|out| out.graph.as_ref())?;

    fallow_engine::module_graph::test_adjacency_for_changed_paths(
        graph,
        root,
        changed_files,
        &|path| fallow_engine::test_paths::is_test_code_path_str(root, path),
    )
}

/// Precompute per-package in-repo importer counts for the dependency decision
/// arm from the retained graph, BEFORE health drops it. `None` when the graph
/// is not retained or saw no package usage.
fn compute_brief_package_importers(
    check: &CheckResult,
    changed_files: &FxHashSet<PathBuf>,
) -> Option<FxHashMap<String, fallow_engine::module_graph::PackageImporters>> {
    let graph = check
        .shared_parse
        .as_ref()
        .and_then(|sp| sp.analysis_output.as_ref())
        .and_then(|out| out.graph.as_ref())?;

    fallow_engine::module_graph::package_importers_for_changed_paths(graph, changed_files)
}

/// Compute the per-file focus graph facts (fan-in/out + the dynamic-dispatch /
/// re-export-indirection confidence-flag signals) for the review brief's stage 4
/// weighted focus map, from the check result's retained graph against the
/// changed-file set.
///
/// Maps each changed absolute path to its graph `FileId`, computes the per-file
/// blast + confidence signals, and path-resolves them. Returns `None` when the
/// graph was not retained (off the brief path) or no changed file maps to a known module.
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
/// Run the audit of `opts` with the run that its options resolve.
#[cfg(test)]
fn execute_audit(opts: &AuditOptions<'_>) -> Result<AuditResult, ExitCode> {
    let head = runs::options_run(opts)?;
    execute_audit_for_host(opts, AuditTypeAwareOptions::default(), opts.root, &head)
}

/// Run the audit of `opts.root` with the resolved run `head`. `host_root` is
/// the checkout that owns the base worktree. It is not `opts.root` when
/// `opts.root` is the temporary worktree of `--staged`.
fn execute_audit_for_host(
    opts: &AuditOptions<'_>,
    type_aware: AuditTypeAwareOptions<'_>,
    host_root: &Path,
    head: &ResolvedRun,
) -> Result<AuditResult, ExitCode> {
    let start = Instant::now();

    let fallow_engine::repo_refs::ResolvedAuditBase {
        git_ref: base_ref,
        description: base_description,
        inherited_refs,
    } = resolve_base_ref(opts)?;

    let mut changed_files = audit_changed_files(opts, &base_ref)?;
    if let Some(walkthrough_file) = opts.walkthrough_file
        && let Ok(walkthrough_file) = dunce::canonicalize(walkthrough_file)
    {
        changed_files.remove(&walkthrough_file);
    }
    if let Some(scope) = opts.scope.as_deref() {
        changed_files.retain(|file| crate::scope_path::scope_covers(scope, file));
    }
    let changed_files_count = changed_files.len();

    let staged = (host_root != opts.root)
        .then(|| runs::staged_run(head, opts.root, opts))
        .transpose()?;
    let backend = CliAuditBackend {
        opts,
        head: staged.as_ref().unwrap_or(head),
        host_root,
        type_aware,
        base_ref: &base_ref,
        workspace_patterns: OnceLock::new(),
        base_run: OnceLock::new(),
    };
    let run = fallow_api::audit_run::run(
        &backend,
        AuditRunInput {
            root: opts.root,
            gate: opts.gate,
            worsened: opts.worsened,
            base_ref: &base_ref,
            cache_dir: Some(opts.cache_dir),
            changed_files,
            inherited_base_refs: &inherited_refs,
            compare_line_counts: opts.file_lines_check,
        },
    )?;
    let Some(run) = run else {
        return Ok(empty_audit_result(
            base_ref,
            base_description,
            opts,
            start.elapsed(),
        ));
    };
    Ok(finish_audit_result(
        AuditFinishInput {
            opts,
            changed_files_count,
            base_ref,
            base_description,
            head_sha: AuditHeadSha::Production,
            start,
        },
        run,
        compute_audit_brief_data,
    ))
}

/// Inputs threaded from the audit prelude into [`finish_audit_result`].
struct AuditFinishInput<'a> {
    opts: &'a AuditOptions<'a>,
    changed_files_count: usize,
    base_ref: String,
    base_description: Option<String>,
    head_sha: AuditHeadSha,
    start: Instant,
}

enum AuditHeadSha {
    Production,
    Preloaded(Option<String>),
}

/// Build the final `AuditResult` from a completed audit run: report a degraded
/// type-aware comparison, compute the review-brief data, and keep every part
/// the renderers read.
fn finish_audit_result(
    input: AuditFinishInput<'_>,
    run: AuditRun<HeadAnalyses>,
    build_brief: impl FnOnce(AuditBriefDataInput<'_>) -> AuditBriefData,
) -> AuditResult {
    let opts = input.opts;
    let AuditRun {
        analyses,
        changed_files,
        outcome,
    } = run;
    if let Some(warning) = outcome.type_aware_degrade_warning.as_deref()
        && matches!(opts.output, fallow_config::OutputFormat::Human)
        && !opts.quiet
    {
        eprintln!(
            "{}",
            crate::report::human_status_line(
                crate::report::HumanStatus::Warning,
                format_args!("Type-aware: {warning}")
            )
        );
    }
    let summary = outcome.summary;
    crate::telemetry::note_final_result_count(
        summary.dead_code_issues + summary.complexity_findings + summary.duplication_clone_groups,
    );
    let (base_sha, head_sha) = match input.head_sha {
        AuditHeadSha::Production => (
            crate::base_worktree::git_short_merge_base(opts.root, &input.base_ref),
            short_head_sha(opts.root),
        ),
        AuditHeadSha::Preloaded(head_sha) => (None, head_sha),
    };
    let HeadAnalyses {
        check,
        dupes,
        health,
        brief_diff_evidence,
        own_config_unused_code: _,
    } = analyses;
    let brief = build_brief(AuditBriefDataInput {
        opts,
        check: check.as_ref(),
        dupes: dupes.as_ref(),
        health: health.as_ref(),
        base_snapshot: outcome.base_snapshot.as_ref(),
        changed_files: &changed_files,
        base_ref: &input.base_ref,
        head_sha: head_sha.as_deref(),
        diff_evidence: brief_diff_evidence,
        diff_risk: &outcome.diff_risk,
    });

    build_audit_result(AuditResultParts {
        verdict: outcome.verdict,
        summary,
        attribution: outcome.attribution,
        dupe_demotion_diff_source: outcome.dupe_demotion_diff_source,
        base_snapshot: outcome.base_snapshot,
        comparison: Some(outcome.comparison),
        base_snapshot_skipped: outcome.base_snapshot_skipped,
        changed_files_count: input.changed_files_count,
        changed_files,
        base_ref: input.base_ref,
        base_description: input.base_description,
        base_sha,
        head_sha,
        output: opts.output,
        performance: opts.performance,
        check,
        dupes,
        health,
        elapsed: input.start.elapsed(),
        review_deltas: brief.review_deltas,
        weakening_signals: brief.weakening_signals,
        diff_risk: outcome.diff_risk,
        routing: brief.routing,
        ownership: brief.ownership,
        decision_surface: brief.decision_surface,
        graph_snapshot_hash: brief.graph_snapshot_hash,
        change_anchors: brief.change_anchors,
        diff_index: brief.diff_index,
    })
}

fn compute_audit_brief_data(input: AuditBriefDataInput<'_>) -> AuditBriefData {
    if !input.opts.brief {
        return AuditBriefData::default();
    }

    let root = input
        .check
        .map(|check| check.config.root.clone())
        .unwrap_or_default();
    let head_source = |rel: &str| std::fs::read_to_string(root.join(rel)).ok();
    compute_audit_brief_data_with_lookups(input, None, &head_source)
}

struct AuditBriefExternalData {
    security_step_signals: Vec<weakening::WeakeningSignal>,
    routing: Option<routing::RoutingFacts>,
    diff_evidence: BriefDiffEvidence,
    dependency_anchors: Vec<crate::audit_decision_surface::DependencyAnchor>,
}

fn prepare_audit_brief_external_data(
    opts: &AuditOptions<'_>,
    check: Option<&CheckResult>,
    changed_files: &FxHashSet<PathBuf>,
    base_ref: &str,
) -> AuditBriefExternalData {
    let security_step_signals = compute_security_step_signals(opts.root, base_ref, changed_files);
    let routing = check.map(|check| routing::brief_routing(opts.root, check, changed_files));
    let diff_evidence = compute_brief_diff_evidence(opts.root, base_ref, opts.walkthrough_file);
    let dependency_anchors = compute_dependency_anchors(
        opts.root,
        base_ref,
        changed_files,
        check.and_then(|check| check.package_importers.as_ref()),
        &|rel| diff_rename_old_path(diff_evidence.diff_index.as_ref(), rel),
    );
    AuditBriefExternalData {
        security_step_signals,
        routing,
        diff_evidence,
        dependency_anchors,
    }
}

#[path = "audit_benchmark_options.rs"]
mod benchmark_options;
use benchmark_options::*;

/// Build the analysis corpus and preload every external input used by the
/// review-brief assembly benchmark. This is not a supported API.
#[doc(hidden)]
pub fn create_audit_review_benchmark_corpus(
    root: &Path,
    changed_files: &[PathBuf],
    threads: usize,
) -> Result<AuditReviewBenchmarkCorpus, ExitCode> {
    let (root, changed_files) = runs::canonical_changed_files(root, changed_files);
    let root = root.as_path();
    let config_path = None;
    let cache_dir = root.join(".fallow-cache-benchmark");
    let opts = audit_review_benchmark_options(root, &config_path, &cache_dir, threads);
    let run = runs::options_run(&opts)?;
    let mut head = run_audit_head_analyses(
        &opts,
        &run,
        AuditTypeAwareOptions::default(),
        None,
        &changed_files,
    )?;

    // The benchmark targets review assembly. Keeping the duplication and health
    // domains empty prevents their optional diff and snapshot side effects from
    // entering the timed path while dead-code attribution remains scalable.
    head.dupes = None;
    head.health = None;
    if let Some(check) = head.check.as_mut() {
        check.public_api_keys = Some(
            changed_files
                .iter()
                .filter_map(|path| {
                    let relative = path.strip_prefix(root).ok()?;
                    let index = path.file_stem()?.to_str()?.strip_prefix("module")?;
                    let relative = relative.to_string_lossy().replace('\\', "/");
                    Some([
                        format!("{relative}::used{index}"),
                        format!("{relative}::unused{index}"),
                    ])
                })
                .flatten()
                .collect(),
        );
    }
    let mut base_snapshot = AuditKeySnapshot::from_view(&head.view());
    let dead_code_keys = sorted_keys(&base_snapshot.dead_code);
    for key in dead_code_keys.into_iter().step_by(2) {
        base_snapshot.dead_code.remove(&key);
    }
    let public_api_keys = sorted_keys(&base_snapshot.public_api);
    for key in public_api_keys.into_iter().step_by(2) {
        base_snapshot.public_api.remove(&key);
    }

    let external = prepare_audit_brief_external_data(
        &opts,
        head.check.as_ref(),
        &changed_files,
        "benchmark-base",
    );
    let head_sources = changed_files
        .iter()
        .filter_map(|path| {
            let relative = path.strip_prefix(root).ok()?;
            let source = std::fs::read_to_string(path).ok()?;
            Some((relative.to_string_lossy().replace('\\', "/"), source))
        })
        .collect();

    Ok(AuditReviewBenchmarkCorpus {
        root: root.to_path_buf(),
        state: Some(AuditReviewBenchmarkState {
            head,
            base_snapshot,
            changed_files,
            external,
        }),
        head_sources,
    })
}

/// Finding counts that the audit analyses benchmark checks after each run.
/// This is not a supported API.
#[doc(hidden)]
#[derive(Debug, PartialEq, Eq)]
pub struct AuditAnalysesBenchmarkResult {
    pub introduced_count: usize,
    pub inherited_count: usize,
    pub clone_group_count: usize,
    pub health_finding_count: usize,
}

/// Run the HEAD analyses and the focused base analyses of one audit in
/// parallel, as the production audit does, then attribute the dead-code
/// findings. The base side reads a prepared tree instead of a git worktree,
/// so no git process runs. This is not a supported API.
#[doc(hidden)]
pub fn benchmark_audit_analyses(
    head_root: &Path,
    base_root: &Path,
    changed_files: &[PathBuf],
    threads: usize,
) -> Result<AuditAnalysesBenchmarkResult, ExitCode> {
    let (head_root, changed_files) = runs::canonical_changed_files(head_root, changed_files);
    let head_root = head_root.as_path();
    let config_path = None;
    let cache_dir = head_root.join(".fallow-cache-benchmark");
    let mut opts = audit_review_benchmark_options(head_root, &config_path, &cache_dir, threads);
    // The review brief has its own benchmark and reads git history.
    opts.brief = false;
    let focus = fallow_api::audit_run::base_focus_files(&changed_files, &[]);
    // Production canonicalizes the base root, because a session
    // canonicalizes its root and a focus set under a symbolic link would
    // match no finding.
    let base_root = dunce::canonicalize(base_root).unwrap_or_else(|_| base_root.to_path_buf());
    let base_root = base_root.as_path();
    let base_focus = fallow_api::audit_run::remap_focus_files(&focus, head_root, base_root);
    let run = runs::options_run(&opts)?;
    let base_run = BaseRunInputs::resolve(&opts);
    let (head, base) = rayon::join(
        || {
            let type_aware = AuditTypeAwareOptions::default();
            run_audit_head_analyses(&opts, &run, type_aware, None, &changed_files)
        },
        || {
            run_audit_base_analyses(
                &opts,
                &base_run,
                &run,
                AuditTypeAwareOptions::default(),
                BaseAnalysisRoots {
                    host: opts.root,
                    base: base_root,
                },
                base_focus.as_ref(),
            )
        },
    );
    let mut head = head?;
    let mut base = base?;
    scope_benchmark_dependency_findings(&mut head, &changed_files);
    if let Some(focus) = base_focus.as_ref() {
        scope_benchmark_dependency_findings(&mut base, focus);
    }
    let base_snapshot = AuditKeySnapshot::from_view(&base.view());
    let run = attribute_preloaded_run(&opts, head, base_snapshot, changed_files);
    let clone_group_count = run
        .analyses
        .dupes
        .as_ref()
        .map_or(0, |dupes| dupes.report.clone_groups.len());
    let health_finding_count = run
        .analyses
        .health
        .as_ref()
        .map_or(0, |health| health.report.findings.len());
    Ok(AuditAnalysesBenchmarkResult {
        introduced_count: run.outcome.attribution.dead_code_introduced,
        inherited_count: run.outcome.attribution.dead_code_inherited,
        clone_group_count,
        health_finding_count,
    })
}

/// Scope the dependency findings of one side to `files`, under the root that
/// the dead-code analysis reports, as the production audit does.
fn scope_benchmark_dependency_findings(analyses: &mut HeadAnalyses, files: &FxHashSet<PathBuf>) {
    let Some(root) = analyses
        .view()
        .dead_code
        .map(|dead_code| dead_code.root.to_path_buf())
    else {
        return;
    };
    if let Some(results) = analyses.dead_code_results_mut() {
        fallow_api::audit_run::scope_dependency_findings(results, &root, files);
    }
}

/// Attribute preloaded head analyses against a preloaded base snapshot, with
/// no renames, through the production attribution.
fn attribute_preloaded_run(
    opts: &AuditOptions<'_>,
    mut head: HeadAnalyses,
    base_snapshot: AuditKeySnapshot,
    changed_files: FxHashSet<PathBuf>,
) -> AuditRun<HeadAnalyses> {
    let outcome = fallow_api::audit_run::attribute(
        &mut head,
        fallow_api::audit_run::AuditAttributionInput {
            root: opts.root,
            gate: opts.gate,
            worsened: opts.worsened,
            base_ref: "benchmark-base",
            base: fallow_api::audit_run::AuditBase {
                snapshot: Some(base_snapshot),
                skipped: false,
                inherited: Vec::new(),
            },
            renames: &[],
            changed_files: &changed_files,
            shared_diff: shared_diff(),
        },
    );
    AuditRun {
        analyses: head,
        changed_files,
        outcome,
    }
}

/// Run production audit assembly and compact tagged review-brief JSON rendering
/// over a fully preloaded corpus. This is not a supported API.
#[doc(hidden)]
pub fn benchmark_audit_review_brief_many_changed_files_json(
    corpus: &mut AuditReviewBenchmarkCorpus,
) -> Result<AuditReviewBenchmarkResult, ExitCode> {
    benchmark_audit_review_brief_started_at(corpus, Instant::now())
}

/// Benchmark body with an explicit start time, so tests can simulate a slow
/// timed path without sleeping.
fn benchmark_audit_review_brief_started_at(
    corpus: &mut AuditReviewBenchmarkCorpus,
    start: Instant,
) -> Result<AuditReviewBenchmarkResult, ExitCode> {
    let AuditReviewBenchmarkState {
        head,
        base_snapshot,
        changed_files,
        external,
    } = corpus.state.take().ok_or_else(exit_invalid_input)?;
    let config_path = None;
    let cache_dir = corpus.root.join(".fallow-cache-benchmark");
    let opts = audit_review_benchmark_options(&corpus.root, &config_path, &cache_dir, 1);
    let head_source = |relative: &str| corpus.head_sources.get(relative).cloned();
    let changed_files_count = changed_files.len();
    let run = attribute_preloaded_run(&opts, head, base_snapshot, changed_files);
    let mut result = finish_audit_result(
        AuditFinishInput {
            opts: &opts,
            changed_files_count,
            base_ref: "benchmark-base".to_owned(),
            base_description: None,
            head_sha: AuditHeadSha::Preloaded(Some("benchmark-head".to_owned())),
            start,
        },
        run,
        |input| compute_audit_brief_data_with_lookups(input, Some(external), &head_source),
    );
    if result.verdict != AuditVerdict::Fail {
        return Err(exit_invalid_input());
    }
    let decision_count = result
        .decision_surface
        .as_ref()
        .map_or(0, |surface| surface.decisions.len());
    // The wall clock and the process-wide telemetry run id are ambient inputs.
    // They must not change the rendered size between two runs of one corpus.
    result.elapsed = Duration::ZERO;
    let output = crate::audit_brief::build_brief_json(&result, result.diff_index.as_ref())?;
    let value = fallow_output::serialize_review_brief_json_output(output, None)
        .map_err(|_| exit_invalid_input())?;
    if value.get("kind").and_then(serde_json::Value::as_str) != Some("audit-brief") {
        return Err(exit_invalid_input());
    }
    let rendered = crate::json_style::JsonStyle::Compact
        .serialize(&value)
        .map_err(|_| exit_invalid_input())?;
    let benchmark_result = AuditReviewBenchmarkResult {
        introduced_count: result.attribution.dead_code_introduced,
        inherited_count: result.attribution.dead_code_inherited,
        public_api_added_count: result
            .review_deltas
            .as_ref()
            .map_or(0, |deltas| deltas.public_api_added.len()),
        decision_count,
        rendered_bytes: rendered.len(),
    };
    let changed_files: FxHashSet<PathBuf> = result.changed_files.drain(..).collect();
    let dependency_anchors = compute_dependency_anchors(
        &corpus.root,
        &result.base_ref,
        &changed_files,
        result
            .check
            .as_ref()
            .and_then(|check| check.package_importers.as_ref()),
        &|rel| diff_rename_old_path(result.diff_index.as_ref(), rel),
    );
    corpus.state = Some(AuditReviewBenchmarkState {
        head: HeadAnalyses {
            check: result.check.take(),
            dupes: result.dupes.take(),
            health: result.health.take(),
            brief_diff_evidence: None,
            own_config_unused_code: None,
        },
        base_snapshot: result.base_snapshot.take().ok_or_else(exit_invalid_input)?,
        changed_files,
        external: AuditBriefExternalData {
            // Keep only the brief-local CI signals: the next iteration projects
            // the diff-risk signals from its own report again.
            security_step_signals: std::mem::take(&mut result.weakening_signals)
                .into_iter()
                .filter(|signal| signal.kind == weakening::WeakeningKind::SecurityCheckRemoved)
                .collect(),
            routing: result.routing.take(),
            diff_evidence: BriefDiffEvidence {
                change_anchors: std::mem::take(&mut result.change_anchors),
                diff_index: result.diff_index.take(),
            },
            dependency_anchors,
        },
    });
    Ok(benchmark_result)
}

fn compute_audit_brief_data_with_lookups(
    input: AuditBriefDataInput<'_>,
    preloaded: Option<AuditBriefExternalData>,
    head_source: &dyn Fn(&str) -> Option<String>,
) -> AuditBriefData {
    if !input.opts.brief {
        return AuditBriefData::default();
    }

    let mut review_deltas = compute_review_deltas(input.check, input.base_snapshot);
    // Every git-backed pass (security steps, routing, diff evidence, manifest
    // diff) comes from the preloaded package when one exists, so the benchmark
    // path never re-spawns git per iteration.
    let (preloaded_diff_evidence, preloaded) = match preloaded {
        Some(external) => (
            Some(external.diff_evidence),
            Some((
                external.security_step_signals,
                external.routing,
                external.dependency_anchors,
            )),
        ),
        None => (None, None),
    };
    let diff_evidence = preloaded_diff_evidence
        .or(input.diff_evidence)
        .unwrap_or_else(|| {
            compute_brief_diff_evidence(
                input.opts.root,
                input.base_ref,
                input.opts.walkthrough_file,
            )
        });
    let rename_old_path = |rel: &str| diff_rename_old_path(diff_evidence.diff_index.as_ref(), rel);
    let (security_step_signals, routing, dependency_anchors) = preloaded.unwrap_or_else(|| {
        (
            compute_security_step_signals(input.opts.root, input.base_ref, input.changed_files),
            input
                .check
                .map(|check| routing::brief_routing(input.opts.root, check, input.changed_files)),
            compute_dependency_anchors(
                input.opts.root,
                input.base_ref,
                input.changed_files,
                input
                    .check
                    .and_then(|check| check.package_importers.as_ref()),
                &rename_old_path,
            ),
        )
    });
    if let Some(deltas) = review_deltas.as_mut() {
        fallow_api::dependency_deltas::fill_dependency_delta_keys(deltas, &dependency_anchors);
    }
    let weakening_signals = brief_weakening_signals(input.diff_risk, security_step_signals);

    // Decision surface: classify the SOLID-3 candidates, rank, cap, and route.
    let decision_surface = Some(compute_decision_surface_with_lookups(
        input.opts,
        input.check,
        review_deltas.as_ref(),
        routing.as_ref(),
        &DecisionSurfaceLookups {
            dependency_anchors: &dependency_anchors,
            head_source,
            rename_old_path: &rename_old_path,
        },
    ));

    let change_anchors = diff_evidence.change_anchors;

    // Graph-snapshot hash pins key sets, resolved base, head sha, and anchors.
    let graph_snapshot_hash = Some(compute_graph_snapshot_hash(
        input.check,
        input.dupes,
        input.health,
        input.base_ref,
        input.head_sha,
        &change_anchors,
    ));

    let ownership = input
        .check
        .and_then(|check| compute_ownership(check, input.changed_files));

    AuditBriefData {
        review_deltas,
        weakening_signals,
        routing,
        ownership,
        decision_surface,
        graph_snapshot_hash,
        change_anchors,
        diff_index: diff_evidence.diff_index,
    }
}

/// Compute the owner-group reach of the change from the CODEOWNERS file.
/// `None` when no CODEOWNERS file is found or the file cannot be read. A
/// configured path that fails prints a warning. Reads no git history, so
/// the section does not depend on the churn walk behind routing.
fn compute_ownership(
    check: &CheckResult,
    changed_files: &FxHashSet<PathBuf>,
) -> Option<fallow_output::OwnershipFacts> {
    let root = check.config.root.as_path();
    let codeowners = match fallow_api::ownership::load_codeowners(root, &check.config) {
        Ok(codeowners) => codeowners?,
        Err(reason) => {
            tracing::warn!("{reason}. The review brief has no ownership section.");
            return None;
        }
    };
    let mut changed: Vec<String> = changed_files
        .iter()
        .map(|path| keys::relative_key_path(path, root))
        .collect();
    changed.sort_unstable();
    changed.dedup();
    let affected = check
        .impact_closure
        .as_ref()
        .map_or(&[][..], |closure| closure.affected_not_shown.as_slice());
    Some(fallow_api::ownership::compute_ownership_facts(
        &codeowners,
        &changed,
        affected,
        check.partition_order.as_ref(),
    ))
}

#[derive(Default)]
struct BriefDiffEvidence {
    change_anchors: Vec<crate::audit_walkthrough::ChangeAnchor>,
    diff_index: Option<fallow_output::DiffIndex>,
}

/// Derive anchors and triage metrics from the SAME diff source the run used:
/// the opt-in shared diff when present, else the committed merge-base diff.
/// The normal git diff is fetched once and parsed into both representations.
fn compute_brief_diff_evidence(
    root: &std::path::Path,
    base_ref: &str,
    walkthrough_file: Option<&std::path::Path>,
) -> BriefDiffEvidence {
    let excluded_file = walkthrough_file_relative_to_root(root, walkthrough_file);
    let shared = crate::report::ci::diff_filter::shared_diff_raw()
        .zip(crate::report::ci::diff_filter::shared_diff_index());
    let worktree;
    let (raw, diff_index) = if let Some((raw, index)) = shared {
        (raw, index.clone())
    } else {
        let Ok(diff) = fallow_engine::changed_files::try_get_changed_diff(root, base_ref) else {
            return BriefDiffEvidence::default();
        };
        worktree = diff;
        let index = fallow_output::DiffIndex::from_unified_diff(&worktree);
        (worktree.as_str(), index)
    };
    let mut change_anchors = crate::audit_walkthrough::parse_change_anchors(raw);
    if let Some(excluded) = excluded_file.as_deref() {
        change_anchors.retain(|anchor| anchor.file != excluded);
    }
    BriefDiffEvidence {
        change_anchors,
        diff_index: Some(diff_index),
    }
}

/// The pre-rename path of a head root-relative path in the diff of the run.
fn diff_rename_old_path(index: Option<&fallow_output::DiffIndex>, rel: &str) -> Option<String> {
    index?
        .old_path_for_root_relative(rel)
        .map(std::borrow::Cow::into_owned)
}

fn walkthrough_file_relative_to_root(
    root: &Path,
    walkthrough_file: Option<&Path>,
) -> Option<String> {
    let root = dunce::canonicalize(root).ok()?;
    let file = dunce::canonicalize(walkthrough_file?).ok()?;
    let relative = file.strip_prefix(root).ok()?;
    Some(relative.to_string_lossy().replace('\\', "/"))
}

/// Compute the decision surface from the assembled brief inputs: gather the
/// boundary anchors (one representative per introduced zone-pair), the
/// coordination gaps, and the impact-closure blast magnitude, then run the
/// extractor. The cap is taken from the audit options (clamped to [3, 5] by the
/// extractor). Returns an empty surface when no check result is available.
/// The per-run lookups the decision extractor needs beyond the brief data:
/// head sources for suppression checks, the rename map for review memory, and
/// the dependency candidates read from the changed manifests.
struct DecisionSurfaceLookups<'a> {
    dependency_anchors: &'a [crate::audit_decision_surface::DependencyAnchor],
    head_source: &'a dyn Fn(&str) -> Option<String>,
    rename_old_path: &'a dyn Fn(&str) -> Option<String>,
}

fn compute_decision_surface_with_lookups(
    opts: &AuditOptions<'_>,
    check: Option<&CheckResult>,
    review_deltas: Option<&crate::audit_brief::ReviewDeltas>,
    routing: Option<&routing::RoutingFacts>,
    lookups: &DecisionSurfaceLookups<'_>,
) -> crate::audit_decision_surface::DecisionSurface {
    use crate::audit_decision_surface::{
        CoordinationAnchor, DecisionInputs, extract_decision_surface,
    };

    let (Some(check), Some(deltas)) = (check, review_deltas) else {
        return crate::audit_decision_surface::DecisionSurface::default();
    };
    let root = &check.config.root;

    let boundary_anchors = decision_boundary_anchors(check, deltas, root);

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

    // Resolve a contract symbol's 1-based declaration line from the per-file
    // export-line map precomputed on the brief path (the graph is already dropped
    // by health here, so we cannot re-derive it now). Lets coordination /
    // public-API decisions deep-link to the exact export instead of the file head.
    for anchor in &mut coordination {
        anchor.line = resolve_export_line(
            check.export_lines.as_ref(),
            &anchor.changed_file,
            &anchor.consumed_symbols,
        );
    }
    let public_api_anchor_line = deltas.public_api_added.first().map_or(0, |key| {
        let mut parts = key.splitn(2, "::");
        let path = parts.next().unwrap_or_default();
        let name = parts.next().unwrap_or_default();
        resolve_export_line(check.export_lines.as_ref(), path, &[name.to_string()])
    });

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
        dependency_anchors: lookups.dependency_anchors,
        public_api_anchor_line,
        affected_not_shown,
        routing,
        head_source: lookups.head_source,
        rename_old_path: lookups.rename_old_path,
        internal_consumers: &internal_consumers,
        cap: opts.max_decisions,
    })
}

fn decision_boundary_anchors(
    check: &CheckResult,
    deltas: &crate::audit_brief::ReviewDeltas,
    root: &std::path::Path,
) -> Vec<crate::audit_decision_surface::BoundaryAnchor> {
    use crate::audit_decision_surface::BoundaryAnchor;

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
    boundary_anchors
}

fn resolve_export_line(
    export_lines: Option<&FxHashMap<String, Vec<(String, u32)>>>,
    rel: &str,
    symbols: &[String],
) -> u32 {
    let Some(exports) = export_lines.and_then(|map| map.get(rel)) else {
        return 0;
    };
    exports
        .iter()
        .find(|(name, _)| symbols.iter().any(|s| name == s))
        .or_else(|| exports.first())
        .map_or(0, |(_, line)| *line)
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

/// Compute the review-brief deltas from already assembled head and base data.
fn compute_review_deltas(
    check: Option<&CheckResult>,
    base_snapshot: Option<&AuditKeySnapshot>,
) -> Option<crate::audit_brief::ReviewDeltas> {
    check.zip(base_snapshot).map(|(check, base)| {
        let head_boundary = review_deltas::boundary_edge_keys(&check.results.boundary_violations);
        let head_cycles =
            review_deltas::cycle_keys(&check.results.circular_dependencies, &check.config.root);
        let head_public_api = check.public_api_keys.clone().unwrap_or_default();
        crate::audit_brief::build_review_deltas(
            &head_boundary,
            &base.boundary_edges,
            &head_cycles,
            &base.cycles,
            &head_public_api,
            &base.public_api,
        )
    })
}

/// The weakening signals of the brief, ordered by file: the diff-risk
/// findings for tests, thresholds and suppressions, then the CI security-step
/// signals that only the brief computes.
fn brief_weakening_signals(
    diff_risk: &fallow_output::DiffRiskReport,
    security_step_signals: Vec<weakening::WeakeningSignal>,
) -> Vec<weakening::WeakeningSignal> {
    let mut signals = weakening::signals_from_diff_risk(diff_risk);
    signals.extend(security_step_signals);
    signals.sort_by(|a, b| a.file.cmp(&b.file));
    signals
}

/// Read every changed `package.json` at head and at base (through the batch
/// git reader) and project the pairs onto dependency anchors through the
/// shared builder in `fallow_api::dependency_deltas`, so the CLI and the typed
/// runtime cannot drift. Manifest keys are root-relative like every other
/// anchor; the git read uses the repository-relative path. Best-effort like the
/// weakening scan: an unreadable manifest yields nothing, and a base-reader
/// error stops the scan.
fn compute_dependency_anchors(
    root: &Path,
    base_ref: &str,
    changed_files: &FxHashSet<PathBuf>,
    package_importers: Option<&FxHashMap<String, fallow_engine::module_graph::PackageImporters>>,
    rename_old_path: &dyn Fn(&str) -> Option<String>,
) -> Vec<crate::audit_decision_surface::DependencyAnchor> {
    use fallow_api::dependency_deltas::{
        ManifestPair, dependency_anchors_from_manifests, is_manifest_path,
    };

    let Some(git_root) = git_toplevel(root) else {
        return Vec::new();
    };
    let root_prefix = root
        .strip_prefix(&git_root)
        .unwrap_or_else(|_| Path::new(""));
    let mut manifests: Vec<(String, PathBuf, &PathBuf)> = Vec::new();
    for abs in changed_files {
        let (Ok(root_relative), Ok(git_relative)) =
            (abs.strip_prefix(root), abs.strip_prefix(&git_root))
        else {
            continue;
        };
        let manifest = root_relative.to_string_lossy().replace('\\', "/");
        if is_manifest_path(&manifest) {
            manifests.push((manifest, git_relative.to_path_buf(), abs));
        }
    }
    if manifests.is_empty() {
        return Vec::new();
    }
    manifests.sort();
    let Some(mut reader) = BaseFileReader::spawn(root) else {
        return Vec::new();
    };

    let mut pairs = Vec::new();
    for (manifest, git_relative, abs) in manifests {
        let Ok(head) = std::fs::read_to_string(abs) else {
            continue;
        };
        let mut base = match reader.read(base_ref, &git_relative) {
            BaseRead::Content(base) => Some(base),
            BaseRead::Missing => None,
            BaseRead::Error => break,
        };
        // A manifest that moved with its package (`git mv packages/a
        // packages/b`) is not new: read it at its pre-rename path so its
        // dependency list is diffed, not reported wholesale as added.
        if base.is_none()
            && let Some(old) = rename_old_path(&manifest)
        {
            base = match reader.read(base_ref, &root_prefix.join(old)) {
                BaseRead::Content(base) => Some(base),
                BaseRead::Missing => None,
                BaseRead::Error => break,
            };
        }
        pairs.push(ManifestPair {
            manifest,
            base,
            head,
        });
    }
    dependency_anchors_from_manifests(&pairs, package_importers)
}

fn build_audit_result(parts: AuditResultParts) -> AuditResult {
    AuditResult {
        verdict: parts.verdict,
        summary: parts.summary,
        attribution: parts.attribution,
        dupe_demotion_diff_source: parts.dupe_demotion_diff_source,
        base_snapshot: parts.base_snapshot,
        comparison: parts.comparison,
        base_snapshot_skipped: parts.base_snapshot_skipped,
        changed_files_count: parts.changed_files_count,
        changed_files: parts.changed_files.into_iter().collect(),
        base_ref: parts.base_ref,
        base_description: parts.base_description,
        base_sha: parts.base_sha,
        shallow_clone_without_base: false,
        head_sha: parts.head_sha,
        output: parts.output,
        performance: parts.performance,
        check: parts.check,
        dupes: parts.dupes,
        health: parts.health,
        elapsed: parts.elapsed,
        review_deltas: parts.review_deltas,
        weakening_signals: parts.weakening_signals,
        diff_risk: parts.diff_risk,
        routing: parts.routing,
        ownership: parts.ownership,
        decision_surface: parts.decision_surface,
        graph_snapshot_hash: parts.graph_snapshot_hash,
        change_anchors: parts.change_anchors,
        diff_index: parts.diff_index,
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

    let base_sha = crate::base_worktree::git_short_merge_base(opts.root, &base_ref);
    let head_sha = short_head_sha(opts.root);
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
            dead_code_skipped: !opts.analyses.dead_code,
            complexity_skipped: !opts.analyses.health,
            duplication_skipped: !opts.analyses.dupes,
            complexity_findings: 0,
            max_cyclomatic: None,
            duplication_clone_groups: 0,
        },
        attribution: AuditAttribution {
            gate: opts.gate,
            worsened: opts.worsened,
            ..AuditAttribution::default()
        },
        dupe_demotion_diff_source: None,
        base_snapshot: None,
        comparison: None,
        base_snapshot_skipped: false,
        changed_files_count: 0,
        changed_files: Vec::new(),
        shallow_clone_without_base: base_ref::shallow_clone_base_is_head(opts, &base_ref),
        base_ref,
        base_description,
        base_sha,
        head_sha,
        output: opts.output,
        performance: opts.performance,
        check: None,
        dupes: None,
        health: None,
        elapsed,
        review_deltas: None,
        weakening_signals: Vec::new(),
        diff_risk: fallow_output::DiffRiskReport::default(),
        routing: None,
        ownership: None,
        decision_surface: None,
        graph_snapshot_hash,
        change_anchors: Vec::new(),
        diff_index: None,
    }
}

/// Run duplication analysis for the audit pipeline.
///
/// Reads the duplication settings of the duplication analysis of `run`, so
/// that user options like `ignoreImports`, `crossLanguage`, and `skipLocal`
/// are respected (same as combined mode).
fn run_audit_dupes<'a>(
    opts: &'a AuditOptions<'a>,
    run: &'a ResolvedRun,
    changed_since: Option<&'a str>,
    changed_files: Option<&'a FxHashSet<PathBuf>>,
    pre_discovered: Option<Vec<fallow_types::discover::DiscoveredFile>>,
) -> Result<Option<DupesResult>, ExitCode> {
    let Some(runnable) = runs::sub_analysis(run, PlannedAnalysis::Duplication, opts.analyses.dupes)
    else {
        return Ok(None);
    };
    let dupes_cfg = &runnable.config().duplicates;
    let mut dupes_opts = build_audit_dupes_options(opts, changed_since, changed_files, dupes_cfg);
    dupes_opts.production = *runnable.production().value();
    dupes_opts.production_override = Some(dupes_opts.production);
    dupes_opts.run = Some(runnable);
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
        json_style: opts.json_style,
        no_cache: opts.no_cache,
        threads: opts.threads,
        quiet: opts.quiet,
        allow_remote_extends: opts.allow_remote_extends,
        overrides: DupesOverrides {
            mode: Some(DupesMode::from(dupes_cfg.mode)),
            near: dupes_cfg.near,
            min_tokens: Some(dupes_cfg.min_tokens),
            min_lines: Some(dupes_cfg.min_lines),
            min_occurrences: Some(dupes_cfg.min_occurrences),
            threshold: Some(dupes_cfg.threshold),
            skip_local: dupes_cfg.skip_local,
            ignore_symlinks: Some(dupes_cfg.ignore_symlinks),
            cross_language: dupes_cfg.cross_language,
            ignore_imports: Some(dupes_cfg.ignore_imports),
        },
        top: None,
        baseline_path: opts.dupes_baseline,
        baseline_flag: "--dupes-baseline",
        save_baseline_path: None,
        // See the dead-code sub-pass: audit answers the flag once itself.
        fail_on_stale_baseline: false,
        // Audit ignores `--fail-on-issues`; its own verdict gates clone groups.
        fail_on_issues: false,
        production: false,
        production_override: None,
        run: None,
        trace: None,
        changed_since,
        diff_index: None,
        use_shared_diff_index: true,
        changed_files,
        // Audit compares head and base against its own changed files.
        change_scope_owner: fallow_engine::change_scope::ChangeScopeOwner::Caller,
        no_package_baselines: false,
        workspace: opts.workspace,
        changed_workspaces: opts.changed_workspaces,
        explain: opts.explain,
        explain_skipped: opts.explain_skipped,
        summary: false,
        group_by: opts.group_by,
        performance: false,
        include_fragments: true,
        retain_unfiltered_report: false,
        // Scope travels with the changed set (already intersected at the
        // audit prelude); the sub-passes stay unscoped.
        scope: None,
    }
}

/// Run complexity analysis for the audit pipeline (findings only, no scores/hotspots/targets).
///
/// `coverage_relocated` marks the base-worktree pass, whose Istanbul map was
/// recorded against the HEAD checkout; see `base_worktree_coverage_root`.
fn run_audit_health<'a>(
    opts: &'a AuditOptions<'a>,
    run: &'a ResolvedRun,
    changed_since: Option<&'a str>,
    shared_parse: Option<fallow_engine::health::HealthSharedParseData>,
    coverage_relocated: bool,
) -> Result<Option<HealthResult>, ExitCode> {
    let Some(runnable) = runs::sub_analysis(run, PlannedAnalysis::Health, opts.analyses.health)
    else {
        return Ok(None);
    };
    let runtime_coverage = match opts.runtime_coverage {
        Some(path) => Some(crate::health::coverage::prepare_options(
            path,
            opts.min_invocations_hot,
            None,
            None,
            opts.output,
        )?),
        None => None,
    };

    let mut health_opts =
        build_audit_health_options(opts, changed_since, runtime_coverage, coverage_relocated);
    health_opts.production = *runnable.production().value();
    health_opts.production_override = Some(health_opts.production);
    let health_run = if let Some(shared) = shared_parse {
        let run = Some(runnable);
        crate::health::execute_health_with_shared_parse_in(&health_opts, shared, None, run)
    } else {
        crate::health::execute_health_in(&health_opts, Some(runnable))
    };
    match health_run {
        Ok(mut r) => {
            // The standalone command says this at its own print site, which
            // audit never reaches, so an audit pointed at another command's
            // health baseline was the one of its three that stayed silent about
            // it. The dead-code and duplication notes come from the load sites
            // audit shares.
            crate::health::note_unrecognised_health_baseline(
                &mut r,
                opts.health_baseline,
                "--health-baseline",
            );
            Ok(Some(r))
        }
        Err(code) => Err(code),
    }
}

/// Build the findings-only `HealthOptions` for an audit run (no scores, hotspots,
/// ownership, or targets; `--churn-file` is health-only).
fn build_audit_health_options<'a>(
    opts: &'a AuditOptions<'a>,
    changed_since: Option<&'a str>,
    runtime_coverage: Option<fallow_engine::health::RuntimeCoverageOptions>,
    coverage_relocated: bool,
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
            max_file_lines: None,
        },
        top: None,
        sort: fallow_engine::health::HealthSort::Cyclomatic,
        production: false,
        production_override: None,
        allow_remote_extends: opts.allow_remote_extends,
        changed_since,
        diff_index: None,
        use_shared_diff_index: true,
        workspace: opts.workspace,
        changed_workspaces: opts.changed_workspaces,
        baseline: opts.health_baseline,
        save_baseline: None,
        baseline_mode: opts.health_baseline_mode,
        baseline_mode_explicit: false,
        complexity: true,
        file_scores: false,
        coverage_gaps: false,
        shallow_directories: false,
        config_activates_coverage_gaps: false,
        hotspots: false,
        ownership: false,
        ownership_emails: None,
        targets: false,
        // Styling analytics are in `fallow audit` next to dead code and complexity.
        // They read the changed files only, need the styling dependencies, and
        // never change the verdict.
        css: opts.css,
        css_deep: opts.css_deep,
        force_full: false,
        score_only_output: false,
        enforce_coverage_gap_gate: false,
        effort: None,
        score: false,
        // The flag reaches the health config here; the audit applies the
        // parse-error gate itself, so the health print never does.
        gates: fallow_engine::health::HealthGateOptions {
            fail_on_parse_error: opts.fail_on_parse_error,
            ..fallow_engine::health::HealthGateOptions::default()
        },
        since: None,
        min_commits: None,
        explain: opts.explain,
        summary: false,
        save_snapshot: None,
        trend: false,
        coverage_inputs: fallow_engine::health::HealthCoverageInputs {
            coverage: opts.coverage,
            coverage_root: opts.coverage_root,
            coverage_relocated,
        },
        performance: opts.performance,
        runtime_coverage,
        churn_file: None,
        analysis_identity: fallow_types::semantic::SemanticAnalysisIdentity::default(),
        complexity_breakdown: false,
        group_by: opts.group_by.map(Into::into),
        group_filter: None,
        trend_from: None,
        // Scope travels with the changed set (already intersected at the
        // audit prelude); the sub-passes stay unscoped.
        scope: None,
    }
}

#[path = "audit_impact_record.rs"]
mod impact_record;
#[path = "audit_output.rs"]
mod output;
use impact_record::record_audit_impact;

pub use output::audit_json_header_input;
pub use output::{
    insert_audit_dead_code_json, insert_audit_duplication_json, insert_audit_health_json,
    print_audit_findings, print_audit_result,
};

/// Run and print one audit with the resolved run `head`. `host_root` is the
/// checkout of `--staged` when `opts.root` is its temporary worktree,
/// otherwise `None`.
pub fn run_audit_with_type_aware(
    opts: &AuditOptions<'_>,
    head: &ResolvedRun,
    gate_marker: Option<&str>,
    type_aware: AuditTypeAwareOptions<'_>,
    host_root: Option<&Path>,
) -> ExitCode {
    let host_root = host_root.unwrap_or(opts.root);
    if let Err(e) = fallow_engine::health::validate_coverage_root_absolute(opts.coverage_root) {
        return crate::error::emit_input_error_with_style(&e, opts.output, opts.json_style);
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
        scope: opts.scope.clone(),
        ..*opts
    };
    match execute_audit_for_host(&resolved_opts, type_aware, host_root, head) {
        Ok(mut result) => {
            let _ = record_audit_impact(opts, host_root, gate_marker, &result);
            let report_exit = print_audit_command_result(opts, &mut result, opts.json_style);
            notes::print_run_notes(opts, type_aware, &result);
            if report_exit != ExitCode::SUCCESS {
                return report_exit;
            }
            ExitCode::from(crate::exit_codes::gate_failed_exit_code(
                fallow_output::GateName::TypeAwareRequire,
                audit_type_aware_completeness_failed(&result),
            ))
        }
        Err(code) => code,
    }
}

fn audit_type_aware_completeness_failed(result: &AuditResult) -> bool {
    type_aware_meta_completeness_failed(
        result
            .check
            .as_ref()
            .and_then(|check| check.type_aware_meta.as_ref()),
    )
}

fn type_aware_meta_completeness_failed(
    meta: Option<&fallow_types::envelope::TypeAwareMeta>,
) -> bool {
    crate::report::ci::required_type_aware_incomplete(meta)
}

fn print_audit_command_result(
    opts: &AuditOptions<'_>,
    result: &mut AuditResult,
    json_style: crate::json_style::JsonStyle,
) -> ExitCode {
    if opts.walkthrough_guide {
        return crate::audit_brief::print_walkthrough_guide_result(result, json_style);
    }
    if opts.walkthrough {
        return crate::audit_brief::print_walkthrough_human_result(
            result,
            opts.root,
            opts.cache_dir,
            opts.mark_viewed,
            opts.show_cleared,
            opts.quiet,
            json_style,
        );
    }
    if let Some(path) = opts.walkthrough_file {
        return crate::audit_brief::print_walkthrough_file_result(result, path, json_style);
    }
    if opts.brief {
        return crate::audit_brief::print_brief_result(
            result,
            result.diff_index.as_ref(),
            opts.quiet,
            opts.explain,
            opts.show_deprioritized,
            json_style,
        );
    }
    output::print_audit_report(opts, result, json_style)
}

/// Run the standalone `fallow decision-surface` command: the separable, cheap
/// apex. Executes the SAME changed-code analysis the review brief runs (it is
/// the brief path, NOT the full project pipeline), then emits ONLY the decision
/// surface envelope. Always exit 0 (the surface is advisory, never a gate).
///
/// The MCP `decision_surface` tool wraps this command. It is callable without the
/// full pipeline because it reuses `execute_audit` in brief mode (changed-code
/// scope), not bare `fallow`.
pub fn run_decision_surface(opts: &AuditOptions<'_>, head: &ResolvedRun) -> ExitCode {
    // Force brief mode: the decision surface is only computed on the brief path.
    let brief_opts = AuditOptions {
        brief: true,
        scope: opts.scope.clone(),
        ..*opts
    };
    let type_aware = AuditTypeAwareOptions::default();
    match execute_audit_for_host(&brief_opts, type_aware, opts.root, head) {
        Ok(result) => {
            crate::audit_brief::print_decision_surface_result(&result, opts.quiet, opts.json_style)
        }
        Err(code) => code,
    }
}

#[cfg(test)]
#[path = "audit_tests.rs"]
mod tests;
