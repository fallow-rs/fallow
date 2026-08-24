//! Security command output contracts.

use std::collections::BTreeMap;

use crate::root_envelopes::{RootEnvelopeMode, attach_telemetry_meta, serialize_named_json_output};
use fallow_types::envelope::{ElapsedMs, Meta, ToolVersion};
use fallow_types::results::{
    SecurityAttackSurfaceEntry, SecurityFinding, SecurityFindingKind, SecurityRuntimeState,
    SecuritySeverity, TaintConfidence,
};
use fallow_types::workspace::WorkspaceDiagnostic;
use serde::{Deserialize, Serialize, de::DeserializeOwned};

/// Current `fallow security --format json` schema version.
pub const SECURITY_SCHEMA_VERSION: u32 = 8;

/// The `fallow security --format json` schema version. Independently versioned
/// from the main contract, mirroring `ImpactReportSchemaVersion`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
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
    /// Expands the required semantic omission reason-code enum.
    #[serde(rename = "8")]
    V8,
}

/// Gate verdict on the wire. `fail` is the CI-state token; human output renders
/// it as "REVIEW REQUIRED" because these stay unverified candidates, never
/// confirmed vulnerabilities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
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
#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SecurityGate<Mode> {
    /// Gate mode that was selected on the command line.
    pub mode: Mode,
    /// Gate outcome for this run.
    pub verdict: SecurityGateVerdict,
    /// Number of candidates matching the selected gate mode.
    pub new_count: usize,
}

/// Allowlisted config context for `fallow security --format json`.
#[derive(Debug, Clone, Deserialize, Serialize)]
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

/// Per-rule severity context inside [`SecurityOutputConfig::rules`].
#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SecurityOutputRulesConfig<Severity> {
    /// Severity context for the client-server-leak rule.
    pub security_client_server_leak: SecurityRuleSeverityConfig<Severity>,
    /// Severity context for the security-sink rule.
    pub security_sink: SecurityRuleSeverityConfig<Severity>,
}

/// Configured-versus-effective severity for one security rule.
#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
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
    /// Diagnostics owned by this security analysis run.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub workspace_diagnostics: Vec<WorkspaceDiagnostic>,
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
#[derive(Debug, Clone, Deserialize, Serialize)]
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
#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SecurityUnresolvedCalleeSample {
    /// File path relative to the analysed root.
    pub path: String,
    /// 1-based line of the skipped call site.
    pub line: u32,
    /// 1-based column of the skipped call site.
    pub col: u32,
    /// Why the callee could not be resolved.
    pub reason: fallow_types::extract::SkippedSecurityCalleeReason,
    /// Compact syntax shape of the skipped callee.
    pub expression_kind: fallow_types::extract::SkippedSecurityCalleeExpressionKind,
}

/// Count of unresolved callees in one file.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SecurityUnresolvedCalleeTopFile {
    /// File path relative to the analysed root.
    pub path: String,
    /// Number of unresolved callees in this file.
    pub count: usize,
}

/// Count of unresolved callees for one reason.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SecurityUnresolvedCalleeReasonCount {
    /// Why the callees could not be resolved.
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
    /// Diagnostics owned by the full security analysis summarized here.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub workspace_diagnostics: Vec<WorkspaceDiagnostic>,
    /// Aggregate security counts after all filters, gates, and scopes.
    pub summary: SecuritySummary,
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
enum SavedSecurityGateMode {
    New,
    NewlyReachable,
}

#[derive(Deserialize)]
struct SavedSecurityEnvelope {
    #[serde(rename = "version")]
    _version: String,
    #[serde(rename = "elapsed_ms")]
    _elapsed_ms: u64,
    #[serde(rename = "config")]
    _config: SecurityOutputConfig<fallow_config::Severity>,
    #[serde(rename = "_meta", default)]
    meta: Option<serde_json::Value>,
    #[serde(rename = "gate", default)]
    _gate: Option<SecurityGate<SavedSecurityGateMode>>,
}

