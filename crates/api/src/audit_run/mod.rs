//! The changed-code audit.
//!
//! `fallow audit`, the MCP `audit` tool and [`crate::run_audit`] all run an
//! audit through [`run`], so they give the same introduced and inherited split
//! and the same verdict. A surface supplies an [`AuditBackend`]: it runs the
//! three analyses (dead code, duplication, health) and creates the base
//! checkout. This module owns the rest:
//!
//! - the base ref: explicit, then `FALLOW_AUDIT_BASE`, then auto-detection
//!   ([`resolve_audit_base`]),
//! - the check that lets the head run stand in for the base
//!   ([`can_reuse_current_as_base`]),
//! - rename detection, and the base focus set of changed files plus pre-rename
//!   paths,
//! - the base snapshot: checkout, analysis root, focus remap, dependency scope,
//!   and keys,
//! - the rename remap of base keys, the degraded type-aware comparison, the
//!   clone-group demotion, the verdict, and the introduced flags on the head
//!   findings.

mod base_files;
mod base_ref;
mod outcome;
mod scope;
mod snapshot;
#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};

use fallow_config::{AuditGate, ResolvedConfig, RulesConfig};
use fallow_engine::changed_files::RenamedFile;
use fallow_engine::repo_refs::{BaseAnalysisRoot, resolve_base_analysis_root};
use fallow_output::HealthReport;
use fallow_types::duplicates::CloneGroup;
use fallow_types::results::AnalysisResults;
use rustc_hash::FxHashSet;

pub use base_files::{BaseFileReader, BaseRead, can_reuse_current_as_base};
pub use base_ref::{
    AuditBaseError, AuditBaseOrigin, parse_audit_base_override, resolve_audit_base,
};
pub use outcome::{
    DupeDemotionDiffSource, SharedDiff, compare, demote_preexisting_dupe_introductions, outcome,
    styling_finding_gates, styling_rule_severity,
};
pub use scope::{
    AuditProductionFlags, BaseCoverageInputs, base_coverage_inputs, base_focus_files,
    remap_focus_files, renamed_files, scope_dependency_findings,
};
pub use snapshot::{
    AuditKeySnapshot, branching_keys, type_aware_attribution_degrade_reason,
    type_aware_degrade_warning, type_aware_gap_signature,
};

use crate::audit_keys::AuditComparison;
use crate::{AuditAttribution, AuditSummary, AuditVerdict};

/// The dead-code analysis of one audit side, as the audit reads it.
pub struct DeadCodeView<'a> {
    /// Findings after change scoping.
    pub results: &'a AnalysisResults,
    /// Config that decides the effective severity of each finding.
    pub config: &'a ResolvedConfig,
    /// Root that key paths are relative to.
    pub root: &'a Path,
    /// Type-aware metadata of the pass, when it ran type-aware analysis.
    pub type_aware: Option<&'a fallow_types::envelope::TypeAwareMeta>,
    /// Dead-code keys before type-aware refinement, when the surface captured
    /// them.
    pub syntactic_keys: Option<&'a FxHashSet<String>>,
    /// Exports-aware public-export keys, when the surface computed them.
    pub public_api: Option<&'a FxHashSet<String>>,
}

/// The duplication analysis of one audit side, as the audit reads it.
pub struct DuplicationView<'a> {
    /// Clone groups in output order.
    pub clone_groups: Vec<&'a CloneGroup>,
    /// Root that key paths are relative to.
    pub root: &'a Path,
    /// Duplicated share of the analyzed code, in percent.
    pub duplication_percentage: f64,
    /// Duplication percentage above which an introduced group fails the
    /// audit; `0.0` turns the threshold off.
    pub threshold: f64,
}

/// The health analysis of one audit side, as the audit reads it.
pub struct HealthView<'a> {
    /// Health report with complexity and styling findings.
    pub report: &'a HealthReport,
    /// Root that key paths are relative to.
    pub root: &'a Path,
    /// Rules that decide whether a styling finding gates the verdict.
    pub rules: &'a RulesConfig,
    /// Branching totals per file, when the surface computed them.
    pub branching: Option<&'a fallow_engine::health::BranchingByFile>,
}

/// The analyses of one audit side. An analysis the surface did not run is
/// `None`.
#[derive(Default)]
pub struct AuditAnalysesView<'a> {
    /// Dead-code analysis.
    pub dead_code: Option<DeadCodeView<'a>>,
    /// Duplication analysis.
    pub duplication: Option<DuplicationView<'a>>,
    /// Health analysis.
    pub health: Option<HealthView<'a>>,
}

