//! Command-neutral health execution options and runners.

use std::path::{Path, PathBuf};

use fallow_config::{EmailMode, WorkspaceInfo};
use fallow_output::{
    DiffIndex, EffortEstimate, FindingSeverity, GroupByMode, RuntimeCoverageReport,
    RuntimeCoverageWatermark,
};
use fallow_types::output_format::OutputFormat;
use fallow_types::path_util::is_absolute_path_any_platform;
use fallow_types::results::AnalysisResults;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::module_graph::RetainedModuleGraph;
use crate::results::DeadCodeAnalysisArtifacts;

mod actions;
mod analysis_data;
mod assembly;
mod baseline_io;
mod churn_file;
mod component_rollup;
mod core_pipeline;
mod coverage_gaps;
mod coverage_intelligence;
mod coverage_settings;
mod css_analytics;
mod derived_sections;
mod execute;
mod file_scores;
mod filters;
mod finding_sort;
mod findings;
mod findings_pipeline;
mod framework_health;
mod grouping;
mod health_error;
mod hotspots;
mod ignore;
mod large_functions;
mod output_build;
pub mod ownership;
mod package_json;
mod pipeline;
mod react_hooks;
mod result;
mod runner;
mod runtime_filter;
mod runtime_sections;
mod scope;
/// File health scoring: maintainability index, CRAP risk, triage concern
/// classification, and Istanbul coverage ingestion.
pub mod scoring;
pub mod styling_score;
mod tailwind_theme;
mod targets;
mod threshold_overrides;
mod timings;
mod vital_data;
mod vital_signs_scope;

pub use crate::results::HealthAnalysisResult;
pub use churn_file::validate_health_churn_file;
pub use css_analytics::StylingAnalysisArtifacts;
use derived_sections::{
    HealthDerivedSectionInput, HealthDerivedSections, prepare_health_derived_sections,
};
use execute::HealthOptions;
pub use execute::execute_health_inner;
use file_scores::{
    FileScoresAndChurnInput, compute_file_scores_and_churn, health_file_scores_slice,
    print_slow_churn_note,
};
use finding_sort::sort_findings;
pub use health_error::HealthError;
pub use hotspots::{
    TargetChurnEvidence, TargetChurnOptions, TargetChurnOutcome, analyze_target_churn,
};
pub use pipeline::{HealthPipelineInputs, HealthScopeInputs};
pub use runner::{
    PreparedUngroupedHealth, prepare_ungrouped_health, run_prepared_ungrouped_health,
    run_ungrouped_health, run_ungrouped_health_with_session,
    run_ungrouped_health_with_session_artifacts,
};
use vital_data::{HealthVitalData, HealthVitalDataInput, prepare_health_vital_data};
use vital_signs_scope::{
    SubsetFilter, VitalSignsAndCountsInput, apply_duplication_metrics,
    compute_vital_signs_and_counts,
};

pub(crate) fn build_styling_analysis_artifacts(
    files: &[crate::discover::DiscoveredFile],
    config: &fallow_config::ResolvedConfig,
) -> StylingAnalysisArtifacts {
    css_analytics::build_styling_analysis_artifacts(files, config)
}

/// Build health shared parse data from retained dead-code artifacts.
#[must_use]
pub fn shared_parse_data_from_artifacts(
    results: &AnalysisResults,
    graph: Option<RetainedModuleGraph>,
    modules: Option<Vec<crate::source::ModuleInfo>>,
    files: Option<Vec<crate::discover::DiscoveredFile>>,
    workspaces: Vec<WorkspaceInfo>,
    script_used_packages: impl IntoIterator<Item = String>,
) -> Option<HealthSharedParseData> {
    let (Some(modules), Some(files)) = (modules, files) else {
        return None;
    };
    let script_used_packages: FxHashSet<String> = script_used_packages.into_iter().collect();
    let analysis_output = graph.map(|graph| DeadCodeAnalysisArtifacts {
        results: results.clone(),
        timings: None,
        graph: Some(graph),
        modules: None,
        files: None,
        script_used_packages: script_used_packages.clone(),
        file_hashes: FxHashMap::default(),
    });
    Some(HealthSharedParseData {
        files,
        modules,
        dead_code_results: Some(results.clone()),
        workspaces,
        analysis_output,
    })
}

/// Return true when health sections will need dead-code analysis artifacts.
///
/// Callers that already have a session and parsed modules can precompute these
/// artifacts once, then pass them into [`HealthPipelineInputs`] to avoid a
/// second graph and dead-code analysis inside the health pipeline.
#[must_use]
pub fn should_precompute_dead_code_analysis(
    options: &HealthExecutionOptions<'_>,
    config: &fallow_config::ResolvedConfig,
) -> bool {
    let max_crap = options
        .thresholds
        .max_crap
        .unwrap_or(config.health.max_crap);
    options.file_scores
        || options.coverage_gaps
        || options.config_activates_coverage_gaps
        || options.hotspots
        || options.targets
        || options.force_full
        || max_crap > 0.0
        || options.runtime_coverage.is_some()
}

