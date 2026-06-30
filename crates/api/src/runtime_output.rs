//! Typed programmatic runtime outputs and shared output-contract serializers.

use std::path::{Path, PathBuf};

use fallow_output::{
    CheckOutput, DupesOutput, FeatureFlagFinding, FeatureFlagsOutput as FeatureFlagsOutputContract,
    GroupByMode, HealthGroup, HealthGrouping, HealthJsonOutputInput, HealthOutputInput,
    HealthReport, RootEnvelopeMode, health_meta,
};
use fallow_types::output::NextStep;
use fallow_types::output_dead_code::{
    BoundaryCallViolationFinding, BoundaryCoverageViolationFinding, BoundaryViolationFinding,
    CircularDependencyFinding,
};
use fallow_types::results::AnalysisResults;
use fallow_types::workspace::WorkspaceDiagnostic;

use crate::{CloneFamilyFinding, CloneGroupFinding, DupesReportPayload, DuplicationGroup};

pub const HEALTH_SCHEMA_VERSION: u32 = 7;

/// Concrete dead-code output contract returned by typed programmatic runs.
pub type DeadCodeOutput = CheckOutput;

/// Concrete circular-dependency output contract returned by typed runs.
pub type CircularDependenciesOutput = CheckOutput;

/// Concrete boundary-family output contract returned by typed runs.
pub type BoundaryViolationsOutput = CheckOutput;

/// Concrete duplication output contract returned by typed programmatic runs.
pub type DuplicationOutput = DupesOutput<DupesReportPayload, DuplicationGroup>;

/// Concrete feature-flag output contract returned by typed programmatic runs.
pub type FeatureFlagsOutput = FeatureFlagsOutputContract;

/// Concrete export trace output returned by typed programmatic runs.
pub type TraceExportOutput = fallow_types::trace::ExportTrace;

/// Concrete file trace output returned by typed programmatic runs.
pub type TraceFileOutput = fallow_types::trace::FileTrace;

/// Concrete dependency trace output returned by typed programmatic runs.
pub type TraceDependencyOutput = fallow_types::trace::DependencyTrace;

/// Concrete duplicate-code trace output returned by typed programmatic runs.
pub type TraceCloneOutput = fallow_types::trace::CloneTrace;

/// Inputs for serializing health JSON output through the API boundary.
pub struct HealthJsonReportInput<'a> {
    pub report: HealthReport,
    pub root: &'a Path,
    pub elapsed: std::time::Duration,
    pub explain: bool,
    pub grouped_by: Option<GroupByMode>,
    pub groups: Option<Vec<HealthGroup>>,
    pub workspace_diagnostics: Vec<WorkspaceDiagnostic>,
    pub next_steps: Vec<NextStep>,
    pub envelope_mode: RootEnvelopeMode,
    pub telemetry_analysis_run_id: Option<&'a str>,
}

/// Typed programmatic dead-code output before JSON serialization.
///
/// This is the API boundary embedders should use when they need access to the
/// typed engine/output result. Protocol surfaces serialize it explicitly at
/// their JSON boundary.
#[derive(Debug, Clone)]
pub struct DeadCodeProgrammaticOutput {
    pub output: DeadCodeOutput,
    pub root: PathBuf,
    pub envelope_mode: RootEnvelopeMode,
    pub telemetry_analysis_run_id: Option<String>,
}

impl DeadCodeProgrammaticOutput {
    /// Full typed dead-code issue arrays retained by this run.
    #[must_use]
    pub fn results(&self) -> &AnalysisResults {
        &self.output.results
    }

    /// Project-relative root used when serializing stable JSON paths.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }
}

/// Typed programmatic circular-dependency output before JSON serialization.
///
/// The wire envelope stays the dead-code/check contract, but the Rust API
/// surface is family-specific so embedders do not have to treat this as a
/// generic dead-code run.
#[derive(Debug, Clone)]
pub struct CircularDependenciesProgrammaticOutput {
    pub output: CircularDependenciesOutput,
    pub root: PathBuf,
    pub envelope_mode: RootEnvelopeMode,
    pub telemetry_analysis_run_id: Option<String>,
}

