//! Security command output contracts.

use std::collections::BTreeMap;

use fallow_types::envelope::{ElapsedMs, Meta, ToolVersion};
use fallow_types::results::{SecurityAttackSurfaceEntry, SecurityFinding};
use serde::{Deserialize, Serialize};

/// The `fallow security --format json` schema version.
#[derive(Debug, Clone, Copy, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum SecuritySchemaVersion {
    /// First release of the `fallow security --format json` shape.
    #[serde(rename = "1")]
    V1,
    /// Adds per-finding `severity` for verification-priority tiering.
    #[serde(rename = "2")]
    V2,
    /// Adds version, elapsed time, explain metadata, and safe config metadata.
    #[serde(rename = "3")]
    V3,
    /// Adds bounded diagnostics for unresolved callee blind spots.
    #[serde(rename = "4")]
    V4,
    /// Adds summary metadata to security summary JSON.
    #[serde(rename = "5")]
    V5,
    /// Adds `candidate.sink.url_shape` for URL-shaped security candidates.
    #[serde(rename = "6")]
    V6,
    /// Adds the server-only-import category on client-server-leak findings.
    #[serde(rename = "7")]
    V7,
}

/// Gate verdict on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum SecurityGateVerdict {
    /// No new candidate in the changed lines.
    Pass,
    /// At least one new candidate in the changed lines.
    Fail,
}

/// The `gate` block on `SecurityOutput`, present only when `--gate <mode>` ran.
#[derive(Debug, Clone, Copy, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SecurityGate<Mode> {
    pub mode: Mode,
    pub verdict: SecurityGateVerdict,
    pub new_count: usize,
}

/// Allowlisted config context for `fallow security --format json`.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(
    feature = "schema",
    schemars(extend("required" = ["rules", "categories_include", "categories_exclude"]))
)]
pub struct SecurityOutputConfig<Severity> {
    pub rules: SecurityOutputRulesConfig<Severity>,
    pub categories_include: Option<Vec<String>>,
    pub categories_exclude: Option<Vec<String>>,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SecurityOutputRulesConfig<Severity> {
    pub security_client_server_leak: SecurityRuleSeverityConfig<Severity>,
    pub security_sink: SecurityRuleSeverityConfig<Severity>,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SecurityRuleSeverityConfig<Severity> {
    pub configured: Severity,
    pub effective: Severity,
}

/// The `fallow security --format json` envelope.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SecurityOutput<Config, Gate> {
    pub schema_version: SecuritySchemaVersion,
    pub version: ToolVersion,
    pub elapsed_ms: ElapsedMs,
    pub config: Config,
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Meta>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gate: Option<Gate>,
    pub security_findings: Vec<SecurityFinding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attack_surface: Option<Vec<SecurityAttackSurfaceEntry>>,
    pub unresolved_edge_files: usize,
    pub unresolved_callee_sites: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unresolved_callee_diagnostics: Option<SecurityUnresolvedCalleeDiagnostics>,
}

/// Bounded unresolved-callee diagnostics for `fallow security --format json`.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SecurityUnresolvedCalleeDiagnostics {
    pub sampled: Vec<SecurityUnresolvedCalleeSample>,
    pub top_files: Vec<SecurityUnresolvedCalleeTopFile>,
    pub by_reason: Vec<SecurityUnresolvedCalleeReasonCount>,
    pub sample_limit: usize,
    pub top_files_limit: usize,
}

/// One sampled unresolved-callee row.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SecurityUnresolvedCalleeSample {
    pub path: String,
    pub line: u32,
    pub col: u32,
    pub reason: fallow_types::extract::SkippedSecurityCalleeReason,
    pub expression_kind: fallow_types::extract::SkippedSecurityCalleeExpressionKind,
}

/// Count of unresolved callees in one file.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SecurityUnresolvedCalleeTopFile {
    pub path: String,
    pub count: usize,
}

/// Count of unresolved callees for one reason.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SecurityUnresolvedCalleeReasonCount {
    pub reason: fallow_types::extract::SkippedSecurityCalleeReason,
    pub count: usize,
}

/// Compact `fallow security --summary --format json` payload.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SecuritySummaryOutput<Config, Gate> {
    pub schema_version: SecuritySchemaVersion,
    pub version: ToolVersion,
    pub elapsed_ms: ElapsedMs,
    pub config: Config,
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Meta>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gate: Option<Gate>,
    pub summary: SecuritySummary,
}

