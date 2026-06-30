//! Programmatic runtime entry points that avoid depending on `fallow-cli`.

use std::path::PathBuf;

use fallow_output::{HealthGrouping, HealthReport, RootEnvelopeMode};
use fallow_types::output_format::OutputFormat;
use fallow_types::workspace::WorkspaceDiagnostic;

mod dead_code;
mod duplication;
mod feature_flags;
mod trace;

pub use crate::runtime_output::{
    BoundaryViolationsOutput, BoundaryViolationsProgrammaticOutput, CircularDependenciesOutput,
    CircularDependenciesProgrammaticOutput, DeadCodeOutput, DeadCodeProgrammaticOutput,
    DuplicationOutput, DuplicationProgrammaticOutput, FeatureFlagsOutput,
    FeatureFlagsProgrammaticOutput, HealthJsonReportInput, HealthProgrammaticOutput,
    TraceCloneOutput, TraceCloneProgrammaticOutput, TraceDependencyOutput,
    TraceDependencyProgrammaticOutput, TraceExportOutput, TraceExportProgrammaticOutput,
    TraceFileOutput, TraceFileProgrammaticOutput, serialize_health_report_json,
};
pub use dead_code::{run_boundary_violations, run_circular_dependencies, run_dead_code};
pub use duplication::run_duplication;
pub use feature_flags::run_feature_flags;
pub use trace::{run_trace_clone, run_trace_dependency, run_trace_export, run_trace_file};

use crate::{
    ComplexityOptions, ProgrammaticError,
    analysis_context::{ProgrammaticAnalysisContext, resolve_programmatic_analysis_context},
    next_steps::{setup_pointer_applicable, suggestions_enabled},
};

type ProgrammaticResult<T> = Result<T, ProgrammaticError>;

/// Runtime probes used by programmatic health output assembly.
///
/// Concrete runners supply environment and project facts while the stable
/// command strings and output ordering remain owned by `fallow-output`.
pub struct ProgrammaticHealthNextStepFacts {
    pub suggestions_enabled: bool,
    pub offer_setup: bool,
    pub impact_digest: Option<fallow_output::ImpactDigestCounts>,
    pub audit_changed: bool,
}

/// API-owned health analysis payload returned by programmatic runners.
///
/// The engine owns execution, but this type is the public runner contract so
/// embedders do not have to construct or depend on engine result structs.
pub struct ProgrammaticHealthAnalysis {
    pub report: HealthReport,
    pub grouping: Option<HealthGrouping>,
    pub root: PathBuf,
    pub elapsed: std::time::Duration,
}

impl ProgrammaticHealthAnalysis {
    fn from_engine<GroupResolver>(
        analysis: fallow_engine::HealthAnalysisResult<GroupResolver>,
    ) -> Self {
        Self {
            root: analysis.config.root,
            report: analysis.report,
            grouping: analysis.grouping,
            elapsed: analysis.elapsed,
        }
    }
}

/// Health runner output shared by API, NAPI, and compatibility adapters.
///
/// Runtime-only presentation probes stay explicit so the API boundary, not the
/// concrete runner, owns the final programmatic report assembly.
pub struct ProgrammaticHealthRun {
    pub analysis: ProgrammaticHealthAnalysis,
    pub workspace_diagnostics: Vec<WorkspaceDiagnostic>,
    pub next_step_facts: ProgrammaticHealthNextStepFacts,
    pub telemetry_analysis_run_id: Option<String>,
}

/// Runner boundary for programmatic health.
///
/// This keeps embedders on the typed API contract while still allowing tests
/// and compatibility adapters to provide a custom health runner.
pub trait ProgrammaticHealthRunner {
    /// Run health analysis for public programmatic options.
    ///
    /// # Errors
    ///
    /// Returns a structured programmatic error when the concrete runner cannot
    /// resolve options or complete health analysis.
    fn run_programmatic_health(
        &self,
        options: &ComplexityOptions,
    ) -> Result<ProgrammaticHealthRun, ProgrammaticError>;
}

/// Default health runner backed directly by `fallow-engine`.
///
/// This runs the command-neutral health pipeline through the engine health
/// runner without touching the CLI crate: the programmatic
/// path never groups (`--group-by`), never drives the runtime coverage sidecar,
/// and never records CLI telemetry, so the seams are inert no-ops. NAPI and
/// future Rust embedders use this runner; the CLI keeps its own runner for the
/// `fallow health` command path.
#[derive(Debug, Clone, Copy, Default)]
pub struct EngineHealthRunner;

impl ProgrammaticHealthRunner for EngineHealthRunner {
    fn run_programmatic_health(
        &self,
        options: &ComplexityOptions,
    ) -> Result<ProgrammaticHealthRun, ProgrammaticError> {
        let resolved = resolve_programmatic_analysis_context(&options.analysis)?;
        resolved.install(|| run_programmatic_health_on_engine(&resolved, options))
    }
}

fn run_programmatic_health_on_engine(
    resolved: &ProgrammaticAnalysisContext,
    options: &ComplexityOptions,
) -> ProgrammaticResult<ProgrammaticHealthRun> {
    let health_options = derive_programmatic_health_execution_options(resolved, options);
    let result =
        fallow_engine::run_ungrouped_health(&health_options, resolved.workspace_roots.clone())
            .map_err(|_| generic_health_error("health"))?;

    let root = result.config.root.clone();
    let next_step_facts = ProgrammaticHealthNextStepFacts {
        suggestions_enabled: suggestions_enabled(),
        offer_setup: setup_pointer_applicable(&root),
        impact_digest: None,
        audit_changed: fallow_engine::is_git_repo(&root),
    };
    Ok(ProgrammaticHealthRun {
        workspace_diagnostics: result.workspace_diagnostics.clone(),
        analysis: ProgrammaticHealthAnalysis::from_engine(result.without_group_resolver()),
        next_step_facts,
        telemetry_analysis_run_id: None,
    })
}

