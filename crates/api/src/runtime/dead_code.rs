use std::path::Path;
use std::time::Instant;

use fallow_config::ProductionAnalysis;
use fallow_engine::{
    change_scope::ChangeScope,
    dead_code::DeadCodeAnalysisArtifacts,
    project_config::{ProjectConfig, ProjectConfigOptions},
    session::AnalysisSession,
};
use fallow_output::{
    CHECK_SCHEMA_VERSION, CheckOutputInput, DeadCodeNextStepsInput, build_check_output,
    build_dead_code_next_steps, check_meta,
};
use fallow_types::output_format::OutputFormat;
use fallow_types::path_util::is_absolute_path_any_platform;
use fallow_types::results::AnalysisResults;
use rustc_hash::FxHashSet;

use crate::{
    AnalysisOptions, BoundaryViolationsProgrammaticOutput, CircularDependenciesProgrammaticOutput,
    DeadCodeFilters, DeadCodeOptions, DeadCodeProgrammaticOutput, ProgrammaticError,
    analysis_context::{
        ProgrammaticAnalysisContext, changed_files_for_run,
        resolve_programmatic_analysis_context_deferred_workspace, workspace_roots_for_session,
    },
    next_steps::{
        audit_changed_applicable, default_workspace_ref_for_workspaces, setup_pointer_applicable,
        suggestions_enabled,
    },
};

use super::ProgrammaticResult;

pub(super) struct DeadCodeProgrammaticRunWithArtifacts {
    pub output: DeadCodeProgrammaticOutput,
    pub artifacts: DeadCodeAnalysisArtifacts,
}

/// Run dead-code analysis and return typed API output before serialization.
///
/// # Errors
///
/// Returns a structured programmatic error for unsupported options, invalid
/// options, config load failures, analysis failures, or git changed-file
/// failures.
pub fn run_dead_code(options: &DeadCodeOptions) -> ProgrammaticResult<DeadCodeProgrammaticOutput> {
    run_dead_code_with_baseline(options, None)
}

/// Run dead-code analysis and hide the findings of a saved dead-code baseline.
///
/// The baseline is the file that `fallow dead-code --save-baseline` writes. It
/// is applied by the same engine function as `fallow dead-code --baseline`, so
/// both hide the same findings. A file that another command saved suppresses
/// nothing, as on the CLI.
///
/// # Errors
///
/// Returns the errors of [`run_dead_code`], and a structured error when the
/// baseline cannot be read, is not valid, or was saved with an incompatible
/// analysis identity.
pub fn run_dead_code_with_baseline(
    options: &DeadCodeOptions,
    baseline: Option<&Path>,
) -> ProgrammaticResult<DeadCodeProgrammaticOutput> {
    let resolved = resolve_programmatic_analysis_context_deferred_workspace(&options.analysis)?;
    run_dead_code_in_context(options, baseline, &resolved)
}

/// Run dead-code analysis in a context that the caller resolved. Audit uses
/// this to hand the change scope of every section to itself.
pub(super) fn run_dead_code_in_context(
    options: &DeadCodeOptions,
    baseline: Option<&Path>,
    resolved: &ProgrammaticAnalysisContext,
) -> ProgrammaticResult<DeadCodeProgrammaticOutput> {
    resolved.install(|| {
        let start = Instant::now();
        resolved.ensure_not_cancelled("config load and file discovery")?;
        let finding_ids = parse_finding_ids(options)?;
        let session = load_dead_code_session(options, resolved)?;
        resolve_package_map_before_analysis(resolved, &session)?;
        let (mut results, finished) =
            analyze_dead_code_results(options, resolved, &session, finding_ids)?;
        let FinishedDeadCode {
            type_aware_meta,
            mut finding_id_trace,
            change_reason,
            package_baselines,
        } = finished;
        if let Some(trace) = finding_id_trace.as_mut() {
            trace.start_stage(&mut results);
        }
        if let Some(baseline) = baseline {
            apply_baseline(
                &mut results,
                baseline,
                session.root(),
                type_aware_meta.as_ref(),
            )?;
        }
        let finding_id_query = finding_id_trace.map(|mut trace| {
            trace.end_stage(&mut results);
            trace.finish(
                &mut results,
                session.config(),
                finding_id_run_reasons(
                    options,
                    resolved,
                    &session,
                    RunScope {
                        change_reason,
                        baseline: baseline.is_some(),
                    },
                ),
            )
        });
        let mut output = build_dead_code_programmatic_output(
            options,
            resolved,
            &session,
            DeadCodeReport {
                results,
                type_aware_meta,
                package_baselines,
            },
            start,
        );
        output.output.finding_id_query = finding_id_query;
        Ok(output)
    })
}

