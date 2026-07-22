//! JSON protocol serializers for typed programmatic runtime output.
//!
//! Runtime entry points return typed output from [`crate::runtime`]. CLI, MCP,
//! NAPI, and other protocol surfaces call these serializers at their JSON
//! boundary.

use crate::{
    ProgrammaticError,
    runtime::{
        AuditProgrammaticOutput, BoundaryViolationsProgrammaticOutput,
        CircularDependenciesProgrammaticOutput, CombinedProgrammaticOutput,
        DeadCodeProgrammaticOutput, DecisionSurfaceProgrammaticOutput,
        DuplicationProgrammaticOutput, FeatureFlagsProgrammaticOutput, HealthJsonReportInput,
        HealthProgrammaticOutput, TraceCloneProgrammaticOutput, TraceDependencyProgrammaticOutput,
        TraceExportProgrammaticOutput, TraceFileProgrammaticOutput, serialize_health_report_json,
    },
};
use fallow_output::{
    CHECK_SCHEMA_VERSION, CheckOutput, GroupByMode, RootEnvelopeMode,
    build_decision_surface_output, serialize_check_json_output,
    serialize_decision_surface_json_output, serialize_dupes_json_output,
    serialize_feature_flags_json_output, strip_root_prefix,
};
use fallow_types::envelope::{ElapsedMs, SchemaVersion, ToolVersion};
use serde::Serialize;
use std::path::Path;
use std::time::Duration;

type ProgrammaticResult<T> = Result<T, ProgrammaticError>;

/// Serialize typed combined output into the stable JSON compatibility contract.
///
/// # Errors
///
/// Returns a structured error if one of the combined sections cannot serialize.
pub fn serialize_combined_programmatic_json(
    output: CombinedProgrammaticOutput,
) -> ProgrammaticResult<serde_json::Value> {
    let CombinedProgrammaticOutput {
        dead_code,
        duplication,
        health,
        root,
        elapsed,
        explain,
        next_steps,
        envelope_mode,
        telemetry_analysis_run_id,
    } = output;
    crate::serialize_combined_json(crate::CombinedJsonOutputInput {
        check: dead_code
            .as_ref()
            .map(|dead_code| crate::CombinedCheckJsonSection {
                results: &dead_code.output.results,
                root: &dead_code.root,
                elapsed: Duration::from_millis(dead_code.output.elapsed_ms.0),
                config_fixable: dead_code.config_fixable,
                extras: crate::CheckJsonExtraOutputs::default(),
            }),
        dupes: duplication
            .as_ref()
            .map(|duplication| &duplication.output.report),
        health: health.as_ref().map(|health| &health.report),
        root: &root,
        elapsed,
        explain,
        next_steps,
        envelope_mode,
        telemetry_analysis_run_id: telemetry_analysis_run_id.as_deref(),
    })
    .map_err(|err| {
        ProgrammaticError::new(format!("failed to serialize combined report: {err}"), 2)
            .with_code("FALLOW_SERIALIZE_COMBINED_REPORT")
            .with_context("combined")
    })
}

/// Serialize typed decision-surface output into the stable JSON contract.
///
/// # Errors
///
/// Returns a structured error if the decision-surface payload cannot serialize.
pub fn serialize_decision_surface_programmatic_json(
    output: DecisionSurfaceProgrammaticOutput,
) -> ProgrammaticResult<serde_json::Value> {
    let DecisionSurfaceProgrammaticOutput {
        surface,
        elapsed: _,
        envelope_mode,
        telemetry_analysis_run_id,
    } = output;
    let payload = build_decision_surface_output(&surface);
    serialize_decision_surface_json_output(
        payload,
        envelope_mode,
        telemetry_analysis_run_id.as_deref(),
    )
    .map_err(|err| {
        ProgrammaticError::new(format!("failed to serialize decision surface: {err}"), 2)
            .with_code("FALLOW_SERIALIZE_DECISION_SURFACE")
            .with_context("decision-surface")
    })
}

/// Serialize typed audit output into the stable JSON compatibility contract.
///
/// # Errors
///
/// Returns a structured error if one of the audit sections cannot serialize.
pub fn serialize_audit_programmatic_json(
    output: AuditProgrammaticOutput,
) -> ProgrammaticResult<serde_json::Value> {
    let base_snapshot = output.base_snapshot.as_ref();
    let dead_code = output
        .dead_code
        .as_ref()
        .map(|dead_code| serialize_audit_dead_code(dead_code, base_snapshot))
        .transpose()?;
    let duplication = output
        .duplication
        .as_ref()
        .map(|duplication| serialize_audit_duplication(duplication, base_snapshot))
        .transpose()?;
    let complexity = output
        .complexity
        .as_ref()
        .map(|complexity| serialize_audit_complexity(complexity, base_snapshot))
        .transpose()?;

    crate::serialize_audit_json(
        crate::AuditJsonOutputInput {
            header: crate::AuditJsonHeaderInput {
                schema_version: SchemaVersion(CHECK_SCHEMA_VERSION),
                version: ToolVersion(env!("CARGO_PKG_VERSION").to_string()),
                verdict: output.verdict,
                changed_files_count: u32::try_from(output.changed_files_count).unwrap_or(u32::MAX),
                base_ref: output.base_ref,
                base_description: output.base_description,
                head_sha: output.head_sha,
                elapsed_ms: ElapsedMs(
                    u64::try_from(output.elapsed.as_millis()).unwrap_or(u64::MAX),
                ),
                base_snapshot_skipped: output.base_snapshot_skipped,
                summary: output.summary,
                attribution: output.attribution,
            },
            dead_code,
            duplication,
            complexity,
            next_steps: output.next_steps,
        },
        output.envelope_mode,
        output.telemetry_analysis_run_id.as_deref(),
    )
    .map_err(|err| {
        ProgrammaticError::new(format!("failed to serialize audit report: {err}"), 2)
            .with_code("FALLOW_SERIALIZE_AUDIT_REPORT")
            .with_context("audit")
    })
}

