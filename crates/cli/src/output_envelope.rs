//! Typed envelope structs for the JSON output contract.
//!
//! This module is the schema-side source of truth for fallow's top-level JSON
//! envelopes.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

#[allow(
    unused_imports,
    reason = "compatibility re-export while CLI output contracts move to fallow-output"
)]
pub use fallow_output::{
    AuditCommand, BoundariesListRule, BoundariesListZone, CheckGroupedEntry, CheckGroupedOutput,
    CheckOutput, CheckOutputInput, CodeClimateIssue, CodeClimateIssueKind, CodeClimateLines,
    CodeClimateLocation, CodeClimateOutput, CodeClimateSeverity, CombinedMeta,
    CoverageAnalyzeOutput, CoverageAnalyzeSchemaVersion, CoverageSetupFileToEdit,
    CoverageSetupFramework, CoverageSetupMember, CoverageSetupOutput, CoverageSetupPackageManager,
    CoverageSetupRuntimeTarget, CoverageSetupSchemaVersion, CoverageSetupSnippet, DupesOutput,
    DupesOutputInput, ExplainOutput, GitHubReviewComment, GitHubReviewSide, GitLabReviewComment,
    GitLabReviewPosition, GitLabReviewPositionType, GroupByMode, HealthOutput, HealthOutputInput,
    InspectEvidence, InspectEvidenceScope, InspectEvidenceSection, InspectFileIdentity,
    InspectIdentity, InspectOutput, InspectSectionStatus, InspectSymbolIdentity,
    InspectTargetDescriptor, MARKER_REGEX_FLAGS_V2, MARKER_REGEX_V2, ReviewCheckConclusion,
    ReviewComment, ReviewEnvelopeEvent, ReviewEnvelopeMeta, ReviewEnvelopeOutput,
    ReviewEnvelopeSchema, ReviewEnvelopeSummary, ReviewProvider, ReviewReconcileOutput,
    ReviewReconcileSchema, WorkspaceDiagnosticOutput, WorkspaceInfo,
    apply_config_fixable_to_duplicate_exports, build_check_output, build_check_summary,
    build_dupes_output, build_health_output, default_marker_regex, default_marker_regex_flags,
    is_false,
};
use serde::Serialize;

use crate::audit::{AuditAttribution, AuditSummary, AuditVerdict};
use crate::health_types::{HealthGroup, HealthReport};
use crate::output_dupes::DupesReportPayload;
use crate::report::dupes_grouping::DuplicationGroup;

static LEGACY_ENVELOPE: AtomicBool = AtomicBool::new(false);
static TELEMETRY_ANALYSIS_RUN_ID: Mutex<Option<String>> = Mutex::new(None);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvelopeMode {
    Tagged,
    Legacy,
}

impl EnvelopeMode {
    #[must_use]
    pub fn current() -> Self {
        if LEGACY_ENVELOPE.load(Ordering::Relaxed) {
            Self::Legacy
        } else {
            Self::Tagged
        }
    }
}

pub fn set_legacy_envelope(enabled: bool) {
    LEGACY_ENVELOPE.store(enabled, Ordering::Relaxed);
}

pub fn set_telemetry_analysis_run_id(run_id: Option<String>) {
    if let Ok(mut current) = TELEMETRY_ANALYSIS_RUN_ID.lock() {
        *current = run_id;
    }
}

fn telemetry_analysis_run_id() -> Option<String> {
    TELEMETRY_ANALYSIS_RUN_ID
        .lock()
        .ok()
        .and_then(|id| id.clone())
}

pub fn serialize_root_output(output: FallowOutput) -> Result<serde_json::Value, serde_json::Error> {
    serialize_root_output_with_mode(output, EnvelopeMode::current())
}

pub fn serialize_root_output_without_telemetry(
    output: FallowOutput,
) -> Result<serde_json::Value, serde_json::Error> {
    let mut value = serde_json::to_value(output)?;
    if EnvelopeMode::current() == EnvelopeMode::Legacy {
        remove_root_kind(&mut value);
    }
    Ok(value)
}

pub fn serialize_root_output_with_mode(
    output: FallowOutput,
    mode: EnvelopeMode,
) -> Result<serde_json::Value, serde_json::Error> {
    let mut value = serde_json::to_value(output)?;
    if mode == EnvelopeMode::Legacy {
        remove_root_kind(&mut value);
    }
    attach_telemetry_meta(&mut value);
    Ok(value)
}

