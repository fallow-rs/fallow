//! Typed envelope structs for the JSON output contract.
//!
//! This module is the schema-side source of truth for fallow's top-level JSON
//! envelopes.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use fallow_core::results::AnalysisResults;
use fallow_types::envelope::{
    BaselineDeltas, BaselineMatch, CheckSummary, ElapsedMs, EntryPoints, Meta, RegressionResult,
    SchemaVersion, TelemetryMeta, ToolVersion,
};
use fallow_types::output::NextStep;
use serde::Serialize;

use crate::audit::{AuditAttribution, AuditSummary, AuditVerdict};
use crate::health_types::{HealthGroup, HealthReport, RuntimeCoverageReport};
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
/// `fallow coverage setup --json` envelope.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "schema", schemars(title = "fallow coverage setup --json"))]
pub struct CoverageSetupOutput {
    pub schema_version: CoverageSetupSchemaVersion,
    pub framework_detected: CoverageSetupFramework,
    pub package_manager: Option<CoverageSetupPackageManager>,
    pub runtime_targets: Vec<CoverageSetupRuntimeTarget>,
    pub members: Vec<CoverageSetupMember>,
    pub config_written: Option<serde_json::Value>,
    pub commands: Vec<String>,
    pub files_to_edit: Vec<CoverageSetupFileToEdit>,
    pub snippets: Vec<CoverageSetupSnippet>,
    pub dockerfile_snippet: Option<String>,
    pub next_steps: Vec<String>,
    pub warnings: Vec<String>,
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum CoverageSetupSchemaVersion {
    #[serde(rename = "1")]
    V1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum CoverageSetupFramework {
    #[serde(rename = "nextjs")]
    NextJs,
    #[serde(rename = "nestjs")]
    NestJs,
    Nuxt,
    #[serde(rename = "sveltekit")]
    SvelteKit,
    Astro,
    Remix,
    Vite,
    PlainNode,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum CoverageSetupPackageManager {
    Npm,
    Pnpm,
    Yarn,
    Bun,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum CoverageSetupRuntimeTarget {
    Node,
    Browser,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CoverageSetupMember {
    pub name: String,
    pub path: String,
    pub framework_detected: CoverageSetupFramework,
    pub package_manager: Option<CoverageSetupPackageManager>,
    pub runtime_targets: Vec<CoverageSetupRuntimeTarget>,
    pub files_to_edit: Vec<CoverageSetupFileToEdit>,
    pub snippets: Vec<CoverageSetupSnippet>,
    pub dockerfile_snippet: Option<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CoverageSetupFileToEdit {
    pub path: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CoverageSetupSnippet {
    pub label: String,
    pub path: String,
    pub content: String,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum CoverageAnalyzeSchemaVersion {
    #[serde(rename = "1")]
    V1,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(
    feature = "schema",
    schemars(title = "fallow coverage analyze --format json")
)]
pub struct CoverageAnalyzeOutput {
    pub schema_version: CoverageAnalyzeSchemaVersion,
    pub version: ToolVersion,
    pub elapsed_ms: ElapsedMs,
    pub runtime_coverage: RuntimeCoverageReport,
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Meta>,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "schema", schemars(title = "fallow dupes --format json"))]
pub struct DupesOutput {
    pub schema_version: SchemaVersion,
    pub version: ToolVersion,
    pub elapsed_ms: ElapsedMs,
    #[serde(flatten)]
    pub report: DupesReportPayload,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grouped_by: Option<GroupByMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_issues: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub groups: Option<Vec<DuplicationGroup>>,
    /// `_meta` block with metric / rule definitions, emitted when `--explain`
    /// is passed (always present in MCP responses).
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Meta>,
    /// Workspace-discovery diagnostics surfaced during config load
    /// (issue #473). See [`CheckOutput::workspace_diagnostics`] for the full
    /// contract; the same list is repeated on each top-level command's
    /// envelope so single-command consumers see it without having to look at
    /// a separate top-level field.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub workspace_diagnostics: Vec<fallow_config::WorkspaceDiagnostic>,
    /// Read-only follow-up commands computed from this run's findings. See
    /// [`CheckOutput::next_steps`] for the contract.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub next_steps: Vec<NextStep>,
}

/// Envelope emitted by `fallow dead-code --format json` (plus the `check`
/// block inside the combined and audit envelopes).
///
/// The body is the full `AnalysisResults` flattened into the envelope so
/// every issue array (`unused_files`, `unused_exports`, ...) lives at the
/// top level, matching the existing wire shape. `entry_points` lifts the
/// otherwise `#[serde(skip)]`'d `AnalysisResults::entry_point_summary` back
/// into the JSON output. `summary` carries the per-category counts the
/// JSON layer always emits.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "schema", schemars(title = "fallow dead-code --format json"))]
pub struct CheckOutput {
    pub schema_version: SchemaVersion,
    pub version: ToolVersion,
    pub elapsed_ms: ElapsedMs,
    pub total_issues: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry_points: Option<EntryPoints>,
    pub summary: CheckSummary,
    #[serde(flatten)]
    pub results: AnalysisResults,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_deltas: Option<BaselineDeltas>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline: Option<BaselineMatch>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub regression: Option<RegressionResult>,
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Meta>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub workspace_diagnostics: Vec<fallow_config::WorkspaceDiagnostic>,
    /// Read-only follow-up commands computed from this run's findings, emitted
    /// at the JSON root so an agent acting on the output is pointed at fallow's
    /// adjacent verification capabilities (trace, complexity breakdown, audit,
    /// workspace scoping). Each command is runnable as-is and never mutating;
    /// see [`NextStep`] for both contracts. Omitted when empty or when
    /// `FALLOW_SUGGESTIONS=off`; does NOT contribute to `total_issues`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub next_steps: Vec<NextStep>,
}

/// Envelope emitted by `fallow dead-code --group-by ... --format json`.
///
/// Issues are partitioned into resolver buckets (CODEOWNERS team, directory
/// prefix, workspace package, or GitLab CODEOWNERS section) instead of flat
/// arrays. Each bucket carries the same issue-array shape as the ungrouped
/// `CheckOutput` body, plus per-group `key` / `owners` / `total_issues`.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(
    feature = "schema",
    schemars(
        title = "fallow dead-code --group-by <owner|directory|package|section> --format json"
    )
)]
pub struct CheckGroupedOutput {
    pub schema_version: SchemaVersion,
    pub version: ToolVersion,
    pub elapsed_ms: ElapsedMs,
    pub grouped_by: GroupByMode,
    pub total_issues: usize,
    pub groups: Vec<CheckGroupedEntry>,
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Meta>,
    /// Read-only follow-up commands computed from the full (ungrouped) findings.
    /// See [`CheckOutput::next_steps`] for the contract.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub next_steps: Vec<NextStep>,
}

/// Single resolver bucket inside `CheckGroupedOutput`. Carries the group's
/// identifier, optional section owners, and a per-group flattened
/// `AnalysisResults`.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CheckGroupedEntry {
    pub key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owners: Option<Vec<String>>,
    pub total_issues: usize,
    #[serde(flatten)]
    pub results: AnalysisResults,
}

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
pub struct HealthOutput {
    pub schema_version: SchemaVersion,
    pub version: ToolVersion,
    pub elapsed_ms: ElapsedMs,
    #[serde(flatten)]
    pub report: HealthReport,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grouped_by: Option<GroupByMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub groups: Option<Vec<HealthGroup>>,
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Meta>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub workspace_diagnostics: Vec<fallow_config::WorkspaceDiagnostic>,
    /// Read-only follow-up commands computed from this run's findings. See
    /// [`CheckOutput::next_steps`] for the contract.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub next_steps: Vec<NextStep>,
}

/// Envelope emitted by `fallow explain <issue-type> --format json`.
///
/// Standalone rule explanation. This command does not run project analysis
/// and intentionally returns a compact object without `schema_version` /
/// `version` metadata; consumers that need those should call any other
/// fallow JSON-producing command.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(
    feature = "schema",
    schemars(title = "fallow explain <issue-type> --format json")
)]
pub struct ExplainOutput {
    pub id: String,
    pub name: String,
    pub summary: String,
    pub rationale: String,
    pub example: String,
    pub how_to_fix: String,
    pub docs: String,
}

/// Envelope emitted by `fallow --format codeclimate` and
/// `fallow --format gitlab-codequality`. GitLab Code Quality consumes the
/// same shape. The wire form is a bare JSON array, not an object.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(
    feature = "schema",
    schemars(title = "fallow --format codeclimate / gitlab-codequality")
)]
#[serde(transparent)]
#[allow(
    dead_code,
    reason = "schema-source-of-truth wrapper: runtime emits a `Vec<CodeClimateIssue>` directly via `codeclimate::issues_to_value`; this newtype exists so `schemars` can title and document the bare-array shape for the drift gate."
)]
pub struct CodeClimateOutput(pub Vec<CodeClimateIssue>);

