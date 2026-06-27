//! Typed health result contracts exposed through the engine boundary.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use fallow_config::{EmailMode, OutputFormat, ResolvedConfig};
use fallow_output::{
    DiffIndex, EffortEstimate, FindingSeverity, GroupByMode, HealthGrouping, HealthReport,
    HealthTimings, RuntimeCoverageReport, RuntimeCoverageWatermark,
};
use fallow_types::path_util::is_absolute_path_any_platform;

mod assembly;
mod coverage_gaps;
mod coverage_intelligence;
mod execute;
mod grouping;
mod hotspots;
pub mod ownership;
mod react_hooks;
mod runtime_filter;
pub mod scoring;
mod tailwind_theme;
mod targets;

pub(crate) use execute::{
    HealthOptions, HealthReportAssembly, SubsetFilter, VitalSignsAndCountsInput,
    apply_duplication_metrics, compute_vital_signs_and_counts,
};
pub use execute::{
    HealthPipelineInputs, HealthResultGeneric, HealthScopeInputs, execute_health_inner,
    validate_health_churn_file,
};

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
pub type RuntimeCoverageAnalyzer<'a> = dyn Fn(
        &RuntimeCoverageOptions,
        RuntimeCoverageSeamInput<'_>,
    ) -> Result<RuntimeCoverageReport, ExitCode>
    + 'a;

/// Inputs the runtime coverage seam needs from the analysis core.
pub struct RuntimeCoverageSeamInput<'a> {
    pub root: &'a Path,
    pub modules: &'a [fallow_types::extract::ModuleInfo],
    pub analysis_output: &'a crate::DeadCodeAnalysisArtifacts,
    pub istanbul_coverage: Option<&'a scoring::IstanbulCoverage>,
    pub file_paths: &'a rustc_hash::FxHashMap<fallow_types::discover::FileId, &'a PathBuf>,
    pub ignore_set: &'a globset::GlobSet,
    pub changed_files: Option<&'a rustc_hash::FxHashSet<PathBuf>>,
    pub ws_roots: Option<&'a [PathBuf]>,
    pub top: Option<usize>,
    pub codeowners_path: Option<&'a str>,
    pub quiet: bool,
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

/// Telemetry facts the engine surfaces for the CLI to record.
///
/// Telemetry sinks are process-global CLI state; the engine accumulates the raw
/// counts so the CLI wrapper can feed `telemetry::note_*` without the engine
/// depending on the CLI telemetry module.
#[derive(Debug, Clone, Copy, Default)]
pub struct HealthTelemetryFacts {
    /// Module-graph node and edge counts when a graph was built.
    pub graph_structure: Option<(usize, usize)>,
}

/// Command-neutral sort criteria for health complexity findings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthSort {
    Severity,
    Cyclomatic,
    Cognitive,
    Lines,
}

/// Command-neutral threshold overrides for health complexity findings.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct HealthThresholdOverrides {
    pub max_cyclomatic: Option<u16>,
    pub max_cognitive: Option<u16>,
    /// Maximum CRAP score threshold. Functions meeting or exceeding this score
    /// are reported as complexity findings.
    pub max_crap: Option<f64>,
}

/// Command-neutral Istanbul coverage inputs for health CRAP scoring.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HealthCoverageInputs<'a> {
    pub coverage: Option<&'a Path>,
    /// Absolute coverage-path prefix to strip before rebasing files onto the
    /// project root.
    pub coverage_root: Option<&'a Path>,
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
    pub min_score: Option<f64>,
    pub min_severity: Option<FindingSeverity>,
    /// Render the score and findings but never fail CI on a health gate.
    pub report_only: bool,
}

/// Input for deriving effective health sections from command-neutral flags.
#[derive(Debug, Clone)]
pub struct HealthSectionOptions {
    pub output: OutputFormat,
    pub complexity: bool,
    pub file_scores: bool,
    pub coverage_gaps: bool,
    pub hotspots: bool,
    pub targets: bool,
    pub css: bool,
    pub score: bool,
    pub score_gate: bool,
    pub snapshot_requested: bool,
    pub trend: bool,
}

/// Derived section selection for health runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DerivedHealthSections {
    pub any_section: bool,
    pub complexity: bool,
    pub file_scores: bool,
    pub coverage_gaps: bool,
    pub hotspots: bool,
    pub targets: bool,
    pub css: bool,
    pub score: bool,
    pub force_full: bool,
    pub score_only_output: bool,
}