/// Aggregate counts for `fallow security --summary --format json`.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SecuritySummary {
    pub security_findings: usize,
    pub by_severity: SecuritySeverityCounts,
    pub by_category: BTreeMap<String, usize>,
    pub by_reachability: SecurityReachabilityCounts,
    pub by_runtime_state: SecurityRuntimeStateCounts,
    pub unresolved_edge_files: usize,
    pub unresolved_callee_sites: usize,
    pub attack_surface_entries: usize,
}

/// Fixed severity counters for summary JSON.
#[derive(Debug, Clone, Copy, Default, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SecuritySeverityCounts {
    pub high: usize,
    pub medium: usize,
    pub low: usize,
}

/// Fixed reachability counters for summary JSON.
#[derive(Debug, Clone, Copy, Default, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SecurityReachabilityCounts {
    pub entry_reachable: usize,
    pub untrusted_source_reachable: usize,
    pub arg_level: usize,
    pub module_level: usize,
    pub crosses_boundary: usize,
    pub source_backed: usize,
}

/// Fixed runtime coverage counters for summary JSON.
#[derive(Debug, Clone, Copy, Default, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SecurityRuntimeStateCounts {
    pub runtime_hot: usize,
    pub runtime_cold: usize,
    pub never_executed: usize,
    pub low_traffic: usize,
    pub coverage_unavailable: usize,
    pub runtime_unknown: usize,
    pub not_collected: usize,
}

/// The `fallow security survivors --format json` schema version.
#[derive(Debug, Clone, Copy, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum SecuritySurvivorsSchemaVersion {
    /// Adds `summary.unverdicted` for incomplete verdict files.
    #[serde(rename = "2")]
    V2,
}

/// Verifier verdict status accepted by `fallow security survivors`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum SecurityVerifierVerdictStatus {
    /// The verifier could not dismiss the candidate from supplied evidence.
    Survivor,
    /// The verifier dismissed the candidate from supplied evidence.
    Dismissed,
    /// The verifier needs human review before dismissal or remediation.
    NeedsHumanReview,
}

/// One supported verifier verdict input row.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SecurityVerifierVerdict {
    pub schema_version: String,
    pub finding_id: String,
    pub verdict: SecurityVerifierVerdictStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rationale: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub impact: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fix_direction: Option<String>,
}

/// The `fallow security survivors --format json` envelope.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SecuritySurvivorsOutput {
    pub schema_version: SecuritySurvivorsSchemaVersion,
    pub version: ToolVersion,
    pub elapsed_ms: ElapsedMs,
    pub summary: SecuritySurvivorsSummary,
    pub survivors: BTreeMap<String, SecuritySurvivor>,
    pub needs_human_review: BTreeMap<String, SecuritySurvivor>,
}

/// Aggregate counts for survivor rendering.
#[derive(Debug, Clone, Copy, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SecuritySurvivorsSummary {
    pub candidates: usize,
    pub verdicts: usize,
    pub survivors: usize,
    pub dismissed: usize,
    pub needs_human_review: usize,
    pub unverdicted: usize,
}

/// One verifier-retained candidate row.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SecuritySurvivor {
    pub finding_id: String,
    pub verdict: SecurityVerifierVerdictStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rationale: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub impact: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fix_direction: Option<String>,
    pub candidate: SecurityFinding,
}

/// The `fallow security blind-spots --format json` schema version.
#[derive(Debug, Clone, Copy, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum SecurityBlindSpotsSchemaVersion {
    /// Initial blind-spot grouping output contract.
    #[serde(rename = "1")]
    V1,
}

/// The `fallow security blind-spots --format json` envelope.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SecurityBlindSpotsOutput {
    pub schema_version: SecurityBlindSpotsSchemaVersion,
    pub version: ToolVersion,
    pub elapsed_ms: ElapsedMs,
    pub summary: SecurityBlindSpotsSummary,
    pub groups: Vec<SecurityBlindSpotGroup>,
}

/// Aggregate counts for blind-spot output.
#[derive(Debug, Clone, Copy, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SecurityBlindSpotsSummary {
    pub unresolved_edge_files: usize,
    pub unresolved_callee_sites: usize,
    pub sampled_callee_sites: usize,
}

/// One actionable blind-spot group.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SecurityBlindSpotGroup {
    pub reason: fallow_types::extract::SkippedSecurityCalleeReason,
    pub expression_kind: fallow_types::extract::SkippedSecurityCalleeExpressionKind,
    pub sampled_count: usize,
    pub files: Vec<SecurityBlindSpotFile>,
    pub suggestion: String,
}

/// One file inside a blind-spot group.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SecurityBlindSpotFile {
    pub path: String,
    pub sampled_count: usize,
}