/// The analyses a surface ran for one audit side.
pub trait AuditAnalyses {
    /// A read view of the analyses.
    fn view(&self) -> AuditAnalysesView<'_>;
    /// The dead-code findings, for the dependency scope and the introduced
    /// flags.
    fn dead_code_results_mut(&mut self) -> Option<&mut AnalysisResults>;
    /// The health report, for the introduced flags.
    fn health_report_mut(&mut self) -> Option<&mut HealthReport>;
    /// Record the warning of a degraded type-aware comparison on the
    /// type-aware metadata of the run.
    fn record_type_aware_warning(&mut self, warning: &str);
}

/// A checkout of the base commit.
pub trait BaseCheckout {
    /// Root of the checkout.
    fn path(&self) -> &Path;
}

impl BaseCheckout for fallow_engine::repo_refs::TemporaryBaseWorktree {
    fn path(&self) -> &Path {
        Self::path(self)
    }
}

/// How one surface runs the analyses of an audit.
pub trait AuditBackend: Sync {
    /// The analyses of one side.
    type Analyses: AuditAnalyses + Send;
    /// A checkout of the base commit. The base pass keeps it until the base
    /// analyses complete.
    type Checkout: BaseCheckout;
    /// Key of a cached base snapshot.
    type CacheKey: Sync;
    /// The error of the surface.
    type Error: Send;

    /// Called once when the run has changed files, before any base work.
    fn prepare(&self) {}

    /// Run the head analyses, scoped to `changed_files`.
    ///
    /// # Errors
    ///
    /// Returns the surface error when an analysis fails.
    fn run_head(&self, changed_files: &FxHashSet<PathBuf>) -> Result<Self::Analyses, Self::Error>;

    /// Create a checkout of `base_ref`. `base_sha` is the full SHA when a
    /// cache key resolved it.
    ///
    /// # Errors
    ///
    /// Returns the surface error when the checkout cannot be created.
    fn create_base_checkout(
        &self,
        base_ref: &str,
        base_sha: Option<&str>,
    ) -> Result<Self::Checkout, Self::Error>;

    /// Run the base analyses in `base_root`. With `focus`, scope the findings
    /// to those files; without it, leave them unscoped.
    ///
    /// # Errors
    ///
    /// Returns the surface error when an analysis fails.
    fn run_base(
        &self,
        base_root: &Path,
        focus: Option<&FxHashSet<PathBuf>>,
    ) -> Result<Self::Analyses, Self::Error>;

    /// The cache key of the base snapshot for `base_ref` and `focus`, or
    /// `None` when the surface keeps no cache.
    ///
    /// # Errors
    ///
    /// Returns the surface error when the key inputs cannot be read.
    fn base_cache_key(
        &self,
        _base_ref: &str,
        _focus: &FxHashSet<PathBuf>,
    ) -> Result<Option<Self::CacheKey>, Self::Error> {
        Ok(None)
    }

    /// The full base SHA that `key` records.
    fn cached_base_sha<'k>(&self, _key: &'k Self::CacheKey) -> Option<&'k str> {
        None
    }

    /// A cached base snapshot for `key`.
    fn load_cached_base(&self, _key: &Self::CacheKey) -> Option<AuditKeySnapshot> {
        None
    }

    /// Store a fresh base snapshot under `key`.
    fn save_cached_base(&self, _key: &Self::CacheKey, _snapshot: &AuditKeySnapshot) {}

    /// The opt-in shared diff of the run, when one is active.
    fn shared_diff(&self) -> Option<SharedDiff<'_>> {
        None
    }
}

/// Inputs of one audit run.
pub struct AuditRunInput<'a> {
    /// Head analysis root.
    pub root: &'a Path,
    /// Gate mode. `new-only` compares with the base snapshot.
    pub gate: AuditGate,
    /// Resolved base ref.
    pub base_ref: &'a str,
    /// Cache directory of the run. A changed file inside it never blocks the
    /// reuse of the head run as the base snapshot.
    pub cache_dir: Option<&'a Path>,
    /// Changed files of the run, after any narrowing of the surface.
    pub changed_files: FxHashSet<PathBuf>,
}

/// The base snapshot that attribution compares with.
#[derive(Debug, Default)]
pub struct AuditBase {
    /// The snapshot; `None` under `--gate all`.
    pub snapshot: Option<AuditKeySnapshot>,
    /// `true` when the head keys stand in for the base (no finding can have
    /// changed), so every finding is inherited.
    pub skipped: bool,
}