/// Command-neutral inputs used to normalize a health run before it reaches a
/// concrete runner.
#[derive(Debug, Clone)]
pub struct HealthRunOptionsInput<'a> {
    pub output: OutputFormat,
    pub thresholds: HealthThresholdOverrides,
    pub top: Option<usize>,
    pub sort: HealthSort,
    pub complexity: bool,
    pub file_scores: bool,
    pub coverage_gaps: bool,
    pub hotspots: bool,
    pub ownership: bool,
    pub ownership_emails: Option<EmailMode>,
    pub targets: bool,
    pub css: bool,
    pub effort: Option<EffortEstimate>,
    pub score: bool,
    pub gates: HealthGateOptions,
    pub snapshot_requested: bool,
    pub trend: bool,
    pub since: Option<&'a str>,
    pub min_commits: Option<u32>,
    pub coverage_inputs: HealthCoverageInputs<'a>,
    pub runtime_coverage: Option<RuntimeCoverageOptions>,
}

/// Normalized health inputs shared by CLI, API, NAPI, and future runners.
#[derive(Debug, Clone)]
pub struct HealthRunOptions<'a> {
    pub thresholds: HealthThresholdOverrides,
    pub top: Option<usize>,
    pub sort: HealthSort,
    pub sections: DerivedHealthSections,
    pub ownership: bool,
    pub ownership_emails: Option<EmailMode>,
    pub effort: Option<EffortEstimate>,
    pub gates: HealthGateOptions,
    pub since: Option<&'a str>,
    pub min_commits: Option<u32>,
    pub coverage_inputs: HealthCoverageInputs<'a>,
    pub runtime_coverage: Option<RuntimeCoverageOptions>,
}

/// Command-neutral inputs needed to execute a health analysis.
///
/// These fields are shared runner inputs rather than rendering concerns.
#[derive(Debug, Clone)]
pub struct HealthExecutionOptions<'a> {
    pub root: &'a Path,
    pub config_path: &'a Option<PathBuf>,
    pub output: OutputFormat,
    pub no_cache: bool,
    pub threads: usize,
    pub quiet: bool,
    /// Include per-decision-point complexity contributions in typed findings.
    ///
    /// This changes the produced health result shape, so it belongs to the
    /// runner input contract rather than CLI rendering options.
    pub complexity_breakdown: bool,
    pub thresholds: HealthThresholdOverrides,
    pub top: Option<usize>,
    pub sort: HealthSort,
    pub production: bool,
    pub production_override: Option<bool>,
    pub changed_since: Option<&'a str>,
    pub diff_index: Option<&'a DiffIndex>,
    pub use_shared_diff_index: bool,
    pub workspace: Option<&'a [String]>,
    pub changed_workspaces: Option<&'a str>,
    pub baseline: Option<&'a Path>,
    pub save_baseline: Option<&'a Path>,
    pub complexity: bool,
    pub file_scores: bool,
    pub coverage_gaps: bool,
    pub config_activates_coverage_gaps: bool,
    pub hotspots: bool,
    pub ownership: bool,
    pub ownership_emails: Option<EmailMode>,
    pub targets: bool,
    pub css: bool,
    pub force_full: bool,
    pub score_only_output: bool,
    pub enforce_coverage_gap_gate: bool,
    pub effort: Option<EffortEstimate>,
    pub score: bool,
    pub gates: HealthGateOptions,
    pub since: Option<&'a str>,
    pub min_commits: Option<u32>,
    pub explain: bool,
    pub summary: bool,
    pub save_snapshot: Option<PathBuf>,
    pub trend: bool,
    pub coverage_inputs: HealthCoverageInputs<'a>,
    pub performance: bool,
    pub runtime_coverage: Option<RuntimeCoverageOptions>,
    pub churn_file: Option<&'a Path>,
    /// Optional grouping mode for typed health output.
    pub group_by: Option<GroupByMode>,
}

/// Derive effective health section flags for CLI and embedders.
#[must_use]
pub fn derive_health_sections(options: &HealthSectionOptions) -> DerivedHealthSections {
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
    pub complexity: bool,
    pub file_scores: bool,
    pub coverage_gaps: bool,
    pub hotspots: bool,
    pub ownership: bool,
    pub targets: bool,
    pub css: bool,
    pub score: bool,
}

