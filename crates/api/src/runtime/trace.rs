use fallow_engine::AnalysisSession;
use fallow_types::output_format::OutputFormat;
use rustc_hash::FxHashSet;

use crate::{
    ProgrammaticAnalysisContext, ProgrammaticError, TraceCloneOptions,
    TraceCloneProgrammaticOutput, TraceCloneTarget, TraceDependencyOptions,
    TraceDependencyProgrammaticOutput, TraceExportOptions, TraceExportProgrammaticOutput,
    TraceFileOptions, TraceFileProgrammaticOutput,
};

use super::{ProgrammaticResult, duplication, resolve_programmatic_analysis_context};

struct TraceArtifacts {
    graph: fallow_engine::RetainedModuleGraph,
    script_used_packages: FxHashSet<String>,
}

/// Trace why an export is considered used or unused.
///
/// # Errors
///
/// Returns a structured programmatic error for invalid options, config load
/// failures, graph construction failures, or missing trace targets.
pub fn run_trace_export(
    options: &TraceExportOptions,
) -> ProgrammaticResult<TraceExportProgrammaticOutput> {
    validate_non_empty("file", &options.file)?;
    validate_non_empty("export_name", &options.export_name)?;
    let resolved = resolve_programmatic_analysis_context(&options.analysis)?;
    resolved.install(|| {
        let session = load_trace_session(&resolved)?;
        let artifacts = trace_artifacts(&session)?;
        let output = fallow_engine::trace_export(
            &artifacts.graph,
            session.root(),
            &options.file,
            &options.export_name,
        )
        .ok_or_else(|| {
            ProgrammaticError::new(
                format!(
                    "export '{}' not found in '{}'",
                    options.export_name, options.file
                ),
                2,
            )
            .with_code("FALLOW_TRACE_TARGET_NOT_FOUND")
            .with_context("trace_export")
        })?;
        Ok(TraceExportProgrammaticOutput { output })
    })
}

/// Trace all graph edges for a file.
///
/// # Errors
///
/// Returns a structured programmatic error for invalid options, config load
/// failures, graph construction failures, or missing trace targets.
pub fn run_trace_file(
    options: &TraceFileOptions,
) -> ProgrammaticResult<TraceFileProgrammaticOutput> {
    validate_non_empty("file", &options.file)?;
    let resolved = resolve_programmatic_analysis_context(&options.analysis)?;
    resolved.install(|| {
        let session = load_trace_session(&resolved)?;
        let artifacts = trace_artifacts(&session)?;
        let output = fallow_engine::trace_file(&artifacts.graph, session.root(), &options.file)
            .ok_or_else(|| {
                ProgrammaticError::new(
                    format!("file '{}' not found in module graph", options.file),
                    2,
                )
                .with_code("FALLOW_TRACE_TARGET_NOT_FOUND")
                .with_context("trace_file")
            })?;
        Ok(TraceFileProgrammaticOutput { output })
    })
}

/// Trace where a dependency is used.
///
/// # Errors
///
/// Returns a structured programmatic error for invalid options, config load, or
/// graph construction failures.
pub fn run_trace_dependency(
    options: &TraceDependencyOptions,
) -> ProgrammaticResult<TraceDependencyProgrammaticOutput> {
    validate_non_empty("package_name", &options.package_name)?;
    let resolved = resolve_programmatic_analysis_context(&options.analysis)?;
    resolved.install(|| {
        let session = load_trace_session(&resolved)?;
        let artifacts = trace_artifacts(&session)?;
        let output = fallow_engine::trace_dependency(
            &artifacts.graph,
            session.root(),
            &options.package_name,
            &artifacts.script_used_packages,
        );
        Ok(TraceDependencyProgrammaticOutput { output })
    })
}

