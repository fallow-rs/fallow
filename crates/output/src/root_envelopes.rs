//! Root JSON output envelopes shared by CLI and programmatic consumers.

use fallow_types::envelope::{ElapsedMs, Meta, SchemaVersion, TelemetryMeta, ToolVersion};
use fallow_types::output::NextStep;
use fallow_types::workspace::WorkspaceDiagnostic;
use serde::Serialize;

/// Current schema version for `fallow audit --format json`.
pub const AUDIT_SCHEMA_VERSION: u32 = 10;

/// Current schema version for bare combined JSON output.
///
/// Version 11 tracks the expanded required semantic omission reason-code enum
/// embedded by its analysis subreports.
pub const COMBINED_SCHEMA_VERSION: u32 = 11;

/// Schema projection for the audit envelope's exact version.
#[cfg(feature = "schema")]
#[allow(dead_code, reason = "schema-only type used by the field projection")]
#[derive(schemars::JsonSchema)]
#[schemars(extend("const" = AUDIT_SCHEMA_VERSION))]
struct AuditSchemaVersion(u32);

/// Schema projection for the combined envelope's exact version.
#[cfg(feature = "schema")]
#[allow(dead_code, reason = "schema-only type used by the field projection")]
#[derive(schemars::JsonSchema)]
#[schemars(extend("const" = COMBINED_SCHEMA_VERSION))]
struct CombinedSchemaVersion(u32);

/// JSON root envelope discriminator policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RootEnvelopeMode {
    /// Emit a top-level `kind` discriminator on the JSON root.
    Tagged,
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
    let _ = mode;
    serde_json::to_value(output)
}

/// Serialize an output envelope and apply an explicit root discriminator.
///
/// Use this for command surfaces whose runtime shape is already a typed
/// envelope struct and does not need to pass through the schema-only
/// [`FallowOutput`] enum just to get a top-level `kind`.
///
/// # Errors
///
/// Returns a serde error when the provided envelope cannot be converted to a
/// JSON value.
pub fn serialize_named_json_output<T: Serialize>(
    output: T,
    kind: &'static str,
    mode: RootEnvelopeMode,
) -> Result<serde_json::Value, serde_json::Error> {
    let mut value = serde_json::to_value(output)?;
    apply_root_kind(&mut value, kind, mode);
    Ok(value)
}

/// Serialize a typed `fallow audit --format json` envelope with the standard
/// root discriminator policy.
///
/// # Errors
///
/// Returns a serde error when the provided envelope cannot be converted to a
/// JSON value.
pub fn serialize_audit_json_output<
    Verdict,
    Summary,
    Attribution,
    DeadCode,
    Duplication,
    Complexity,
>(
    output: AuditOutput<Verdict, Summary, Attribution, DeadCode, Duplication, Complexity>,
    mode: RootEnvelopeMode,
    analysis_run_id: Option<&str>,
) -> Result<serde_json::Value, serde_json::Error>
where
    Verdict: Serialize,
    Summary: Serialize,
    Attribution: Serialize,
    DeadCode: Serialize,
    Duplication: Serialize,
    Complexity: Serialize,
{
    let mut value = serde_json::to_value(output)?;
    apply_root_kind(&mut value, "audit", mode);
    attach_telemetry_meta(&mut value, analysis_run_id);
    Ok(value)
}

/// Serialize a typed bare `fallow --format json` combined envelope with the
/// standard root discriminator policy.
///
/// # Errors
///
/// Returns a serde error when the provided envelope cannot be converted to a
/// JSON value.
pub fn serialize_combined_json_output<Check, Dupes, Health>(
    output: CombinedOutput<Check, Dupes, Health>,
    mode: RootEnvelopeMode,
    analysis_run_id: Option<&str>,
) -> Result<serde_json::Value, serde_json::Error>
where
    Check: Serialize,
    Dupes: Serialize,
    Health: Serialize,
{
    let mut value = serde_json::to_value(output)?;
    apply_root_kind(&mut value, "combined", mode);
    attach_telemetry_meta(&mut value, analysis_run_id);
    Ok(value)
}

/// Apply a document-root discriminator.
pub fn apply_root_kind(value: &mut serde_json::Value, kind: &'static str, mode: RootEnvelopeMode) {
    let _ = mode;
    if let serde_json::Value::Object(map) = value {
        let previous = map.shift_insert(
            0,
            "kind".to_string(),
            serde_json::Value::String(kind.to_string()),
        );
        if let Some(previous) = previous
            && let Some(current) = map.get_mut("kind")
        {
            *current = previous;
        }
    }
}

/// Attach telemetry metadata to a JSON root object when a run id is available.
pub fn attach_telemetry_meta(value: &mut serde_json::Value, analysis_run_id: Option<&str>) {
    let Some(analysis_run_id) = analysis_run_id else {
        return;
    };
    let serde_json::Value::Object(map) = value else {
        return;
    };
    let meta = map
        .entry("_meta".to_string())
        .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
    if !meta.is_object() {
        *meta = serde_json::Value::Object(serde_json::Map::new());
    }
    if let serde_json::Value::Object(meta_map) = meta {
        meta_map.insert(
            "telemetry".to_string(),
            serde_json::json!({ "analysis_run_id": analysis_run_id }),
        );
    }
}

