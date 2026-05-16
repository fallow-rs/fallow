//! Typed envelope structs for the JSON output contract.
//!
//! Each top-level fallow command (`check`, `dupes`, `health`, `audit`,
//! `explain`, `coverage setup`, plus the bare combined invocation and the
//! CodeClimate / review-envelope side outputs) emits a distinct envelope
//! shape. This module is the schema-side source of truth for those shapes:
//! every type carries `Serialize` plus a cfg-gated `JsonSchema` derive so the
//! committed `docs/output-schema.json` can be regenerated from Rust.
//!
//! Living in `fallow-cli` rather than `fallow-types` because the body fields
//! pull in `DuplicationReport` (from `fallow-core`) and `HealthReport` (from
//! this crate), neither of which is reachable from the lower-level types
//! crate. The shared utility shapes (`SchemaVersion`, `Meta`,
//! `BaselineDeltas`, ...) still live in `fallow_types::envelope` because they
//! depend only on serde primitives.
//!
//! Runtime construction of these envelopes happens in
//! `crates/cli/src/report/json.rs`; the JSON layer builds an envelope struct
//! and converts it to a `serde_json::Value` via `serde_json::to_value`. Path
//! relativisation and the per-finding `actions` injection still run as
//! post-passes on the `Value` tree because they cross result-type boundaries
//! that typed wrappers do not.
//!
//! The CodeClimate (`CodeClimateOutput` + sub-types) and review-envelope
//! (`ReviewEnvelopeOutput` + sub-types) shapes are NOT constructed at
//! runtime today: `crates/cli/src/report/codeclimate.rs` builds CodeClimate
//! issues via a `cc_issue` helper that returns `serde_json::Value`, and the
//! review-envelope renderer (`crates/cli/src/report/ci/review.rs`) builds
//! provider-specific comment payloads via `serde_json::json!`. Migrating
//! those builders is a follow-up; the types here exist so the drift gate
//! can lock the schema shape against Rust source even though the runtime
//! emit is still dynamic.

#![allow(
    dead_code,
    reason = "review and codeclimate envelope structs document the schema shape; runtime emit is still Value-based pending a follow-up that swaps each builder"
)]

use fallow_core::duplicates::DuplicationReport;
use fallow_core::results::AnalysisResults;
use fallow_types::envelope::{
    BaselineDeltas, BaselineMatch, CheckSummary, ElapsedMs, EntryPoints, Meta, RegressionResult,
    SchemaVersion, ToolVersion,
};
use serde::Serialize;

use crate::audit::{AuditAttribution, AuditSummary, AuditVerdict};
use crate::health_types::{HealthGroup, HealthReport};

/// Envelope emitted by `fallow coverage setup --json`. Deterministic
/// agent-readable runtime coverage setup instructions. In workspaces,
/// `members` carries one entry per detected runtime package; `runtime_targets`
/// is the union of all member targets.
///
/// The runtime path in `crates/cli/src/coverage/mod.rs::build_setup_json`
/// still constructs the wire shape via `serde_json::json!` macros (one per
/// member, snippet, and file-to-edit). The typed struct here serves as the
/// schema source of truth via the drift gate; a follow-up can swap the
/// runtime over without changing the wire.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "schema", schemars(title = "fallow coverage setup --json"))]
pub struct CoverageSetupOutput {
    /// Standalone coverage setup envelope version (always `"1"`).
    pub schema_version: CoverageSetupSchemaVersion,
    /// Primary detected runtime framework. For workspaces this mirrors the
    /// first emitted runtime member; `unknown` means no runtime member was
    /// detected.
    pub framework_detected: CoverageSetupFramework,
    /// Detected JavaScript package manager. `null` when none could be
    /// resolved.
    pub package_manager: Option<CoverageSetupPackageManager>,
    /// Union of runtime targets across emitted members.
    pub runtime_targets: Vec<CoverageSetupRuntimeTarget>,
    /// Per-runtime-workspace setup recipes. Pure aggregator roots and
    /// build-only library packages are omitted.
    pub members: Vec<CoverageSetupMember>,
    /// Always `null` today. Reserved for a future "config has been written
    /// to disk" indicator.
    pub config_written: Option<serde_json::Value>,
    /// Shell commands the agent should run from the workspace root.
    pub commands: Vec<String>,
    /// Compatibility copy of the primary member's files, with workspace
    /// prefixes when the primary member is not the root.
    pub files_to_edit: Vec<CoverageSetupFileToEdit>,
    /// Compatibility copy of the primary member's snippets, with workspace
    /// prefixes when the primary member is not the root.
    pub snippets: Vec<CoverageSetupSnippet>,
    /// Optional Dockerfile RUN/COPY snippet to enable the beacon in
    /// containerised deployments.
    pub dockerfile_snippet: Option<String>,
    /// Ordered next-step instructions for the agent / human operator.
    pub next_steps: Vec<String>,
    /// Non-fatal warnings raised during setup detection.
    pub warnings: Vec<String>,
    /// `_meta` block emitted only when `--explain` is passed.
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<serde_json::Value>,
}

