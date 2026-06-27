//! Security command output contracts.

use std::collections::BTreeMap;

use crate::root_envelopes::{RootEnvelopeMode, attach_telemetry_meta, serialize_named_json_output};
use fallow_types::envelope::{ElapsedMs, Meta, ToolVersion};
use fallow_types::results::{
    SecurityAttackSurfaceEntry, SecurityFinding, SecurityFindingKind, SecurityRuntimeState,
    SecuritySeverity, TaintConfidence,
};
use serde::{Deserialize, Serialize};

/// The `fallow security --format json` schema version. Independently versioned
/// from the main contract, mirroring `ImpactReportSchemaVersion`.
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

/// Gate verdict on the wire. `fail` is the CI-state token; human output renders
/// it as "REVIEW REQUIRED" because these stay unverified candidates, never
/// confirmed vulnerabilities.
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
/// Invariant: `verdict == Fail  IFF  exit code 8  IFF  new_count > 0`.
#[derive(Debug, Clone, Copy, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SecurityGate<Mode> {
    pub mode: Mode,
    pub verdict: SecurityGateVerdict,
    /// Number of candidates matching the selected gate mode.
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
    /// Relevant rule severities before and after this command applies its
    /// default-on behavior for security-only rules.
    pub rules: SecurityOutputRulesConfig<Severity>,
    /// `security.categories.include` from config. `null` means unset, `[]`
    /// means explicitly empty.
    pub categories_include: Option<Vec<String>>,
    /// `security.categories.exclude` from config. `null` means unset, `[]`
    /// means explicitly empty.
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
    /// Severity read from resolved config before the security command applies
    /// its default-on behavior.
    pub configured: Severity,
    /// Severity used for this command run.
    pub effective: Severity,
}

/// The `fallow security --format json` envelope. `FallowOutput` discriminates it
/// by the `kind: "security"` tag; the optional `gate` block is additive and is
/// not part of that discrimination.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SecurityOutput<Config, Gate> {
    /// Schema version of this envelope.
    pub schema_version: SecuritySchemaVersion,
    /// Fallow CLI version that produced this output.
    pub version: ToolVersion,
    /// Wall-clock milliseconds spent producing the report.
    pub elapsed_ms: ElapsedMs,
    /// Privacy-safe config context relevant to security candidate generation.
    pub config: Config,
    /// Security-specific rule and field metadata, emitted with `--explain`.
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Meta>,
    /// Gate verdict, present only when `--gate <mode>` was set (issue #886).
    /// Emitted on pass too (`verdict: "pass"`, `new_count: 0`) so consumers
    /// distinguish "gate ran and passed" from "gate did not run" (absent).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gate: Option<Gate>,
    /// Security candidates. Paths are project-root-relative, forward-slash.
    pub security_findings: Vec<SecurityFinding>,
    /// Opt-in attack-surface inventory from untrusted entry points to reachable
    /// sinks. Present only when `--surface` was requested.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attack_surface: Option<Vec<SecurityAttackSurfaceEntry>>,
    /// In-band blind spot: number of `"use client"` files whose transitive
    /// import cone contains a dynamic `import()` the reachability BFS could not
    /// follow. A leak hidden behind such an edge would not be reported, so a
    /// zero finding count with a non-zero value here is NOT a clean bill.
    pub unresolved_edge_files: usize,
    /// In-band blind spot: number of sink-shaped nodes the catalogue detector
    /// could not flatten to a static callee path (dynamic dispatch, computed
    /// members, aliased bindings). A zero finding count with a non-zero value
    /// here is NOT a clean bill.
    pub unresolved_callee_sites: usize,
    /// Bounded diagnostics for unresolved callee blind spots.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unresolved_callee_diagnostics: Option<SecurityUnresolvedCalleeDiagnostics>,
}

/// Bounded unresolved-callee diagnostics for `fallow security --format json`.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SecurityUnresolvedCalleeDiagnostics {
    /// Deterministic sample rows, capped by `sample_limit`.
    pub sampled: Vec<SecurityUnresolvedCalleeSample>,
    /// Files with the most unresolved callees, capped by `top_files_limit`.
    pub top_files: Vec<SecurityUnresolvedCalleeTopFile>,
    /// Full count by unresolved-callee reason, sorted by count then reason.
    pub by_reason: Vec<SecurityUnresolvedCalleeReasonCount>,
    /// Maximum number of sample rows emitted.
    pub sample_limit: usize,
    /// Maximum number of top-file rows emitted.
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
    /// Compact syntax shape of the skipped callee.
    pub expression_kind: fallow_types::extract::SkippedSecurityCalleeExpressionKind,
}