/// Derived section selection for programmatic health / complexity runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DerivedComplexityOptions {
    pub any_section: bool,
    pub complexity: bool,
    pub file_scores: bool,
    pub coverage_gaps: bool,
    pub hotspots: bool,
    pub ownership: bool,
    pub targets: bool,
    pub force_full: bool,
    pub score_only_output: bool,
    pub score: bool,
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
    pub thresholds: HealthThresholdOverrides,
    pub top: Option<usize>,
    pub sort: HealthSort,
    pub sections: DerivedComplexityOptions,
    pub ownership_emails: Option<EmailMode>,
    pub effort: Option<EffortEstimate>,
    pub css: bool,
    pub since: Option<&'a str>,
    pub min_commits: Option<u32>,
    pub coverage_inputs: HealthCoverageInputs<'a>,
}

/// Command-neutral runtime coverage input for health analysis.
#[derive(Debug, Clone)]
pub struct RuntimeCoverageOptions {
    pub path: PathBuf,
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
    pub license_jwt: String,
    pub watermark: Option<RuntimeCoverageWatermark>,
}

/// Pre-parsed health input reused from another analysis in the same process.
pub struct HealthSharedParseData {
    pub files: Vec<fallow_types::discover::DiscoveredFile>,
    pub modules: Vec<fallow_types::extract::ModuleInfo>,
    /// Full analysis output (graph + results) for file scoring.
    pub analysis_output: Option<crate::DeadCodeAnalysisArtifacts>,
}

/// Typed health analysis result shared by CLI, API, NAPI, and future embedders.
///
/// The result contract belongs at the engine boundary so downstream callers can
/// depend on a command-neutral shape.
#[derive(Debug)]
pub struct HealthAnalysisResult<GroupResolver = ()> {
    pub report: HealthReport,
    /// Per-group health output when grouping is active.
    ///
    /// `None` for the default run; `Some` for any grouped invocation. The
    /// top-level report reflects the active run scope; consumers that want
    /// per-group metrics read from `grouping.groups`.
    pub grouping: Option<HealthGrouping>,
    /// Optional grouping resolver retained by callers that need to tag findings
    /// after analysis without rediscovering ownership or package metadata.
    pub group_resolver: Option<GroupResolver>,
    pub config: ResolvedConfig,
    pub elapsed: Duration,
    pub timings: Option<HealthTimings>,
    pub coverage_gaps_has_findings: bool,
    pub should_fail_on_coverage_gaps: bool,
}

impl<GroupResolver> HealthAnalysisResult<GroupResolver> {
    /// Drop presentation-only grouping resolver state while preserving the
    /// command-neutral health analysis payload.
    #[must_use]
    pub fn without_group_resolver(self) -> HealthAnalysisResult<()> {
        HealthAnalysisResult {
            report: self.report,
            grouping: self.grouping,
            group_resolver: None,
            config: self.config,
            elapsed: self.elapsed,
            timings: self.timings,
            coverage_gaps_has_findings: self.coverage_gaps_has_findings,
            should_fail_on_coverage_gaps: self.should_fail_on_coverage_gaps,
        }
    }
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
    fn health_analysis_result_drops_presentation_resolver() {
        let project = tempfile::tempdir().expect("temp dir");
        let project_config = crate::config_for_project_analysis(
            project.path(),
            None,
            crate::ProjectConfigOptions {
                output: OutputFormat::Json,
                no_cache: true,
                threads: 1,
                production_override: None,
                quiet: true,
                analysis: fallow_config::ProductionAnalysis::Health,
            },
        )
        .expect("project config loads");
        let result = HealthAnalysisResult {
            report: HealthReport::default(),
            grouping: None,
            group_resolver: Some("resolver"),
            config: project_config.config,
            elapsed: Duration::from_millis(7),
            timings: None,
            coverage_gaps_has_findings: true,
            should_fail_on_coverage_gaps: true,
        };

        let neutral = result.without_group_resolver();

        assert!(neutral.group_resolver.is_none());
        assert_eq!(neutral.elapsed, Duration::from_millis(7));
        assert!(neutral.coverage_gaps_has_findings);
        assert!(neutral.should_fail_on_coverage_gaps);
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
            changed_since: Some("HEAD~1"),
            diff_index: Some(&diff),
            use_shared_diff_index: false,
            workspace: Some(&workspace),
            changed_workspaces: None,
            baseline: Some(Path::new(".fallow/health-baseline.json")),
            save_baseline: None,
            complexity: true,
            file_scores: true,
            coverage_gaps: false,
            config_activates_coverage_gaps: false,
            hotspots: true,
            ownership: false,
            ownership_emails: None,
            targets: true,
            css: false,
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