/// Validate `finding_ids`. A malformed id is an error, so a typo never reads
/// as "the finding is gone".
fn parse_finding_ids(
    options: &DeadCodeOptions,
) -> ProgrammaticResult<Option<fallow_engine::dead_code::FindingIdFilter>> {
    fallow_engine::dead_code::FindingIdFilter::parse(&options.finding_ids).map_err(|message| {
        ProgrammaticError::new(message, 2)
            .with_code("FALLOW_INVALID_FINDING_ID")
            .with_context("findingIds")
    })
}

/// Refuse `finding_ids` on the analyses that narrow the result to one issue
/// family: a missing id there says nothing about the other families.
fn reject_finding_ids(options: &DeadCodeOptions) -> ProgrammaticResult<()> {
    if options.finding_ids.is_empty() {
        return Ok(());
    }
    Err(
        ProgrammaticError::new("findingIds is supported only by the dead-code analysis", 2)
            .with_code("FALLOW_UNSUPPORTED_OPTION")
            .with_context("findingIds"),
    )
}

/// The run facts that finding-id reasons read beyond the options.
#[derive(Clone, Copy)]
struct RunScope {
    /// The change channel that narrowed the run: a global ref or the package
    /// map.
    change_reason: Option<fallow_output::ScopeReason>,
    /// A saved baseline hid findings.
    baseline: bool,
}

/// The options of this run that can hide a finding without a fix. Mirrors the
/// CLI list: the scope channels, the issue-type filters, production mode and
/// the baseline.
fn finding_id_run_reasons(
    options: &DeadCodeOptions,
    resolved: &ProgrammaticAnalysisContext,
    session: &AnalysisSession,
    run: RunScope,
) -> Vec<fallow_output::FindingIdQueryReason> {
    use fallow_output::FindingIdQueryReason as Reason;

    [
        (resolved.diff.is_some(), Reason::Diff),
        (
            run.change_reason == Some(fallow_output::ScopeReason::ChangedSince)
                || options.analysis.ambient_changed_since.is_some(),
            Reason::ChangedSince,
        ),
        (
            run.change_reason == Some(fallow_output::ScopeReason::PackageBaselines),
            Reason::PackageBaselines,
        ),
        (resolved.workspace().is_some(), Reason::Workspace),
        (
            resolved.changed_workspaces().is_some(),
            Reason::ChangedWorkspaces,
        ),
        (!options.files.is_empty(), Reason::File),
        (options.filters.any_active(), Reason::IssueTypeFilter),
        (session.config().production, Reason::Production),
        (
            session.config().include_entry_exports,
            Reason::IncludeEntryExports,
        ),
        (run.baseline, Reason::Baseline),
    ]
    .into_iter()
    .filter_map(|(active, reason)| active.then_some(reason))
    .collect()
}

/// Turn an engine failure into a programmatic error, keeping a cancelled run
/// distinguishable from a failed one.
///
/// The engine's cancellation message already names the pipeline boundary the
/// run stopped at, so it is carried through rather than replaced.
pub(super) fn map_engine_error(
    err: &fallow_engine::EngineError,
    failure_message: &str,
    failure_code: &'static str,
    context: &'static str,
) -> ProgrammaticError {
    if err.is_cancelled() {
        return crate::analysis_context::cancelled_error_message(err.message())
            .with_context(context);
    }
    ProgrammaticError::new(format!("{failure_message}: {err}"), 2)
        .with_code(failure_code)
        .with_context(context)
}

/// Run circular-dependency analysis and return typed API output before JSON.
///
/// # Errors
///
/// Returns the same structured errors as [`run_dead_code`].
pub fn run_circular_dependencies(
    options: &DeadCodeOptions,
) -> ProgrammaticResult<CircularDependenciesProgrammaticOutput> {
    reject_finding_ids(options)?;
    let resolved = resolve_programmatic_analysis_context_deferred_workspace(&options.analysis)?;
    resolved.install(|| {
        run_dead_code_inner(options, &resolved, keep_circular_dependencies).map(Into::into)
    })
}

