//! Typed envelope structs for the JSON output contract.
//!
//! This module is the schema-side source of truth for fallow's top-level JSON
//! envelopes.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

#[allow(
    unused_imports,
    reason = "compatibility re-export while CodeClimate output contracts move to fallow-output"
)]
pub use fallow_output::{
    CheckGroupedEntry, CheckGroupedOutput, CheckOutput, CheckOutputInput, CodeClimateIssue,
    CodeClimateIssueKind, CodeClimateLines, CodeClimateLocation, CodeClimateOutput,
    CodeClimateSeverity, CoverageAnalyzeOutput, CoverageAnalyzeSchemaVersion,
    CoverageSetupFileToEdit, CoverageSetupFramework, CoverageSetupMember, CoverageSetupOutput,
    CoverageSetupPackageManager, CoverageSetupRuntimeTarget, CoverageSetupSchemaVersion,
    CoverageSetupSnippet, DupesOutput, DupesOutputInput, ExplainOutput, GitHubReviewComment,
    GitHubReviewSide, GitLabReviewComment, GitLabReviewPosition, GitLabReviewPositionType,
    GroupByMode, HealthOutput, HealthOutputInput, InspectEvidence, InspectEvidenceScope,
    InspectEvidenceSection, InspectFileIdentity, InspectIdentity, InspectOutput,
    InspectSectionStatus, InspectSymbolIdentity, InspectTargetDescriptor, MARKER_REGEX_FLAGS_V2,
    MARKER_REGEX_V2, ReviewCheckConclusion, ReviewComment, ReviewEnvelopeEvent, ReviewEnvelopeMeta,
    ReviewEnvelopeOutput, ReviewEnvelopeSchema, ReviewEnvelopeSummary, ReviewProvider,
    ReviewReconcileOutput, ReviewReconcileSchema, WorkspaceDiagnosticOutput,
    apply_config_fixable_to_duplicate_exports, build_check_output, build_check_summary,
    build_dupes_output, build_health_output, default_marker_regex, default_marker_regex_flags,
    is_false,
};
use fallow_types::envelope::{ElapsedMs, Meta, SchemaVersion, TelemetryMeta, ToolVersion};
use fallow_types::output::NextStep;
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
/// `fallow audit --format json` envelope.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "schema", schemars(title = "fallow audit --format json"))]
#[allow(
    dead_code,
    reason = "schema-source-of-truth: audit.rs still builds the wire via serde_json::json!; this struct locks the schema shape via the drift gate. Migration is a follow-up to issue #384 items 3a/3b/3c."
)]
pub struct AuditOutput {
    pub schema_version: SchemaVersion,
    pub version: ToolVersion,
    pub command: AuditCommand,
    pub verdict: AuditVerdict,
    pub changed_files_count: u32,
    pub base_ref: String,
    /// Human-readable provenance of `base_ref`, e.g. `merge-base with
    /// origin/main`, `local main`, or `FALLOW_AUDIT_BASE=upstream/main`.
    /// Present when the base was auto-detected or set via `FALLOW_AUDIT_BASE`;
    /// absent for an explicit `--base` (the ref the user typed is already
    /// self-describing).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_sha: Option<String>,
    pub elapsed_ms: ElapsedMs,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_snapshot_skipped: Option<bool>,
    pub summary: AuditSummary,
    pub attribution: AuditAttribution,
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Meta>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dead_code: Option<CheckOutput>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duplication: Option<DupesReportPayload>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub complexity: Option<HealthReport>,
    /// Read-only follow-up commands computed from this run's findings. See
    /// [`CheckOutput::next_steps`] for the contract.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub next_steps: Vec<NextStep>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
#[allow(dead_code, reason = "schema-source-of-truth: see `AuditOutput`.")]
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
pub struct CombinedOutput {
    pub schema_version: SchemaVersion,
    pub version: ToolVersion,
    pub elapsed_ms: ElapsedMs,
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<CombinedMeta>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub check: Option<CheckOutput>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dupes: Option<DupesReportPayload>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub health: Option<HealthReport>,
    /// Read-only follow-up commands aggregated across the combined run's
    /// findings. See [`CheckOutput::next_steps`] for the contract.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub next_steps: Vec<NextStep>,
}

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

/// Envelope emitted by `fallow list --boundaries --format json`. Surfaces
/// the architecture boundary zones, rules, and (issue #373) the user's
/// pre-expansion `autoDiscover` logical groups so consumers can render
/// grouping intent that `expand_auto_discover` would otherwise flatten out
/// of `zones[]`.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(
    feature = "schema",
    schemars(title = "fallow list --boundaries --format json")
)]
#[allow(
    dead_code,
    reason = "schema-source-of-truth: list.rs still builds the wire via serde_json::json!; this struct and its sub-types lock the schema shape via the drift gate. Migration is a follow-up to issue #384 items 3a/3b/3c."
)]
pub struct ListBoundariesOutput {
    pub boundaries: BoundariesListing,
}