/// Singleton schema-version discriminator for [`CoverageSetupOutput`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum CoverageSetupSchemaVersion {
    /// First release of the coverage setup envelope.
    #[serde(rename = "1")]
    V1,
}

/// Framework label inside coverage setup output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum CoverageSetupFramework {
    /// Next.js (`framework: "nextjs"`).
    #[serde(rename = "nextjs")]
    NextJs,
    /// NestJS (`framework: "nestjs"`).
    #[serde(rename = "nestjs")]
    NestJs,
    /// Nuxt (`framework: "nuxt"`).
    Nuxt,
    /// SvelteKit (`framework: "sveltekit"`).
    #[serde(rename = "sveltekit")]
    SvelteKit,
    /// Astro (`framework: "astro"`).
    Astro,
    /// Remix (`framework: "remix"`).
    Remix,
    /// Vite (`framework: "vite"`).
    Vite,
    /// Plain Node.js (no framework).
    PlainNode,
    /// Could not determine.
    Unknown,
}

/// Package manager label inside coverage setup output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum CoverageSetupPackageManager {
    /// `npm`.
    Npm,
    /// `pnpm`.
    Pnpm,
    /// `yarn`.
    Yarn,
    /// `bun`.
    Bun,
}

/// Runtime target inside coverage setup output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum CoverageSetupRuntimeTarget {
    /// Node.js runtime target.
    Node,
    /// Browser runtime target.
    Browser,
}

/// Per-workspace setup recipe inside [`CoverageSetupOutput::members`].
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CoverageSetupMember {
    /// Workspace package name (or root marker for single-package projects).
    pub name: String,
    /// Workspace path relative to the analysed root, or `.` for the root
    /// member.
    pub path: String,
    /// Framework detected for this member.
    pub framework_detected: CoverageSetupFramework,
    /// Package manager detected for this member.
    pub package_manager: Option<CoverageSetupPackageManager>,
    /// Runtime targets supported by this member's framework.
    pub runtime_targets: Vec<CoverageSetupRuntimeTarget>,
    /// Files the agent should edit to wire in the beacon.
    pub files_to_edit: Vec<CoverageSetupFileToEdit>,
    /// Code snippets the agent should paste into the edited files.
    pub snippets: Vec<CoverageSetupSnippet>,
    /// Optional Dockerfile snippet specific to this member.
    pub dockerfile_snippet: Option<String>,
    /// Member-scoped warnings.
    pub warnings: Vec<String>,
}

/// Single file to edit inside [`CoverageSetupMember::files_to_edit`] or
/// [`CoverageSetupOutput::files_to_edit`].
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CoverageSetupFileToEdit {
    /// Workspace-relative path to the file to edit.
    pub path: String,
    /// Why the file needs editing (e.g. `"Mount the beacon middleware"`).
    pub reason: String,
}

/// Single code snippet inside [`CoverageSetupMember::snippets`] or
/// [`CoverageSetupOutput::snippets`].
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CoverageSetupSnippet {
    /// Short label identifying the snippet (used by the human renderer).
    pub label: String,
    /// Workspace-relative path the snippet should be pasted into.
    pub path: String,
    /// Snippet content (literal source text).
    pub content: String,
}

