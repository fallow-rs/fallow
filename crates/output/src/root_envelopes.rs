//! Root JSON output envelopes shared by CLI and programmatic consumers.

use fallow_types::envelope::{ElapsedMs, Meta, SchemaVersion, TelemetryMeta, ToolVersion};
use fallow_types::output::NextStep;
use serde::Serialize;

/// `fallow audit --format json` envelope.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "schema", schemars(title = "fallow audit --format json"))]
pub struct AuditOutput<Verdict, Summary, Attribution, DeadCode, Duplication, Complexity> {
    pub schema_version: SchemaVersion,
    pub version: ToolVersion,
    pub command: AuditCommand,
    pub verdict: Verdict,
    pub changed_files_count: u32,
    pub base_ref: String,
    /// Human-readable provenance of `base_ref`, e.g. `merge-base with
    /// origin/main`, `local main`, or `FALLOW_AUDIT_BASE=upstream/main`.
    /// Present when the base was auto-detected or set via `FALLOW_AUDIT_BASE`;
    /// absent for an explicit `--base`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_sha: Option<String>,
    pub elapsed_ms: ElapsedMs,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_snapshot_skipped: Option<bool>,
    pub summary: Summary,
    pub attribution: Attribution,
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Meta>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dead_code: Option<DeadCode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duplication: Option<Duplication>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub complexity: Option<Complexity>,
    /// Read-only follow-up commands computed from this run's findings. See
    /// `CheckOutput::next_steps` for the contract.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub next_steps: Vec<NextStep>,
}

/// Audit command singleton carried by [`AuditOutput`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum AuditCommand {
    Audit,
}

/// Bare `fallow --format json` envelope.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(
    feature = "schema",
    schemars(title = "fallow --format json (bare, combined)")
)]
pub struct CombinedOutput<Check, Dupes, Health> {
    pub schema_version: SchemaVersion,
    pub version: ToolVersion,
    pub elapsed_ms: ElapsedMs,
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<CombinedMeta>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub check: Option<Check>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dupes: Option<Dupes>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub health: Option<Health>,
    /// Read-only follow-up commands aggregated across the combined run's
    /// findings. See `CheckOutput::next_steps` for the contract.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub next_steps: Vec<NextStep>,
}

/// Optional `_meta` block for [`CombinedOutput`].
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CombinedMeta {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub check: Option<Meta>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dupes: Option<Meta>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub health: Option<Meta>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub telemetry: Option<TelemetryMeta>,
}