/// Command-neutral grouping resolver contract for `--group-by` health output.
///
/// The CLI owns the concrete resolver (CODEOWNERS parsing, package discovery);
/// the engine grouping pass only needs these three read operations, so it stays
/// generic over the resolver instead of depending on the CLI type.
pub trait HealthGroupResolver {
    /// Stable label for the active grouping mode (`owner` / `directory` / ...).
    fn mode_label(&self) -> &'static str;
    /// Resolve a repo-relative path to its group key and the matching rule.
    fn resolve_with_rule(&self, rel_path: &Path) -> (String, Option<String>);
    /// Section owners for the group a path belongs to, when known.
    fn section_owners_of(&self, rel_path: &Path) -> Option<&[String]>;
}

/// Placeholder grouping resolver for runs without `--group-by` (the programmatic
/// API path). Constructed only as `None`, so its methods are never invoked.
#[derive(Debug, Clone, Copy)]
pub enum NoGroupResolver {}

#[expect(
    clippy::uninhabited_references,
    reason = "NoGroupResolver is uninhabited; these methods are unreachable and exist only to satisfy the trait bound for the group-less programmatic path"
)]
impl HealthGroupResolver for NoGroupResolver {
    fn mode_label(&self) -> &'static str {
        match *self {}
    }
    fn resolve_with_rule(&self, _rel_path: &Path) -> (String, Option<String>) {
        match *self {}
    }
    fn section_owners_of(&self, _rel_path: &Path) -> Option<&[String]> {
        match *self {}
    }
}

/// Runtime coverage analysis seam.
///
/// Runtime coverage execution drives the closed-source `fallow-cov` sidecar
/// (license verification, subprocess spawning), which stays in the CLI. The
/// engine calls this callback only when [`HealthExecutionOptions::runtime_coverage`]
/// is set, so the default and programmatic paths never touch it.
///
/// The seam prints its own errors (license / sidecar diagnostics), so it returns
/// the already-printed exit code as a bare `u8`. The engine wraps that code in
/// [`HealthError::Printed`] so the CLI boundary honors the code without emitting
/// a second error document.
pub type RuntimeCoverageAnalyzer<'a> = dyn Fn(&RuntimeCoverageOptions, RuntimeCoverageSeamInput<'_>) -> Result<RuntimeCoverageReport, u8>
    + 'a;

/// Inputs the runtime coverage seam needs from the analysis core.
pub struct RuntimeCoverageSeamInput<'a> {
    /// Project root the analysis ran against.
    pub root: &'a Path,
    /// Parsed modules from the extract phase, for correlating trace symbols.
    pub modules: &'a [fallow_types::extract::ModuleInfo],
    /// Retained dead-code artifacts (graph plus results) the sidecar joins
    /// runtime traces against.
    pub analysis_output: &'a DeadCodeAnalysisArtifacts,
    /// Parsed Istanbul test coverage when the run also supplied it.
    pub istanbul_coverage: Option<&'a scoring::IstanbulCoverage>,
    /// `FileId` to absolute-path lookup for resolving finding locations.
    pub file_paths: &'a rustc_hash::FxHashMap<fallow_types::discover::FileId, &'a PathBuf>,
    /// Compiled ignore globs; matching files are excluded from verdicts.
    pub ignore_set: &'a globset::GlobSet,
    /// Diff scope when the run is limited to changed files.
    pub changed_files: Option<&'a rustc_hash::FxHashSet<PathBuf>>,
    /// Workspace roots when the run is workspace-scoped.
    pub ws_roots: Option<&'a [PathBuf]>,
    /// Cap on rendered findings, forwarded from `--top`.
    pub top: Option<usize>,
    /// CODEOWNERS override path for ownership attribution on findings.
    pub codeowners_path: Option<&'a str>,
    /// Suppress progress notes on stderr.
    pub quiet: bool,
    /// Output format the seam should render its own diagnostics in.
    pub output: OutputFormat,
}

/// CLI-supplied callbacks the command-neutral health pipeline needs.
///
/// The pipeline itself stays cli-free; these are the seams the CLI threads in.
pub struct HealthSeams<'a> {
    /// Runs the runtime coverage sidecar (only when runtime coverage is set).
    pub runtime_coverage_analyzer: &'a RuntimeCoverageAnalyzer<'a>,
    /// Records module-graph structure facts (graph node count, edge count) into
    /// the CLI's process-global telemetry sinks. Best-effort; the engine never
    /// owns telemetry state.
    pub note_graph_structure: &'a dyn Fn(usize, usize),
}

