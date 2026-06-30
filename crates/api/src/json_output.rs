//! Shared JSON output assembly for CLI and programmatic consumers.

use std::path::Path;
use std::time::Duration;

use fallow_output::{
    CHECK_SCHEMA_VERSION, CheckGroupedEntry, CheckGroupedOutput, CheckOutput, CheckOutputInput,
    DupesOutput, DupesOutputInput, GroupByMode, RootEnvelopeMode,
    apply_config_fixable_to_duplicate_exports, build_check_output, build_dupes_output,
    harmonize_multi_kind_suppress_line_actions as harmonize_typed_suppress_line_actions,
    strip_root_prefix,
};
use fallow_types::duplicates::DuplicationReport;
use fallow_types::envelope::{
    BaselineDeltas, BaselineMatch, ElapsedMs, Meta, RegressionResult, SchemaVersion, ToolVersion,
};
use fallow_types::output::NextStep;
use fallow_types::results::AnalysisResults;
use fallow_types::workspace::WorkspaceDiagnostic;

use crate::{DupesReportPayload, DuplicationGroup, DuplicationGrouping, ResultGroup};

/// Inputs for `fallow dead-code --format json` output assembly.
pub struct CheckJsonOutputInput<'a> {
    pub results: &'a AnalysisResults,
    pub root: &'a Path,
    pub elapsed: Duration,
    pub config_fixable: bool,
    pub meta: Option<Meta>,
    pub extras: CheckJsonExtraOutputs,
    pub workspace_diagnostics: Vec<WorkspaceDiagnostic>,
    pub next_steps: Vec<NextStep>,
    pub envelope_mode: RootEnvelopeMode,
    pub telemetry_analysis_run_id: Option<&'a str>,
}

/// Inputs for the dead-code JSON payload without a root envelope.
pub struct CheckJsonPayloadInput<'a> {
    pub results: &'a AnalysisResults,
    pub root: &'a Path,
    pub elapsed: Duration,
    pub config_fixable: bool,
    pub extras: CheckJsonExtraOutputs,
    pub workspace_diagnostics: Vec<WorkspaceDiagnostic>,
}

/// Optional root sections for dead-code JSON envelopes.
///
/// These fields are part of the output contract, but they are computed by
/// caller-specific workflows such as baseline and regression gates.
#[derive(Debug, Clone, Default)]
pub struct CheckJsonExtraOutputs {
    pub baseline_deltas: Option<BaselineDeltas>,
    pub baseline: Option<BaselineMatch>,
    pub regression: Option<RegressionResult>,
}

struct CheckJsonEnvelopeInput<'a> {
    results: &'a AnalysisResults,
    elapsed: Duration,
    config_fixable: bool,
    meta: Option<Meta>,
    extras: CheckJsonExtraOutputs,
    workspace_diagnostics: Vec<WorkspaceDiagnostic>,
    next_steps: Vec<NextStep>,
}

/// Inputs for grouped dead-code JSON output assembly.
pub struct GroupedCheckJsonOutputInput<'a> {
    pub groups: &'a [ResultGroup],
    pub original: &'a AnalysisResults,
    pub root: &'a Path,
    pub elapsed: Duration,
    pub grouped_by: GroupByMode,
    pub config_fixable: bool,
    pub meta: Option<Meta>,
    pub next_steps: Vec<NextStep>,
    pub envelope_mode: RootEnvelopeMode,
    pub telemetry_analysis_run_id: Option<&'a str>,
}

/// Inputs for `fallow dupes --format json` output assembly.
pub struct DuplicationJsonOutputInput<'a> {
    pub report: &'a DuplicationReport,
    pub root: &'a Path,
    pub elapsed: Duration,
    pub meta: Option<Meta>,
    pub workspace_diagnostics: Vec<WorkspaceDiagnostic>,
    pub next_steps: Vec<NextStep>,
    pub envelope_mode: RootEnvelopeMode,
    pub telemetry_analysis_run_id: Option<&'a str>,
}