/// Count of unresolved callees in one file.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SecurityUnresolvedCalleeTopFile {
    pub path: String,
    /// Number of unresolved callees in this file.
    pub count: usize,
}

/// Count of unresolved callees for one reason.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SecurityUnresolvedCalleeReasonCount {
    pub reason: fallow_types::extract::SkippedSecurityCalleeReason,
    /// Number of unresolved callees with this reason.
    pub count: usize,
}

/// Compact `fallow security --summary --format json` payload. Uses the same
/// `kind: "security"` discriminator as the full payload, but omits candidate
/// arrays and exposes only aggregate counts.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SecuritySummaryOutput<Config, Gate> {
    /// Schema version of this envelope.
    pub schema_version: SecuritySchemaVersion,
    /// Fallow CLI version that produced this output.
    pub version: ToolVersion,
    /// Wall-clock milliseconds spent producing the report.
    pub elapsed_ms: ElapsedMs,
    /// Privacy-safe config context relevant to security candidate generation.
    pub config: Config,
    /// Security-specific rule and field metadata, emitted with `--explain`.
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Meta>,
    /// Gate verdict, present only when `--gate <mode>` was set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gate: Option<Gate>,
    /// Aggregate security counts after all filters, gates, and scopes.
    pub summary: SecuritySummary,
}

/// Build the compact aggregate payload for `fallow security --summary --format json`.
#[must_use]
pub fn build_security_summary<Config, Gate>(
    output: &SecurityOutput<Config, Gate>,
) -> SecuritySummary {
    let mut counts = SecuritySummaryCounts::default();

    for finding in &output.security_findings {
        counts.record(finding);
    }

    SecuritySummary {
        security_findings: output.security_findings.len(),
        by_severity: counts.severity,
        by_category: counts.category,
        by_reachability: counts.reachability,
        by_runtime_state: counts.runtime_state,
        unresolved_edge_files: output.unresolved_edge_files,
        unresolved_callee_sites: output.unresolved_callee_sites,
        attack_surface_entries: output.attack_surface.as_ref().map_or(0, Vec::len),
    }
}

#[derive(Default)]
struct SecuritySummaryCounts {
    severity: SecuritySeverityCounts,
    category: BTreeMap<String, usize>,
    reachability: SecurityReachabilityCounts,
    runtime_state: SecurityRuntimeStateCounts,
}

impl SecuritySummaryCounts {
    fn record(&mut self, finding: &SecurityFinding) {
        record_security_severity(finding.severity, &mut self.severity);
        record_security_category(finding, &mut self.category);
        record_security_reachability(finding, &mut self.reachability);
        record_security_runtime_state(finding, &mut self.runtime_state);
    }
}

fn record_security_severity(severity: SecuritySeverity, by_severity: &mut SecuritySeverityCounts) {
    match severity {
        SecuritySeverity::High => by_severity.high += 1,
        SecuritySeverity::Medium => by_severity.medium += 1,
        SecuritySeverity::Low => by_severity.low += 1,
    }
}

fn record_security_category(finding: &SecurityFinding, by_category: &mut BTreeMap<String, usize>) {
    let category = finding
        .category
        .clone()
        .unwrap_or_else(|| security_kind_key(finding.kind).to_owned());
    *by_category.entry(category).or_insert(0) += 1;
}

fn security_kind_key(kind: SecurityFindingKind) -> &'static str {
    match kind {
        SecurityFindingKind::ClientServerLeak => "client-server-leak",
        SecurityFindingKind::TaintedSink => "tainted-sink",
    }
}