#[derive(Deserialize)]
struct SavedSecurityFullPayload {
    #[serde(rename = "security_findings")]
    _security_findings: Vec<SecurityFinding>,
    #[serde(rename = "attack_surface", default)]
    _attack_surface: Option<Vec<SecurityAttackSurfaceEntry>>,
    #[serde(rename = "unresolved_edge_files")]
    _unresolved_edge_files: usize,
    #[serde(rename = "unresolved_callee_sites")]
    _unresolved_callee_sites: usize,
    #[serde(rename = "unresolved_callee_diagnostics", default)]
    _unresolved_callee_diagnostics: Option<SecurityUnresolvedCalleeDiagnostics>,
}

#[derive(Deserialize)]
struct SavedSecuritySummaryPayload {
    #[serde(rename = "summary")]
    _summary: SecuritySummary,
}

/// Validate a saved security envelope against the current output-owned schema.
///
/// Older known schemas remain readable for compatibility. Current envelopes
/// fail closed when required fields or nested payloads are malformed.
pub fn validate_saved_security_envelope(value: &serde_json::Value) -> Result<(), String> {
    let raw_version = value
        .get("schema_version")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "saved security envelope is missing a string `schema_version`".to_owned())?;
    let version = raw_version.parse::<u32>().map_err(|_| {
        format!("saved security envelope has invalid schema version `{raw_version}`")
    })?;
    let current = SECURITY_SCHEMA_VERSION;
    if version == 0 || version > current {
        return Err(format!(
            "unsupported saved security schema version {version}; this Fallow version supports versions 1 through {current}"
        ));
    }
    if version < current {
        return Ok(());
    }

    let envelope: SavedSecurityEnvelope = parse_saved_security(value, "envelope")?;
    if let Some(meta) = envelope.meta {
        let meta = meta
            .as_object()
            .ok_or_else(|| "saved security envelope field `_meta` must be an object".to_owned())?;
        if let Some(type_aware) = meta.get("type_aware").filter(|value| !value.is_null()) {
            parse_saved_security::<fallow_types::envelope::TypeAwareMeta>(
                type_aware,
                "type-aware metadata",
            )?;
        }
    }

    if value.get("security_findings").is_some() {
        parse_saved_security::<SavedSecurityFullPayload>(value, "full payload")?;
    } else if value.get("summary").is_some() {
        parse_saved_security::<SavedSecuritySummaryPayload>(value, "summary payload")?;
    } else {
        return Err(
            "saved security envelope is missing `security_findings` or `summary`".to_owned(),
        );
    }
    Ok(())
}

fn parse_saved_security<T: DeserializeOwned>(
    value: &serde_json::Value,
    label: &str,
) -> Result<T, String> {
    serde_json::from_value(value.clone()).map_err(|error| {
        format!("saved security {label} is incompatible with this Fallow version: {error}")
    })
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
        workspace_diagnostics: output.workspace_diagnostics.clone(),
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
#[derive(Debug, Clone, Deserialize, Serialize)]
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
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SecuritySeverityCounts {
    /// High-severity candidates.
    pub high: usize,
    /// Medium-severity candidates.
    pub medium: usize,
    /// Low-severity candidates.
    pub low: usize,
}

/// Fixed reachability counters for summary JSON.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SecurityReachabilityCounts {
    /// Candidates reachable from an entry point.
    pub entry_reachable: usize,
    /// Candidates reachable from an untrusted input source.
    pub untrusted_source_reachable: usize,
    /// Candidates where taint flows through a call argument.
    pub arg_level: usize,
    /// Candidates where taint is only module-level.
    pub module_level: usize,
    /// Candidates whose flow crosses a client/server boundary.
    pub crosses_boundary: usize,
    /// Candidates backed by a concrete taint source.
    pub source_backed: usize,
}

