//! Engine-owned health runners for non-CLI callers.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

use fallow_config::ProductionAnalysis;
use fallow_types::output_format::OutputFormat;
use rustc_hash::FxHashSet;

use crate::{
    duplicates::DuplicationReport,
    project_config::{ProjectConfigOptions, config_for_project_analysis},
    results::DeadCodeAnalysisArtifacts,
    session::{AnalysisSession, ParsedAnalysisSessionParts},
};

use super::{
    HealthAnalysisResult, HealthExecutionOptions, HealthPipelineInputs, HealthScopeInputs,
    HealthSeams, NoGroupResolver, RuntimeCoverageOptions, RuntimeCoverageSeamInput,
    validate_health_churn_file,
};

/// Run health analysis without a presentation grouping resolver.
///
/// This runner owns config loading, discovery, parser-cache use, parsing, and
/// command-neutral health execution for API and NAPI callers. CLI-only concerns
/// still stay outside this path: runtime coverage sidecar execution, grouping
/// resolver construction, process-global telemetry, and error rendering.
///
/// # Errors
///
/// Returns the health command exit code for invalid inputs or analysis failures.
pub fn run_ungrouped_health(
    options: &HealthExecutionOptions<'_>,
    ws_roots: Option<Vec<PathBuf>>,
) -> Result<HealthAnalysisResult<NoGroupResolver>, ExitCode> {
    validate_health_churn_file(options).map_err(|_| ExitCode::from(2))?;

    let start = Instant::now();
    let project_config = config_for_project_analysis(
        options.root,
        options.config_path.as_deref(),
        ProjectConfigOptions {
            output: OutputFormat::Human,
            no_cache: options.no_cache,
            threads: options.threads,
            production_override: options.production_override,
            quiet: true,
            analysis: ProductionAnalysis::Health,
        },
    )
    .map_err(|_| ExitCode::from(2))?;
    let config_ms = start.elapsed().as_secs_f64() * 1000.0;

    let session = AnalysisSession::from_config(project_config);
    let changed_files = options
        .changed_since
        .and_then(|git_ref| session.changed_files_since(git_ref).ok());
    let parts = session.parsed_parts_uncached(true);
    let pre_computed_analysis =
        super::should_precompute_dead_code_analysis(options, session.config())
            .then(|| session.analyze_dead_code_with_parsed_modules(&parts.modules))
            .transpose()
            .map_err(|_| ExitCode::from(2))?;

    run_ungrouped_health_from_parts(HealthRunPartsInput {
        options,
        ws_roots,
        parts,
        changed_files,
        config_ms,
        shared_parse: false,
        pre_computed_analysis,
        pre_computed_duplication: None,
        styling_artifacts: None,
    })
}

/// Run health analysis from an existing analysis session.
///
/// This lets audit and other compound programmatic surfaces share config,
/// discovery, and parser-cache state across analysis families.
///
/// # Errors
///
/// Returns the health command exit code for invalid inputs or analysis failures.
pub fn run_ungrouped_health_with_session(
    options: &HealthExecutionOptions<'_>,
    ws_roots: Option<Vec<PathBuf>>,
    session: &AnalysisSession,
    changed_files: Option<Vec<PathBuf>>,
) -> Result<HealthAnalysisResult<NoGroupResolver>, ExitCode> {
    run_ungrouped_health_with_session_artifacts(
        options,
        ws_roots,
        session,
        changed_files,
        None,
        None,
    )
}

/// Run health analysis from an existing analysis session and retained
/// dead-code artifacts.
///
/// # Errors
///
/// Returns the health command exit code for invalid inputs or analysis failures.
pub fn run_ungrouped_health_with_session_artifacts(
    options: &HealthExecutionOptions<'_>,
    ws_roots: Option<Vec<PathBuf>>,
    session: &AnalysisSession,
    changed_files: Option<Vec<PathBuf>>,
    pre_computed_analysis: Option<DeadCodeAnalysisArtifacts>,
    pre_computed_duplication: Option<DuplicationReport>,
) -> Result<HealthAnalysisResult<NoGroupResolver>, ExitCode> {
    validate_health_churn_file(options).map_err(|_| ExitCode::from(2))?;

    let changed_files = changed_files.map(FxHashSet::from_iter).or_else(|| {
        options
            .changed_since
            .and_then(|git_ref| session.changed_files_since(git_ref).ok())
    });
    let parts = session.parsed_parts(true);
    let shared_parse = parts.parse_ms == 0.0;

    let styling_artifacts = options.css.then(|| session.styling_analysis_artifacts());
    run_ungrouped_health_from_parts(HealthRunPartsInput {
        options,
        ws_roots,
        parts,
        changed_files,
        config_ms: 0.0,
        shared_parse,
        pre_computed_analysis,
        pre_computed_duplication,
        styling_artifacts,
    })
}

struct HealthRunPartsInput<'a> {
    options: &'a HealthExecutionOptions<'a>,
    ws_roots: Option<Vec<PathBuf>>,
    parts: ParsedAnalysisSessionParts,
    changed_files: Option<FxHashSet<PathBuf>>,
    config_ms: f64,
    shared_parse: bool,
    pre_computed_analysis: Option<DeadCodeAnalysisArtifacts>,
    pre_computed_duplication: Option<DuplicationReport>,
    styling_artifacts: Option<super::StylingAnalysisArtifacts>,
}

fn run_ungrouped_health_from_parts(
    input: HealthRunPartsInput<'_>,
) -> Result<HealthAnalysisResult<NoGroupResolver>, ExitCode> {
    let HealthRunPartsInput {
        options,
        ws_roots,
        parts,
        changed_files,
        config_ms,
        shared_parse,
        pre_computed_analysis,
        pre_computed_duplication,
        styling_artifacts,
    } = input;
    let config = parts.config;
    let files = parts.files;
    let modules = parts.modules;
    let workspaces = parts.workspaces;
    let workspace_diagnostics = parts.workspace_diagnostics;
    let parse_ms = parts.parse_ms;
    let parse_cpu_ms = parts.parse_cpu_ms;

    let scope_inputs = HealthScopeInputs::<NoGroupResolver> {
        changed_files,
        diff_index: options.diff_index,
        ws_roots,
        group_resolver: None,
    };
    let seams = HealthSeams {
        runtime_coverage_analyzer: &programmatic_runtime_coverage_seam,
        note_graph_structure: &|_module_count, _edge_count| {},
    };

    super::execute_health_inner(
        options,
        HealthPipelineInputs {
            config,
            files,
            modules,
            config_ms,
            discover_ms: 0.0,
            parse_ms,
            parse_cpu_ms,
            shared_parse,
            pre_computed_analysis,
            dead_code_results: None,
            styling_artifacts,
            pre_computed_duplication,
            workspaces,
            workspace_diagnostics,
        },
        scope_inputs,
        &seams,
    )
}

fn programmatic_runtime_coverage_seam(
    _options: &RuntimeCoverageOptions,
    _input: RuntimeCoverageSeamInput<'_>,
) -> Result<fallow_output::RuntimeCoverageReport, ExitCode> {
    Err(ExitCode::from(2))
}