fn serialize_audit_dead_code(
    output: &DeadCodeProgrammaticOutput,
    base_snapshot: Option<&crate::AuditProgrammaticKeySnapshot>,
) -> ProgrammaticResult<serde_json::Value> {
    let mut json = crate::serialize_check_json_payload(crate::CheckJsonPayloadInput {
        results: &output.output.results,
        root: &output.root,
        elapsed: Duration::from_millis(output.output.elapsed_ms.0),
        config_fixable: output.config_fixable,
        extras: crate::CheckJsonExtraOutputs::default(),
        workspace_diagnostics: Vec::new(),
    })
    .map_err(|err| {
        ProgrammaticError::new(format!("failed to serialize audit dead-code: {err}"), 2)
            .with_code("FALLOW_SERIALIZE_AUDIT_DEAD_CODE")
            .with_context("audit.deadCode")
    })?;
    if let Some(base) = base_snapshot {
        if has_persisted_introduced_flags(&json) {
            crate::audit_keys::annotate_stale_suppressions_json(
                &mut json,
                &output.output.results,
                &output.root,
                &base.dead_code,
            );
        } else {
            crate::audit_keys::annotate_dead_code_json(
                &mut json,
                &output.output.results,
                &output.root,
                &base.dead_code,
            );
        }
    }
    Ok(json)
}

fn serialize_audit_duplication(
    output: &DuplicationProgrammaticOutput,
    base_snapshot: Option<&crate::AuditProgrammaticKeySnapshot>,
) -> ProgrammaticResult<serde_json::Value> {
    let mut json = serde_json::to_value(&output.output.report).map_err(|err| {
        ProgrammaticError::new(format!("failed to serialize audit duplication: {err}"), 2)
            .with_code("FALLOW_SERIALIZE_AUDIT_DUPLICATION")
            .with_context("audit.duplication")
    })?;
    let root_prefix = format!("{}/", output.root.display());
    strip_root_prefix(&mut json, &root_prefix);
    if let Some(base) = base_snapshot
        && !has_persisted_introduced_flags(&json)
    {
        annotate_audit_duplication_json(&mut json, output, &base.dupes);
    }
    Ok(json)
}

fn serialize_audit_complexity(
    output: &HealthProgrammaticOutput,
    base_snapshot: Option<&crate::AuditProgrammaticKeySnapshot>,
) -> ProgrammaticResult<serde_json::Value> {
    let mut json = serde_json::to_value(&output.report).map_err(|err| {
        ProgrammaticError::new(format!("failed to serialize audit complexity: {err}"), 2)
            .with_code("FALLOW_SERIALIZE_AUDIT_COMPLEXITY")
            .with_context("audit.complexity")
    })?;
    let root_prefix = format!("{}/", output.root.display());
    strip_root_prefix(&mut json, &root_prefix);
    if let Some(base) = base_snapshot {
        crate::audit_keys::annotate_health_json(
            &mut json,
            &output.report,
            &output.root,
            &base.health,
        );
    }
    Ok(json)
}

fn has_persisted_introduced_flags(json: &serde_json::Value) -> bool {
    json.as_object().is_some_and(|object| {
        object.values().any(|value| {
            value
                .as_array()
                .is_some_and(|items| items.iter().any(|item| item.get("introduced").is_some()))
        })
    })
}

fn annotate_audit_duplication_json(
    json: &mut serde_json::Value,
    output: &DuplicationProgrammaticOutput,
    base: &rustc_hash::FxHashSet<String>,
) {
    let Some(items) = json
        .get_mut("clone_groups")
        .and_then(serde_json::Value::as_array_mut)
    else {
        return;
    };
    for (item, group) in items.iter_mut().zip(&output.output.report.clone_groups) {
        if let serde_json::Value::Object(map) = item {
            let key = crate::audit_keys::dupe_group_key(&group.group, &output.root);
            map.insert(
                "introduced".to_string(),
                serde_json::json!(!base.contains(&key)),
            );
        }
    }
}

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
        config_fixable: _,
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
        threshold: _,
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