/// Run boundary-family analysis and return typed API output before JSON.
///
/// # Errors
///
/// Returns the same structured errors as [`run_dead_code`].
pub fn run_boundary_violations(
    options: &DeadCodeOptions,
) -> ProgrammaticResult<BoundaryViolationsProgrammaticOutput> {
    reject_finding_ids(options)?;
    let resolved = resolve_programmatic_analysis_context_deferred_workspace(&options.analysis)?;
    resolved.install(|| {
        run_dead_code_inner(options, &resolved, keep_boundary_violations).map(Into::into)
    })
}

fn run_dead_code_inner(
    options: &DeadCodeOptions,
    resolved: &ProgrammaticAnalysisContext,
    post_filter: impl FnOnce(&mut AnalysisResults),
) -> ProgrammaticResult<DeadCodeProgrammaticOutput> {
    let start = Instant::now();
    resolved.ensure_not_cancelled("config load and file discovery")?;
    let session = load_dead_code_session(options, resolved)?;
    resolve_package_map_before_analysis(resolved, &session)?;
    run_dead_code_with_session(options, resolved, &session, None, post_filter, start)
}

pub(super) fn run_dead_code_with_session(
    options: &DeadCodeOptions,
    resolved: &ProgrammaticAnalysisContext,
    session: &AnalysisSession,
    changed_files: Option<&FxHashSet<std::path::PathBuf>>,
    post_filter: impl FnOnce(&mut AnalysisResults),
    start: Instant,
) -> ProgrammaticResult<DeadCodeProgrammaticOutput> {
    resolved.ensure_not_cancelled("dead-code analysis")?;
    let mut results = analyze_session_dead_code(session)?;
    let unfiltered_unused_files = results.unused_files.clone();
    let finished = finish_dead_code_results(
        DeadCodeReportInputs {
            options,
            resolved,
            session,
            changed_files,
            unfiltered_unused_files,
            finding_ids: None,
        },
        &mut results,
        post_filter,
    )?;
    Ok(build_dead_code_programmatic_output(
        options,
        resolved,
        session,
        finished.into_report(results),
        start,
    ))
}

/// The reported dead-code findings of one session run and what the reporting
/// tail learned about them.
fn analyze_dead_code_results(
    options: &DeadCodeOptions,
    resolved: &ProgrammaticAnalysisContext,
    session: &AnalysisSession,
    finding_ids: Option<fallow_engine::dead_code::FindingIdFilter>,
) -> ProgrammaticResult<(AnalysisResults, FinishedDeadCode)> {
    resolved.ensure_not_cancelled("dead-code analysis")?;
    let mut results = analyze_session_dead_code(session)?;
    let unfiltered_unused_files = results.unused_files.clone();

    let finished = finish_dead_code_results(
        DeadCodeReportInputs {
            options,
            resolved,
            session,
            changed_files: None,
            unfiltered_unused_files,
            finding_ids,
        },
        &mut results,
        |_| {},
    )?;
    Ok((results, finished))
}

fn analyze_session_dead_code(session: &AnalysisSession) -> ProgrammaticResult<AnalysisResults> {
    session
        .analyze_dead_code()
        .map(|analysis| analysis.results)
        .map_err(|err| {
            map_engine_error(
                &err,
                "dead-code analysis failed",
                "FALLOW_DEAD_CODE_FAILED",
                "dead-code",
            )
        })
}

/// Hide the findings of a saved dead-code baseline, as `--baseline` does.
fn apply_baseline(
    results: &mut AnalysisResults,
    baseline: &Path,
    root: &Path,
    type_aware_meta: Option<&fallow_types::envelope::TypeAwareMeta>,
) -> ProgrammaticResult<()> {
    use fallow_engine::baseline::{DeadCodeBaselineError, apply_dead_code_baseline};

    let path = if is_absolute_path_any_platform(baseline) {
        baseline.to_path_buf()
    } else {
        root.join(baseline)
    };
    let content = std::fs::read_to_string(&path).map_err(|err| {
        ProgrammaticError::new(
            format!("failed to read baseline {}: {err}", path.display()),
            2,
        )
        .with_code("FALLOW_BASELINE_READ_FAILED")
        .with_context("baseline")
    })?;
    let identity = type_aware_meta
        .and_then(|meta| meta.identity.clone())
        .unwrap_or_default();
    apply_dead_code_baseline(results, &content, root, &identity, false)
        .map(|_| ())
        .map_err(|err| match err {
            DeadCodeBaselineError::Parse(message) => ProgrammaticError::new(
                format!("failed to parse baseline {}: {message}", path.display()),
                2,
            )
            .with_code("FALLOW_BASELINE_INVALID")
            .with_context("baseline"),
            DeadCodeBaselineError::IncompatibleIdentity(fields) => ProgrammaticError::new(
                format!(
                    "baseline analysis identity is incompatible in: {}. Save it again with the same analysis options",
                    fields.join(", ")
                ),
                2,
            )
            .with_code("FALLOW_BASELINE_IDENTITY_INCOMPATIBLE")
            .with_context("baseline"),
        })
}