/// Envelope emitted by `fallow audit --format json`. Combines dead code,
/// complexity, and duplication scoped to changed files with a verdict
/// (`pass` / `warn` / `fail`), a per-category summary, optional
/// new-vs-inherited attribution, and full sub-results.
///
/// Like [`CombinedOutput`], `audit`'s `duplication` and `complexity`
/// sub-keys hold bare body types (`DuplicationReport` / `HealthReport`)
/// rather than the per-command envelope shapes; `dead_code` is the full
/// [`CheckOutput`] envelope. The committed schema points `duplication`
/// at `#/definitions/DuplicationReport` and `complexity` at
/// `#/definitions/HealthReport` so the documented shape matches the
/// wire; the `committed_property_refs_match_derived_property_refs`
/// drift test enforces the alignment.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "schema", schemars(title = "fallow audit --format json"))]
pub struct AuditOutput {
    /// Schema version for this output format.
    pub schema_version: SchemaVersion,
    /// Fallow tool version that produced this output.
    pub version: ToolVersion,
    /// Singleton command discriminator (always `"audit"`).
    pub command: AuditCommand,
    /// Overall verdict: `pass` (no issues), `warn` (warn-severity only,
    /// exit 0), or `fail` (error-severity issues, exit 1).
    pub verdict: AuditVerdict,
    /// Number of files changed between base ref and HEAD.
    pub changed_files_count: u32,
    /// Git ref used as comparison base (explicit or auto-detected).
    pub base_ref: String,
    /// Short SHA of HEAD. Omitted when git is unavailable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_sha: Option<String>,
    /// Analysis duration in milliseconds.
    pub elapsed_ms: ElapsedMs,
    /// Only emitted when --performance is set. true means audit reused the
    /// current run's keys as the base snapshot because every changed file was
    /// either a non-behavioral doc or token-equivalent at the base ref (the
    /// docs-only-diff fast path); false means the regular base worktree
    /// analysis ran.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_snapshot_skipped: Option<bool>,
    /// Per-category summary counts.
    pub summary: AuditSummary,
    /// Counts split by whether each finding was introduced by the current
    /// changeset or already existed at the base ref. The default audit gate is
    /// new-only, so inherited findings are context. With audit.gate or --gate
    /// set to all, audit skips the extra base-snapshot attribution pass and
    /// these counts stay zero.
    pub attribution: AuditAttribution,
    /// Full dead code results (omitted if no changed files). Issue objects
    /// include introduced: true/false when audit can compare against the base
    /// ref.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dead_code: Option<CheckOutput>,
    /// Full duplication results (omitted if no changed files). Clone groups
    /// include introduced: true/false when audit can compare against the base
    /// ref.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duplication: Option<DuplicationReport>,
    /// Full complexity results (omitted if no changed files). Findings include
    /// introduced: true/false when audit can compare against the base ref.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub complexity: Option<HealthReport>,
}

/// Singleton `command` discriminator for [`AuditOutput`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum AuditCommand {
    /// The only valid command discriminator for `AuditOutput`.
    Audit,
}

/// Envelope emitted by bare `fallow --format json` (the combined
/// invocation). Wraps the per-analysis sub-results inside a single envelope
/// with the standard `schema_version` / `version` / `elapsed_ms` header.
///
/// Each sub-result is `Option<...>` so `--only` / `--skip` can suppress a
/// pass without leaving an empty key on the wire. The `check` sub-result is
/// the full [`CheckOutput`] envelope (including its own `schema_version` /
/// `version` / `elapsed_ms`), but `dupes` and `health` are the bare body
/// types: the runtime emit calls `serde_json::to_value(&report)` on
/// `DuplicationReport` / `HealthReport` directly rather than wrapping them
/// in their per-command envelope. The committed schema points `dupes` at
/// `#/definitions/DuplicationReport` and `health` at
/// `#/definitions/HealthReport` so the documented shape matches the
/// wire; the `committed_property_refs_match_derived_property_refs`
/// drift test enforces the alignment.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(
    feature = "schema",
    schemars(title = "fallow --format json (bare, combined)")
)]
pub struct CombinedOutput {
    /// Schema version for this output format.
    pub schema_version: SchemaVersion,
    /// Fallow tool version that produced this output.
    pub version: ToolVersion,
    /// Analysis duration in milliseconds.
    pub elapsed_ms: ElapsedMs,
    /// Dead-code analysis sub-envelope. Absent when `--skip check`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub check: Option<CheckOutput>,
    /// Duplication analysis body (bare `DuplicationReport`, not the full
    /// `DupesOutput` envelope). Absent when `--skip dupes`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dupes: Option<DuplicationReport>,
    /// Complexity analysis body (bare `HealthReport`, not the full
    /// `HealthOutput` envelope). Absent when `--skip health`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub health: Option<HealthReport>,
}

