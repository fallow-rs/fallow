//! Root JSON output envelopes shared by CLI and programmatic consumers.

use fallow_types::envelope::{ElapsedMs, Meta, SchemaVersion, TelemetryMeta, ToolVersion};
use fallow_types::output::NextStep;
use serde::Serialize;

/// Whether a JSON root envelope keeps the top-level `kind` discriminator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RootEnvelopeMode {
    Tagged,
    Legacy,
}

/// Serialize a typed fallow root envelope with the requested discriminator
/// mode.
///
/// # Errors
///
/// Returns a serde error when the provided envelope cannot be converted to a
/// JSON value.
pub fn serialize_json_root_output<T: Serialize>(
    output: T,
    mode: RootEnvelopeMode,
) -> Result<serde_json::Value, serde_json::Error> {
    let mut value = serde_json::to_value(output)?;
    if mode == RootEnvelopeMode::Legacy {
        remove_root_kind(&mut value);
    }
    Ok(value)
}

/// Remove only the document-root discriminator. Nested objects may carry their
/// own meaningful `kind` fields, so this intentionally does not recurse.
pub fn remove_root_kind(value: &mut serde_json::Value) {
    if let serde_json::Value::Object(map) = value {
        map.remove("kind");
    }
}

/// Apply a document-root discriminator unless the caller requested the legacy
/// envelope shape.
pub fn apply_root_kind(value: &mut serde_json::Value, kind: &'static str, mode: RootEnvelopeMode) {
    if mode == RootEnvelopeMode::Tagged
        && let serde_json::Value::Object(map) = value
    {
        map.insert(
            "kind".to_string(),
            serde_json::Value::String(kind.to_string()),
        );
    }
}

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

/// Typed root of every fallow JSON envelope shape that serializes as a JSON
/// object and participates in the documented `FallowOutput` contract.
///
/// The default wire shape carries a top-level `kind` discriminator so agents and
/// schema-validating clients can select the variant without probing for unique
/// field presence. CodeClimate output is intentionally not in this enum because
/// it serializes as a bare JSON array per the Code Climate / GitLab Code Quality
/// spec.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(
    feature = "schema",
    schemars(title = "fallow --format json (typed root)")
)]
#[serde(tag = "kind")]
#[allow(
    dead_code,
    reason = "some variants are schema-emit only, but runtime roots serialize through this enum where practical"
)]
pub enum FallowOutput<
    Audit,
    Explain,
    Inspect,
    Trace,
    ReviewEnvelope,
    ReviewReconcile,
    CoverageSetup,
    CoverageAnalyze,
    ListBoundaries,
    Workspaces,
    Health,
    Dupes,
    CheckGrouped,
    Impact,
    ImpactCrossRepo,
    SecuritySummary,
    Security,
    SecuritySurvivors,
    SecurityBlindSpots,
    Check,
    Combined,
    AuditBrief,
    DecisionSurface,
    WalkthroughGuide,
    WalkthroughValidation,
> {
    /// `fallow audit --format json`.
    #[serde(rename = "audit")]
    Audit(Audit),
    /// `fallow explain <issue-type> --format json`.
    #[serde(rename = "explain")]
    Explain(Explain),
    /// `fallow inspect --format json`.
    #[serde(rename = "inspect_target")]
    Inspect(Inspect),
    /// `fallow trace <symbol> --format json`.
    #[serde(rename = "trace")]
    Trace(Trace),
    /// `fallow --format review-github` / `--format review-gitlab`.
    #[serde(rename = "review-envelope")]
    ReviewEnvelope(ReviewEnvelope),
    /// `fallow ci reconcile-review --format json`.
    #[serde(rename = "review-reconcile")]
    ReviewReconcile(ReviewReconcile),
    /// `fallow coverage setup --json`.
    #[serde(rename = "coverage-setup")]
    CoverageSetup(CoverageSetup),
    /// `fallow coverage analyze --format json`.
    #[serde(rename = "coverage-analyze")]
    CoverageAnalyze(CoverageAnalyze),
    /// `fallow list --boundaries --format json`.
    #[serde(rename = "list-boundaries")]
    ListBoundaries(ListBoundaries),
    /// `fallow workspaces --format json`.
    #[serde(rename = "list-workspaces")]
    Workspaces(Workspaces),
    /// `fallow health --format json`.
    #[serde(rename = "health")]
    Health(Health),
    /// `fallow dupes --format json`.
    #[serde(rename = "dupes")]
    Dupes(Dupes),
    /// `fallow dead-code --format json --group-by <mode>`.
    #[serde(rename = "dead-code-grouped")]
    CheckGrouped(CheckGrouped),
    /// `fallow impact --format json`.
    #[serde(rename = "impact")]
    Impact(Impact),
    /// `fallow impact --all --format json`.
    #[serde(rename = "impact-cross-repo")]
    ImpactCrossRepo(ImpactCrossRepo),
    /// `fallow security --summary --format json`.
    #[serde(rename = "security")]
    SecuritySummary(SecuritySummary),
    /// `fallow security --format json`.
    #[serde(rename = "security")]
    Security(Security),
    /// `fallow security survivors --format json`.
    #[serde(rename = "security-survivors")]
    SecuritySurvivors(SecuritySurvivors),
    /// `fallow security blind-spots --format json`.
    #[serde(rename = "security-blind-spots")]
    SecurityBlindSpots(SecurityBlindSpots),
    /// `fallow dead-code --format json`.
    #[serde(rename = "dead-code")]
    Check(Check),
    /// Bare `fallow --format json`.
    #[serde(rename = "combined")]
    Combined(Combined),
    /// `fallow audit --brief --format json`.
    #[serde(rename = "audit-brief")]
    AuditBrief(AuditBrief),
    /// `fallow decision-surface --format json`.
    #[serde(rename = "decision-surface")]
    DecisionSurface(DecisionSurface),
    /// `fallow review --walkthrough-guide --format json`.
    #[serde(rename = "review-walkthrough-guide")]
    WalkthroughGuide(WalkthroughGuide),
    /// `fallow review --walkthrough-file --format json`.
    #[serde(rename = "review-walkthrough-validation")]
    WalkthroughValidation(WalkthroughValidation),
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn legacy_mode_removes_only_root_kind() {
        let mut value = json!({
            "kind": "root",
            "action": {
                "kind": "suppress"
            }
        });

        remove_root_kind(&mut value);

        assert!(value.get("kind").is_none());
        assert_eq!(value["action"]["kind"], "suppress");
    }

    #[test]
    fn apply_root_kind_respects_legacy_mode() {
        let mut value = json!({});

        apply_root_kind(&mut value, "dead_code", RootEnvelopeMode::Legacy);

        assert!(value.get("kind").is_none());
    }

    #[test]
    fn apply_root_kind_sets_tagged_mode() {
        let mut value = json!({});

        apply_root_kind(&mut value, "dead_code", RootEnvelopeMode::Tagged);

        assert_eq!(value["kind"], "dead_code");
    }

    #[test]
    fn serialize_json_root_output_removes_root_kind_in_legacy_mode() {
        let value = serialize_json_root_output(
            json!({
                "kind": "combined",
                "schema_version": 1
            }),
            RootEnvelopeMode::Legacy,
        )
        .expect("root should serialize");

        assert!(value.get("kind").is_none());
        assert_eq!(value["schema_version"], 1);
    }
}
