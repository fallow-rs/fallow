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
    AUDIT_SCHEMA_VERSION, CheckOutput, GroupByMode, RootEnvelopeMode,
    build_decision_surface_output, serialize_check_json_output,
    serialize_decision_surface_json_output, serialize_dupes_json_output,
    serialize_feature_flags_json_output, strip_root_prefix,
};
use fallow_types::envelope::{ElapsedMs, SchemaVersion, ToolVersion};
use fallow_types::workspace::{WorkspaceDiagnostic, merge_workspace_diagnostics};
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
    let workspace_diagnostics = combined_workspace_diagnostics(
        dead_code.as_ref(),
        health.as_ref(),
        duplication.as_ref(),
        &root,
    );
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
        type_aware: None,
        workspace_diagnostics,
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

/// Union the combined run's workspace diagnostics across its typed sections.
///
/// Each section captured the list as of the moment its own analysis finished,
/// and those lists can differ: a combined run walks the project once per
/// analysis, per-analysis `production` modes can give those walks different
/// file sets, and each walk clears the previous walk's source-discovery
/// entries. No single section therefore holds everything the run recorded, so
/// the root carries the deduplicated union in section order (dead code, then
/// health, then duplication) and a run missing a section (`--skip check`,
/// `--only health`, `--only dupes`) still reports what its remaining analyses
/// recorded.
///
/// [`fallow_config::registry_diagnostics_to_fold`] closes the fold, exactly as
/// the CLI's fold does, because a section list is captured when its analysis
/// finishes and an analysis can record after that: the health run's own
/// dead-code precompute records the analysis-stage kinds after
/// `HealthProgrammaticOutput::workspace_diagnostics` was taken, so a
/// `dead_code: false` run would otherwise report an empty array where the CLI's
/// `--skip check` reports the diagnostic. The leg drops walk-recorded kinds, so
/// it cannot import another walk's file set (issue #2366).
fn combined_workspace_diagnostics(
    dead_code: Option<&DeadCodeProgrammaticOutput>,
    health: Option<&HealthProgrammaticOutput>,
    duplication: Option<&DuplicationProgrammaticOutput>,
    root: &Path,
) -> Vec<WorkspaceDiagnostic> {
    let merged = merge_workspace_diagnostics(
        dead_code.map_or_else(Vec::new, |dead_code| {
            dead_code.output.workspace_diagnostics.clone()
        }),
        health.map_or_else(Vec::new, |health| health.workspace_diagnostics.clone()),
    );
    let merged = merge_workspace_diagnostics(
        merged,
        duplication.map_or_else(Vec::new, |duplication| {
            duplication.output.workspace_diagnostics.clone()
        }),
    );
    merge_workspace_diagnostics(
        merged,
        fallow_config::registry_diagnostics_to_fold(combined_diagnostics_root(
            dead_code,
            health,
            duplication,
            root,
        )),
    )
}

/// Root the diagnostics registry is keyed on: the root a section resolved,
/// which is the resolved config root the run recorded against, with the
/// combined output's own root as the fallback. Mirrors the CLI's
/// `combined_diagnostics_root`.
fn combined_diagnostics_root<'a>(
    dead_code: Option<&'a DeadCodeProgrammaticOutput>,
    health: Option<&'a HealthProgrammaticOutput>,
    duplication: Option<&'a DuplicationProgrammaticOutput>,
    root: &'a Path,
) -> &'a Path {
    dead_code
        .map(|dead_code| dead_code.root.as_path())
        .or_else(|| health.map(|health| health.root.as_path()))
        .or_else(|| duplication.map(|duplication| duplication.root.as_path()))
        .unwrap_or(root)
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
                schema_version: SchemaVersion(AUDIT_SCHEMA_VERSION),
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
            meta: None,
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