pub fn attach_telemetry_meta(value: &mut serde_json::Value) {
    let Some(run_id) = telemetry_analysis_run_id() else {
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
            serde_json::json!({ "analysis_run_id": run_id }),
        );
    }
}

/// Remove only the document-root discriminator for the one-cycle
/// compatibility mode. Nested objects may carry their own meaningful `kind`
/// fields, so this intentionally does not recurse.
pub fn remove_root_kind(value: &mut serde_json::Value) {
    if let serde_json::Value::Object(map) = value {
        map.remove("kind");
    }
}

pub fn apply_root_kind(value: &mut serde_json::Value, kind: &'static str) {
    if EnvelopeMode::current() == EnvelopeMode::Tagged
        && let serde_json::Value::Object(map) = value
    {
        map.insert(
            "kind".to_string(),
            serde_json::Value::String(kind.to_string()),
        );
    }
}
pub type AuditOutput = fallow_output::AuditOutput<
    AuditVerdict,
    AuditSummary,
    AuditAttribution,
    CheckOutput,
    DupesReportPayload,
    HealthReport,
>;

pub type CombinedOutput =
    fallow_output::CombinedOutput<CheckOutput, DupesReportPayload, HealthReport>;

pub type ListBoundariesOutput = fallow_output::ListBoundariesOutput<
    fallow_config::LogicalGroupStatus,
    fallow_config::AuthoredRule,
>;

pub type WorkspacesOutput = fallow_output::WorkspacesOutput<fallow_config::WorkspaceDiagnostic>;

#[allow(
    dead_code,
    reason = "schema compatibility alias for the concrete fallow-config boundary contract"
)]
pub type BoundariesListing = fallow_output::BoundariesListing<
    fallow_config::LogicalGroupStatus,
    fallow_config::AuthoredRule,
>;

#[allow(
    dead_code,
    reason = "schema compatibility alias for the concrete fallow-config boundary contract"
)]
pub type BoundariesListLogicalGroup = fallow_output::BoundariesListLogicalGroup<
    fallow_config::LogicalGroupStatus,
    fallow_config::AuthoredRule,
>;