/// Command-neutral sort criteria for health complexity findings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthSort {
    /// Worst first: exceeded-threshold class, then severity, CRAP presence,
    /// and raw complexity metrics as tie-breakers.
    Severity,
    /// Descending cyclomatic complexity.
    Cyclomatic,
    /// Descending cognitive complexity.
    Cognitive,
    /// Descending function line count.
    Lines,
}

/// Command-neutral threshold overrides for health complexity findings.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct HealthThresholdOverrides {
    /// Overrides the configured maximum cyclomatic complexity threshold.
    pub max_cyclomatic: Option<u16>,
    /// Overrides the configured maximum cognitive complexity threshold.
    pub max_cognitive: Option<u16>,
    /// Maximum CRAP score threshold. Functions meeting or exceeding this score
    /// are reported as complexity findings.
    pub max_crap: Option<f64>,
}

/// Command-neutral Istanbul coverage inputs for health CRAP scoring.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HealthCoverageInputs<'a> {
    /// Path to an Istanbul `coverage-final.json` used for CRAP scoring.
    pub coverage: Option<&'a Path>,
    /// Absolute coverage-path prefix to strip before rebasing files onto the
    /// project root.
    pub coverage_root: Option<&'a Path>,
    /// The coverage map was recorded against a different checkout of this
    /// project (the audit base-worktree pass), so function line numbers may
    /// have drifted arbitrarily. Enables the distance-free unambiguous-name
    /// match in the Istanbul lookup; keep `false` for same-checkout coverage,
    /// where the bounded fuzz protects against stale data (#2347).
    pub coverage_relocated: bool,
}

/// Validate that a coverage-data root is absolute under Unix or Windows path
/// conventions.
///
/// Istanbul coverage paths often come from a Linux CI runner even when fallow
/// is invoked on another host, so POSIX-rooted paths and Windows drive paths
/// are both accepted on every platform.
pub fn validate_coverage_root_absolute(coverage_root: Option<&Path>) -> Result<(), String> {
    if let Some(path) = coverage_root
        && !is_absolute_path_any_platform(path)
    {
        return Err(format!(
            "--coverage-root expects an absolute path prefix from the coverage data, got '{}'. Use the checkout prefix from the machine that generated coverage, for example '/home/runner/work/myapp'.",
            path.display()
        ));
    }
    Ok(())
}

/// Command-neutral health exit gate options.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct HealthGateOptions {
    /// Fail the run when the health score (0-100) falls below this value.
    pub min_score: Option<f64>,
    /// Fail the run when any finding at or above this severity exists.
    pub min_severity: Option<FindingSeverity>,
    /// Render the score and findings but never fail CI on a health gate.
    pub report_only: bool,
}

/// Input for deriving effective health sections from command-neutral flags.
#[derive(Debug, Clone)]
pub struct HealthSectionOptions {
    output: OutputFormat,
    complexity: bool,
    file_scores: bool,
    coverage_gaps: bool,
    hotspots: bool,
    targets: bool,
    css: bool,
    score: bool,
    score_gate: bool,
    snapshot_requested: bool,
    trend: bool,
}

/// Derived section selection for health runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DerivedHealthSections {
    /// True when at least one section was explicitly requested; a request with
    /// no explicit sections defaults to the full section set.
    pub any_section: bool,
    /// Render the complexity findings section.
    pub complexity: bool,
    /// Render the per-file health scores section.
    pub file_scores: bool,
    /// Render the static coverage gaps section.
    pub coverage_gaps: bool,
    /// Render the churn-based hotspots section.
    pub hotspots: bool,
    /// Render the refactoring targets section.
    pub targets: bool,
    /// Render the CSS / styling analytics section.
    pub css: bool,
    /// Compute and render the overall health score.
    pub score: bool,
    /// Analyze the full project even when only a subset of sections was
    /// requested, because scores and snapshots need complete data.
    pub force_full: bool,
    /// True when the score is the only requested surface, so output can skip
    /// section rendering entirely.
    pub score_only_output: bool,
}

