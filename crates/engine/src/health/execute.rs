//! Command-neutral health analysis execution.
//!
//! This module owns the health pipeline (scoring, hotspots, targets, grouping,
//! coverage gaps, vital signs, report assembly) so that the CLI and the
//! programmatic API can both run health analysis without the CLI orchestration
//! layer. CLI-only concerns (config loading, telemetry sinks, the runtime
//! coverage sidecar, ownership-resolver construction, and error rendering) are
//! threaded in through the [`HealthSeams`] carrier and the typed result.

use std::process::ExitCode;
use std::time::Instant;

use super::{HealthExecutionOptions, HealthSeams};

use super::core_pipeline::{
    HealthCoreSectionsInput, HealthPreparedCore, prepare_health_core_sections,
};
use super::output_build::{
    HealthOutputContext, HealthOutputContextInput, build_health_output_parts,
    prepare_health_output_context,
};
use super::pipeline::{HealthPipelineInputs, HealthPipelineTimings, HealthScopeInputs};
use super::result::{HealthFinalizeInput, finalize_health_result};
use super::scope::prepare_health_scope;

pub type HealthOptions<'a> = HealthExecutionOptions<'a>;

/// Typed health analysis result generic over the CLI-owned grouping resolver.
pub type HealthResultGeneric<R> = super::HealthAnalysisResult<R>;

/// Run the command-neutral health analysis pipeline.
///
/// Config loading, discovery, and parsing are the CLI's responsibility (they
/// touch the parser cache and config telemetry); the caller passes the resolved
/// [`HealthPipelineInputs`] plus the pre-resolved [`HealthScopeInputs`] and the
/// [`HealthSeams`] callbacks. The returned result carries the typed health
/// report plus the caller's grouping resolver for downstream rendering.
///
/// # Errors
///
/// Returns the CLI exit code emitted by a failing analysis or invalid input.
pub fn execute_health_inner<'a, R: super::HealthGroupResolver>(
    opts: &HealthOptions<'a>,
    input: HealthPipelineInputs,
    scope_inputs: HealthScopeInputs<'a, R>,
    seams: &HealthSeams<'_>,
) -> Result<HealthResultGeneric<R>, ExitCode> {
    let start = Instant::now();
    let HealthPipelineInputs {
        config,
        files,
        modules,
        config_ms,
        discover_ms,
        parse_ms,
        parse_cpu_ms,
        shared_parse,
        pre_computed_analysis,
        workspace_diagnostics,
    } = input;
    let timings = HealthPipelineTimings {
        config: config_ms,
        discover: discover_ms,
        parse: parse_ms,
        parse_cpu: parse_cpu_ms,
        shared_parse,
    };

    let scope = prepare_health_scope(opts, &config, &files, scope_inputs);

    let HealthPreparedCore {
        findings_data,
        analysis_data,
        derived_sections,
        vital_data,
        report_coverage_gaps,
        enforce_coverage_gaps,
        has_istanbul_coverage,
        needs_file_scores,
    } = prepare_health_core_sections(HealthCoreSectionsInput {
        opts,
        config: &config,
        files: &files,
        modules: &modules,
        scope: &scope,
        pre_computed_analysis,
        seams,
    })?;

    let HealthOutputContext { build, sections } =
        prepare_health_output_context(HealthOutputContextInput {
            config: &config,
            files: &files,
            modules: &modules,
            scope: &scope,
            needs_file_scores,
            report_coverage_gaps,
            has_istanbul_coverage,
            findings_data,
            analysis_data,
            derived_sections,
            vital_data,
            timings,
            start: &start,
        });

    let output = build_health_output_parts(opts, &build, sections);

    Ok(finalize_health_result(HealthFinalizeInput {
        opts,
        config,
        files: &files,
        scope,
        output,
        elapsed: start.elapsed(),
        should_fail_on_coverage_gaps: enforce_coverage_gaps,
        workspace_diagnostics,
    }))
}

#[cfg(test)]
#[path = "execute_tests.rs"]
mod execute_tests;