/// Typed root of every fallow JSON envelope shape that serializes as a JSON
/// object and participates in the documented `FallowOutput` contract. The
/// schema derived from this enum drives the document-root `oneOf` in
/// `docs/output-schema.json`.
///
/// The default wire shape now carries a top-level `kind` discriminator so
/// agents and schema-validating clients can select the variant in O(1) instead
/// of probing for unique field presence. `--legacy-envelope` is a one-cycle
/// compatibility flag that removes only this document-root `kind` field from
/// CLI JSON output; nested report objects are not rewritten.
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
pub enum FallowOutput {
    /// `fallow audit --format json`. Required `command: "audit"` singleton
    /// plus `verdict` and `summary`.
    #[serde(rename = "audit")]
    Audit(AuditOutput),
    /// `fallow explain <issue-type> --format json`. Required `id`, `name`,
    /// `rationale`, `example`, `how_to_fix`, `docs`; no `schema_version`.
    #[serde(rename = "explain")]
    Explain(ExplainOutput),
    /// `fallow inspect --format json`. Required `target`, `identity`,
    /// `evidence`, and `warnings`; no `schema_version`.
    #[serde(rename = "inspect_target")]
    Inspect(InspectOutput),
    /// `fallow trace <symbol> --format json` (symbol-level call chains).
    /// Required `file`, `symbol`, `symbol_found`, `depth`, `best_effort`,
    /// `reason`; optional `callers`, `callees`, `unresolved_callees`. Its OWN
    /// surface, best-effort and EXPLICITLY OFF the ranked path: NEVER folded
    /// into the ranked brief and NEVER a focus-map input.
    #[serde(rename = "trace")]
    Trace(fallow_core::trace_chain::SymbolChainTrace),
    /// `fallow --format review-github` / `--format review-gitlab`. Required
    /// `body`, `comments`, `meta`; no `schema_version`.
    #[serde(rename = "review-envelope")]
    ReviewEnvelope(ReviewEnvelopeOutput),
    /// `fallow ci reconcile-review --format json`. Required `schema`
    /// singleton plus `provider`, `comments`, and the various
    /// `*_fingerprints` arrays.
    #[serde(rename = "review-reconcile")]
    ReviewReconcile(ReviewReconcileOutput),
    /// `fallow coverage setup --json`. Required `schema_version` singleton
    /// plus `framework_detected`, `members`, `commands`, `snippets`.
    #[serde(rename = "coverage-setup")]
    CoverageSetup(CoverageSetupOutput),
    /// `fallow coverage analyze --format json`. Required
    /// `schema_version: "1"` singleton plus `version`, `elapsed_ms`,
    /// `runtime_coverage`.
    #[serde(rename = "coverage-analyze")]
    CoverageAnalyze(CoverageAnalyzeOutput),
    /// `fallow list --boundaries --format json`. Required `boundaries`
    /// sub-object; no `schema_version`.
    #[serde(rename = "list-boundaries")]
    ListBoundaries(ListBoundariesOutput),
    /// `fallow workspaces --format json`. Required `workspace_count`,
    /// `workspaces`, and `workspace_diagnostics`.
    #[serde(rename = "list-workspaces")]
    Workspaces(WorkspacesOutput),
    /// `fallow health --format json`. Required `report: HealthReport`.
    #[serde(rename = "health")]
    Health(HealthOutput<HealthReport, HealthGroup>),
    /// `fallow dupes --format json`. Required `report: DupesReportPayload`
    /// (typed wrapper payload carrying `clone_groups[]: CloneGroupFinding`
    /// and `clone_families[]: CloneFamilyFinding`).
    #[serde(rename = "dupes")]
    Dupes(DupesOutput<DupesReportPayload, DuplicationGroup>),
    /// `fallow dead-code --format json --group-by <mode>`. Required `grouped_by`
    /// plus a `groups` array.
    #[serde(rename = "dead-code-grouped")]
    CheckGrouped(CheckGroupedOutput),
    /// `fallow impact --format json`. Required `enabled`, `record_count`,
    /// `containment_count`, `recent_containment`; no global `schema_version`,
    /// `command`, `total_issues`, or `report`.
    #[serde(rename = "impact")]
    Impact(crate::impact::ImpactReport),
    /// `fallow impact --all --format json`. Required `project_count`,
    /// `tracked_count`, `totals`, `projects`; the cross-repo roll-up. Each
    /// `projects[]` entry embeds a per-project `report` (the same shape as the
    /// `impact` variant). Independently versioned via `CrossRepoImpactSchemaVersion`.
    #[serde(rename = "impact-cross-repo")]
    ImpactCrossRepo(crate::impact::CrossRepoImpactReport),
    /// `fallow security --summary --format json`. Required `summary`; no
    /// per-finding arrays.
    #[serde(rename = "security")]
    SecuritySummary(crate::security::SecuritySummaryOutput),
    /// `fallow security --format json`. Required `security_findings`,
    /// `unresolved_edge_files`, and `unresolved_callee_sites`; ordered before the
    /// broader variants because the `security_findings` discriminator is uniquely
    /// present here.
    #[serde(rename = "security")]
    Security(crate::security::SecurityOutput),
    /// `fallow security survivors --format json`. Required `survivors` and
    /// `needs_human_review`, both keyed by `finding_id`.
    #[serde(rename = "security-survivors")]
    SecuritySurvivors(crate::security::SecuritySurvivorsOutput),
    /// `fallow security blind-spots --format json`. Required `summary` and
    /// grouped unresolved-callee diagnostics.
    #[serde(rename = "security-blind-spots")]
    SecurityBlindSpots(crate::security::SecurityBlindSpotsOutput),
    /// `fallow dead-code --format json`.
    /// Required `total_issues` plus `summary: CheckSummary`.
    #[serde(rename = "dead-code")]
    Check(CheckOutput),
    /// Bare `fallow --format json` (combined dead-code + dupes + health).
    /// Required `schema_version`, `version`, and `elapsed_ms`, with optional
    /// `check`, `dupes`, and `health` subreports.
    #[serde(rename = "combined")]
    Combined(CombinedOutput),
    /// `fallow audit --brief --format json` (alias `fallow review`). Required
    /// `schema_version`, `version`, `command: "audit-brief"`, `triage`, and
    /// `graph_facts`. Independently versioned via `ReviewBriefSchemaVersion`;
    /// always emitted with exit 0.
    #[serde(rename = "audit-brief")]
    AuditBrief(crate::audit_brief::ReviewBriefOutput),
    /// `fallow decision-surface --format json` (the `decision_surface` MCP tool's
    /// output). The separable, cheap apex: a ranked, capped, signal_id-anchored
    /// set of consequential structural decisions framed as judgment questions,
    /// each with structured `actions[]`. Independently versioned; always exit 0.
    #[serde(rename = "decision-surface")]
    DecisionSurface(crate::audit_decision_surface::DecisionSurfaceOutput),
    /// `fallow review --walkthrough-guide --format json`. The digest +
    /// schema the agent fetches: the brief + decision surface, the review
    /// direction, the graph-snapshot pin, and the embedded agent schema. The
    /// digest is graph-derived only (injection-resistant). Always exit 0.
    #[serde(rename = "review-walkthrough-guide")]
    WalkthroughGuide(crate::audit_walkthrough::WalkthroughGuide),
    /// `fallow review --walkthrough-file --format json`. The post-validation
    /// of an agent's judgment JSON against the live graph: accepted (anchored,
    /// framing fenced), rejected (unanchored), and a stale flag (the tree moved).
    /// The verifier is the graph, never a second model. Always exit 0.
    #[serde(rename = "review-walkthrough-validation")]
    WalkthroughValidation(crate::audit_walkthrough::WalkthroughValidation),
}