/// Fixed runtime coverage counters for summary JSON.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SecurityRuntimeStateCounts {
    /// Candidates on frequently executed runtime paths.
    pub runtime_hot: usize,
    /// Candidates on rarely executed runtime paths.
    pub runtime_cold: usize,
    /// Candidates on paths never seen executing.
    pub never_executed: usize,
    /// Candidates on low-traffic paths.
    pub low_traffic: usize,
    /// Candidates in files runtime coverage did not observe.
    pub coverage_unavailable: usize,
    /// Candidates whose runtime state could not be classified.
    pub runtime_unknown: usize,
    /// Candidates analysed without any runtime coverage data.
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
    /// Verifier's verdict for the candidate.
    pub verdict: SecurityVerifierVerdictStatus,
    /// Short machine-oriented verdict reason.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Longer free-form verdict explanation.
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
    /// Diagnostics preserved from the candidate security report.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub workspace_diagnostics: Vec<WorkspaceDiagnostic>,
    /// Aggregate verdict counts.
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
    /// Candidates in the input security report.
    pub candidates: usize,
    /// Verifier verdicts supplied.
    pub verdicts: usize,
    /// Candidates the verifier retained.
    pub survivors: usize,
    /// Candidates the verifier dismissed.
    pub dismissed: usize,
    /// Candidates the verifier flagged as ambiguous.
    pub needs_human_review: usize,
    /// Candidates without any verdict.
    pub unverdicted: usize,
}

/// One verifier-retained candidate row.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SecuritySurvivor {
    /// Stable candidate id from `security_findings[].finding_id`.
    pub finding_id: String,
    /// Verifier's verdict for the candidate.
    pub verdict: SecurityVerifierVerdictStatus,
    /// Short machine-oriented verdict reason.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Longer free-form verdict explanation.
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
    /// Diagnostics owned by the security analysis used for this view.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub workspace_diagnostics: Vec<WorkspaceDiagnostic>,
    /// Aggregate blind-spot counts from the security analysis.
    pub summary: SecurityBlindSpotsSummary,
    /// Grouped unresolved callee diagnostics, derived from existing samples.
    pub groups: Vec<SecurityBlindSpotGroup>,
}

/// Aggregate counts for blind-spot output.
#[derive(Debug, Clone, Copy, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SecurityBlindSpotsSummary {
    /// Files containing at least one unresolved callee.
    pub unresolved_edge_files: usize,
    /// Total unresolved callee sites in the analysis.
    pub unresolved_callee_sites: usize,
    /// Callee sites captured in the bounded diagnostic sample.
    pub sampled_callee_sites: usize,
}