/// Single CodeClimate-compatible issue inside [`CodeClimateOutput`].
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CodeClimateIssue {
    #[serde(rename = "type")]
    pub kind: CodeClimateIssueKind,
    pub check_name: String,
    pub description: String,
    pub categories: Vec<String>,
    pub severity: CodeClimateSeverity,
    pub fingerprint: String,
    pub location: CodeClimateLocation,
}

/// Discriminator value for [`CodeClimateIssue::kind`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum CodeClimateIssueKind {
    /// The only valid CodeClimate type today.
    Issue,
}

/// CodeClimate severity scale.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum CodeClimateSeverity {
    /// Informational. Reserved for future severity mappings; not produced
    /// by the current runtime path (which only emits Minor / Major /
    /// Critical via `severity_to_codeclimate` and the health / runtime-
    /// coverage match arms).
    #[allow(
        dead_code,
        reason = "schema-source-of-truth: documents the full CodeClimate severity spec; runtime never produces this variant today, but the schema needs it so consumers can validate against either fallow output or a third-party CodeClimate emitter without spec divergence."
    )]
    Info,
    /// Minor finding.
    Minor,
    /// Major finding.
    Major,
    /// Critical finding.
    Critical,
    /// Blocker (highest severity). Reserved for future severity
    /// mappings; not produced by the current runtime path.
    #[allow(
        dead_code,
        reason = "schema-source-of-truth: documents the full CodeClimate severity spec; runtime never produces this variant today, but the schema needs it so consumers can validate against either fallow output or a third-party CodeClimate emitter without spec divergence."
    )]
    Blocker,
}