pub(super) fn run_dead_code_with_session_artifacts(
    options: &DeadCodeOptions,
    resolved: &ProgrammaticAnalysisContext,
    session: &AnalysisSession,
    changed_files: Option<&FxHashSet<std::path::PathBuf>>,
    post_filter: impl FnOnce(&mut AnalysisResults),
    start: Instant,
) -> ProgrammaticResult<DeadCodeProgrammaticRunWithArtifacts> {
    resolved.ensure_not_cancelled("dead-code analysis")?;
    let mut artifacts = session
        .analyze_dead_code_with_artifacts(true, true)
        .map_err(|err| {
            map_engine_error(
                &err,
                "dead-code analysis failed",
                "FALLOW_DEAD_CODE_FAILED",
                "dead-code",
            )
        })?;
    let unfiltered_unused_files = artifacts.results.unused_files.clone();

    let finished = finish_dead_code_results(
        DeadCodeReportInputs {
            options,
            resolved,
            session,
            changed_files,
            unfiltered_unused_files,
            finding_ids: None,
        },
        &mut artifacts.results,
        post_filter,
    )?;

    Ok(build_dead_code_run_with_artifacts(
        options, resolved, session, artifacts, finished, start,
    ))
}

pub(super) fn run_dead_code_from_artifacts(
    options: &DeadCodeOptions,
    resolved: &ProgrammaticAnalysisContext,
    session: &AnalysisSession,
    changed_files: Option<&FxHashSet<std::path::PathBuf>>,
    mut artifacts: DeadCodeAnalysisArtifacts,
    start: Instant,
) -> ProgrammaticResult<DeadCodeProgrammaticRunWithArtifacts> {
    let unfiltered_unused_files = artifacts.results.unused_files.clone();
    let finished = finish_dead_code_results(
        DeadCodeReportInputs {
            options,
            resolved,
            session,
            changed_files,
            unfiltered_unused_files,
            finding_ids: None,
        },
        &mut artifacts.results,
        |_| {},
    )?;

    Ok(build_dead_code_run_with_artifacts(
        options, resolved, session, artifacts, finished, start,
    ))
}

/// Inputs the reporting tail needs from whichever entry point produced the
/// analysis.
struct DeadCodeReportInputs<'a> {
    options: &'a DeadCodeOptions,
    resolved: &'a ProgrammaticAnalysisContext,
    session: &'a AnalysisSession,
    changed_files: Option<&'a FxHashSet<std::path::PathBuf>>,
    /// Unused files as the engine reported them, before scope narrowing, which
    /// type-aware refinement needs to reason about the whole project.
    unfiltered_unused_files: Vec<fallow_types::output_dead_code::UnusedFileFinding>,
    /// The requested finding ids, when the caller asked for some. The trace
    /// starts on the full result set, before the scope.
    finding_ids: Option<fallow_engine::dead_code::FindingIdFilter>,
}

/// What the reporting tail learned about one run.
struct FinishedDeadCode {
    type_aware_meta: Option<fallow_types::envelope::TypeAwareMeta>,
    finding_id_trace: Option<fallow_engine::dead_code::FindingIdTrace>,
    /// The change channel that narrowed the run.
    change_reason: Option<fallow_output::ScopeReason>,
    /// The package baselines that narrowed the run.
    package_baselines: Vec<fallow_output::PackageBaselineStatus>,
}

impl FinishedDeadCode {
    fn into_report(self, results: AnalysisResults) -> DeadCodeReport {
        DeadCodeReport {
            results,
            type_aware_meta: self.type_aware_meta,
            package_baselines: self.package_baselines,
        }
    }
}

/// The findings of one run and the envelope facts that travel with them.
struct DeadCodeReport {
    results: AnalysisResults,
    type_aware_meta: Option<fallow_types::envelope::TypeAwareMeta>,
    package_baselines: Vec<fallow_output::PackageBaselineStatus>,
}