/// Inputs for grouped duplication JSON output assembly.
pub struct GroupedDuplicationJsonOutputInput<'a> {
    pub report: &'a DuplicationReport,
    pub grouping: &'a DuplicationGrouping,
    pub root: &'a Path,
    pub elapsed: Duration,
    pub meta: Option<Meta>,
    pub workspace_diagnostics: Vec<WorkspaceDiagnostic>,
    pub next_steps: Vec<NextStep>,
    pub envelope_mode: RootEnvelopeMode,
    pub telemetry_analysis_run_id: Option<&'a str>,
}

/// Build and serialize dead-code JSON through the API-owned output boundary.
///
/// # Errors
///
/// Returns a serde error when the typed envelope cannot be converted to JSON.
pub fn serialize_check_json(
    input: CheckJsonOutputInput<'_>,
) -> Result<serde_json::Value, serde_json::Error> {
    let envelope = build_check_json_envelope(CheckJsonEnvelopeInput {
        results: input.results,
        elapsed: input.elapsed,
        config_fixable: input.config_fixable,
        meta: input.meta,
        extras: input.extras,
        workspace_diagnostics: input.workspace_diagnostics,
        next_steps: input.next_steps,
    });
    let mut output = fallow_output::serialize_check_json_output(
        envelope,
        input.envelope_mode,
        input.telemetry_analysis_run_id,
    )?;
    strip_json_root_prefix(&mut output, input.root);
    Ok(output)
}

/// Build a dead-code JSON payload without adding a root envelope.
///
/// # Errors
///
/// Returns a serde error when the typed envelope cannot be converted to JSON.
pub fn serialize_check_json_payload(
    input: CheckJsonPayloadInput<'_>,
) -> Result<serde_json::Value, serde_json::Error> {
    let envelope = build_check_json_envelope(CheckJsonEnvelopeInput {
        results: input.results,
        elapsed: input.elapsed,
        config_fixable: input.config_fixable,
        meta: None,
        extras: input.extras,
        workspace_diagnostics: input.workspace_diagnostics,
        next_steps: Vec::new(),
    });
    let mut output = serde_json::to_value(envelope)?;
    strip_json_root_prefix(&mut output, input.root);
    Ok(output)
}

/// Build and serialize grouped dead-code JSON through the API output boundary.
///
/// # Errors
///
/// Returns a serde error when the typed envelope cannot be converted to JSON.
pub fn serialize_grouped_check_json(
    input: GroupedCheckJsonOutputInput<'_>,
) -> Result<serde_json::Value, serde_json::Error> {
    let entries = input
        .groups
        .iter()
        .map(|group| {
            let mut results = group.results.clone();
            apply_config_fixable_to_duplicate_exports(&mut results, input.config_fixable);
            harmonize_typed_suppress_line_actions(&mut results);
            CheckGroupedEntry {
                key: group.key.clone(),
                owners: group.owners.clone(),
                total_issues: results.total_issues(),
                results,
            }
        })
        .collect();

    let envelope = CheckGroupedOutput {
        schema_version: SchemaVersion(CHECK_SCHEMA_VERSION),
        version: ToolVersion(env!("CARGO_PKG_VERSION").to_string()),
        elapsed_ms: ElapsedMs(input.elapsed.as_millis() as u64),
        grouped_by: input.grouped_by,
        total_issues: input.original.total_issues(),
        groups: entries,
        meta: input.meta,
        next_steps: input.next_steps,
    };

    let mut output = fallow_output::serialize_check_grouped_json_output(
        envelope,
        input.envelope_mode,
        input.telemetry_analysis_run_id,
    )?;
    let root_prefix = format!("{}/", input.root.display());
    if let Some(arr) = output
        .get_mut("groups")
        .and_then(serde_json::Value::as_array_mut)
    {
        for entry in arr {
            strip_root_prefix(entry, &root_prefix);
        }
    }
    Ok(output)
}

