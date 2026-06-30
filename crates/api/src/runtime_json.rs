//! JSON protocol serializers for typed programmatic runtime output.
//!
//! Runtime entry points return typed output from [`crate::runtime`]. CLI, MCP,
//! NAPI, and other protocol surfaces call these serializers at their JSON
//! boundary.

use crate::{
    ProgrammaticError,
    runtime::{
        BoundaryViolationsProgrammaticOutput, CircularDependenciesProgrammaticOutput,
        DeadCodeProgrammaticOutput, DuplicationProgrammaticOutput, FeatureFlagsProgrammaticOutput,
        HealthJsonReportInput, HealthProgrammaticOutput, TraceCloneProgrammaticOutput,
        TraceDependencyProgrammaticOutput, TraceExportProgrammaticOutput,
        TraceFileProgrammaticOutput, serialize_health_report_json,
    },
};
use fallow_output::{
    CheckOutput, GroupByMode, RootEnvelopeMode, serialize_check_json_output,
    serialize_dupes_json_output, serialize_feature_flags_json_output, strip_root_prefix,
};
use serde::Serialize;
use std::path::Path;

type ProgrammaticResult<T> = Result<T, ProgrammaticError>;

/// Serialize typed dead-code output into the stable JSON compatibility contract.
///
/// # Errors
///
/// Returns a structured error if the output contract cannot be serialized.
pub fn serialize_dead_code_programmatic_json(
    output: DeadCodeProgrammaticOutput,
) -> ProgrammaticResult<serde_json::Value> {
    let DeadCodeProgrammaticOutput {
        output,
        root,
        envelope_mode,
        telemetry_analysis_run_id,
    } = output;
    serialize_check_programmatic_output(
        output,
        &root,
        envelope_mode,
        telemetry_analysis_run_id.as_deref(),
        "dead-code",
        "FALLOW_SERIALIZE_DEAD_CODE_REPORT",
    )
}

/// Serialize typed circular-dependency output into the JSON compatibility contract.
///
/// # Errors
///
/// Returns a structured error if the output contract cannot be serialized.
pub fn serialize_circular_dependencies_programmatic_json(
    output: CircularDependenciesProgrammaticOutput,
) -> ProgrammaticResult<serde_json::Value> {
    let CircularDependenciesProgrammaticOutput {
        output,
        root,
        envelope_mode,
        telemetry_analysis_run_id,
    } = output;
    serialize_check_programmatic_output(
        output,
        &root,
        envelope_mode,
        telemetry_analysis_run_id.as_deref(),
        "circular-dependencies",
        "FALLOW_SERIALIZE_CIRCULAR_DEPENDENCIES_REPORT",
    )
}

/// Serialize typed boundary-family output into the JSON compatibility contract.
///
/// # Errors
///
/// Returns a structured error if the output contract cannot be serialized.
pub fn serialize_boundary_violations_programmatic_json(
    output: BoundaryViolationsProgrammaticOutput,
) -> ProgrammaticResult<serde_json::Value> {
    let BoundaryViolationsProgrammaticOutput {
        output,
        root,
        envelope_mode,
        telemetry_analysis_run_id,
    } = output;
    serialize_check_programmatic_output(
        output,
        &root,
        envelope_mode,
        telemetry_analysis_run_id.as_deref(),
        "boundary-violations",
        "FALLOW_SERIALIZE_BOUNDARY_VIOLATIONS_REPORT",
    )
}

fn serialize_check_programmatic_output(
    output: CheckOutput,
    root: &Path,
    envelope_mode: RootEnvelopeMode,
    telemetry_analysis_run_id: Option<&str>,
    context: &'static str,
    code: &'static str,
) -> ProgrammaticResult<serde_json::Value> {
    let mut json = serialize_check_json_output(output, envelope_mode, telemetry_analysis_run_id)
        .map_err(|err| {
            ProgrammaticError::new(format!("failed to serialize {context} report: {err}"), 2)
                .with_code(code)
                .with_context(context)
        })?;
    let root_prefix = format!("{}/", root.display());
    strip_root_prefix(&mut json, &root_prefix);
    Ok(json)
}

/// Serialize typed duplication output into the JSON compatibility contract.
///
/// # Errors
///
/// Returns a structured error if the output contract cannot be serialized.
pub fn serialize_duplication_programmatic_json(
    output: DuplicationProgrammaticOutput,
) -> ProgrammaticResult<serde_json::Value> {
    let DuplicationProgrammaticOutput {
        output,
        root,
        envelope_mode,
        telemetry_analysis_run_id,
    } = output;
    let mut json =
        serialize_dupes_json_output(output, envelope_mode, telemetry_analysis_run_id.as_deref())
            .map_err(|err| {
                ProgrammaticError::new(format!("failed to serialize duplication report: {err}"), 2)
                    .with_code("FALLOW_SERIALIZE_DUPLICATION_REPORT")
                    .with_context("dupes")
            })?;
    let root_prefix = format!("{}/", root.display());
    strip_root_prefix(&mut json, &root_prefix);
    Ok(json)
}