/// Envelope emitted by `fallow dupes --format json` (plus the `dupes` block
/// inside the combined and audit envelopes).
///
/// The body is the full `DuplicationReport` flattened into the envelope so
/// the wire shape stays `{ schema_version, version, elapsed_ms, clone_groups,
/// clone_families, stats, ... }` exactly as the existing JSON layer emits.
/// `grouped_by` / `groups` / `total_issues` are populated by the grouped
/// builder; on the ungrouped path they stay `None` and `skip_serializing_if`
/// drops them.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "schema", schemars(title = "fallow dupes --format json"))]
pub struct DupesOutput {
    /// Schema version for this output format.
    pub schema_version: SchemaVersion,
    /// Fallow tool version that produced this output.
    pub version: ToolVersion,
    /// Analysis duration in milliseconds.
    pub elapsed_ms: ElapsedMs,
    /// Project-level duplication payload (`clone_groups`, `clone_families`,
    /// `stats`, optional `mirrored_directories`). Flattened so the wire shape
    /// stays a single object.
    #[serde(flatten)]
    pub report: DuplicationReport,
    /// Resolver mode used for partitioning. Present only when `--group-by` is
    /// active.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grouped_by: Option<GroupByMode>,
    /// Total clone groups across all buckets when `--group-by` is active.
    /// Mirrors the grouped check / health envelopes which expose
    /// `total_issues` so MCP and CI consumers can read the same key across
    /// commands.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_issues: Option<usize>,
    /// Per-group buckets when `--group-by` is active. Each clone group is
    /// attributed to its largest-owner key (most instances; alphabetical
    /// tiebreak). Sort: most clone groups first, then alphabetical, with
    /// `(unowned)` pinned last.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub groups: Option<serde_json::Value>,
    /// `_meta` block with metric / rule definitions, emitted when `--explain`
    /// is passed (always present in MCP responses).
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Meta>,
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
    /// Schema version for this output format.
    pub schema_version: SchemaVersion,
    /// Fallow tool version that produced this output.
    pub version: ToolVersion,
    /// Analysis duration in milliseconds.
    pub elapsed_ms: ElapsedMs,
    /// Total number of issues found across all categories.
    pub total_issues: usize,
    /// Entry-point detection summary. Present when the analysis populated
    /// the metadata block; absent in synthesised fixtures.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry_points: Option<EntryPoints>,
    /// Per-category issue counts. Always present. When --summary is used,
    /// individual issue arrays are omitted.
    pub summary: CheckSummary,
    /// All issue arrays flattened in from `AnalysisResults`.
    #[serde(flatten)]
    pub results: AnalysisResults,
    /// Per-category delta comparison against a saved baseline. Only present
    /// when `--baseline` is used (today only via the combined invocation).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_deltas: Option<BaselineDeltas>,
    /// Baseline match statistics. Only present when `--baseline` is used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline: Option<BaselineMatch>,
    /// Regression check result. Only present when `--fail-on-regression` is
    /// used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub regression: Option<RegressionResult>,
    /// `_meta` block with metric / rule definitions, emitted when `--explain`
    /// is passed (always present in MCP responses).
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Meta>,
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
    /// Schema version for this output format.
    pub schema_version: SchemaVersion,
    /// Fallow tool version that produced this output.
    pub version: ToolVersion,
    /// Analysis duration in milliseconds.
    pub elapsed_ms: ElapsedMs,
    /// The grouping strategy used. 'owner' groups by CODEOWNERS team,
    /// 'directory' groups by top-level directory prefix, 'package' groups by
    /// workspace package name, 'section' groups by GitLab CODEOWNERS
    /// `[Section]` header name.
    pub grouped_by: GroupByMode,
    /// Total number of issues across all groups.
    pub total_issues: usize,
    /// One entry per group; each contains the same issue arrays as
    /// `CheckOutput` plus the group key and per-group total.
    pub groups: Vec<CheckGroupedEntry>,
    /// `_meta` block with metric / rule definitions, emitted when `--explain`
    /// is passed.
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Meta>,
}