impl CircularDependenciesProgrammaticOutput {
    /// Full typed issue arrays retained by this family run.
    #[must_use]
    pub fn results(&self) -> &AnalysisResults {
        &self.output.results
    }

    /// The circular dependency findings retained by this family run.
    #[must_use]
    pub fn circular_dependencies(&self) -> &[CircularDependencyFinding] {
        &self.output.results.circular_dependencies
    }
}

impl From<DeadCodeProgrammaticOutput> for CircularDependenciesProgrammaticOutput {
    fn from(value: DeadCodeProgrammaticOutput) -> Self {
        Self {
            output: value.output,
            root: value.root,
            envelope_mode: value.envelope_mode,
            telemetry_analysis_run_id: value.telemetry_analysis_run_id,
        }
    }
}

/// Typed programmatic boundary-family output before JSON serialization.
///
/// This covers banned imports, boundary coverage, and forbidden call findings
/// while preserving the stable dead-code/check JSON envelope.
#[derive(Debug, Clone)]
pub struct BoundaryViolationsProgrammaticOutput {
    pub output: BoundaryViolationsOutput,
    pub root: PathBuf,
    pub envelope_mode: RootEnvelopeMode,
    pub telemetry_analysis_run_id: Option<String>,
}

impl BoundaryViolationsProgrammaticOutput {
    /// Full typed issue arrays retained by this family run.
    #[must_use]
    pub fn results(&self) -> &AnalysisResults {
        &self.output.results
    }

    /// Banned import boundary findings retained by this family run.
    #[must_use]
    pub fn boundary_violations(&self) -> &[BoundaryViolationFinding] {
        &self.output.results.boundary_violations
    }

    /// Boundary coverage findings retained by this family run.
    #[must_use]
    pub fn boundary_coverage_violations(&self) -> &[BoundaryCoverageViolationFinding] {
        &self.output.results.boundary_coverage_violations
    }

    /// Forbidden call findings retained by this family run.
    #[must_use]
    pub fn boundary_call_violations(&self) -> &[BoundaryCallViolationFinding] {
        &self.output.results.boundary_call_violations
    }
}

impl From<DeadCodeProgrammaticOutput> for BoundaryViolationsProgrammaticOutput {
    fn from(value: DeadCodeProgrammaticOutput) -> Self {
        Self {
            output: value.output,
            root: value.root,
            envelope_mode: value.envelope_mode,
            telemetry_analysis_run_id: value.telemetry_analysis_run_id,
        }
    }
}

/// Typed programmatic duplication output before JSON serialization.
#[derive(Debug, Clone)]
pub struct DuplicationProgrammaticOutput {
    pub output: DuplicationOutput,
    pub root: PathBuf,
    pub envelope_mode: RootEnvelopeMode,
    pub telemetry_analysis_run_id: Option<String>,
}

impl DuplicationProgrammaticOutput {
    /// Typed duplication report payload retained by this run.
    #[must_use]
    pub const fn report(&self) -> &DupesReportPayload {
        &self.output.report
    }

    /// Clone groups retained by this run, with typed actions and fingerprints.
    #[must_use]
    pub fn clone_groups(&self) -> &[CloneGroupFinding] {
        &self.output.report.clone_groups
    }

    /// Clone families retained by this run, with nested typed clone groups.
    #[must_use]
    pub fn clone_families(&self) -> &[CloneFamilyFinding] {
        &self.output.report.clone_families
    }

    /// Grouped duplication buckets when a grouping mode was used.
    #[must_use]
    pub fn groups(&self) -> Option<&[DuplicationGroup]> {
        self.output.groups.as_deref()
    }
}

