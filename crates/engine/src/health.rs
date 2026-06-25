//! Typed health result contracts exposed through the engine boundary.

use std::path::{Path, PathBuf};
use std::time::Duration;

use fallow_config::{OutputFormat, ResolvedConfig};
use fallow_output::{
    FindingSeverity, HealthGrouping, HealthReport, HealthTimings, RuntimeCoverageWatermark,
};

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
    pub analysis_output: Option<fallow_core::AnalysisOutput>,
}

/// Typed health analysis result shared by CLI, API, NAPI, and future embedders.
///
/// The health runner still lives in `fallow-cli` during the staged migration,
/// but the result contract belongs at the engine boundary so downstream callers
/// can depend on a command-neutral shape.
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