/// One actionable blind-spot group.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SecurityBlindSpotGroup {
    /// Why the callees in this group could not be resolved.
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
    /// File path relative to the analysed root.
    pub path: String,
    /// Count in the bounded diagnostic sample.
    pub sampled_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn current_security_envelope() -> serde_json::Value {
        json!({
            "schema_version": "8",
            "version": "test",
            "elapsed_ms": 1,
            "config": {
                "rules": {
                    "security_client_server_leak": {
                        "configured": "warn",
                        "effective": "warn"
                    },
                    "security_sink": {
                        "configured": "warn",
                        "effective": "warn"
                    }
                },
                "categories_include": null,
                "categories_exclude": null
            },
            "security_findings": [],
            "unresolved_edge_files": 0,
            "unresolved_callee_sites": 0
        })
    }

    #[test]
    fn security_summary_json_output_uses_security_root_contract() {
        let output = SecurityOutput {
            schema_version: SecuritySchemaVersion::V8,
            version: ToolVersion("test".to_string()),
            elapsed_ms: ElapsedMs(12),
            config: json!({"rules": {}}),
            meta: None,
            gate: None::<()>,
            workspace_diagnostics: vec![WorkspaceDiagnostic::new(
                std::path::Path::new("/project"),
                std::path::PathBuf::from("package.json"),
                fallow_types::workspace::WorkspaceDiagnosticKind::UndeclaredWorkspace,
            )],
            security_findings: Vec::new(),
            attack_surface: None,
            unresolved_edge_files: 2,
            unresolved_callee_sites: 3,
            unresolved_callee_diagnostics: None,
        };

        let value = serialize_security_summary_json_output(&output, RootEnvelopeMode::Tagged, None)
            .expect("security summary should serialize");

        assert_eq!(value["kind"], "security");
        assert_eq!(value["schema_version"], "8");
        assert_eq!(value["summary"]["security_findings"], 0);
        assert_eq!(value["summary"]["unresolved_edge_files"], 2);
        assert_eq!(value["summary"]["unresolved_callee_sites"], 3);
        assert_eq!(value["workspace_diagnostics"][0]["path"], "package.json");
        assert!(value.get("security_findings").is_none());
    }

    #[test]
    fn saved_security_validator_accepts_current_full_and_summary_payloads() {
        let full = current_security_envelope();
        validate_saved_security_envelope(&full).expect("current full security envelope");

        let mut summary = current_security_envelope();
        summary
            .as_object_mut()
            .expect("security envelope")
            .remove("security_findings");
        summary
            .as_object_mut()
            .expect("security envelope")
            .remove("unresolved_edge_files");
        summary
            .as_object_mut()
            .expect("security envelope")
            .remove("unresolved_callee_sites");
        summary["summary"] = serde_json::to_value(SecuritySummary {
            security_findings: 0,
            by_severity: SecuritySeverityCounts::default(),
            by_category: BTreeMap::new(),
            by_reachability: SecurityReachabilityCounts::default(),
            by_runtime_state: SecurityRuntimeStateCounts::default(),
            unresolved_edge_files: 0,
            unresolved_callee_sites: 0,
            attack_surface_entries: 0,
        })
        .expect("security summary");
        validate_saved_security_envelope(&summary).expect("current summary security envelope");
    }

    #[test]
    fn saved_security_validator_accepts_legacy_v7_type_aware_payload() {
        let mut envelope = current_security_envelope();
        envelope["schema_version"] = json!("7");
        envelope["_meta"] = json!({
            "type_aware": {
                "executed": true,
                "protocol_version": 6,
                "sidecar_version": "0.6.0",
                "backend": "typescript-go",
                "backend_version": "7.0.0-dev",
                "selected_tsconfigs": ["tsconfig.json"],
                "candidate_count": 1,
                "confirmed_used_count": 0,
                "contract_preserved_count": 0,
                "no_static_references_count": 1,
                "fix_eligible_count": 0,
                "unresolved_count": 0,
                "abstained_count": 0,
                "abstention_reasons": {
                    "no_project": 0,
                    "ambiguous_project": 0,
                    "blocking_diagnostics": 0,
                    "svelte_virtual_module_exports": 0,
                    "unknown_symbol": 0,
                    "unsupported_syntax": 0,
                    "capacity": 0
                },
                "projects": [],
                "warning_count": 0,
                "warnings": [],
                "elapsed_ms": 4,
                "phase_timings_ms": {
                    "project_setup": 1,
                    "diagnostics": 1,
                    "symbol_scan": 2
                }
            }
        });

        validate_saved_security_envelope(&envelope)
            .expect("legacy schema 7 type-aware metadata stays readable");
    }

    #[test]
    fn saved_security_validator_rejects_malformed_current_payloads() {
        let mut findings = current_security_envelope();
        findings["security_findings"] = json!("not-an-array");
        assert!(validate_saved_security_envelope(&findings).is_err());

        let mut type_aware = current_security_envelope();
        type_aware["_meta"] = json!({"type_aware": {"queries": "not-an-array"}});
        assert!(validate_saved_security_envelope(&type_aware).is_err());
    }

    #[test]
    fn saved_security_validator_accepts_legacy_and_rejects_future_schema() {
        validate_saved_security_envelope(&json!({"schema_version": "7"}))
            .expect("known legacy security schema");
        let error = validate_saved_security_envelope(&json!({"schema_version": "9"}))
            .expect_err("future security schema must fail closed");
        assert!(error.contains("unsupported saved security schema version 9"));
    }
}