fn generic_health_error(command: &str) -> ProgrammaticError {
    let code = format!(
        "FALLOW_{}_FAILED",
        command.replace('-', "_").to_ascii_uppercase()
    );
    ProgrammaticError::new(format!("{command} failed"), 2)
        .with_code(code)
        .with_context(format!("fallow {command}"))
        .with_help(format!(
            "Re-run `fallow {command} --format json --quiet` in the target project for CLI diagnostics"
        ))
}

/// Run programmatic health / complexity through the engine-backed runner.
///
/// # Errors
///
/// Returns a structured programmatic error for invalid options or analysis
/// failures.
pub fn run_health(options: &ComplexityOptions) -> ProgrammaticResult<HealthProgrammaticOutput> {
    run_health_with_runner(options, &EngineHealthRunner)
}

#[must_use]
fn derive_programmatic_health_execution_options<'a>(
    resolved: &'a ProgrammaticAnalysisContext,
    options: &'a ComplexityOptions,
) -> fallow_engine::HealthExecutionOptions<'a> {
    let run = crate::derive_complexity_run_options(options);

    fallow_engine::HealthExecutionOptions {
        root: resolved.root(),
        config_path: resolved.config_path(),
        output: OutputFormat::Human,
        no_cache: resolved.no_cache(),
        threads: resolved.threads(),
        quiet: true,
        complexity_breakdown: run.complexity_breakdown,
        thresholds: crate::thresholds_to_engine(run.thresholds),
        top: run.top,
        sort: crate::complexity_sort_to_engine(run.sort),
        production: resolved.production_override().unwrap_or(false),
        production_override: resolved.production_override(),
        changed_since: resolved.changed_since(),
        diff_index: resolved.diff_index(),
        use_shared_diff_index: false,
        workspace: resolved.workspace(),
        changed_workspaces: resolved.changed_workspaces(),
        baseline: None,
        save_baseline: None,
        complexity: run.sections.complexity,
        file_scores: run.sections.file_scores,
        coverage_gaps: run.sections.coverage_gaps,
        config_activates_coverage_gaps: !run.sections.any_section,
        hotspots: run.sections.hotspots,
        ownership: run.sections.ownership,
        targets: run.sections.targets,
        css: run.css,
        force_full: run.sections.force_full,
        score_only_output: run.sections.score_only_output,
        enforce_coverage_gap_gate: true,
        effort: run.effort.map(crate::target_effort_to_output),
        score: run.sections.score,
        gates: fallow_engine::HealthGateOptions::default(),
        since: run.since,
        min_commits: run.min_commits,
        explain: resolved.explain_enabled(),
        summary: false,
        save_snapshot: None,
        trend: false,
        coverage_inputs: crate::coverage_inputs_to_engine(run.coverage_inputs),
        performance: false,
        runtime_coverage: None,
        churn_file: None,
        group_by: None,
        ownership_emails: run
            .ownership_emails
            .map(crate::ownership_email_mode_to_config),
    }
}

/// Run programmatic health / complexity and return typed API output.
///
/// The concrete runner is injected while the health implementation is still
/// being migrated out of the CLI crate. Runner-owned responsibilities are
/// limited to typed analysis plus runtime facts; this API crate owns the final
/// programmatic report assembly.
///
/// # Errors
///
/// Returns a structured programmatic error for invalid options or runner
/// failures.
pub fn run_complexity_with_runner(
    options: &ComplexityOptions,
    runner: &impl ProgrammaticHealthRunner,
) -> ProgrammaticResult<HealthProgrammaticOutput> {
    crate::validate_complexity_options(options)?;
    let ProgrammaticHealthRun {
        analysis,
        workspace_diagnostics,
        next_step_facts,
        telemetry_analysis_run_id,
    } = runner.run_programmatic_health(options)?;
    let root = analysis.root.clone();
    let next_steps =
        fallow_output::build_health_next_steps(fallow_output::build_health_next_steps_input(
            &analysis.report,
            next_step_facts.suggestions_enabled,
            next_step_facts.offer_setup,
            next_step_facts.impact_digest,
            next_step_facts.audit_changed,
        ));
    Ok(HealthProgrammaticOutput {
        report: analysis.report,
        grouping: analysis.grouping,
        root,
        elapsed: analysis.elapsed,
        explain: options.analysis.explain,
        workspace_diagnostics,
        next_steps,
        envelope_mode: root_envelope_mode(options.analysis.legacy_envelope),
        telemetry_analysis_run_id,
    })
}

/// Alias for [`run_complexity_with_runner`] with a product-oriented name.
///
/// # Errors
///
/// Returns the same structured errors as [`run_complexity_with_runner`].
pub fn run_health_with_runner(
    options: &ComplexityOptions,
    runner: &impl ProgrammaticHealthRunner,
) -> ProgrammaticResult<HealthProgrammaticOutput> {
    run_complexity_with_runner(options, runner)
}

const fn root_envelope_mode(legacy_envelope: bool) -> RootEnvelopeMode {
    RootEnvelopeMode::from_legacy(legacy_envelope)
}

#[cfg(test)]
mod tests;
