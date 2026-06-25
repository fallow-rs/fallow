//! Typed health result contracts exposed through the engine boundary.

use std::path::PathBuf;
use std::time::Duration;

use fallow_config::ResolvedConfig;
use fallow_output::{HealthGrouping, HealthReport, HealthTimings, RuntimeCoverageWatermark};

/// Command-neutral sort criteria for health complexity findings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthSort {
    Severity,
    Cyclomatic,
    Cognitive,
    Lines,
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
