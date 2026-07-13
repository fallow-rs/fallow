//! Command-neutral health analysis execution.
//!
//! This module owns the health pipeline (scoring, hotspots, targets, grouping,
//! coverage gaps, vital signs, report assembly) so that the CLI and the
//! programmatic API can both run health analysis without the CLI orchestration
//! layer. CLI-only concerns (config loading, telemetry sinks, the runtime
//! coverage sidecar, ownership-resolver construction, and error rendering) are
//! threaded in through the [`HealthSeams`] carrier and the typed result.

use std::time::Instant;

use super::{HealthError, HealthExecutionOptions, HealthSeams};

use super::core_pipeline::{
    HealthCoreSectionsInput, HealthPreparedCore, prepare_health_core_sections,
};
use super::output_build::{
    HealthOutputContext, HealthOutputContextInput, build_health_output_parts,
    prepare_health_output_context,
};
use super::pipeline::{
    HealthPipelineInputs, HealthPipelineRunInputs, HealthPipelineTimings, HealthScopeInputs,
};
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
/// Returns a typed [`HealthError`] for a failing analysis or invalid input. The
/// CLI boundary renders `HealthError::Message` and honors the exit code of an
/// already-printed `HealthError::Printed`.
pub fn execute_health_inner<'a, R: super::HealthGroupResolver>(
    opts: &HealthOptions<'a>,
    input: HealthPipelineInputs,
    scope_inputs: HealthScopeInputs<'a, R>,
    seams: &HealthSeams<'_>,
) -> Result<HealthResultGeneric<R>, HealthError> {
    execute_health_inner_impl(opts, input.into(), scope_inputs, seams)
}

pub(super) fn execute_health_inner_shared<'a, R, M>(
    opts: &HealthOptions<'a>,
    input: HealthPipelineRunInputs<M>,
    scope_inputs: HealthScopeInputs<'a, R>,
    seams: &HealthSeams<'_>,
) -> Result<HealthResultGeneric<R>, HealthError>
where
    R: super::HealthGroupResolver,
    M: AsRef<[fallow_types::extract::ModuleInfo]>,
{
    execute_health_inner_impl(opts, input, scope_inputs, seams)
}

fn execute_health_inner_impl<'a, R, M>(
    opts: &HealthOptions<'a>,
    input: HealthPipelineRunInputs<M>,
    scope_inputs: HealthScopeInputs<'a, R>,
    seams: &HealthSeams<'_>,
) -> Result<HealthResultGeneric<R>, HealthError>
where
    R: super::HealthGroupResolver,
    M: AsRef<[fallow_types::extract::ModuleInfo]>,
{
    let start = Instant::now();
    let HealthPipelineRunInputs {
        config,
        files,
        modules,
        config_ms,
        discover_ms,
        parse_ms,
        parse_cpu_ms,
        shared_parse,
        pre_computed_analysis,
        dead_code_results,
        styling_artifacts,
        pre_computed_duplication,
        workspaces,
        workspace_diagnostics,
    } = input;
    let modules = modules.as_ref();
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
        modules,
        scope: &scope,
        pre_computed_analysis,
        pre_computed_duplication,
        seams,
    })?;

    let HealthOutputContext { build, sections } =
        prepare_health_output_context(HealthOutputContextInput {
            config: &config,
            modules,
            scope: &scope,
            needs_file_scores,
            report_coverage_gaps,
            has_istanbul_coverage,
            findings_data,
            analysis_data,
            derived_sections,
            vital_data,
            timings,
            workspaces: &workspaces,
            start: &start,
        });

    let output = build_health_output_parts(opts, &build, sections);

    Ok(finalize_health_result(HealthFinalizeInput {
        opts,
        config,
        files: &files,
        modules,
        scope,
        output,
        elapsed: start.elapsed(),
        should_fail_on_coverage_gaps: enforce_coverage_gaps,
        dead_code_results: dead_code_results.as_ref(),
        styling_artifacts: styling_artifacts.as_ref(),
        workspace_diagnostics,
    }))
}

#[cfg(test)]
#[path = "execute_tests.rs"]
mod execute_tests;
