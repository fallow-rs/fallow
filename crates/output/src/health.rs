use fallow_types::envelope::{ElapsedMs, Meta, SchemaVersion, ToolVersion};
use fallow_types::output::NextStep;
use serde::Serialize;

use crate::{GroupByMode, WorkspaceDiagnosticOutput};

/// Envelope emitted by `fallow health --format json`.
///
/// The health report body is flattened into the envelope so every report field
/// lives at the top level. `Report` and `Group` are generic while health report
/// internals are still moving out of the CLI crate; the envelope contract itself
/// is owned here so CLI, API, and future embedders share one top-level shape.
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
    pub workspace_diagnostics: Vec<WorkspaceDiagnosticOutput>,
    /// Read-only follow-up commands computed from this run's findings.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub next_steps: Vec<NextStep>,
}