/// Command-neutral inputs used to normalize a health run before it reaches a
/// concrete runner.
#[derive(Debug, Clone)]
pub struct HealthRunOptionsInput<'a> {
    /// Output format; badge output implies the score section.
    pub output: OutputFormat,
    /// Complexity threshold overrides on top of the resolved config.
    pub thresholds: HealthThresholdOverrides,
    /// Cap on rendered findings per section.
    pub top: Option<usize>,
    /// Sort criteria for complexity findings.
    pub sort: HealthSort,
    /// Explicit request for the complexity findings section.
    pub complexity: bool,
    /// Explicit request for the per-file health scores section.
    pub file_scores: bool,
    /// Explicit request for the static coverage gaps section.
    pub coverage_gaps: bool,
    /// Explicit request for the churn-based hotspots section.
    pub hotspots: bool,
    /// Attribute hotspots to owners (implies the hotspots section data).
    pub ownership: bool,
    /// How owner identities are rendered (names or emails).
    pub ownership_emails: Option<EmailMode>,
    /// Explicit request for the refactoring targets section.
    pub targets: bool,
    /// Explicit request for the CSS / styling analytics section.
    pub css: bool,
    /// Effort estimation mode; requesting it also enables the targets section.
    pub effort: Option<EffortEstimate>,
    /// Explicit request for the overall health score.
    pub score: bool,
    /// Exit gate thresholds; a score gate implies the score section.
    pub gates: HealthGateOptions,
    /// True when the run should persist a health snapshot (forces a full run).
    pub snapshot_requested: bool,
    /// Explicit request for the score trend section (implies score).
    pub trend: bool,
    /// Churn lookback window for hotspots (`90d`, `6m`, `1y`, or an ISO date).
    pub since: Option<&'a str>,
    /// Minimum commit count for a file to qualify as churn evidence.
    pub min_commits: Option<u32>,
    /// Istanbul coverage inputs for CRAP scoring.
    pub coverage_inputs: HealthCoverageInputs<'a>,
    /// Runtime coverage sidecar options, when runtime analysis was requested.
    pub runtime_coverage: Option<RuntimeCoverageOptions>,
}

/// Normalized health inputs shared by CLI, API, NAPI, and future runners.
#[derive(Debug, Clone)]
pub struct HealthRunOptions<'a> {
    /// Complexity threshold overrides on top of the resolved config.
    pub thresholds: HealthThresholdOverrides,
    /// Cap on rendered findings per section.
    pub top: Option<usize>,
    /// Sort criteria for complexity findings.
    pub sort: HealthSort,
    /// Effective section selection derived from the raw request flags.
    pub sections: DerivedHealthSections,
    /// Attribute hotspots to owners; already gated on the hotspots section.
    pub ownership: bool,
    /// How owner identities are rendered (names or emails).
    pub ownership_emails: Option<EmailMode>,
    /// Effort estimation mode for refactoring targets.
    pub effort: Option<EffortEstimate>,
    /// Exit gate thresholds.
    pub gates: HealthGateOptions,
    /// Churn lookback window for hotspots (`90d`, `6m`, `1y`, or an ISO date).
    pub since: Option<&'a str>,
    /// Minimum commit count for a file to qualify as churn evidence.
    pub min_commits: Option<u32>,
    /// Istanbul coverage inputs for CRAP scoring.
    pub coverage_inputs: HealthCoverageInputs<'a>,
    /// Runtime coverage sidecar options, when runtime analysis was requested.
    pub runtime_coverage: Option<RuntimeCoverageOptions>,
}