/// Shared reporting tail for every programmatic dead-code entry point: scope,
/// issue-type filters, effective rule severities, the caller's family filter,
/// and type-aware refinement.
///
/// The severity pass belongs here rather than at each entry point. Spelling
/// the sequence out per entry point is what let the programmatic runtime
/// report findings for rules a project had turned off while the CLI and the
/// editor did not. The change scope is resolved here once for the same reason.
fn finish_dead_code_results(
    inputs: DeadCodeReportInputs<'_>,
    results: &mut AnalysisResults,
    post_filter: impl FnOnce(&mut AnalysisResults),
) -> ProgrammaticResult<FinishedDeadCode> {
    let DeadCodeReportInputs {
        options,
        resolved,
        session,
        changed_files,
        unfiltered_unused_files,
        finding_ids,
    } = inputs;
    let mut finding_id_trace =
        finding_ids.map(|filter| fallow_engine::dead_code::FindingIdTrace::start(filter, results));
    let resolved_changed_files = if changed_files.is_some() {
        None
    } else {
        changed_files_for_run(resolved)?
    };
    let global_files = changed_files.or(resolved_changed_files.as_ref());
    if global_files.is_some() {
        resolved
            .measure_changed_since_scope(session.files().iter().map(|file| file.path.as_path()));
    }
    let change_scope =
        resolved.change_scope(global_files, session.config(), session.workspaces())?;
    apply_dead_code_scope(options, resolved, session, &change_scope, results)?;
    apply_dead_code_filters(&options.filters, results);
    fallow_engine::dead_code::apply_rule_severities(results, session.config());
    post_filter(results);
    if let Some(trace) = finding_id_trace.as_mut() {
        trace.end_stage(results);
    }
    let type_aware_meta = refine_with_unfiltered_unused_files(
        &options.analysis.type_aware,
        &options.filters,
        session,
        results,
        unfiltered_unused_files,
    )?;
    if type_aware_meta.is_some() {
        // Refinement can add findings, such as private-type leaks, anywhere in
        // the project. The scope narrows the final result, so it runs again.
        apply_dead_code_scope(options, resolved, session, &change_scope, results)?;
    }
    Ok(FinishedDeadCode {
        type_aware_meta,
        finding_id_trace,
        change_reason: change_scope.scope_reason(),
        package_baselines: change_scope.package_baselines(),
    })
}

fn refine_with_unfiltered_unused_files(
    options: &crate::TypeAwareOptions,
    filters: &DeadCodeFilters,
    session: &AnalysisSession,
    results: &mut AnalysisResults,
    unfiltered_unused_files: Vec<fallow_types::output_dead_code::UnusedFileFinding>,
) -> ProgrammaticResult<Option<fallow_types::envelope::TypeAwareMeta>> {
    let reported_unused_files =
        std::mem::replace(&mut results.unused_files, unfiltered_unused_files);
    let outcome =
        crate::type_aware::refine_programmatic_dead_code(options, filters, session, results);
    results.unused_files = reported_unused_files;
    if options.enabled {
        // Reconciliation can add findings, so rule severities are resolved
        // again over the refined set. The pass removes findings and writes
        // each gate severity again, so repeating it is idempotent. Mirrors
        // EditorAnalysisSession.
        fallow_engine::dead_code::apply_rule_severities(results, session.config());
    }
    outcome
}

fn build_dead_code_run_with_artifacts(
    options: &DeadCodeOptions,
    resolved: &ProgrammaticAnalysisContext,
    session: &AnalysisSession,
    artifacts: DeadCodeAnalysisArtifacts,
    finished: FinishedDeadCode,
    start: Instant,
) -> DeadCodeProgrammaticRunWithArtifacts {
    let output = build_dead_code_programmatic_output(
        options,
        resolved,
        session,
        finished.into_report(artifacts.results.clone()),
        start,
    );
    DeadCodeProgrammaticRunWithArtifacts { output, artifacts }
}