/// Location block inside [`CodeClimateIssue::location`].
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CodeClimateLocation {
    /// File path relative to the analysed root.
    pub path: String,
    /// Wrapper carrying the begin line so the schema lines up with
    /// CodeClimate's spec.
    pub lines: CodeClimateLines,
}

/// `lines.begin` for [`CodeClimateLocation`].
#[derive(Debug, Clone, Copy, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CodeClimateLines {
    /// 1-based start line.
    pub begin: u32,
}

/// Envelope emitted by `fallow --format review-github` / `review-gitlab`.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(
    feature = "schema",
    schemars(title = "fallow --format review-github / review-gitlab")
)]
pub struct ReviewEnvelopeOutput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event: Option<ReviewEnvelopeEvent>,
    pub body: String,
    #[serde(default = "ReviewEnvelopeSummary::empty_default")]
    pub summary: ReviewEnvelopeSummary,
    pub comments: Vec<ReviewComment>,
    #[serde(default = "default_marker_regex")]
    pub marker_regex: String,
    #[serde(default = "default_marker_regex_flags")]
    pub marker_regex_flags: String,
    pub meta: ReviewEnvelopeMeta,
}

/// Default for [`ReviewEnvelopeOutput::marker_regex`].
#[must_use]
pub fn default_marker_regex() -> String {
    MARKER_REGEX_V2.to_owned()
}