/// Trace duplicate-code groups by location or stable fingerprint.
///
/// # Errors
///
/// Returns a structured programmatic error for invalid options, config load
/// failures, duplicate detection failures, or missing trace targets.
pub fn run_trace_clone(
    options: &TraceCloneOptions,
) -> ProgrammaticResult<TraceCloneProgrammaticOutput> {
    validate_trace_clone_target(&options.target)?;
    let resolved = resolve_programmatic_analysis_context(&options.duplication.analysis)?;
    resolved.install(|| {
        let session = duplication::load_duplication_session(&options.duplication, &resolved)?;
        let dupes_config =
            duplication::build_dupes_config(&options.duplication, &session.config().duplicates);
        let cache_dir = (!resolved.no_cache).then_some(session.config().cache_dir.as_path());
        let report = session
            .find_duplicates_with_defaults(&dupes_config, cache_dir)
            .report;
        let (trace, not_found) = match &options.target {
            TraceCloneTarget::Location { file, line } => (
                fallow_engine::trace_clone(&report, session.root(), file, *line),
                format!("no clone found at {file}:{line}"),
            ),
            TraceCloneTarget::Fingerprint(fingerprint) => (
                fallow_engine::trace_clone_by_fingerprint(&report, session.root(), fingerprint),
                format!("no clone group with fingerprint {fingerprint}"),
            ),
        };
        if trace.matched_instance.is_none() {
            return Err(ProgrammaticError::new(not_found, 2)
                .with_code("FALLOW_TRACE_TARGET_NOT_FOUND")
                .with_context("trace_clone"));
        }
        Ok(TraceCloneProgrammaticOutput { output: trace })
    })
}

fn validate_non_empty(field: &str, value: &str) -> ProgrammaticResult<()> {
    if value.trim().is_empty() {
        return Err(
            ProgrammaticError::new(format!("{field} must not be empty"), 2)
                .with_code("FALLOW_INVALID_TRACE_OPTIONS")
                .with_context(field.to_string()),
        );
    }
    Ok(())
}

fn validate_trace_clone_target(target: &TraceCloneTarget) -> ProgrammaticResult<()> {
    match target {
        TraceCloneTarget::Location { file, line } => {
            validate_non_empty("file", file)?;
            if *line == 0 {
                return Err(ProgrammaticError::new("line must be greater than 0", 2)
                    .with_code("FALLOW_INVALID_TRACE_OPTIONS")
                    .with_context("trace_clone.line"));
            }
        }
        TraceCloneTarget::Fingerprint(fingerprint) => {
            validate_non_empty("fingerprint", fingerprint)?;
        }
    }
    Ok(())
}

fn load_trace_session(
    resolved: &ProgrammaticAnalysisContext,
) -> ProgrammaticResult<AnalysisSession> {
    let project_config = fallow_engine::config_for_project_analysis(
        &resolved.root,
        resolved.config_path.as_deref(),
        fallow_engine::ProjectConfigOptions {
            output: OutputFormat::Json,
            no_cache: resolved.no_cache,
            threads: resolved.threads,
            production_override: resolved.production_override,
            quiet: true,
            analysis: fallow_config::ProductionAnalysis::DeadCode,
        },
    )
    .map_err(|err| {
        ProgrammaticError::new(format!("failed to load config: {err}"), 2)
            .with_code("FALLOW_CONFIG_LOAD_FAILED")
            .with_context("analysis.configPath")
    })?;
    Ok(AnalysisSession::from_config(project_config))
}

fn trace_artifacts(session: &AnalysisSession) -> ProgrammaticResult<TraceArtifacts> {
    let artifacts = session
        .analyze_dead_code_with_artifacts(false, true)
        .map_err(|err| {
            ProgrammaticError::new(format!("trace analysis failed: {err}"), 2)
                .with_code("FALLOW_TRACE_FAILED")
                .with_context("trace")
        })?;
    let graph = artifacts.graph.ok_or_else(|| {
        ProgrammaticError::new("trace requires a retained module graph", 2)
            .with_code("FALLOW_TRACE_GRAPH_UNAVAILABLE")
            .with_context("trace.graph")
    })?;
    Ok(TraceArtifacts {
        graph,
        script_used_packages: artifacts.script_used_packages,
    })
}