#[cfg(test)]
mod tests {
    use fallow_types::envelope::{ElapsedMs, Meta, SchemaVersion, ToolVersion};

    use super::*;

    static TEST_TELEMETRY_RUN_ID_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    struct TelemetryRunIdGuard {
        _lock: std::sync::MutexGuard<'static, ()>,
    }

    impl TelemetryRunIdGuard {
        fn set(run_id: Option<&str>) -> Self {
            let lock = TEST_TELEMETRY_RUN_ID_LOCK
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            set_telemetry_analysis_run_id(run_id.map(str::to_owned));
            Self { _lock: lock }
        }
    }

    impl Drop for TelemetryRunIdGuard {
        fn drop(&mut self) {
            set_telemetry_analysis_run_id(None);
        }
    }

    fn combined_output() -> CombinedOutput {
        CombinedOutput {
            schema_version: SchemaVersion(crate::report::SCHEMA_VERSION),
            version: ToolVersion("test".to_string()),
            elapsed_ms: ElapsedMs(0),
            meta: None,
            check: None,
            dupes: None,
            health: None,
            next_steps: Vec::new(),
        }
    }

    #[test]
    fn root_output_serializes_kind_by_default() {
        let _guard = TelemetryRunIdGuard::set(None);
        let value = serialize_root_output_with_mode(
            FallowOutput::Combined(combined_output()),
            EnvelopeMode::Tagged,
        )
        .expect("combined root should serialize");

        assert_eq!(value["kind"], serde_json::Value::String("combined".into()));
        assert_eq!(value["schema_version"], crate::report::SCHEMA_VERSION);
    }

    #[test]
    fn legacy_mode_removes_only_root_kind() {
        let _guard = TelemetryRunIdGuard::set(None);
        let value = serialize_root_output_with_mode(
            FallowOutput::Combined(combined_output()),
            EnvelopeMode::Legacy,
        )
        .expect("combined root should serialize");

        assert!(value.get("kind").is_none());

        let mut nested = serde_json::json!({
            "kind": "root",
            "action": {
                "kind": "suppress"
            }
        });
        remove_root_kind(&mut nested);
        assert!(nested.get("kind").is_none());
        assert_eq!(nested["action"]["kind"], "suppress");
    }

    #[test]
    fn root_output_attaches_telemetry_meta() {
        let _guard = TelemetryRunIdGuard::set(Some("run_test123"));
        let value = serialize_root_output_with_mode(
            FallowOutput::Combined(combined_output()),
            EnvelopeMode::Tagged,
        )
        .expect("combined root should serialize");

        assert_eq!(
            value["_meta"]["telemetry"]["analysis_run_id"].as_str(),
            Some("run_test123")
        );
    }

    #[test]
    fn telemetry_meta_preserves_existing_meta_sections() {
        let mut output = combined_output();
        output.meta = Some(CombinedMeta {
            check: Some(Meta {
                docs: Some("https://example.com/check".to_string()),
                ..Meta::default()
            }),
            dupes: None,
            health: None,
            telemetry: None,
        });

        let _guard = TelemetryRunIdGuard::set(Some("run_test123"));
        let value =
            serialize_root_output_with_mode(FallowOutput::Combined(output), EnvelopeMode::Tagged)
                .expect("combined root should serialize");

        assert_eq!(
            value["_meta"]["check"]["docs"].as_str(),
            Some("https://example.com/check")
        );
        assert_eq!(
            value["_meta"]["telemetry"]["analysis_run_id"].as_str(),
            Some("run_test123")
        );
    }
}