/// Single resolver bucket inside `CheckGroupedOutput`. Carries the group's
/// identifier, optional section owners, and a per-group flattened
/// `AnalysisResults`.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CheckGroupedEntry {
    /// Group identifier produced by the resolver. For `package` grouping:
    /// workspace package name. For `owner` grouping: the CODEOWNERS team.
    /// For `directory` grouping: the top-level directory prefix. For
    /// `section` grouping: the GitLab CODEOWNERS section name (or
    /// `(no section)` / `(unowned)` for unmatched files).
    pub key: String,
    /// Section default owners (GitLab CODEOWNERS `[Section] @owner1
    /// @owner2`). Emitted only when `grouped_by` is `section`. Empty for
    /// the `(no section)` and `(unowned)` buckets.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owners: Option<Vec<String>>,
    /// Total number of issues in this group.
    pub total_issues: usize,
    /// Per-group issue arrays restricted to files in this group.
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
/// so the schema documents the field; `inject_health_actions` still
/// populates it post-construction on the `serde_json::Value` tree because
/// the suppression context lives inside the report builder.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "schema", schemars(title = "fallow health --format json"))]
pub struct HealthOutput {
    /// Schema version for this output format.
    pub schema_version: SchemaVersion,
    /// Fallow tool version that produced this output.
    pub version: ToolVersion,
    /// Analysis duration in milliseconds.
    pub elapsed_ms: ElapsedMs,
    /// All fields from `HealthReport` flattened in so the wire shape stays
    /// a single object.
    #[serde(flatten)]
    pub report: HealthReport,
    /// Resolver mode used when --group-by is active. Present only on grouped
    /// output. The top-level `vital_signs`, `health_score`, and `summary` keep
    /// the active run scope (for example after --workspace); per-group versions
    /// live inside each entry of `groups`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grouped_by: Option<GroupByMode>,
    /// Per-group health output, present only when `--group-by` is active.
    /// Each group recomputes its own `vital_signs` and `health_score` from
    /// the files in that group, mirroring how `--workspace` scopes a single
    /// subset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub groups: Option<Vec<HealthGroup>>,
    /// `_meta` block with metric / rule definitions, emitted when `--explain`
    /// is passed (always present in MCP responses).
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Meta>,
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
#[serde(deny_unknown_fields)]
pub struct ExplainOutput {
    /// Canonical rule id, for example `fallow/unused-export`.
    pub id: String,
    /// Human-readable rule name.
    pub name: String,
    /// Short one-line explanation of the issue.
    pub summary: String,
    /// Why the issue matters and what fallow checks.
    pub rationale: String,
    /// Concrete example of the finding.
    pub example: String,
    /// Recommended fix or suppression guidance.
    pub how_to_fix: String,
    /// Docs URL for the rule.
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
pub struct CodeClimateOutput(pub Vec<CodeClimateIssue>);

/// Single CodeClimate-compatible issue inside [`CodeClimateOutput`].
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CodeClimateIssue {
    /// Always the literal string `"issue"`.
    #[serde(rename = "type")]
    pub kind: CodeClimateIssueKind,
    /// Fallow rule identifier (always starts with `fallow/`).
    pub check_name: String,
    /// Human-readable description of the finding.
    pub description: String,
    /// Free-form categories applied by the report renderer.
    pub categories: Vec<String>,
    /// CodeClimate-style severity.
    pub severity: CodeClimateSeverity,
    /// Stable fingerprint used by CI dashboards to deduplicate findings
    /// across runs.
    pub fingerprint: String,
    /// File path + start line of the finding.
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
    /// Informational.
    Info,
    /// Minor finding.
    Minor,
    /// Major finding.
    Major,
    /// Critical finding.
    Critical,
    /// Blocker (highest severity).
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
/// Consumed by `action/scripts/review.sh` and `ci/scripts/review.sh` to
/// post inline PR / MR review comments.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(
    feature = "schema",
    schemars(title = "fallow --format review-github / review-gitlab")
)]
pub struct ReviewEnvelopeOutput {
    /// GitHub review event. Omitted for GitLab.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event: Option<ReviewEnvelopeEvent>,
    /// Review summary body (rendered above per-line comments).
    pub body: String,
    /// Per-line comments. Each is either a [`GitHubReviewComment`] or a
    /// [`GitLabReviewComment`] depending on `meta.provider`.
    pub comments: Vec<ReviewComment>,
    /// Envelope metadata block.
    pub meta: ReviewEnvelopeMeta,
}

/// Singleton GitHub review-event marker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum ReviewEnvelopeEvent {
    /// GitHub review event for an unblocking comment review.
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
    /// GitHub-shaped pull-request review comment.
    GitHub(GitHubReviewComment),
    /// GitLab-shaped merge-request discussion comment.
    GitLab(GitLabReviewComment),
}