/// `fallow audit --format json` envelope.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "schema", schemars(title = "fallow audit --format json"))]
pub struct AuditOutput<Verdict, Summary, Attribution, DeadCode, Duplication, Complexity> {
    /// Audit output schema version.
    #[cfg_attr(feature = "schema", schemars(with = "AuditSchemaVersion"))]
    pub schema_version: SchemaVersion,
    /// Fallow CLI version that produced this output.
    pub version: ToolVersion,
    /// Command discriminator singleton: always `audit`.
    pub command: AuditCommand,
    /// Gate verdict for the audited change.
    pub verdict: Verdict,
    /// Number of changed files in the audit scope.
    pub changed_files_count: u32,
    /// Git ref the change was diffed against.
    pub base_ref: String,
    /// Human-readable provenance of `base_ref`, e.g. `merge-base with
    /// origin/main`, `local main`, or `FALLOW_AUDIT_BASE=upstream/main`.
    /// Present when the base was auto-detected or set via `FALLOW_AUDIT_BASE`;
    /// absent for an explicit `--base` (the ref the user typed is already
    /// self-describing).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_description: Option<String>,
    /// Commit SHA of the audited head tree, when resolvable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_sha: Option<String>,
    /// Wall-clock analysis duration in milliseconds.
    pub elapsed_ms: ElapsedMs,
    /// True when base-snapshot analysis was skipped, so new-vs-inherited
    /// attribution could not run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_snapshot_skipped: Option<bool>,
    /// Aggregate finding counts for the audited change.
    pub summary: Summary,
    /// New-vs-inherited attribution of findings against the base.
    pub attribution: Attribution,
    /// `_meta` block with metric / rule definitions, when `--explain` was
    /// passed.
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Meta>,
    /// Dead-code findings scoped to the audit changeset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dead_code: Option<DeadCode>,
    /// Duplication findings scoped to the audit changeset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duplication: Option<Duplication>,
    /// Complexity findings scoped to the audit changeset.
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
    /// The only value: `audit`.
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
    /// Combined output schema version.
    #[cfg_attr(feature = "schema", schemars(with = "CombinedSchemaVersion"))]
    pub schema_version: SchemaVersion,
    /// Fallow CLI version that produced this output.
    pub version: ToolVersion,
    /// Wall-clock analysis duration in milliseconds.
    pub elapsed_ms: ElapsedMs,
    /// Per-section `_meta` blocks, when `--explain` was passed.
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<CombinedMeta>,
    /// Dead-code section of the combined run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub check: Option<Check>,
    /// Duplication section of the combined run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dupes: Option<Dupes>,
    /// Health section of the combined run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub health: Option<Health>,
    /// Workspace-discovery, source-discovery, and analysis-stage diagnostics
    /// for the run (issue #2366). See `CheckOutput::workspace_diagnostics` for
    /// the full contract: root-relative paths, omitted when empty. The
    /// combined envelope carries them here rather than inside a section, so a
    /// run that skips a section (`--skip check`, `--only health`,
    /// `--only dupes`) still reports every diagnostic its analyses recorded.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub workspace_diagnostics: Vec<WorkspaceDiagnostic>,
    /// Read-only follow-up commands aggregated across the combined run's
    /// findings. See `CheckOutput::next_steps` for the contract.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub next_steps: Vec<NextStep>,
}

/// Optional `_meta` block for [`CombinedOutput`].
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CombinedMeta {
    /// `_meta` block for the dead-code section.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub check: Option<Meta>,
    /// `_meta` block for the duplication section.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dupes: Option<Meta>,
    /// `_meta` block for the health section.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub health: Option<Meta>,
    /// Telemetry identifiers for the run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub telemetry: Option<TelemetryMeta>,
}

/// Typed root of every fallow JSON envelope shape that serializes as a JSON
/// object and participates in the documented `FallowOutput` contract. The
/// schema derived from this enum drives the document-root `oneOf` in
/// `docs/output-schema.json`.
///
/// The wire shape carries a top-level `kind` discriminator so agents and
/// schema-validating clients can select the variant in O(1) instead of probing
/// for unique field presence.
///
/// One envelope is intentionally NOT in this enum:
/// - `CodeClimateOutput` serializes as a bare JSON array
///   (`#[serde(transparent)]`) per the Code Climate / GitLab Code Quality
///   spec; `#[serde(tag = ...)]` cannot internally tag a non-object
///   variant and wrapping the array would break the spec. The root schema
///   carries it as a sibling `oneOf` branch alongside `FallowOutput`.
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
    FeatureFlags,
    AuditBrief,
    DecisionSurface,
    WalkthroughGuide,
    WalkthroughValidation,
    SuppressionInventory,
    TypeAwareStatus,
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
    /// `fallow flags --format json`.
    #[serde(rename = "feature-flags")]
    FeatureFlags(FeatureFlags),
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
    /// `fallow suppressions --format json`.
    #[serde(rename = "suppression-inventory")]
    SuppressionInventory(SuppressionInventory),
    /// `fallow type-aware status --format json`.
    #[serde(rename = "type-aware-status")]
    TypeAwareStatus(TypeAwareStatus),
}