/// Command-neutral inputs needed to execute a health analysis.
///
/// These fields are shared runner inputs rather than rendering concerns.
#[derive(Debug, Clone)]
pub struct HealthExecutionOptions<'a> {
    /// Project root to analyze.
    pub root: &'a Path,
    /// Explicit config file path; `None` triggers automatic discovery.
    pub config_path: &'a Option<PathBuf>,
    /// Output format of the run; badge output implies the score section.
    pub output: OutputFormat,
    /// Bypass the parse cache for this run.
    pub no_cache: bool,
    /// Worker thread count for parsing and analysis.
    pub threads: usize,
    /// Suppress progress notes on stderr.
    pub quiet: bool,
    /// Include per-decision-point complexity contributions in typed findings.
    ///
    /// This changes the produced health result shape, so it belongs to the
    /// runner input contract rather than CLI rendering options.
    pub complexity_breakdown: bool,
    /// Complexity threshold overrides on top of the resolved config.
    pub thresholds: HealthThresholdOverrides,
    /// Cap on rendered findings per section.
    pub top: Option<usize>,
    /// Sort criteria for complexity findings.
    pub sort: HealthSort,
    /// Raw production-only request flag; folded into `production_override`
    /// when the tri-state override is unset.
    pub production: bool,
    /// Tri-state production override: `Some` forces production-only analysis
    /// on or off regardless of config, `None` defers to the config value.
    pub production_override: Option<bool>,
    /// Permit `extends` config inheritance from remote URLs.
    pub allow_remote_extends: bool,
    /// Git ref limiting findings to files changed since it.
    pub changed_since: Option<&'a str>,
    /// Pre-built diff index scoping findings to changed lines.
    pub diff_index: Option<&'a DiffIndex>,
    /// True when `diff_index` came from the process-shared diff source rather
    /// than a health-specific one.
    pub use_shared_diff_index: bool,
    /// Workspace member paths limiting the analysis scope.
    pub workspace: Option<&'a [String]>,
    /// Git ref selecting only workspaces with changes since it.
    pub changed_workspaces: Option<&'a str>,
    /// Baseline file to compare finding counts against.
    pub baseline: Option<&'a Path>,
    /// Path to write the run's finding counts as a new baseline.
    pub save_baseline: Option<&'a Path>,
    /// Controls both halves of the baseline lifecycle: which buckets
    /// `save_baseline` writes and how a loaded `baseline` is matched. An
    /// identity save writes count and identity buckets, a count save writes
    /// count buckets only.
    pub baseline_mode: crate::baseline::HealthBaselineMode,
    /// Whether `baseline_mode` was requested explicitly rather than defaulted.
    /// A defaulted count save refuses to overwrite a baseline that carries
    /// identity buckets, because dropping them breaks later identity-mode
    /// comparisons; an explicit count request is treated as intent to
    /// downgrade.
    pub baseline_mode_explicit: bool,
    /// Render the complexity findings section.
    pub complexity: bool,
    /// Render the per-file health scores section.
    pub file_scores: bool,
    /// Render the static coverage gaps section.
    pub coverage_gaps: bool,
    /// Let config-enabled coverage settings activate the coverage gaps
    /// section even when it was not requested on this run.
    pub config_activates_coverage_gaps: bool,
    /// Render the churn-based hotspots section.
    pub hotspots: bool,
    /// Attribute hotspots to owners.
    pub ownership: bool,
    /// How owner identities are rendered (names or emails).
    pub ownership_emails: Option<EmailMode>,
    /// Render the refactoring targets section.
    pub targets: bool,
    /// Render the CSS / styling analytics section.
    pub css: bool,
    /// Scan all stylesheets for the CSS section instead of only changed files.
    pub css_deep: bool,
    /// Analyze the full project even when only a subset of sections was
    /// requested, because scores and snapshots need complete data.
    pub force_full: bool,
    /// True when the score is the only requested surface, so output can skip
    /// section rendering entirely.
    pub score_only_output: bool,
    /// Fail the run on coverage gaps instead of reporting them advisorily.
    pub enforce_coverage_gap_gate: bool,
    /// Effort estimation mode for refactoring targets.
    pub effort: Option<EffortEstimate>,
    /// Compute and render the overall health score.
    pub score: bool,
    /// Exit gate thresholds.
    pub gates: HealthGateOptions,
    /// Churn lookback window for hotspots (`90d`, `6m`, `1y`, or an ISO date).
    pub since: Option<&'a str>,
    /// Minimum commit count for a file to qualify as churn evidence.
    pub min_commits: Option<u32>,
    /// Include score-derivation explanations in the rendered output.
    pub explain: bool,
    /// Render the condensed summary view instead of full sections.
    pub summary: bool,
    /// Path to persist a health snapshot for later trend comparison.
    pub save_snapshot: Option<PathBuf>,
    /// Render the score trend against previously saved snapshots.
    pub trend: bool,
    /// Istanbul coverage inputs for CRAP scoring.
    pub coverage_inputs: HealthCoverageInputs<'a>,
    /// Print per-phase timing diagnostics.
    pub performance: bool,
    /// Runtime coverage sidecar options, when runtime analysis was requested.
    pub runtime_coverage: Option<RuntimeCoverageOptions>,
    /// Pre-recorded churn data file replacing live `git log` analysis.
    pub churn_file: Option<&'a Path>,
    /// Compatibility identity persisted with snapshots and checked by trends.
    pub analysis_identity: fallow_types::semantic::SemanticAnalysisIdentity,
    /// Optional grouping mode for typed health output.
    pub group_by: Option<GroupByMode>,
}

/// Derive effective health section flags for CLI and embedders.
#[must_use]
fn derive_health_sections(options: &HealthSectionOptions) -> DerivedHealthSections {
    let score = options.score
        || options.score_gate
        || options.trend
        || matches!(options.output, OutputFormat::Badge);
    let any_section = options.complexity
        || options.file_scores
        || options.coverage_gaps
        || options.hotspots
        || options.targets
        || score;
    let effective_score = if any_section { score } else { true } || options.snapshot_requested;
    let force_full = options.snapshot_requested || effective_score;

    DerivedHealthSections {
        any_section,
        complexity: if any_section {
            options.complexity
        } else {
            true
        },
        file_scores: if any_section {
            options.file_scores
        } else {
            true
        } || force_full,
        coverage_gaps: if any_section {
            options.coverage_gaps
        } else {
            false
        },
        hotspots: if any_section { options.hotspots } else { true }
            || options.snapshot_requested
            || options.trend,
        targets: if any_section { options.targets } else { true },
        css: options.css,
        score: effective_score,
        force_full,
        score_only_output: is_health_score_only_output(options, score),
    }
}