/// GitHub pull-request review comment.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct GitHubReviewComment {
    /// File path the comment targets, repo-root relative.
    pub path: String,
    /// 1-indexed line number the comment targets.
    pub line: u32,
    /// Always the literal string `"RIGHT"`; GitHub review comments target
    /// current-state/new-side lines; deletion-side comments are not modeled
    /// yet.
    pub side: GitHubReviewSide,
    /// Markdown body of the comment.
    pub body: String,
    /// Stable fingerprint for the comment, used by `fallow ci
    /// reconcile-review` to detect carryover comments across PR revisions.
    pub fingerprint: String,
}

/// Singleton side discriminator for [`GitHubReviewComment::side`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum GitHubReviewSide {
    /// GitHub review comments target the new-side line range.
    #[serde(rename = "RIGHT")]
    Right,
}

/// GitLab merge-request discussion comment.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct GitLabReviewComment {
    /// Markdown body of the comment.
    pub body: String,
    /// Position block describing where the comment attaches on the diff.
    pub position: GitLabReviewPosition,
    /// Stable fingerprint for the comment.
    pub fingerprint: String,
}

/// `position` block inside [`GitLabReviewComment`]. Mirrors the GitLab
/// merge-request discussion-position API.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct GitLabReviewPosition {
    /// Merge-request base SHA.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_sha: Option<String>,
    /// Merge-request start SHA.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_sha: Option<String>,
    /// Merge-request head SHA.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_sha: Option<String>,
    /// Always `"text"` today.
    pub position_type: GitLabReviewPositionType,
    /// File path on the base side.
    pub old_path: String,
    /// File path on the head side.
    pub new_path: String,
    /// 1-indexed line on the head side.
    pub new_line: u32,
}

/// Singleton position-type discriminator for [`GitLabReviewPosition`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum GitLabReviewPositionType {
    /// Plain-text diff position (only kind fallow emits today).
    Text,
}

/// `meta` block inside [`ReviewEnvelopeOutput`].
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ReviewEnvelopeMeta {
    /// Envelope schema marker, always `fallow-review-envelope/v1`.
    pub schema: ReviewEnvelopeSchema,
    /// Which provider this envelope is shaped for.
    pub provider: ReviewProvider,
    /// Check conclusion derived from the underlying findings. Emitted only
    /// for GitHub envelopes today.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub check_conclusion: Option<ReviewCheckConclusion>,
}

/// Schema-version discriminator for the review envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum ReviewEnvelopeSchema {
    /// First release of the review envelope format.
    #[serde(rename = "fallow-review-envelope/v1")]
    V1,
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
    /// Envelope schema marker, always `fallow-review-reconcile/v1`.
    pub schema: ReviewReconcileSchema,
    /// Which provider this reconcile pass was for.
    pub provider: ReviewProvider,
    /// PR / MR target identifier supplied to `fallow ci reconcile-review`.
    /// `null` when the command ran without an explicit target.
    pub target: Option<String>,
    /// Whether the reconcile ran in dry-run mode.
    pub dry_run: bool,
    /// Number of comments in the supplied review envelope.
    pub comments: u32,
    /// Total fingerprints discovered in the supplied envelope.
    pub current_fingerprints: u32,
    /// Existing fingerprints already posted on the PR / MR.
    pub existing_fingerprints: u32,
    /// Newly-introduced fingerprints (current minus existing).
    pub new_fingerprints: u32,
    /// Stale fingerprints (existing minus current).
    pub stale_fingerprints: u32,
    /// Identifiers of the new fingerprints (subset of comments).
    pub new: Vec<String>,
    /// Identifiers of the stale fingerprints (subset of existing).
    pub stale: Vec<String>,
    /// Optional warning when the provider API was unreachable or
    /// auth-rejected. `null` on the happy path.
    pub provider_warning: Option<String>,
    /// Resolution comments actually posted (zero on dry runs).
    pub resolution_comments_posted: u32,
    /// Stale review threads actually resolved (zero on dry runs).
    pub threads_resolved: u32,
    /// Errors collected during apply, one entry per failure.
    pub apply_errors: Vec<String>,
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
    /// Group by CODEOWNERS team.
    Owner,
    /// Group by top-level directory prefix.
    Directory,
    /// Group by workspace package name.
    Package,
    /// Group by GitLab CODEOWNERS `[Section]` header name.
    Section,
}