/// Serialize the audit envelope's dead-code sub-result.
///
/// The sub-result is a `CheckOutput` body, so it carries the run's
/// `workspace_diagnostics[]` the way the standalone `dead-code` envelope and
/// the combined `check` section do. The two audit routes agree on everything
/// the dead-code analysis records: this one serializes the typed list, which
/// the runtime already built from the session's own capture plus
/// [`fallow_config::registry_diagnostics_to_fold`] after the analyze pass, and
/// the CLI audit path folds `CheckResult::workspace_diagnostics` with that same
/// filtered registry leg at serialization time. That closing read is the CLI's
/// only extra leg and can only add an entry recorded after the analysis
/// captured its list; because the leg drops walk-recorded kinds, a per-analysis
/// `production` split (which makes the run's walks disagree about which files
/// exist) cannot make the two routes answer differently (issue #2366).
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
        workspace_diagnostics: output.output.workspace_diagnostics.clone(),
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
        type_aware: None,
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

#[cfg(test)]
mod tests {
    use super::{
        RootEnvelopeMode, serialize_audit_dead_code, serialize_combined_programmatic_json,
    };
    use crate::DupesReportPayload;
    use crate::runtime::{
        CombinedProgrammaticOutput, DeadCodeProgrammaticOutput, DuplicationProgrammaticOutput,
        HealthProgrammaticOutput,
    };
    use fallow_output::{
        CHECK_SCHEMA_VERSION, CheckOutputInput, DUPES_SCHEMA_VERSION, DupesOutputInput,
        HealthReport, build_check_output, build_dupes_output,
    };
    use fallow_types::duplicates::DuplicationReport;
    use fallow_types::results::AnalysisResults;
    use fallow_types::workspace::{WorkspaceDiagnostic, WorkspaceDiagnosticKind};
    use std::path::Path;
    use std::time::Duration;

    fn dead_code_output(
        root: &Path,
        workspace_diagnostics: Vec<WorkspaceDiagnostic>,
    ) -> DeadCodeProgrammaticOutput {
        DeadCodeProgrammaticOutput {
            output: build_check_output(CheckOutputInput {
                schema_version: CHECK_SCHEMA_VERSION,
                version: "0.0.0-test".to_owned(),
                elapsed: Duration::ZERO,
                results: AnalysisResults::default(),
                config_fixable: false,
                meta: None,
                workspace_diagnostics,
                next_steps: Vec::new(),
            }),
            root: root.to_path_buf(),
            config_fixable: false,
            envelope_mode: RootEnvelopeMode::Tagged,
            telemetry_analysis_run_id: None,
        }
    }

    /// Issue #2366: the audit envelope's dead-code sub-result carries the run's
    /// workspace diagnostics root-relative, matching what the CLI audit path
    /// reads from the registry, and omits the array when there are none.
    #[test]
    fn audit_dead_code_section_carries_workspace_diagnostics_root_relative_or_omits_them() {
        let root = Path::new("/project");
        let carried = serialize_audit_dead_code(
            &dead_code_output(
                root,
                vec![WorkspaceDiagnostic::new(
                    root,
                    root.join("package.json"),
                    WorkspaceDiagnosticKind::BunLockbOverrideResolutionSkipped,
                )],
            ),
            None,
        )
        .expect("audit dead-code JSON");
        assert_eq!(
            carried["workspace_diagnostics"][0]["kind"],
            "bun-lockb-override-resolution-skipped"
        );
        assert_eq!(carried["workspace_diagnostics"][0]["path"], "package.json");

        let empty = serialize_audit_dead_code(&dead_code_output(root, Vec::new()), None)
            .expect("audit dead-code JSON");
        assert!(
            empty.get("workspace_diagnostics").is_none(),
            "an empty list is omitted from the audit dead-code section: {empty}"
        );
    }

    fn health_output(
        root: &Path,
        workspace_diagnostics: Vec<WorkspaceDiagnostic>,
    ) -> HealthProgrammaticOutput {
        HealthProgrammaticOutput {
            report: HealthReport::default(),
            grouping: None,
            root: root.to_path_buf(),
            elapsed: Duration::ZERO,
            explain: false,
            workspace_diagnostics,
            next_steps: Vec::new(),
            envelope_mode: RootEnvelopeMode::Tagged,
            telemetry_analysis_run_id: None,
        }
    }