/// Inputs of [`attribute`].
pub struct AuditAttributionInput<'a> {
    /// Head analysis root.
    pub root: &'a Path,
    /// Gate mode.
    pub gate: AuditGate,
    /// Resolved base ref, for the demotion diff.
    pub base_ref: &'a str,
    /// The base snapshot.
    pub base: AuditBase,
    /// Renames between base and head.
    pub renames: &'a [RenamedFile],
    /// The opt-in shared diff of the run.
    pub shared_diff: Option<SharedDiff<'a>>,
}

/// Attribution result of one audit run.
#[derive(Debug)]
pub struct AuditOutcome {
    /// Overall verdict.
    pub verdict: AuditVerdict,
    /// Per-domain counts.
    pub summary: AuditSummary,
    /// Introduced and inherited counts, and the gate.
    pub attribution: AuditAttribution,
    /// The classification of every head finding.
    pub comparison: AuditComparison,
    /// The base snapshot after the rename remap.
    pub base_snapshot: Option<AuditKeySnapshot>,
    /// `true` when the head keys stood in for the base.
    pub base_snapshot_skipped: bool,
    /// Which diff decided the clone-group demotion; `None` when it did not
    /// run.
    pub dupe_demotion_diff_source: Option<DupeDemotionDiffSource>,
    /// The warning of a degraded type-aware comparison, already recorded on
    /// the head analyses.
    pub type_aware_degrade_warning: Option<String>,
}

/// The base snapshot of the typed audit output. When the head keys stood in
/// for the base, the head keys are the base snapshot, so this keeps them.
pub(crate) fn programmatic_base_snapshot(
    outcome: &AuditOutcome,
) -> Option<crate::AuditProgrammaticKeySnapshot> {
    outcome
        .base_snapshot
        .as_ref()
        .map(AuditKeySnapshot::to_programmatic)
}

/// One completed audit run.
pub struct AuditRun<A> {
    /// Head analyses, with introduced flags on dead-code and health findings
    /// when a base snapshot exists.
    pub analyses: A,
    /// Changed files of the run.
    pub changed_files: FxHashSet<PathBuf>,
    /// Attribution result.
    pub outcome: AuditOutcome,
}

/// Run one audit. Returns `None` when the run has no changed files.
///
/// # Errors
///
/// Returns the error of the backend when an analysis or the base checkout
/// fails.
pub fn run<B: AuditBackend>(
    backend: &B,
    input: AuditRunInput<'_>,
) -> Result<Option<AuditRun<B::Analyses>>, B::Error> {
    let AuditRunInput {
        root,
        gate,
        base_ref,
        cache_dir,
        changed_files,
    } = input;
    if changed_files.is_empty() {
        return Ok(None);
    }
    backend.prepare();

    let needs_real_base = matches!(gate, AuditGate::NewOnly)
        && !can_reuse_current_as_base(root, cache_dir, base_ref, &changed_files);
    let renames = if needs_real_base {
        renamed_files(root, base_ref)
    } else {
        Vec::new()
    };
    let focus = base_focus_files(&changed_files, &renames);
    let cache_key = if needs_real_base {
        backend.base_cache_key(base_ref, &focus)?
    } else {
        None
    };
    let cached = cache_key
        .as_ref()
        .and_then(|key| backend.load_cached_base(key));

    let (head, fresh_base) = if needs_real_base && cached.is_none() {
        let base_sha = cache_key
            .as_ref()
            .and_then(|key| backend.cached_base_sha(key));
        let (head, base) = rayon::join(
            || backend.run_head(&changed_files),
            || base_snapshot(backend, root, base_ref, &focus, base_sha),
        );
        (head, Some(base))
    } else {
        (backend.run_head(&changed_files), None)
    };
    let mut analyses = head?;
    scope_dependency_findings_of(&mut analyses, &changed_files);

    let base = if !matches!(gate, AuditGate::NewOnly) {
        AuditBase::default()
    } else if let Some(snapshot) = cached {
        AuditBase {
            snapshot: Some(snapshot),
            skipped: false,
        }
    } else if let Some(fresh) = fresh_base {
        let snapshot = fresh?;
        if let Some(key) = cache_key.as_ref() {
            backend.save_cached_base(key, &snapshot);
        }
        AuditBase {
            snapshot: Some(snapshot),
            skipped: false,
        }
    } else {
        AuditBase {
            snapshot: Some(AuditKeySnapshot::from_view(&analyses.view())),
            skipped: true,
        }
    };

    let outcome = attribute(
        &mut analyses,
        AuditAttributionInput {
            root,
            gate,
            base_ref,
            base,
            renames: &renames,
            shared_diff: backend.shared_diff(),
        },
    );
    Ok(Some(AuditRun {
        analyses,
        changed_files,
        outcome,
    }))
}

