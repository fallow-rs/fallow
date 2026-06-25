//! Typed health result contracts exposed through the engine boundary.

use std::time::Duration;

use fallow_config::ResolvedConfig;
use fallow_output::{HealthGrouping, HealthReport, HealthTimings};

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