/// Default for [`ReviewEnvelopeOutput::marker_regex_flags`].
#[must_use]
pub fn default_marker_regex_flags() -> String {
    MARKER_REGEX_FLAGS_V2.to_owned()
}

/// Canonical v2 marker-regex literal.
pub const MARKER_REGEX_V2: &str =
    r"^<!-- fallow-fingerprint:v2: ((?:[a-z]+:)?[0-9a-f]{16}) -->\s*$";

/// Canonical v2 marker-regex flags.
pub const MARKER_REGEX_FLAGS_V2: &str = "m";

/// Summary block on [`ReviewEnvelopeOutput`].
#[derive(Debug, Clone, Serialize, Default)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ReviewEnvelopeSummary {
    pub body: String,
    pub fingerprint: String,
}

impl ReviewEnvelopeSummary {
    /// Empty-default factory for [`ReviewEnvelopeOutput::summary`].
    #[must_use]
    #[allow(
        dead_code,
        reason = "referenced via serde default = \"...\" attr; no direct callsite until Deserialize is derived"
    )]
    pub fn empty_default() -> Self {
        Self::default()
    }
}

/// Singleton GitHub review-event marker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum ReviewEnvelopeEvent {
    #[serde(rename = "COMMENT")]
    Comment,
}

/// Per-line review comment. Schema is an `anyOf` between GitHub and GitLab
/// shapes; at runtime every entry in a single envelope comes from the same
/// provider because the envelope is built from one provider's branch in
/// `crates/cli/src/report/ci/review.rs::render_review_envelope`.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(untagged)]
pub enum ReviewComment {
    GitHub(GitHubReviewComment),
    GitLab(GitLabReviewComment),
}

/// GitHub pull-request review comment.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct GitHubReviewComment {
    pub path: String,
    pub line: u32,
    pub side: GitHubReviewSide,
    pub body: String,
    pub fingerprint: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub truncated: bool,
}

/// Singleton side discriminator for [`GitHubReviewComment::side`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum GitHubReviewSide {
    #[serde(rename = "RIGHT")]
    Right,
}

/// GitLab merge-request discussion comment.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct GitLabReviewComment {
    pub body: String,
    pub position: GitLabReviewPosition,
    pub fingerprint: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub truncated: bool,
}

/// Helper for `skip_serializing_if = "is_false"` on `truncated` fields above.
/// Serde calls `skip_serializing_if` with `&T`, so the reference signature
/// is dictated by the trait and cannot be changed to pass-by-value. Uses
/// `#[allow]` rather than `#[expect]` per `.claude/rules/code-quality.md`:
/// `trivially_copy_pass_by_ref` is a pedantic lint that fires inconsistently
/// across build configurations (lib vs bin), which would trigger
/// `unfulfilled_lint_expectations` under `#[expect]`.
#[must_use]
#[allow(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde's skip_serializing_if requires fn(&T) -> bool"
)]
pub fn is_false(value: &bool) -> bool {
    !*value
}

/// `position` block inside [`GitLabReviewComment`]. Mirrors the GitLab
/// merge-request discussion-position API.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct GitLabReviewPosition {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_sha: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_sha: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_sha: Option<String>,
    pub position_type: GitLabReviewPositionType,
    pub old_path: String,
    pub new_path: String,
    pub new_line: u32,
}

/// Singleton position-type discriminator for [`GitLabReviewPosition`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum GitLabReviewPositionType {
    Text,
}

/// `meta` block inside [`ReviewEnvelopeOutput`].
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ReviewEnvelopeMeta {
    pub schema: ReviewEnvelopeSchema,
    pub provider: ReviewProvider,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub check_conclusion: Option<ReviewCheckConclusion>,
}