    fn duplication_output(
        root: &Path,
        workspace_diagnostics: Vec<WorkspaceDiagnostic>,
    ) -> DuplicationProgrammaticOutput {
        DuplicationProgrammaticOutput {
            output: build_dupes_output(DupesOutputInput {
                schema_version: DUPES_SCHEMA_VERSION,
                version: "0.0.0-test".to_owned(),
                elapsed: Duration::ZERO,
                report: DupesReportPayload::from_report(&DuplicationReport::default()),
                grouped_by: None,
                total_issues: None,
                groups: None,
                meta: None,
                workspace_diagnostics,
                next_steps: Vec::new(),
            }),
            root: root.to_path_buf(),
            threshold: 0.0,
            envelope_mode: RootEnvelopeMode::Tagged,
            telemetry_analysis_run_id: None,
        }
    }

    fn combined_output(
        root: &Path,
        dead_code: Option<DeadCodeProgrammaticOutput>,
        health: Option<HealthProgrammaticOutput>,
    ) -> CombinedProgrammaticOutput {
        combined_output_with_duplication(root, dead_code, health, None)
    }

    fn combined_output_with_duplication(
        root: &Path,
        dead_code: Option<DeadCodeProgrammaticOutput>,
        health: Option<HealthProgrammaticOutput>,
        duplication: Option<DuplicationProgrammaticOutput>,
    ) -> CombinedProgrammaticOutput {
        CombinedProgrammaticOutput {
            dead_code,
            duplication,
            health,
            root: root.to_path_buf(),
            elapsed: Duration::ZERO,
            explain: false,
            next_steps: Vec::new(),
            envelope_mode: RootEnvelopeMode::Tagged,
            telemetry_analysis_run_id: None,
        }
    }

    fn bun_lockb_diagnostic(root: &Path) -> WorkspaceDiagnostic {
        WorkspaceDiagnostic::new(
            root,
            root.join("package.json"),
            WorkspaceDiagnosticKind::BunLockbOverrideResolutionSkipped,
        )
    }

    fn large_file_diagnostic(root: &Path, relative: &str) -> WorkspaceDiagnostic {
        WorkspaceDiagnostic::new(
            root,
            root.join(relative),
            WorkspaceDiagnosticKind::SkippedLargeFile {
                size_bytes: 6_000_000,
            },
        )
    }