/// Normalize health run inputs into the engine-owned run contract.
#[must_use]
pub fn derive_health_run_options(input: HealthRunOptionsInput<'_>) -> HealthRunOptions<'_> {
    let targets = input.targets || input.effort.is_some();
    let sections = derive_health_sections(&HealthSectionOptions {
        output: input.output,
        complexity: input.complexity,
        file_scores: input.file_scores,
        coverage_gaps: input.coverage_gaps,
        hotspots: input.hotspots,
        targets,
        css: input.css,
        score: input.score,
        score_gate: input.gates.min_score.is_some(),
        snapshot_requested: input.snapshot_requested,
        trend: input.trend,
    });

    HealthRunOptions {
        thresholds: input.thresholds,
        top: input.top,
        sort: input.sort,
        sections,
        ownership: input.ownership && sections.hotspots,
        ownership_emails: input.ownership_emails,
        effort: input.effort,
        gates: input.gates,
        since: input.since,
        min_commits: input.min_commits,
        coverage_inputs: input.coverage_inputs,
        runtime_coverage: input.runtime_coverage,
    }
}

fn is_health_score_only_output(options: &HealthSectionOptions, score: bool) -> bool {
    score
        && !options.complexity
        && !options.file_scores
        && !options.coverage_gaps
        && !options.hotspots
        && !options.targets
        && !options.trend
}

/// Input for deriving effective programmatic complexity sections.
#[derive(Debug, Clone)]
pub struct ComplexitySectionOptions {
    complexity: bool,
    file_scores: bool,
    coverage_gaps: bool,
    hotspots: bool,
    ownership: bool,
    targets: bool,
    css: bool,
    score: bool,
}

/// Derived section selection for programmatic health / complexity runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DerivedComplexityOptions {
    any_section: bool,
    complexity: bool,
    file_scores: bool,
    coverage_gaps: bool,
    hotspots: bool,
    ownership: bool,
    targets: bool,
    force_full: bool,
    score_only_output: bool,
    score: bool,
}

/// Derive effective programmatic health / complexity section flags.
#[must_use]
pub fn derive_complexity_sections(options: &ComplexitySectionOptions) -> DerivedComplexityOptions {
    let requested_hotspots = options.hotspots || options.ownership;
    let sections = derive_health_sections(&HealthSectionOptions {
        output: OutputFormat::Human,
        complexity: options.complexity,
        file_scores: options.file_scores,
        coverage_gaps: options.coverage_gaps,
        hotspots: requested_hotspots,
        targets: options.targets,
        css: options.css,
        score: options.score,
        score_gate: false,
        snapshot_requested: false,
        trend: false,
    });

    DerivedComplexityOptions {
        any_section: sections.any_section,
        complexity: sections.complexity,
        file_scores: sections.file_scores,
        coverage_gaps: sections.coverage_gaps,
        hotspots: sections.hotspots,
        ownership: options.ownership && sections.hotspots,
        targets: sections.targets,
        force_full: sections.force_full,
        score_only_output: sections.score_only_output,
        score: sections.score,
    }
}

/// Normalized programmatic complexity / health inputs shared by API, NAPI, and
/// engine-backed runners.
#[derive(Debug, Clone, PartialEq)]
pub struct ComplexityRunOptions<'a> {
    thresholds: HealthThresholdOverrides,
    top: Option<usize>,
    sort: HealthSort,
    complexity_breakdown: bool,
    sections: DerivedComplexityOptions,
    ownership_emails: Option<EmailMode>,
    effort: Option<EffortEstimate>,
    css: bool,
    since: Option<&'a str>,
    min_commits: Option<u32>,
    coverage_inputs: HealthCoverageInputs<'a>,
}

/// Command-neutral runtime coverage input for health analysis.
#[derive(Debug, Clone)]
pub struct RuntimeCoverageOptions {
    /// Path to the runtime coverage artifact captured by the sidecar.
    pub path: PathBuf,
    /// Minimum invocation count for a function to classify as hot-path.
    pub min_invocations_hot: u64,
    /// Minimum total trace volume before high-confidence `safe_to_delete` /
    /// `review_required` verdicts may be emitted. Below this the sidecar caps
    /// confidence at `medium`. `None` lets the sidecar use its spec-default
    /// (5000).
    pub min_observation_volume: Option<u32>,
    /// Fraction of total trace count below which an invoked function is
    /// classified as `low_traffic` rather than `active`. `None` lets the
    /// sidecar use its spec-default (0.001 = 0.1%).
    pub low_traffic_threshold: Option<f64>,
    /// Verified license JWT forwarded to the closed-source sidecar.
    pub license_jwt: String,
    /// License or trial watermark to stamp on the runtime coverage output.
    pub watermark: Option<RuntimeCoverageWatermark>,
}