fn build_dead_code_programmatic_output(
    options: &DeadCodeOptions,
    resolved: &ProgrammaticAnalysisContext,
    session: &AnalysisSession,
    report: DeadCodeReport,
    start: Instant,
) -> DeadCodeProgrammaticOutput {
    let DeadCodeReport {
        results,
        type_aware_meta,
        package_baselines,
    } = report;
    let root = session.root();
    let next_steps = build_dead_code_next_steps(DeadCodeNextStepsInput {
        suggestions_enabled: suggestions_enabled(),
        results: &results,
        root,
        offer_setup: setup_pointer_applicable(root),
        impact_digest: None,
        workspace_ref: default_workspace_ref_for_workspaces(root, session.workspaces()).as_deref(),
        audit_changed: audit_changed_applicable(root),
        has_external_plugins: !fallow_config::discover_external_plugins(root, &[]).is_empty(),
        // The programmatic runtime loads no baseline, so there is never one to
        // re-check.
        baseline_recheck: None,
    });
    let config_fixable =
        fallow_config::is_config_fixable(&resolved.root, resolved.config_path.as_ref());
    let mut meta = options.analysis.explain.then(check_meta);
    if let Some(type_aware) = type_aware_meta {
        meta.get_or_insert_with(Default::default).type_aware = Some(type_aware);
    }
    // A `files` run drops dependency findings, like a filter without the
    // dependency issue types. The CLI applies the same rule.
    let reports_dependencies =
        options.filters.reports_dependency_findings() && options.files.is_empty();
    let workspace_diagnostics = fallow_types::workspace::merge_workspace_diagnostics(
        session.current_workspace_diagnostics(),
        fallow_engine::dead_code::config_pattern_diagnostics(
            session.config(),
            reports_dependencies,
        ),
    );
    let mut output = build_check_output(CheckOutputInput {
        schema_version: CHECK_SCHEMA_VERSION,
        version: env!("CARGO_PKG_VERSION").to_string(),
        elapsed: start.elapsed(),
        results,
        config_fixable,
        meta,
        workspace_diagnostics,
        next_steps,
    });
    output.request_outcomes = resolved.request_outcomes();
    output.package_baselines = package_baselines;
    DeadCodeProgrammaticOutput {
        output,
        root: session.root().to_path_buf(),
        config_fixable,
        telemetry_analysis_run_id: None,
    }
}

fn keep_circular_dependencies(results: &mut AnalysisResults) {
    let entry_point_summary = results.entry_point_summary.take();
    let circular_dependencies = std::mem::take(&mut results.circular_dependencies);
    *results = AnalysisResults::default();
    results.entry_point_summary = entry_point_summary;
    results.circular_dependencies = circular_dependencies;
}

fn keep_boundary_violations(results: &mut AnalysisResults) {
    let entry_point_summary = results.entry_point_summary.take();
    let boundary_violations = std::mem::take(&mut results.boundary_violations);
    let boundary_coverage_violations = std::mem::take(&mut results.boundary_coverage_violations);
    let boundary_call_violations = std::mem::take(&mut results.boundary_call_violations);
    *results = AnalysisResults::default();
    results.entry_point_summary = entry_point_summary;
    results.boundary_violations = boundary_violations;
    results.boundary_coverage_violations = boundary_coverage_violations;
    results.boundary_call_violations = boundary_call_violations;
}

pub(super) fn load_dead_code_session(
    options: &DeadCodeOptions,
    resolved: &ProgrammaticAnalysisContext,
) -> ProgrammaticResult<AnalysisSession> {
    let project_config = fallow_engine::project_config::config_for_project_analysis(
        &resolved.root,
        resolved.config_path.as_deref(),
        ProjectConfigOptions {
            output: OutputFormat::Json,
            no_cache: resolved.no_cache,
            threads: resolved.threads,
            production_override: resolved.production_override,
            quiet: true,
            analysis: ProductionAnalysis::DeadCode,
            allow_remote_extends: resolved.allow_remote_extends,
        },
    )
    .map_err(|err| {
        ProgrammaticError::new(format!("failed to load config: {err}"), 2)
            .with_code("FALLOW_CONFIG_LOAD_FAILED")
            .with_context("analysis.configPath")
    })?;
    let project_config = configure_project_for_dead_code(project_config, options);
    Ok(attach_cancellation(
        AnalysisSession::from_config(project_config),
        resolved,
    ))
}

/// Resolve the package map before the analysis starts, so a malformed map
/// fails fast. The call context keeps the result for the scope step.
///
/// Only the entry points that apply the change scope call this. Their changed
/// files come from the call's own ref, so the request here matches the one
/// the scope step makes. Trace and decision-surface sessions never read the map.
pub(super) fn resolve_package_map_before_analysis(
    resolved: &ProgrammaticAnalysisContext,
    session: &AnalysisSession,
) -> ProgrammaticResult<()> {
    resolved
        .change_scope(None, session.config(), session.workspaces())
        .map(drop)
}