/// `fallow workspaces --format json` envelope.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(
    feature = "schema",
    schemars(title = "fallow workspaces --format json")
)]
pub struct WorkspacesOutput {
    /// Number of workspace package entries in `workspaces`.
    pub workspace_count: usize,
    /// Workspace packages discovered from package manager and tsconfig workspace
    /// declarations. Paths are project-root-relative and use forward slashes.
    pub workspaces: Vec<WorkspaceInfo>,
    /// Workspace discovery diagnostics produced while reading workspace
    /// declarations. Present for compatibility with the current wire contract,
    /// even when empty.
    pub workspace_diagnostics: Vec<fallow_config::WorkspaceDiagnostic>,
}

/// One workspace package emitted by `fallow workspaces --format json`.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct WorkspaceInfo {
    /// Package name from the workspace package.json. This is the value accepted
    /// by `--workspace <name>`.
    pub name: String,
    /// Project-root-relative path to the workspace directory, normalized to
    /// forward slashes for cross-platform JSON consumers.
    pub path: String,
    /// Whether the package is a generated or platform-specific dependency
    /// package rather than a hand-authored workspace.
    pub is_internal_dependency: bool,
}

/// `boundaries` block carried by [`ListBoundariesOutput`].
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[allow(
    dead_code,
    reason = "schema-source-of-truth: see `ListBoundariesOutput`."
)]
pub struct BoundariesListing {
    pub configured: bool,
    pub zone_count: usize,
    pub zones: Vec<BoundariesListZone>,
    pub rule_count: usize,
    pub rules: Vec<BoundariesListRule>,
    pub logical_group_count: usize,
    pub logical_groups: Vec<BoundariesListLogicalGroup>,
}

/// A boundary zone after preset and `autoDiscover` expansion. Each entry
/// classifies files into a single zone via glob patterns.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[allow(
    dead_code,
    reason = "schema-source-of-truth: see `ListBoundariesOutput`."
)]
pub struct BoundariesListZone {
    pub name: String,
    pub patterns: Vec<String>,
    pub file_count: usize,
}

/// A boundary import rule, expanded to operate on concrete child zone
/// names after `autoDiscover` flattening. The user's pre-expansion rule
/// (keyed on the logical parent name, if any) is preserved on the
/// corresponding [`BoundariesListLogicalGroup::authored_rule`].
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[allow(
    dead_code,
    reason = "schema-source-of-truth: see `ListBoundariesOutput`."
)]
pub struct BoundariesListRule {
    pub from: String,
    pub allow: Vec<String>,
}

/// A pre-expansion `autoDiscover` logical group surfaced for observability
/// (issue #373). Captured during `expand_auto_discover` so consumers can
/// see the user-authored parent name and grouping intent after expansion
/// would otherwise flatten it out of [`BoundariesListing::zones`].
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[allow(
    dead_code,
    reason = "schema-source-of-truth: see `ListBoundariesOutput`."
)]
pub struct BoundariesListLogicalGroup {
    pub name: String,
    pub children: Vec<String>,
    pub auto_discover: Vec<String>,
    pub status: fallow_config::LogicalGroupStatus,
    pub source_zone_index: usize,
    pub file_count: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authored_rule: Option<fallow_config::AuthoredRule>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_zone: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merged_from: Option<Vec<usize>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_zone_root: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub child_source_indices: Vec<usize>,
}

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
    use fallow_types::envelope::{ElapsedMs, SchemaVersion, ToolVersion};

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