    fn root_kinds(document: &serde_json::Value) -> Vec<String> {
        document["workspace_diagnostics"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .map(|diagnostic| diagnostic["kind"].as_str().unwrap_or_default().to_owned())
            .collect()
    }

    /// Issue #2366: the programmatic combined envelope (MCP `analyze` in code
    /// mode, NAPI, embedders) carries the run's workspace diagnostics on the
    /// combined root, root-relative, and omits the array when there are none.
    #[test]
    fn combined_programmatic_root_carries_workspace_diagnostics_or_omits_them() {
        let root = Path::new("/project");
        let carried = serialize_combined_programmatic_json(combined_output(
            root,
            Some(dead_code_output(root, vec![bun_lockb_diagnostic(root)])),
            None,
        ))
        .expect("combined JSON");
        assert_eq!(
            carried["workspace_diagnostics"][0]["kind"],
            "bun-lockb-override-resolution-skipped"
        );
        assert_eq!(carried["workspace_diagnostics"][0]["path"], "package.json");
        assert!(
            carried["check"].is_object(),
            "the check section is present, so the absence check below is not vacuous: {carried}"
        );
        assert!(
            carried["check"].get("workspace_diagnostics").is_none(),
            "the check section is not a second carrier: {carried}"
        );

        let empty = serialize_combined_programmatic_json(combined_output(
            root,
            Some(dead_code_output(root, Vec::new())),
            None,
        ))
        .expect("combined JSON");
        assert!(
            empty.get("workspace_diagnostics").is_none(),
            "an empty list is omitted from the combined root: {empty}"
        );
    }

    /// Issue #2366: a programmatic combined run without a dead-code section
    /// (the `--skip check` / `--only health` shape) still reports the
    /// diagnostics, taken from the section that did run.
    #[test]
    fn combined_programmatic_root_carries_workspace_diagnostics_without_a_dead_code_section() {
        let root = Path::new("/project");
        let carried = serialize_combined_programmatic_json(combined_output(
            root,
            None,
            Some(health_output(root, vec![bun_lockb_diagnostic(root)])),
        ))
        .expect("combined JSON");
        assert!(
            carried.get("check").is_none(),
            "this run has no check section: {carried}"
        );
        assert_eq!(
            carried["workspace_diagnostics"][0]["kind"],
            "bun-lockb-override-resolution-skipped"
        );
        assert_eq!(carried["workspace_diagnostics"][0]["path"], "package.json");
    }

    /// Issue #2366: a duplication-only combined run (an embedder driving
    /// `CombinedOptions` with just `duplication`) reports what that section
    /// recorded.
    #[test]
    fn combined_programmatic_root_carries_workspace_diagnostics_from_a_duplication_only_run() {
        let root = Path::new("/project");
        let carried = serialize_combined_programmatic_json(combined_output_with_duplication(
            root,
            None,
            None,
            Some(duplication_output(
                root,
                vec![large_file_diagnostic(root, "src/generated.ts")],
            )),
        ))
        .expect("combined JSON");
        assert!(
            carried.get("check").is_none() && carried.get("health").is_none(),
            "only the dupes section ran: {carried}"
        );
        assert_eq!(root_kinds(&carried), ["skipped-large-file"]);
        assert_eq!(
            carried["workspace_diagnostics"][0]["path"],
            "src/generated.ts"
        );
    }

    /// Issue #2366: sections of one combined run can record different lists,
    /// because each analysis walks the project itself and a per-analysis
    /// `production` mode changes which files that walk sees. The root carries
    /// the union so nothing the run recorded is dropped, deduplicated so a
    /// diagnostic two sections both saw is reported once, and in section order
    /// so a run whose analyses agree matches the standalone `dead-code`
    /// envelope exactly.
    #[test]
    fn combined_programmatic_root_unions_sections_that_recorded_different_diagnostics() {
        let root = Path::new("/project");
        let shared = bun_lockb_diagnostic(root);
        let carried = serialize_combined_programmatic_json(combined_output_with_duplication(
            root,
            Some(dead_code_output(
                root,
                vec![shared.clone(), large_file_diagnostic(root, "src/big.ts")],
            )),
            Some(health_output(root, vec![shared.clone()])),
            Some(duplication_output(
                root,
                vec![shared, large_file_diagnostic(root, "src/other.ts")],
            )),
        ))
        .expect("combined JSON");
        assert_eq!(
            root_kinds(&carried),
            [
                "bun-lockb-override-resolution-skipped",
                "skipped-large-file",
                "skipped-large-file",
            ],
            "the union keeps dead-code order first and drops the repeats: {}",
            carried["workspace_diagnostics"]
        );
        let paths: Vec<&str> = carried["workspace_diagnostics"]
            .as_array()
            .expect("array")
            .iter()
            .map(|diagnostic| diagnostic["path"].as_str().unwrap_or_default())
            .collect();
        assert_eq!(paths, ["package.json", "src/big.ts", "src/other.ts"]);
    }

    /// Issue #2366: a diagnostic only the health or duplication section
    /// recorded still reaches the root when the dead-code section recorded
    /// nothing, the direction that a `production: { deadCode: true }` split
    /// produces on a real project.
    #[test]
    fn combined_programmatic_root_keeps_diagnostics_an_empty_dead_code_section_missed() {
        let root = Path::new("/project");
        let carried = serialize_combined_programmatic_json(combined_output_with_duplication(
            root,
            Some(dead_code_output(root, Vec::new())),
            Some(health_output(
                root,
                vec![large_file_diagnostic(root, "src/big.test.ts")],
            )),
            None,
        ))
        .expect("combined JSON");
        assert_eq!(root_kinds(&carried), ["skipped-large-file"]);
        assert_eq!(
            carried["workspace_diagnostics"][0]["path"],
            "src/big.test.ts"
        );
    }
}
