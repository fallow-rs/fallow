//! Engine-owned health runners for non-CLI callers.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

use fallow_config::ProductionAnalysis;
use fallow_types::output_format::OutputFormat;

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
    let project_config = crate::config_for_project_analysis(
        options.root,
        options.config_path.as_deref(),
        crate::ProjectConfigOptions {
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

    let session = crate::AnalysisSession::from_config(project_config);
    let parts = session.into_parsed_parts(true);
    let config = parts.config;
    let files = parts.files;
    let modules = parts.modules;
    let workspace_diagnostics = parts.workspace_diagnostics;
    let parse_ms = parts.parse_ms;
    let parse_cpu_ms = parts.parse_cpu_ms;

    let scope_inputs = HealthScopeInputs::<NoGroupResolver> {
        changed_files: options
            .changed_since
            .and_then(|git_ref| crate::changed_files(&config.root, git_ref).ok()),
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
            shared_parse: false,
            pre_computed_analysis: None,
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