#[cfg(test)]
mod tests {
    use fallow_types::envelope::{ElapsedMs, SchemaVersion, ToolVersion};
    use serde_json::json;

    use super::*;

    #[test]
    fn apply_root_kind_sets_tagged_mode() {
        let mut value = json!({});

        apply_root_kind(&mut value, "dead_code", RootEnvelopeMode::Tagged);

        assert_eq!(value["kind"], "dead_code");
    }

    #[test]
    fn apply_root_kind_prepends_without_reordering_existing_fields() {
        let mut value = json!({ "schema_version": 1, "summary": { "total": 0 } });

        apply_root_kind(&mut value, "example", RootEnvelopeMode::Tagged);

        assert_eq!(
            serde_json::to_string(&value).expect("root output should serialize"),
            r#"{"kind":"example","schema_version":1,"summary":{"total":0}}"#
        );
    }

    #[test]
    fn apply_root_kind_preserves_existing_value_and_moves_it_first() {
        let mut value = json!({ "before": 1, "kind": "custom", "after": 2 });

        apply_root_kind(&mut value, "replacement", RootEnvelopeMode::Tagged);

        assert_eq!(
            serde_json::to_string(&value).expect("root output should serialize"),
            r#"{"kind":"custom","before":1,"after":2}"#
        );
    }

    #[test]
    fn apply_root_kind_preserves_non_object_roots() {
        let mut value = json!(["not", "an", "object"]);

        apply_root_kind(&mut value, "example", RootEnvelopeMode::Tagged);

        assert_eq!(value, json!(["not", "an", "object"]));
    }

    #[test]
    fn attach_telemetry_meta_sets_analysis_run_id() {
        let mut value = json!({});

        attach_telemetry_meta(&mut value, Some("run-123"));

        assert_eq!(
            value["_meta"]["telemetry"]["analysis_run_id"],
            json!("run-123")
        );
    }

    #[test]
    fn attach_telemetry_meta_preserves_non_object_roots() {
        let mut value = json!(["not", "an", "object"]);

        attach_telemetry_meta(&mut value, Some("run-123"));

        assert_eq!(value, json!(["not", "an", "object"]));
    }

    #[test]
    fn serialize_named_json_output_applies_explicit_kind() {
        let value = serialize_named_json_output(
            json!({
                "schema_version": 1,
                "summary": { "total": 0 }
            }),
            "example",
            RootEnvelopeMode::Tagged,
        )
        .expect("named output should serialize");

        assert_eq!(value["kind"], "example");
        assert_eq!(value["summary"]["total"], 0);
    }

    #[test]
    fn serialize_audit_json_output_applies_audit_kind() {
        let value = serialize_audit_json_output(
            AuditOutput {
                schema_version: SchemaVersion(7),
                version: ToolVersion("1.2.3".to_string()),
                command: AuditCommand::Audit,
                verdict: "pass",
                changed_files_count: 2,
                base_ref: "origin/main".to_string(),
                base_description: Some("merge-base with origin/main".to_string()),
                head_sha: Some("abc123".to_string()),
                elapsed_ms: ElapsedMs(42),
                base_snapshot_skipped: Some(false),
                summary: json!({ "dead_code_issues": 0 }),
                attribution: json!({ "gate": "new_only" }),
                meta: None,
                dead_code: Some(json!({ "summary": { "total_issues": 0 } })),
                duplication: None::<serde_json::Value>,
                complexity: None::<serde_json::Value>,
                next_steps: Vec::new(),
            },
            RootEnvelopeMode::Tagged,
            Some("run-audit"),
        )
        .expect("audit output should serialize");

        assert_eq!(value["kind"], "audit");
        assert_eq!(value["command"], "audit");
        assert_eq!(value["dead_code"]["summary"]["total_issues"], 0);
        assert_eq!(value["_meta"]["telemetry"]["analysis_run_id"], "run-audit");
    }

    #[test]
    fn serialize_combined_json_output_applies_combined_kind() {
        let value = serialize_combined_json_output(
            CombinedOutput {
                schema_version: SchemaVersion(7),
                version: ToolVersion("1.2.3".to_string()),
                elapsed_ms: ElapsedMs(42),
                meta: None,
                check: Some(json!({ "summary": { "total_issues": 0 } })),
                dupes: None::<serde_json::Value>,
                health: None::<serde_json::Value>,
                workspace_diagnostics: Vec::new(),
                next_steps: Vec::new(),
            },
            RootEnvelopeMode::Tagged,
            Some("run-combined"),
        )
        .expect("combined output should serialize");

        assert_eq!(value["kind"], "combined");
        assert_eq!(value["check"]["summary"]["total_issues"], 0);
        assert_eq!(
            value["_meta"]["telemetry"]["analysis_run_id"],
            "run-combined"
        );
    }
}