/// Typed programmatic feature-flag output before JSON serialization.
#[derive(Debug, Clone)]
pub struct FeatureFlagsProgrammaticOutput {
    pub output: FeatureFlagsOutput,
    pub envelope_mode: RootEnvelopeMode,
    pub telemetry_analysis_run_id: Option<String>,
}

impl FeatureFlagsProgrammaticOutput {
    /// Feature flag findings retained by this run.
    #[must_use]
    pub fn feature_flags(&self) -> &[FeatureFlagFinding] {
        &self.output.feature_flags
    }

    /// Number of feature flags retained by this run after scoping and limits.
    #[must_use]
    pub const fn total_flags(&self) -> usize {
        self.output.total_flags
    }
}

/// Typed programmatic export-trace output before JSON serialization.
#[derive(Debug)]
pub struct TraceExportProgrammaticOutput {
    pub output: TraceExportOutput,
}

impl TraceExportProgrammaticOutput {
    /// Typed export trace retained by this run.
    #[must_use]
    pub const fn trace(&self) -> &TraceExportOutput {
        &self.output
    }
}

/// Typed programmatic file-trace output before JSON serialization.
#[derive(Debug)]
pub struct TraceFileProgrammaticOutput {
    pub output: TraceFileOutput,
}

impl TraceFileProgrammaticOutput {
    /// Typed file trace retained by this run.
    #[must_use]
    pub const fn trace(&self) -> &TraceFileOutput {
        &self.output
    }
}

/// Typed programmatic dependency-trace output before JSON serialization.
#[derive(Debug)]
pub struct TraceDependencyProgrammaticOutput {
    pub output: TraceDependencyOutput,
}

impl TraceDependencyProgrammaticOutput {
    /// Typed dependency trace retained by this run.
    #[must_use]
    pub const fn trace(&self) -> &TraceDependencyOutput {
        &self.output
    }
}

/// Typed programmatic duplicate-code trace output before JSON serialization.
#[derive(Debug)]
pub struct TraceCloneProgrammaticOutput {
    pub output: TraceCloneOutput,
}

impl TraceCloneProgrammaticOutput {
    /// Typed clone trace retained by this run.
    #[must_use]
    pub const fn trace(&self) -> &TraceCloneOutput {
        &self.output
    }
}

/// Typed programmatic health / complexity output before JSON serialization.
#[derive(Debug, Clone)]
pub struct HealthProgrammaticOutput {
    pub report: HealthReport,
    pub grouping: Option<HealthGrouping>,
    pub root: PathBuf,
    pub elapsed: std::time::Duration,
    pub explain: bool,
    pub workspace_diagnostics: Vec<WorkspaceDiagnostic>,
    pub next_steps: Vec<NextStep>,
    pub envelope_mode: RootEnvelopeMode,
    pub telemetry_analysis_run_id: Option<String>,
}

/// Serialize a health / complexity report into the stable JSON output contract.
///
/// # Errors
///
/// Returns a serde error when the report cannot be converted to JSON.
pub fn serialize_health_report_json(
    input: HealthJsonReportInput<'_>,
) -> Result<serde_json::Value, serde_json::Error> {
    let root_prefix = format!("{}/", input.root.display());
    fallow_output::serialize_health_json_output(HealthJsonOutputInput {
        output: HealthOutputInput {
            schema_version: HEALTH_SCHEMA_VERSION,
            version: env!("CARGO_PKG_VERSION").to_string(),
            elapsed: input.elapsed,
            report: input.report,
            grouped_by: input.grouped_by,
            groups: input.groups,
            meta: input.explain.then(health_meta),
            workspace_diagnostics: input.workspace_diagnostics,
            next_steps: input.next_steps,
        },
        root_prefix: Some(&root_prefix),
        envelope_mode: input.envelope_mode,
        analysis_run_id: input.telemetry_analysis_run_id,
    })
}