/// Serialize typed feature-flag output into the JSON compatibility contract.
///
/// # Errors
///
/// Returns a structured error if the output contract cannot be serialized.
pub fn serialize_feature_flags_programmatic_json(
    output: FeatureFlagsProgrammaticOutput,
) -> ProgrammaticResult<serde_json::Value> {
    serialize_feature_flags_json_output(
        output.output,
        output.envelope_mode,
        output.telemetry_analysis_run_id.as_deref(),
    )
    .map_err(|err| {
        ProgrammaticError::new(
            format!("failed to serialize feature flags report: {err}"),
            2,
        )
        .with_code("FALLOW_SERIALIZE_FEATURE_FLAGS_REPORT")
        .with_context("feature-flags")
    })
}

/// Serialize typed export-trace output into the JSON compatibility contract.
///
/// # Errors
///
/// Returns a structured error if the trace output cannot be serialized.
pub fn serialize_trace_export_programmatic_json(
    output: TraceExportProgrammaticOutput,
) -> ProgrammaticResult<serde_json::Value> {
    serialize_trace_programmatic_output(
        output.output,
        "export trace",
        "FALLOW_SERIALIZE_TRACE_EXPORT",
        "trace_export",
    )
}

/// Serialize typed file-trace output into the JSON compatibility contract.
///
/// # Errors
///
/// Returns a structured error if the trace output cannot be serialized.
pub fn serialize_trace_file_programmatic_json(
    output: TraceFileProgrammaticOutput,
) -> ProgrammaticResult<serde_json::Value> {
    serialize_trace_programmatic_output(
        output.output,
        "file trace",
        "FALLOW_SERIALIZE_TRACE_FILE",
        "trace_file",
    )
}

/// Serialize typed dependency-trace output into the JSON compatibility contract.
///
/// # Errors
///
/// Returns a structured error if the trace output cannot be serialized.
pub fn serialize_trace_dependency_programmatic_json(
    output: TraceDependencyProgrammaticOutput,
) -> ProgrammaticResult<serde_json::Value> {
    serialize_trace_programmatic_output(
        output.output,
        "dependency trace",
        "FALLOW_SERIALIZE_TRACE_DEPENDENCY",
        "trace_dependency",
    )
}

/// Serialize typed clone-trace output into the JSON compatibility contract.
///
/// # Errors
///
/// Returns a structured error if the trace output cannot be serialized.
pub fn serialize_trace_clone_programmatic_json(
    output: TraceCloneProgrammaticOutput,
) -> ProgrammaticResult<serde_json::Value> {
    serialize_trace_programmatic_output(
        output.output,
        "clone trace",
        "FALLOW_SERIALIZE_TRACE_CLONE",
        "trace_clone",
    )
}

fn serialize_trace_programmatic_output<T: Serialize>(
    output: T,
    context: &'static str,
    code: &'static str,
    error_context: &'static str,
) -> ProgrammaticResult<serde_json::Value> {
    serde_json::to_value(output).map_err(|err| {
        ProgrammaticError::new(format!("failed to serialize {context}: {err}"), 2)
            .with_code(code)
            .with_context(error_context)
    })
}

/// Serialize typed health / complexity output into the JSON compatibility contract.
///
/// # Errors
///
/// Returns a structured error if the health output contract cannot be serialized.
pub fn serialize_health_programmatic_json(
    output: HealthProgrammaticOutput,
) -> ProgrammaticResult<serde_json::Value> {
    let HealthProgrammaticOutput {
        report,
        grouping,
        root,
        elapsed,
        explain,
        workspace_diagnostics,
        next_steps,
        envelope_mode,
        telemetry_analysis_run_id,
    } = output;
    let (grouped_by, groups) = grouping.map_or((None, None), |grouping| {
        (
            group_by_mode_from_label(grouping.mode),
            Some(grouping.groups),
        )
    });
    serialize_health_report_json(HealthJsonReportInput {
        report,
        root: &root,
        elapsed,
        explain,
        grouped_by,
        groups,
        workspace_diagnostics,
        next_steps,
        envelope_mode,
        telemetry_analysis_run_id: telemetry_analysis_run_id.as_deref(),
    })
    .map_err(|err| {
        ProgrammaticError::new(format!("failed to serialize health report: {err}"), 2)
            .with_code("FALLOW_SERIALIZE_HEALTH_REPORT")
            .with_context("health")
    })
}

fn group_by_mode_from_label(label: &str) -> Option<GroupByMode> {
    match label {
        "owner" => Some(GroupByMode::Owner),
        "directory" => Some(GroupByMode::Directory),
        "package" => Some(GroupByMode::Package),
        "section" => Some(GroupByMode::Section),
        _ => None,
    }
}