fn record_security_reachability(
    finding: &SecurityFinding,
    by_reachability: &mut SecurityReachabilityCounts,
) {
    if finding.source_backed {
        by_reachability.source_backed += 1;
    }
    let Some(reachability) = &finding.reachability else {
        return;
    };

    if reachability.reachable_from_entry {
        by_reachability.entry_reachable += 1;
    }
    if reachability.reachable_from_untrusted_source {
        by_reachability.untrusted_source_reachable += 1;
    }
    if reachability.crosses_boundary {
        by_reachability.crosses_boundary += 1;
    }
    match reachability.taint_confidence {
        Some(TaintConfidence::ArgLevel) => by_reachability.arg_level += 1,
        Some(TaintConfidence::ModuleLevel) => by_reachability.module_level += 1,
        None => {}
    }
}

fn record_security_runtime_state(
    finding: &SecurityFinding,
    by_runtime_state: &mut SecurityRuntimeStateCounts,
) {
    match finding.runtime.as_ref().map(|runtime| runtime.state) {
        Some(SecurityRuntimeState::RuntimeHot) => by_runtime_state.runtime_hot += 1,
        Some(SecurityRuntimeState::RuntimeCold) => by_runtime_state.runtime_cold += 1,
        Some(SecurityRuntimeState::NeverExecuted) => by_runtime_state.never_executed += 1,
        Some(SecurityRuntimeState::LowTraffic) => by_runtime_state.low_traffic += 1,
        Some(SecurityRuntimeState::CoverageUnavailable) => {
            by_runtime_state.coverage_unavailable += 1;
        }
        Some(SecurityRuntimeState::RuntimeUnknown) => by_runtime_state.runtime_unknown += 1,
        None => by_runtime_state.not_collected += 1,
    }
}

/// Serialize the full `fallow security --format json` envelope.
///
/// # Errors
///
/// Returns a serde error when the envelope cannot be converted to JSON.
pub fn serialize_security_json_output<Config, Gate>(
    output: SecurityOutput<Config, Gate>,
    mode: RootEnvelopeMode,
    analysis_run_id: Option<&str>,
) -> Result<serde_json::Value, serde_json::Error>
where
    Config: Serialize,
    Gate: Serialize,
{
    let mut value = serialize_named_json_output(output, "security", mode)?;
    attach_telemetry_meta(&mut value, analysis_run_id);
    Ok(value)
}

/// Serialize the compact `fallow security --summary --format json` envelope.
///
/// # Errors
///
/// Returns a serde error when the envelope cannot be converted to JSON.
pub fn serialize_security_summary_json_output<Config, Gate>(
    output: &SecurityOutput<Config, Gate>,
    mode: RootEnvelopeMode,
    analysis_run_id: Option<&str>,
) -> Result<serde_json::Value, serde_json::Error>
where
    Config: Clone + Serialize,
    Gate: Copy + Serialize,
{
    let summary = SecuritySummaryOutput {
        schema_version: output.schema_version,
        version: output.version.clone(),
        elapsed_ms: output.elapsed_ms,
        config: output.config.clone(),
        meta: output.meta.clone(),
        gate: output.gate,
        summary: build_security_summary(output),
    };
    let mut value = serialize_named_json_output(summary, "security", mode)?;
    attach_telemetry_meta(&mut value, analysis_run_id);
    Ok(value)
}

/// Serialize the `fallow security survivors --format json` envelope.
///
/// # Errors
///
/// Returns a serde error when the envelope cannot be converted to JSON.
pub fn serialize_security_survivors_json_output(
    output: SecuritySurvivorsOutput,
    mode: RootEnvelopeMode,
) -> Result<serde_json::Value, serde_json::Error> {
    serialize_named_json_output(output, "security-survivors", mode)
}

/// Serialize the `fallow security blind-spots --format json` envelope.
///
/// # Errors
///
/// Returns a serde error when the envelope cannot be converted to JSON.
pub fn serialize_security_blind_spots_json_output(
    output: SecurityBlindSpotsOutput,
    mode: RootEnvelopeMode,
) -> Result<serde_json::Value, serde_json::Error> {
    serialize_named_json_output(output, "security-blind-spots", mode)
}