/// Schema-version discriminator for the review envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum ReviewEnvelopeSchema {
    /// First release of the review envelope format. Historical only; no v1
    /// emit path remains on the current code. Retained on the enum so a
    /// future Deserialize derive can still parse v1 captures (e.g. from
    /// committed snapshots predating the issue #528 migration) without
    /// erroring on an unknown variant.
    #[serde(rename = "fallow-review-envelope/v1")]
    #[allow(
        dead_code,
        reason = "kept for forward-compat with v1 historical inputs once Deserialize is derived"
    )]
    V1,
    /// Issue #528 evolution. Adds (1) the [`ReviewEnvelopeOutput::summary`]
    /// block, (2) [`ReviewEnvelopeOutput::marker_regex`], (3) same-line
    /// `(path, line)` merging in `comments[]` with a
    /// `merged:<16-char hash>` primary fingerprint over sorted constituent
    /// fingerprints (identity shifts whenever the set of constituents
    /// changes, so the bundled skip-if-fingerprint-exists wrappers
    /// correctly re-post on content change), (4) UTF-8-safe body
    /// truncation at the GitLab/GitHub note-size floor (65,536 bytes)
    /// with paired `truncated: bool` + `<!-- fallow-truncated -->`
    /// signals, (5) `:v2:`-namespaced marker shape
    /// (`<!-- fallow-fingerprint:v2: <fingerprint> -->`) preventing v1
    /// marker collision and user-paste spoofing, and (6) diff-aware
    /// `position.old_path` for renamed files on GitLab.
    #[serde(rename = "fallow-review-envelope/v2")]
    V2,
}

/// Review-envelope provider tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum ReviewProvider {
    /// GitHub pull-request review envelope.
    Github,
    /// GitLab merge-request discussion envelope.
    Gitlab,
}

/// `meta.check_conclusion` for the GitHub review envelope. Maps to the
/// GitHub Checks API conclusion field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum ReviewCheckConclusion {
    /// No findings.
    Success,
    /// Findings but none gated as failure.
    Neutral,
    /// At least one finding gated as failure.
    Failure,
}

/// Envelope emitted by `fallow ci reconcile-review --format json`. Used by
/// CI integrations to drive comment carry-over and stale-comment cleanup
/// across PR / MR revisions.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(
    feature = "schema",
    schemars(title = "fallow ci reconcile-review --format json")
)]
pub struct ReviewReconcileOutput {
    pub schema: ReviewReconcileSchema,
    pub provider: ReviewProvider,
    pub target: Option<String>,
    pub dry_run: bool,
    pub comments: u32,
    pub current_fingerprints: u32,
    pub existing_fingerprints: u32,
    pub new_fingerprints: u32,
    pub stale_fingerprints: u32,
    pub new: Vec<String>,
    pub stale: Vec<String>,
    pub provider_warning: Option<String>,
    pub resolution_comments_posted: u32,
    pub threads_resolved: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub apply_hint: Option<String>,
    pub apply_errors: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub failed_fingerprints: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unapplied_fingerprints: Vec<String>,
}

/// Schema-version discriminator for the review reconcile envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum ReviewReconcileSchema {
    /// First release of the review reconcile format.
    #[serde(rename = "fallow-review-reconcile/v1")]
    V1,
}

/// Resolver mode label for grouped envelopes (dead-code, dupes, health).
///
/// `owner` groups by CODEOWNERS team, `directory` groups by top-level
/// directory prefix, `package` groups by workspace package name, `section`
/// groups by GitLab CODEOWNERS `[Section]` header name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum GroupByMode {
    Owner,
    Directory,
    Package,
    Section,
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
    Health(HealthOutput),
    /// `fallow dupes --format json`. Required `report: DupesReportPayload`
    /// (typed wrapper payload carrying `clone_groups[]: CloneGroupFinding`
    /// and `clone_families[]: CloneFamilyFinding`).
    #[serde(rename = "dupes")]
    Dupes(DupesOutput),
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