/// Hand the caller's cancellation token to the engine session.
///
/// Without this the session runs to completion no matter what the API layer
/// checks between its own stages.
pub(super) fn attach_cancellation(
    session: AnalysisSession,
    resolved: &ProgrammaticAnalysisContext,
) -> AnalysisSession {
    match resolved.cancellation() {
        Some(cancellation) => session.with_cancellation(std::sync::Arc::clone(cancellation)),
        None => session,
    }
}

pub(super) fn default_dead_code_options_for_context(
    resolved: &ProgrammaticAnalysisContext,
) -> DeadCodeOptions {
    DeadCodeOptions {
        finding_ids: Vec::new(),
        analysis: AnalysisOptions {
            root: Some(resolved.root().to_path_buf()),
            config_path: resolved.config_path().clone(),
            no_cache: resolved.no_cache(),
            threads: Some(resolved.threads()),
            production_override: resolved.production_override(),
            changed_since: resolved.changed_since().map(str::to_owned),
            workspace: resolved.workspace().map(<[String]>::to_vec),
            changed_workspaces: resolved.changed_workspaces().map(str::to_owned),
            explain: resolved.explain_enabled(),
            ..AnalysisOptions::default()
        },
        filters: DeadCodeFilters::default(),
        files: Vec::new(),
        include_entry_exports: false,
    }
}

fn configure_project_for_dead_code(
    mut project_config: ProjectConfig,
    options: &DeadCodeOptions,
) -> ProjectConfig {
    if options.include_entry_exports {
        project_config.config.include_entry_exports = true;
    }
    activate_explicit_dead_code_opt_ins(&options.filters, &mut project_config.config.rules);
    project_config
}

fn activate_explicit_dead_code_opt_ins(
    filters: &DeadCodeFilters,
    rules: &mut fallow_config::RulesConfig,
) {
    if filters.absent_component_props
        && rules.absent_component_props == fallow_config::Severity::Off
    {
        rules.absent_component_props = fallow_config::Severity::Warn;
    }
    if filters.private_type_leaks && rules.private_type_leaks == fallow_config::Severity::Off {
        rules.private_type_leaks = fallow_config::Severity::Warn;
    }
    if filters.deprecated_exports_in_use
        && rules.deprecated_exports_in_use == fallow_config::Severity::Off
    {
        rules.deprecated_exports_in_use = fallow_config::Severity::Warn;
    }
}

fn apply_dead_code_scope(
    options: &DeadCodeOptions,
    resolved: &ProgrammaticAnalysisContext,
    session: &AnalysisSession,
    change_scope: &ChangeScope,
    results: &mut AnalysisResults,
) -> ProgrammaticResult<()> {
    let workspace_roots = workspace_roots_for_session(resolved, session.workspaces())?;
    let files = file_scope(options, session.root());
    fallow_engine::dead_code::apply_scope(
        results,
        &fallow_engine::dead_code::DeadCodeScope {
            workspace_roots: workspace_roots.as_deref(),
            changes: Some(change_scope),
            diff: resolved.diff.as_ref().map(|diff| (diff, session.root())),
            files: files.as_ref(),
        },
        session.config(),
    );
    Ok(())
}

/// The `files` option resolved against the root, or `None` when it is empty.
fn file_scope(options: &DeadCodeOptions, root: &Path) -> Option<FxHashSet<std::path::PathBuf>> {
    if options.files.is_empty() {
        return None;
    }
    Some(
        options
            .files
            .iter()
            .map(|path| {
                if is_absolute_path_any_platform(path) {
                    path.clone()
                } else {
                    root.join(path)
                }
            })
            .collect(),
    )
}

fn apply_dead_code_filters(filters: &DeadCodeFilters, results: &mut AnalysisResults) {
    if !dead_code_filters_active(filters) {
        return;
    }
    apply_dead_code_core_filters(filters, results);
    apply_dead_code_component_filters(filters, results);
    apply_dead_code_graph_filters(filters, results);
    apply_dead_code_policy_filters(filters, results);
    apply_dead_code_catalog_filters(filters, results);
}