/// Pre-parsed health input reused from another analysis in the same process.
pub struct HealthSharedParseData {
    /// Discovered files reused from the upstream analysis.
    pub files: Vec<fallow_types::discover::DiscoveredFile>,
    /// Parsed modules reused from the upstream analysis.
    pub modules: Vec<fallow_types::extract::ModuleInfo>,
    /// Dead-code results reused by advisory health surfaces that do not need the graph.
    pub dead_code_results: Option<AnalysisResults>,
    /// Workspace metadata discovered during config resolution.
    pub workspaces: Vec<WorkspaceInfo>,
    /// Full analysis output (graph + results) for file scoring.
    pub analysis_output: Option<DeadCodeAnalysisArtifacts>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn health_run_input() -> HealthRunOptionsInput<'static> {
        HealthRunOptionsInput {
            output: OutputFormat::Json,
            thresholds: HealthThresholdOverrides::default(),
            top: None,
            sort: HealthSort::Cyclomatic,
            complexity: false,
            file_scores: false,
            coverage_gaps: false,
            hotspots: false,
            ownership: false,
            ownership_emails: None,
            targets: false,
            css: false,
            effort: None,
            score: false,
            gates: HealthGateOptions::default(),
            snapshot_requested: false,
            trend: false,
            since: None,
            min_commits: None,
            coverage_inputs: HealthCoverageInputs::default(),
            runtime_coverage: None,
        }
    }

    #[test]
    fn health_execution_options_own_shared_runner_scope() {
        let root = Path::new("/project");
        let config_path = None;
        let workspace = vec!["packages/app".to_string()];
        let diff = DiffIndex::from_unified_diff(
            "diff --git a/src/a.ts b/src/a.ts\n\
             --- a/src/a.ts\n\
             +++ b/src/a.ts\n\
             @@ -0,0 +1,1 @@\n\
             +new line\n",
        );
        let runtime_coverage = RuntimeCoverageOptions {
            path: PathBuf::from("coverage/v8"),
            min_invocations_hot: 10,
            min_observation_volume: Some(500),
            low_traffic_threshold: Some(0.01),
            license_jwt: "test.jwt".to_string(),
            watermark: None,
        };

        let options = HealthExecutionOptions {
            root,
            config_path: &config_path,
            output: OutputFormat::Json,
            no_cache: true,
            threads: 2,
            quiet: true,
            complexity_breakdown: true,
            thresholds: HealthThresholdOverrides::default(),
            top: Some(5),
            sort: HealthSort::Cognitive,
            production: true,
            production_override: Some(true),
            allow_remote_extends: false,
            changed_since: Some("HEAD~1"),
            diff_index: Some(&diff),
            use_shared_diff_index: false,
            workspace: Some(&workspace),
            changed_workspaces: None,
            baseline: Some(Path::new(".fallow/health-baseline.json")),
            save_baseline: None,
            baseline_mode: crate::baseline::HealthBaselineMode::Count,
            baseline_mode_explicit: false,
            complexity: true,
            file_scores: true,
            coverage_gaps: false,
            config_activates_coverage_gaps: false,
            hotspots: true,
            ownership: false,
            ownership_emails: None,
            targets: true,
            css: false,
            css_deep: false,
            force_full: true,
            score_only_output: false,
            enforce_coverage_gap_gate: true,
            effort: Some(EffortEstimate::Low),
            score: true,
            gates: HealthGateOptions {
                min_score: Some(80.0),
                min_severity: None,
                report_only: false,
            },
            since: Some("30d"),
            min_commits: Some(2),
            explain: true,
            summary: false,
            save_snapshot: Some(PathBuf::from(".fallow/snapshots/health.json")),
            trend: true,
            coverage_inputs: HealthCoverageInputs::default(),
            performance: true,
            runtime_coverage: Some(runtime_coverage),
            churn_file: Some(Path::new("churn.json")),
            analysis_identity: fallow_types::semantic::SemanticAnalysisIdentity::default(),
            group_by: Some(GroupByMode::Directory),
        };

        assert_eq!(options.root, root);
        assert!(
            options
                .diff_index
                .is_some_and(|index| index.line_is_added("src/a.ts", 1))
        );
        assert_eq!(options.workspace, Some(workspace.as_slice()));
        assert!(options.runtime_coverage.is_some());
        assert_eq!(options.group_by, Some(GroupByMode::Directory));
        assert_eq!(
            options.save_snapshot.as_deref(),
            Some(Path::new(".fallow/snapshots/health.json"))
        );
    }

    #[test]
    fn health_run_options_default_sections_match_health_defaults() {
        let run = derive_health_run_options(health_run_input());

        assert!(run.sections.complexity);
        assert!(run.sections.file_scores);
        assert!(run.sections.hotspots);
        assert!(run.sections.targets);
        assert!(run.sections.score);
        assert!(!run.ownership);
    }

    #[test]
    fn health_run_options_effort_requests_targets() {
        let mut input = health_run_input();
        input.effort = Some(EffortEstimate::Low);

        let run = derive_health_run_options(input);

        assert!(run.sections.targets);
        assert_eq!(run.effort, Some(EffortEstimate::Low));
    }

    struct HealthExecutionOptionsFixture {
        config_path: Option<PathBuf>,
    }

    impl HealthExecutionOptionsFixture {
        const fn new() -> Self {
            Self { config_path: None }
        }

        fn options<'a>(&'a self, root: &'a Path) -> HealthExecutionOptions<'a> {
            HealthExecutionOptions {
                root,
                config_path: &self.config_path,
                output: OutputFormat::Human,
                no_cache: true,
                threads: 1,
                quiet: true,
                complexity_breakdown: false,
                thresholds: HealthThresholdOverrides::default(),
                top: None,
                sort: HealthSort::Cyclomatic,
                production: false,
                production_override: None,
                allow_remote_extends: false,
                changed_since: None,
                diff_index: None,
                use_shared_diff_index: false,
                workspace: None,
                changed_workspaces: None,
                baseline: None,
                save_baseline: None,
                baseline_mode: crate::baseline::HealthBaselineMode::Count,
                baseline_mode_explicit: false,
                complexity: true,
                file_scores: false,
                coverage_gaps: false,
                config_activates_coverage_gaps: false,
                hotspots: false,
                ownership: false,
                ownership_emails: None,
                targets: false,
                css: false,
                css_deep: false,
                force_full: false,
                score_only_output: false,
                enforce_coverage_gap_gate: true,
                effort: None,
                score: false,
                gates: HealthGateOptions::default(),
                since: None,
                min_commits: None,
                explain: false,
                summary: false,
                save_snapshot: None,
                trend: false,
                coverage_inputs: HealthCoverageInputs::default(),
                performance: false,
                runtime_coverage: None,
                churn_file: None,
                analysis_identity: fallow_types::semantic::SemanticAnalysisIdentity::default(),
                group_by: None,
            }
        }
    }

    #[test]
    fn standalone_health_precomputes_dead_code_when_default_crap_can_use_graph() {
        let project = tempfile::tempdir().expect("temp dir");
        let fixture = HealthExecutionOptionsFixture::new();
        let options = fixture.options(project.path());
        let config = crate::project_config::default_project_config(project.path()).config;

        assert!(should_precompute_dead_code_analysis(&options, &config));
    }

    #[test]
    fn standalone_health_skips_precompute_when_no_section_needs_analysis_artifacts() {
        let project = tempfile::tempdir().expect("temp dir");
        let fixture = HealthExecutionOptionsFixture::new();
        let mut options = fixture.options(project.path());
        options.thresholds.max_crap = Some(0.0);
        let config = crate::project_config::default_project_config(project.path()).config;

        assert!(!should_precompute_dead_code_analysis(&options, &config));
    }

    #[test]
    fn standalone_health_precomputes_dead_code_for_target_sections() {
        let project = tempfile::tempdir().expect("temp dir");
        let fixture = HealthExecutionOptionsFixture::new();
        let mut options = fixture.options(project.path());
        options.thresholds.max_crap = Some(0.0);
        options.targets = true;
        let config = crate::project_config::default_project_config(project.path()).config;

        assert!(should_precompute_dead_code_analysis(&options, &config));
    }

    #[test]
    fn health_run_options_ownership_requires_hotspots() {
        let mut input = health_run_input();
        input.complexity = true;
        input.ownership = true;

        let run = derive_health_run_options(input);

        assert!(!run.sections.hotspots);
        assert!(!run.ownership);

        let mut input = health_run_input();
        input.ownership = true;
        input.hotspots = true;

        let run = derive_health_run_options(input);

        assert!(run.sections.hotspots);
        assert!(run.ownership);
    }

    #[test]
    fn health_run_options_score_gate_forces_score() {
        let mut input = health_run_input();
        input.gates.min_score = Some(90.0);

        let run = derive_health_run_options(input);

        assert!(run.sections.score);
        assert_eq!(run.gates.min_score, Some(90.0));
    }

    #[test]
    fn coverage_root_accepts_posix_absolute() {
        assert!(validate_coverage_root_absolute(Some(Path::new("/ci/workspace"))).is_ok());
        assert!(
            validate_coverage_root_absolute(Some(Path::new("/home/runner/work/myapp"))).is_ok()
        );
    }

    #[test]
    fn coverage_root_rejects_relative() {
        assert!(validate_coverage_root_absolute(Some(Path::new("src"))).is_err());
        assert!(validate_coverage_root_absolute(Some(Path::new("./coverage"))).is_err());
        assert!(validate_coverage_root_absolute(Some(Path::new("a/b/c"))).is_err());
    }

    #[test]
    fn coverage_root_accepts_none() {
        assert!(validate_coverage_root_absolute(None).is_ok());
    }

    #[test]
    fn coverage_root_accepts_windows_absolute_on_all_hosts() {
        assert!(validate_coverage_root_absolute(Some(Path::new(r"C:\ci\workspace"))).is_ok());
    }
}