/// Aggregate counts for `fallow security --summary --format json`.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SecuritySummary {
    /// Number of security candidates after all filters, gates, and scopes.
    pub security_findings: usize,
    /// Fixed severity counts for the closed security severity enum.
    pub by_severity: SecuritySeverityCounts,
    /// Finding counts by catalogue category, or by kind for findings without a
    /// catalogue category.
    pub by_category: BTreeMap<String, usize>,
    /// Fixed reachability counts for ranking and triage signals.
    pub by_reachability: SecurityReachabilityCounts,
    /// Fixed runtime coverage counts for runtime-state triage signals.
    pub by_runtime_state: SecurityRuntimeStateCounts,
    /// Number of client files whose dynamic imports could not be followed.
    pub unresolved_edge_files: usize,
    /// Number of sink-shaped callees that could not be statically flattened.
    pub unresolved_callee_sites: usize,
    /// Number of attack-surface entries included in the prepared full output.
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
    /// Must be `fallow-security-verdict/v1`.
    pub schema_version: String,
    /// Stable candidate id from `security_findings[].finding_id`.
    pub finding_id: String,
    pub verdict: SecurityVerifierVerdictStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rationale: Option<String>,
    /// Optional verifier-provided confidence or review priority.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<String>,
    /// Optional verifier-provided impact statement.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub impact: Option<String>,
    /// Optional verifier-owned remediation direction.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fix_direction: Option<String>,
}

/// The `fallow security survivors --format json` envelope.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SecuritySurvivorsOutput {
    /// Schema version of this envelope.
    pub schema_version: SecuritySurvivorsSchemaVersion,
    /// Fallow CLI version that produced this output.
    pub version: ToolVersion,
    /// Wall-clock milliseconds spent producing the report.
    pub elapsed_ms: ElapsedMs,
    pub summary: SecuritySurvivorsSummary,
    /// Verifier-retained candidates keyed by finding id.
    pub survivors: BTreeMap<String, SecuritySurvivor>,
    /// Ambiguous candidates keyed by finding id. These are not dismissed and are
    /// kept explicit so queues can decide whether to include them.
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
    /// Stable candidate id from `security_findings[].finding_id`.
    pub finding_id: String,
    pub verdict: SecurityVerifierVerdictStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rationale: Option<String>,
    /// Optional verifier-provided confidence or review priority.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<String>,
    /// Optional verifier-provided impact statement.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub impact: Option<String>,
    /// Optional verifier-owned remediation direction.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fix_direction: Option<String>,
    /// Original typed fallow security candidate.
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
    /// Schema version of this envelope.
    pub schema_version: SecurityBlindSpotsSchemaVersion,
    /// Fallow CLI version that produced this output.
    pub version: ToolVersion,
    /// Wall-clock milliseconds spent producing the report.
    pub elapsed_ms: ElapsedMs,
    /// Aggregate blind-spot counts from the security analysis.
    pub summary: SecurityBlindSpotsSummary,
    /// Grouped unresolved callee diagnostics, derived from existing samples.
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
    /// Compact syntax shape of the skipped callee.
    pub expression_kind: fallow_types::extract::SkippedSecurityCalleeExpressionKind,
    /// Count in the bounded diagnostic sample.
    pub sampled_count: usize,
    /// Top files in this bounded diagnostic sample.
    pub files: Vec<SecurityBlindSpotFile>,
    /// Suggested next action for this group.
    pub suggestion: String,
}

/// One file inside a blind-spot group.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SecurityBlindSpotFile {
    pub path: String,
    /// Count in the bounded diagnostic sample.
    pub sampled_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn security_summary_json_output_uses_security_root_contract() {
        let output = SecurityOutput {
            schema_version: SecuritySchemaVersion::V7,
            version: ToolVersion("test".to_string()),
            elapsed_ms: ElapsedMs(12),
            config: json!({"rules": {}}),
            meta: None,
            gate: None::<()>,
            security_findings: Vec::new(),
            attack_surface: None,
            unresolved_edge_files: 2,
            unresolved_callee_sites: 3,
            unresolved_callee_diagnostics: None,
        };

        let value = serialize_security_summary_json_output(&output, RootEnvelopeMode::Tagged, None)
            .expect("security summary should serialize");

        assert_eq!(value["kind"], "security");
        assert_eq!(value["schema_version"], "7");
        assert_eq!(value["summary"]["security_findings"], 0);
        assert_eq!(value["summary"]["unresolved_edge_files"], 2);
        assert_eq!(value["summary"]["unresolved_callee_sites"], 3);
        assert!(value.get("security_findings").is_none());
    }
}