fn dead_code_filters_active(filters: &DeadCodeFilters) -> bool {
    filters.unused_files
        || filters.unused_exports
        || filters.unused_deps
        || filters.unused_types
        || filters.private_type_leaks
        || filters.deprecated_exports_in_use
        || filters.unused_enum_members
        || filters.unused_class_members
        || filters.unused_store_members
        || filters.unprovided_injects
        || filters.unrendered_components
        || filters.unused_component_props
        || filters.absent_component_props
        || filters.unused_component_emits
        || filters.unused_component_inputs
        || filters.unused_component_outputs
        || filters.unused_svelte_events
        || filters.unused_server_actions
        || filters.unused_load_data_keys
        || filters.unresolved_imports
        || filters.unlisted_deps
        || filters.duplicate_exports
        || filters.circular_deps
        || filters.re_export_cycles
        || filters.package_cycles
        || filters.boundary_violations
        || filters.policy_violations
        || filters.stale_suppressions
        || filters.unused_catalog_entries
        || filters.empty_catalog_groups
        || filters.unresolved_catalog_references
        || filters.unused_dependency_overrides
        || filters.misconfigured_dependency_overrides
}

fn apply_dead_code_core_filters(filters: &DeadCodeFilters, results: &mut AnalysisResults) {
    if !filters.unused_files {
        results.unused_files.clear();
    }
    if !filters.unused_exports {
        results.unused_exports.clear();
    }
    if !filters.unused_types {
        results.unused_types.clear();
    }
    if !filters.private_type_leaks {
        results.private_type_leaks.clear();
    }
    if !filters.deprecated_exports_in_use {
        results.deprecated_exports_in_use.clear();
    }
    if !filters.unused_deps {
        clear_dead_code_dependency_findings(results);
    }
    if !filters.unused_enum_members {
        results.unused_enum_members.clear();
    }
    if !filters.unused_class_members {
        results.unused_class_members.clear();
    }
    if !filters.unused_store_members {
        results.unused_store_members.clear();
    }
    if !filters.unlisted_deps {
        results.unlisted_dependencies.clear();
    }
}

fn clear_dead_code_dependency_findings(results: &mut AnalysisResults) {
    results.unused_dependencies.clear();
    results.unused_dev_dependencies.clear();
    results.unused_optional_dependencies.clear();
    results.type_only_dependencies.clear();
    results.test_only_dependencies.clear();
    results.dev_dependencies_in_production.clear();
}

fn apply_dead_code_component_filters(filters: &DeadCodeFilters, results: &mut AnalysisResults) {
    if !filters.unprovided_injects {
        results.unprovided_injects.clear();
    }
    if !filters.unrendered_components {
        results.unrendered_components.clear();
    }
    if !filters.absent_component_props {
        results.absent_component_props.clear();
    }
    if !filters.unused_component_props {
        results.unused_component_props.clear();
    }
    if !filters.unused_component_emits {
        results.unused_component_emits.clear();
    }
    if !filters.unused_component_inputs {
        results.unused_component_inputs.clear();
    }
    if !filters.unused_component_outputs {
        results.unused_component_outputs.clear();
    }
    if !filters.unused_svelte_events {
        results.unused_svelte_events.clear();
    }
    if !filters.unused_server_actions {
        results.unused_server_actions.clear();
    }
    if !filters.unused_load_data_keys {
        results.unused_load_data_keys.clear();
    }
    if !filters.unresolved_imports {
        results.unresolved_imports.clear();
    }
}

fn apply_dead_code_graph_filters(filters: &DeadCodeFilters, results: &mut AnalysisResults) {
    if !filters.duplicate_exports {
        results.duplicate_exports.clear();
    }
    if !filters.circular_deps {
        results.circular_dependencies.clear();
    }
    if !filters.re_export_cycles {
        results.re_export_cycles.clear();
    }
    if !filters.package_cycles {
        results.package_cycles.clear();
    }
    if !filters.boundary_violations {
        results.boundary_violations.clear();
        results.boundary_coverage_violations.clear();
        results.boundary_call_violations.clear();
    }
}

fn apply_dead_code_policy_filters(filters: &DeadCodeFilters, results: &mut AnalysisResults) {
    if !filters.policy_violations {
        results.policy_violations.clear();
    }
    if !filters.stale_suppressions {
        results.stale_suppressions.clear();
    }
}

fn apply_dead_code_catalog_filters(filters: &DeadCodeFilters, results: &mut AnalysisResults) {
    if !filters.unused_catalog_entries {
        results.unused_catalog_entries.clear();
    }
    if !filters.empty_catalog_groups {
        results.empty_catalog_groups.clear();
    }
    if !filters.unresolved_catalog_references {
        results.unresolved_catalog_references.clear();
    }
    if !filters.unused_dependency_overrides {
        results.unused_dependency_overrides.clear();
    }
    if !filters.misconfigured_dependency_overrides {
        results.misconfigured_dependency_overrides.clear();
    }
}
