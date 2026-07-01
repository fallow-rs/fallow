use std::time::Duration;

use fallow_types::envelope::{ElapsedMs, Meta, SchemaVersion, ToolVersion};
use fallow_types::output::NextStep;
use serde::Serialize;

use fallow_types::workspace::WorkspaceDiagnostic;

use crate::{
    GroupByMode, RootEnvelopeMode, apply_root_kind, attach_telemetry_meta, strip_root_prefix,
};

/// Envelope emitted by `fallow health --format json` (plus the `health` block
/// inside the combined and audit envelopes).
///
/// The body is `HealthReport` flattened into the envelope so every report
/// field (`findings`, `summary`, `vital_signs`, `hotspots`, `actions_meta`,
/// ...) lives at the top level. Grouped runs populate `grouped_by` +
/// `groups` with per-bucket recomputed metrics. The `actions_meta`
/// breadcrumb is modeled on `HealthReport` as an `Option<HealthActionsMeta>`
/// and is set at construction time by the report builder when the active
/// `HealthActionContext` requests suppress-line omission, so the schema
/// documents the field and serde populates it natively.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "schema", schemars(title = "fallow health --format json"))]
pub struct HealthOutput<Report, Group> {
    pub schema_version: SchemaVersion,
    pub version: ToolVersion,
    pub elapsed_ms: ElapsedMs,
    #[serde(flatten)]
    pub report: Report,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grouped_by: Option<GroupByMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub groups: Option<Vec<Group>>,
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Meta>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub workspace_diagnostics: Vec<WorkspaceDiagnostic>,
    /// Read-only follow-up commands computed from this run's findings. See
    /// `CheckOutput::next_steps` for the contract.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub next_steps: Vec<NextStep>,
}

/// Inputs for constructing a [`HealthOutput`] without exposing envelope
/// assembly details to callers.
#[derive(Debug, Clone)]
pub struct HealthOutputInput<Report, Group> {
    pub schema_version: u32,
    pub version: String,
    pub elapsed: Duration,
    pub report: Report,
    pub grouped_by: Option<GroupByMode>,
    pub groups: Option<Vec<Group>>,
    pub meta: Option<Meta>,
    pub workspace_diagnostics: Vec<WorkspaceDiagnostic>,
    pub next_steps: Vec<NextStep>,
}

/// Inputs for serializing a health report into the root JSON contract.
#[derive(Debug, Clone)]
pub struct HealthJsonOutputInput<'a, Report, Group> {
    pub output: HealthOutputInput<Report, Group>,
    pub root_prefix: Option<&'a str>,
    pub envelope_mode: RootEnvelopeMode,
    pub analysis_run_id: Option<&'a str>,
}

/// Build a health JSON envelope from caller-owned report data.
#[must_use]
pub fn build_health_output<Report, Group>(
    input: HealthOutputInput<Report, Group>,
) -> HealthOutput<Report, Group> {
    HealthOutput {
        schema_version: SchemaVersion(input.schema_version),
        version: ToolVersion(input.version),
        elapsed_ms: ElapsedMs(input.elapsed.as_millis() as u64),
        report: input.report,
        grouped_by: input.grouped_by,
        groups: input.groups,
        meta: input.meta,
        workspace_diagnostics: input.workspace_diagnostics,
        next_steps: input.next_steps,
    }
}

/// Build and serialize a health root JSON envelope.
///
/// This keeps the health contract serialization in `fallow-output` while
/// callers still own report assembly, workspace diagnostics, and follow-up
/// suggestion policy.
///
/// # Errors
///
/// Returns a serde error when the provided report or group payload cannot be
/// converted to JSON.
pub fn serialize_health_json_output<Report, Group>(
    input: HealthJsonOutputInput<'_, Report, Group>,
) -> Result<serde_json::Value, serde_json::Error>
where
    Report: Serialize,
    Group: Serialize,
{
    let envelope = build_health_output(input.output);
    let mut output = serde_json::to_value(envelope)?;
    apply_root_kind(&mut output, "health", input.envelope_mode);
    if let Some(root_prefix) = input.root_prefix {
        strip_root_prefix(&mut output, root_prefix);
    }
    attach_telemetry_meta(&mut output, input.analysis_run_id);
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serialize_health_json_output_tags_and_strips_root_paths() {
        let output = serialize_health_json_output(HealthJsonOutputInput {
            output: HealthOutputInput {
                schema_version: 7,
                version: "test".to_string(),
                elapsed: Duration::ZERO,
                report: serde_json::json!({ "findings": [{ "path": "/repo/src/a.ts" }] }),
                grouped_by: None,
                groups: None::<Vec<serde_json::Value>>,
                meta: None,
                workspace_diagnostics: Vec::new(),
                next_steps: Vec::new(),
            },
            root_prefix: Some("/repo/"),
            envelope_mode: RootEnvelopeMode::Tagged,
            analysis_run_id: Some("run-health"),
        })
        .expect("health output should serialize");

        assert_eq!(output["kind"], "health");
        assert_eq!(output["findings"][0]["path"], "src/a.ts");
        assert_eq!(
            output["_meta"]["telemetry"]["analysis_run_id"],
            "run-health"
        );
    }
}