/// Build and serialize duplication JSON through the API-owned output boundary.
///
/// # Errors
///
/// Returns a serde error when the typed envelope cannot be converted to JSON.
pub fn serialize_duplication_json(
    input: DuplicationJsonOutputInput<'_>,
) -> Result<serde_json::Value, serde_json::Error> {
    let payload = DupesReportPayload::from_report(input.report);
    let envelope: DupesOutput<DupesReportPayload, DuplicationGroup> =
        build_dupes_output(DupesOutputInput {
            schema_version: CHECK_SCHEMA_VERSION,
            version: env!("CARGO_PKG_VERSION").to_string(),
            elapsed: input.elapsed,
            report: payload,
            grouped_by: None,
            total_issues: None,
            groups: None,
            meta: input.meta,
            workspace_diagnostics: input.workspace_diagnostics,
            next_steps: input.next_steps,
        });
    let mut output = fallow_output::serialize_dupes_json_output(
        envelope,
        input.envelope_mode,
        input.telemetry_analysis_run_id,
    )?;
    let root_prefix = format!("{}/", input.root.display());
    strip_root_prefix(&mut output, &root_prefix);
    Ok(output)
}

/// Build and serialize grouped duplication JSON through the API output boundary.
///
/// # Errors
///
/// Returns a serde error when the typed envelope cannot be converted to JSON.
pub fn serialize_grouped_duplication_json(
    input: GroupedDuplicationJsonOutputInput<'_>,
) -> Result<serde_json::Value, serde_json::Error> {
    let root_prefix = format!("{}/", input.root.display());
    let payload = DupesReportPayload::from_report(input.report);
    let envelope: DupesOutput<DupesReportPayload, DuplicationGroup> =
        build_dupes_output(DupesOutputInput {
            schema_version: CHECK_SCHEMA_VERSION,
            version: env!("CARGO_PKG_VERSION").to_string(),
            elapsed: input.elapsed,
            report: payload,
            grouped_by: Some(group_by_mode_from_label(input.grouping.mode)),
            total_issues: Some(input.report.clone_groups.len()),
            groups: None,
            meta: input.meta,
            workspace_diagnostics: input.workspace_diagnostics,
            next_steps: input.next_steps,
        });
    let mut output = fallow_output::serialize_dupes_json_output(
        envelope,
        input.envelope_mode,
        input.telemetry_analysis_run_id,
    )?;
    strip_root_prefix(&mut output, &root_prefix);

    let group_values = input
        .grouping
        .groups
        .iter()
        .map(|group| {
            let mut value = serde_json::to_value(group)?;
            strip_root_prefix(&mut value, &root_prefix);
            Ok(value)
        })
        .collect::<Result<Vec<_>, serde_json::Error>>()?;

    if let serde_json::Value::Object(ref mut map) = output {
        map.insert("groups".to_string(), serde_json::Value::Array(group_values));
    }

    Ok(output)
}

fn build_check_json_envelope(input: CheckJsonEnvelopeInput<'_>) -> CheckOutput {
    let mut output = build_check_output(CheckOutputInput {
        schema_version: CHECK_SCHEMA_VERSION,
        version: env!("CARGO_PKG_VERSION").to_string(),
        elapsed: input.elapsed,
        results: input.results.clone(),
        config_fixable: input.config_fixable,
        meta: input.meta,
        workspace_diagnostics: input.workspace_diagnostics,
        next_steps: input.next_steps,
    });
    output.baseline_deltas = input.extras.baseline_deltas;
    output.baseline = input.extras.baseline;
    output.regression = input.extras.regression;
    output
}

fn strip_json_root_prefix(output: &mut serde_json::Value, root: &Path) {
    let root_prefix = format!("{}/", root.display());
    strip_root_prefix(output, &root_prefix);
}

fn group_by_mode_from_label(label: &str) -> GroupByMode {
    match label {
        "directory" => GroupByMode::Directory,
        "package" => GroupByMode::Package,
        "section" => GroupByMode::Section,
        _ => GroupByMode::Owner,
    }
}