/// Compare head analyses with a base snapshot, decide the verdict, and set
/// the introduced flags on the dead-code and health findings.
pub fn attribute<A: AuditAnalyses>(
    analyses: &mut A,
    input: AuditAttributionInput<'_>,
) -> AuditOutcome {
    let AuditAttributionInput {
        root,
        gate,
        base_ref,
        base,
        renames,
        shared_diff,
    } = input;
    let AuditBase {
        snapshot: mut base_snapshot,
        skipped,
    } = base;
    // The head keys that stand in for a skipped base are head paths already.
    if !skipped && let Some(snapshot) = base_snapshot.as_mut() {
        snapshot.remap_for_renames(renames, root);
    }
    let degrade_reason = {
        let view = analyses.view();
        type_aware_attribution_degrade_reason(
            base_snapshot.as_ref(),
            view.dead_code
                .as_ref()
                .and_then(|dead_code| dead_code.type_aware),
        )
    };
    let type_aware_degrade_warning = degrade_reason.map(type_aware_degrade_warning);
    if let Some(warning) = type_aware_degrade_warning.as_deref() {
        analyses.record_type_aware_warning(warning);
    }

    let (comparison, dupe_demotion_diff_source, (attribution, verdict, summary)) = {
        let view = analyses.view();
        let mut comparison = compare(&view, base_snapshot.as_ref(), degrade_reason.is_some());
        let source = demote_preexisting_dupe_introductions(
            &mut comparison,
            &view,
            root,
            base_ref,
            shared_diff,
        );
        let result = outcome(gate, &view, &comparison, base_snapshot.is_some());
        (comparison, source, result)
    };

    if base_snapshot.is_some() {
        if let Some(results) = analyses.dead_code_results_mut() {
            comparison.dead_code.annotate_results(results);
        }
        if let Some(report) = analyses.health_report_mut() {
            for (finding, introduced) in report
                .findings
                .iter_mut()
                .zip(comparison.health.introduced())
            {
                finding.introduced = Some(introduced);
            }
        }
    }

    AuditOutcome {
        verdict,
        summary,
        attribution,
        comparison,
        base_snapshot,
        base_snapshot_skipped: skipped,
        dupe_demotion_diff_source,
        type_aware_degrade_warning,
    }
}

/// Analyze the base checkout and take its attribution keys.
fn base_snapshot<B: AuditBackend>(
    backend: &B,
    root: &Path,
    base_ref: &str,
    focus: &FxHashSet<PathBuf>,
    base_sha: Option<&str>,
) -> Result<AuditKeySnapshot, B::Error> {
    let checkout = backend.create_base_checkout(base_ref, base_sha)?;
    let base_root = match resolve_base_analysis_root(root, checkout.path()) {
        // The canonical spelling: an analysis session canonicalizes its root,
        // so a focus set spelled through a symbolic link (the macOS temporary
        // directory) would match no finding and empty the base snapshot.
        BaseAnalysisRoot::Present(base_root) => {
            dunce::canonicalize(&base_root).unwrap_or(base_root)
        }
        // A root that the base commit does not contain (a package added on
        // the branch) has an empty base snapshot, so every finding under it
        // is introduced.
        BaseAnalysisRoot::NewInHead(_) => return Ok(AuditKeySnapshot::default()),
    };
    let base_focus = remap_focus_files(focus, root, &base_root);
    let mut base = backend.run_base(&base_root, base_focus.as_ref())?;
    if let Some(focus) = base_focus.as_ref() {
        scope_dependency_findings_of(&mut base, focus);
    }
    let snapshot = AuditKeySnapshot::from_view(&base.view());
    drop(checkout);
    Ok(snapshot)
}

/// Apply [`scope_dependency_findings`] to the dead-code findings of one side.
fn scope_dependency_findings_of<A: AuditAnalyses>(
    analyses: &mut A,
    changed_files: &FxHashSet<PathBuf>,
) {
    let Some(root) = analyses
        .view()
        .dead_code
        .map(|dead_code| dead_code.root.to_path_buf())
    else {
        return;
    };
    if let Some(results) = analyses.dead_code_results_mut() {
        scope_dependency_findings(results, &root, changed_files);
    }
}
