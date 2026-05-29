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
//! and converts it to a `serde_json::Value` via `serde_json::to_value`. The
//! only remaining work on the `Value` tree is path relativisation
//! (`strip_root_prefix`) and the cross-result-type suppress-line action
//! harmonizer (`harmonize_multi_kind_suppress_line_actions`); both span
//! envelope boundaries that typed wrappers do not.
//!
//! Runtime emit for the CodeClimate, review-envelope, and coverage-setup
//! shapes now flows through the typed structs in this module:
//! `crates/cli/src/report/codeclimate.rs` constructs `CodeClimateIssue`
//! directly via `cc_issue`,
//! `crates/cli/src/report/ci/review.rs::render_review_envelope` constructs
//! `ReviewEnvelopeOutput`, and
//! `crates/cli/src/coverage/mod.rs::build_setup_envelope` constructs
//! `CoverageSetupOutput`. The wire `serde_json::Value` is the
//! `serde_json::to_value(&envelope)` of those typed structs, so adding a
//! field to one of those structs automatically flows to the wire. The
//! `AuditOutput` and `ListBoundariesOutput` families remain
//! schema-source-of-truth only (their wire is still hand-built via
//! `serde_json::json!`); the drift gate keeps them honest.

use fallow_core::results::AnalysisResults;
use fallow_types::envelope::{
    BaselineDeltas, BaselineMatch, CheckSummary, ElapsedMs, EntryPoints, Meta, RegressionResult,
    SchemaVersion, ToolVersion,
};
use serde::Serialize;

use crate::audit::{AuditAttribution, AuditSummary, AuditVerdict};
use crate::health_types::{HealthGroup, HealthReport, RuntimeCoverageReport};
use crate::output_dupes::DupesReportPayload;
use crate::report::dupes_grouping::DuplicationGroup;

/// Envelope emitted by `fallow coverage setup --json`. Deterministic
/// agent-readable runtime coverage setup instructions. In workspaces,
/// `members` carries one entry per detected runtime package; `runtime_targets`
/// is the union of all member targets.
///
/// Constructed at runtime by
/// `crates/cli/src/coverage/mod.rs::build_setup_envelope`; the wire is
/// `serde_json::to_value(&envelope)`. The drift gate keeps this struct
/// aligned with `docs/output-schema.json`.
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
/// sub-keys hold body shapes rather than per-command envelopes:
/// `duplication` is [`DupesReportPayload`] (the typed wrapper payload
/// emitted via `crate::output_dupes::DupesReportPayload::from_report`),
/// `complexity` is [`HealthReport`]. `dead_code` is the full
/// [`CheckOutput`] envelope. The committed schema points `duplication`
/// at `#/definitions/DupesReportPayload` and `complexity` at
/// `#/definitions/HealthReport` so the documented shape matches the
/// wire; the `committed_property_refs_match_derived_property_refs`
/// drift test enforces the alignment.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "schema", schemars(title = "fallow audit --format json"))]
#[allow(
    dead_code,
    reason = "schema-source-of-truth: audit.rs still builds the wire via serde_json::json!; this struct locks the schema shape via the drift gate. Migration is a follow-up to issue #384 items 3a/3b/3c."
)]
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
    /// ref. Carries typed [`crate::output_dupes::CloneGroupFinding`] and
    /// [`crate::output_dupes::CloneFamilyFinding`] wrappers (matches what
    /// `crates/cli/src/audit.rs` emits via
    /// `crate::output_dupes::DupesReportPayload::from_report`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duplication: Option<DupesReportPayload>,
    /// Full complexity results (omitted if no changed files). Findings include
    /// introduced: true/false when audit can compare against the base ref.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub complexity: Option<HealthReport>,
}

/// Singleton `command` discriminator for [`AuditOutput`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
#[allow(dead_code, reason = "schema-source-of-truth: see `AuditOutput`.")]
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
/// `version` / `elapsed_ms`), `dupes` is the typed [`DupesReportPayload`]
/// emitted via `crate::output_dupes::DupesReportPayload::from_report`, and
/// `health` is the bare [`HealthReport`] body: the runtime emit calls
/// `serde_json::to_value(&report)` directly rather than wrapping it in the
/// per-command envelope. The committed schema points `dupes` at
/// `#/definitions/DupesReportPayload` and `health` at
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
    /// Sectioned `_meta` block emitted only when `--explain` is passed.
    /// Contains `check`, `dupes`, and/or `health` keys matching the analyses
    /// enabled for the combined run.
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<CombinedMeta>,
    /// Dead-code analysis sub-envelope. Absent when `--skip check`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub check: Option<CheckOutput>,
    /// Duplication analysis body (typed [`DupesReportPayload`], not the full
    /// `DupesOutput` envelope). Absent when `--skip dupes`. The payload
    /// wraps each clone group / family with its typed `actions[]` array via
    /// `crate::output_dupes::DupesReportPayload::from_report`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dupes: Option<DupesReportPayload>,
    /// Complexity analysis body (bare `HealthReport`, not the full
    /// `HealthOutput` envelope). Absent when `--skip health`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub health: Option<HealthReport>,
}

/// Sectioned `_meta` block for the bare combined JSON envelope.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CombinedMeta {
    /// Dead-code metadata from `crate::explain::check_meta()`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub check: Option<Meta>,
    /// Duplication metadata from `crate::explain::dupes_meta()`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dupes: Option<Meta>,
    /// Health metadata from `crate::explain::health_meta()`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub health: Option<Meta>,
}

/// Singleton schema-version discriminator for [`CoverageAnalyzeOutput`].
/// Independent from the global [`SchemaVersion`] because the runtime
/// coverage envelope versions independently from the rest of the
/// JSON contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum CoverageAnalyzeSchemaVersion {
    /// First release of the standalone `fallow coverage analyze` envelope.
    #[serde(rename = "1")]
    V1,
}

/// Envelope emitted by `fallow coverage analyze --format json`.
///
/// Focused runtime coverage analysis output. Local mode reads
/// `--runtime-coverage <path>`. Cloud mode requires explicit `--cloud` /
/// `--runtime-coverage-cloud` or `FALLOW_RUNTIME_COVERAGE_SOURCE=cloud`;
/// `FALLOW_API_KEY` alone does NOT select cloud mode.
///
/// Constructed at runtime in
/// `crates/cli/src/coverage/analyze.rs::print_runtime_json`; the wire is
/// `serde_json::to_value(&envelope)`. The drift gate keeps this struct
/// aligned with `docs/output-schema.json`. Carries its own schema-version
/// discriminator ([`CoverageAnalyzeSchemaVersion`]) because runtime
/// coverage iterates independently of the main JSON contract version.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(
    feature = "schema",
    schemars(title = "fallow coverage analyze --format json")
)]
pub struct CoverageAnalyzeOutput {
    /// Standalone coverage analyze envelope version.
    pub schema_version: CoverageAnalyzeSchemaVersion,
    /// fallow CLI version.
    pub version: ToolVersion,
    /// Analysis duration in milliseconds.
    pub elapsed_ms: ElapsedMs,
    /// The same runtime coverage block emitted by health JSON.
    pub runtime_coverage: RuntimeCoverageReport,
    /// `_meta` block with metric / rule definitions, emitted when `--explain`
    /// is passed. Populated via the post-pass injection in
    /// `print_runtime_json` (matches the pattern used by every other typed
    /// envelope; the typed struct sets this to `None` and the JSON layer
    /// merges in the `crate::explain::coverage_analyze_meta()` payload).
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Meta>,
}

/// Envelope emitted by `fallow dupes --format json` (plus the `dupes` block
/// inside the combined and audit envelopes).
///
/// The body is the typed [`DupesReportPayload`] flattened into the envelope
/// so the wire shape stays `{ schema_version, version, elapsed_ms,
/// clone_groups, clone_families, stats, ... }` exactly as the existing JSON
/// layer emits. The payload's `clone_groups` and `clone_families` carry
/// typed [`crate::output_dupes::CloneGroupFinding`] /
/// [`crate::output_dupes::CloneFamilyFinding`] wrappers so the `actions[]`
/// field is part of the schema-derived contract.
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
    /// stays a single object. Carries typed [`crate::output_dupes::CloneGroupFinding`]
    /// and [`crate::output_dupes::CloneFamilyFinding`] wrappers instead of bare
    /// findings so the `actions[]` array (and audit-mode `introduced`) are part
    /// of the schema-derived contract rather than a JSON post-pass.
    #[serde(flatten)]
    pub report: DupesReportPayload,
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
    ///
    /// Each bucket's `clone_groups` and `clone_families` carry the typed
    /// finding wrappers ([`crate::output_dupes::AttributedCloneGroupFinding`],
    /// [`crate::output_dupes::CloneFamilyFinding`]) so the `actions[]`
    /// augmentation is part of the schema-derived contract.
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
    /// Workspace-discovery diagnostics surfaced by
    /// `discover_workspaces_with_diagnostics` (issue #473): malformed
    /// declared-workspace `package.json`, glob matches with no `package.json`,
    /// malformed `tsconfig.json`, missing tsconfig reference paths. Omitted
    /// when empty so consumers on monorepos without discovery noise see no
    /// new field. Pairing of `#[serde(default, skip_serializing_if = ...)]`
    /// is required for schemars to mark the field non-required.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub workspace_diagnostics: Vec<fallow_config::WorkspaceDiagnostic>,
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
/// and is set at construction time by the report builder when the active
/// `HealthActionContext` requests suppress-line omission, so the schema
/// documents the field and serde populates it natively.
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
    /// Workspace-discovery diagnostics surfaced during config load
    /// (issue #473). Mirror of [`CheckOutput::workspace_diagnostics`] so
    /// stand-alone `fallow health --format json` consumers see the same
    /// signal.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub workspace_diagnostics: Vec<fallow_config::WorkspaceDiagnostic>,
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
#[allow(
    dead_code,
    reason = "schema-source-of-truth wrapper: runtime emits a `Vec<CodeClimateIssue>` directly via `codeclimate::issues_to_value`; this newtype exists so `schemars` can title and document the bare-array shape for the drift gate."
)]
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
    /// Review summary body (rendered above per-line comments). Deprecated in
    /// v2 envelopes: prefer [`summary.body`](`ReviewEnvelopeSummary::body`),
    /// which is byte-identical to this field but carries a stable
    /// fingerprint for reconciliation. Kept on v2 emit so v1 consumers that
    /// only look at `body` keep working.
    pub body: String,
    /// Sticky summary block (v2). Always present on v2 emit. Consumers
    /// reconcile a single sticky PR/MR summary comment by
    /// [`ReviewEnvelopeSummary::fingerprint`] matching, then upsert
    /// [`ReviewEnvelopeSummary::body`] in place. Synthesized empty when
    /// deserializing v1 historical input.
    #[serde(default = "ReviewEnvelopeSummary::empty_default")]
    pub summary: ReviewEnvelopeSummary,
    /// Per-line comments. Each is either a [`GitHubReviewComment`] or a
    /// [`GitLabReviewComment`] depending on `meta.provider`.
    pub comments: Vec<ReviewComment>,
    /// Regex consumers run against every existing PR/MR comment body to
    /// extract a fallow-emitted fingerprint marker. Capture group 1 is the
    /// fingerprint string (a bare 16-char hex hash for single-finding
    /// comments, or `<kind>:<16-char-hex>` for compositions such as
    /// `merged:` for same-line collapsed comments).
    ///
    /// The pattern is anchored with `^` / `$` and relies on multiline
    /// matching to anchor at line boundaries inside a multi-line comment
    /// body. Multiline is NOT baked into the pattern via `(?m)` (which
    /// JavaScript RegExp rejects as `Invalid group`); instead the consumer
    /// passes [`Self::marker_regex_flags`] as the flags argument to its
    /// regex engine. JavaScript: `new RegExp(env.marker_regex,
    /// env.marker_regex_flags)`. Rust: `regex::RegexBuilder::new(pat)
    /// .multi_line(flags.contains('m')).build()` (or any equivalent).
    #[serde(default = "default_marker_regex")]
    pub marker_regex: String,
    /// Flags consumers pass alongside [`Self::marker_regex`] when
    /// constructing their regex engine. Currently always `"m"` (multiline
    /// so the anchored `^` / `$` match at every line boundary within a
    /// comment body). Emitting flags as a separate field instead of
    /// baking `(?m)` into the pattern keeps the wire compatible with
    /// JavaScript RegExp, which rejects inline flag groups outside a
    /// `(?flags:X)` grouping.
    #[serde(default = "default_marker_regex_flags")]
    pub marker_regex_flags: String,
    /// Envelope metadata block.
    pub meta: ReviewEnvelopeMeta,
}

/// Default for [`ReviewEnvelopeOutput::marker_regex`]. The canonical regex is
/// stable across the v2 schema. Consumers that hardcode this string instead
/// of reading the field stay correct until a v3 bump.
#[must_use]
pub fn default_marker_regex() -> String {
    MARKER_REGEX_V2.to_owned()
}

/// Default for [`ReviewEnvelopeOutput::marker_regex_flags`]. Always `"m"`
/// today; emitted as a sibling field rather than baked into the regex
/// because JavaScript RegExp rejects the standalone `(?m)` inline flag
/// group with `SyntaxError: Invalid regular expression ... Invalid group`.
#[must_use]
pub fn default_marker_regex_flags() -> String {
    MARKER_REGEX_FLAGS_V2.to_owned()
}

/// Canonical v2 marker-regex literal. Mirrored by
/// [`MARKER_PREFIX_V2`](`crate::report::ci::review::MARKER_PREFIX_V2`) on the
/// render side; if you change one, change the other and refresh both
/// snapshots. NO `(?m)` baked into the pattern; consumers pass
/// [`MARKER_REGEX_FLAGS_V2`] as the second arg to their regex engine so
/// the `^` / `$` anchors match at line boundaries inside a multi-line
/// comment body. Pairing pattern + flags lets the wire stay compatible
/// with both Rust's `regex` crate (via `RegexBuilder::multi_line(true)`)
/// and JavaScript RegExp (`new RegExp(pat, "m")`).
pub const MARKER_REGEX_V2: &str =
    r"^<!-- fallow-fingerprint:v2: ((?:[a-z]+:)?[0-9a-f]{16}) -->\s*$";

/// Canonical v2 marker-regex flags. Paired with [`MARKER_REGEX_V2`].
pub const MARKER_REGEX_FLAGS_V2: &str = "m";

/// Summary block on [`ReviewEnvelopeOutput`]. Always present on v2 emit;
/// `serde(default)` keeps schemars from marking it required so a future
/// Deserialize derivation against v1 historical input synthesizes an empty
/// value rather than erroring.
#[derive(Debug, Clone, Serialize, Default)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ReviewEnvelopeSummary {
    /// Markdown body of the summary. Byte-identical to the legacy top-level
    /// [`ReviewEnvelopeOutput::body`] field; the duplication is intentional
    /// so v1 consumers see no behavior change.
    pub body: String,
    /// FNV-1a 64-bit hash (16 lowercase hex chars) of the summary body
    /// BEFORE the trailing fallow-fingerprint marker line is appended.
    /// (Computing the hash from the post-marker body would be circular:
    /// the marker contains the fingerprint, so the fingerprint cannot
    /// depend on the marker.) To reproduce from [`Self::body`], strip the
    /// line matching [`ReviewEnvelopeOutput::marker_regex`] together with
    /// its leading separator newlines and hash the remainder. Stable
    /// across runs that produce the same summary content; consumers
    /// upsert the sticky summary comment by matching this fingerprint
    /// against the marker_regex extraction of every existing comment body.
    pub fingerprint: String,
}

impl ReviewEnvelopeSummary {
    /// Empty-default factory used by `#[serde(default = "...")]` on
    /// [`ReviewEnvelopeOutput::summary`]. Returns a zero-body, zero-
    /// fingerprint value so v1 historical inputs deserialize without
    /// inventing fabricated content.
    ///
    /// Referenced from the `default = "ReviewEnvelopeSummary::empty_default"`
    /// attribute on the field; serde's macro resolves it lazily at derive
    /// time without registering a direct call site, so without the explicit
    /// allow the function tripped `dead_code` until a Deserialize derive
    /// pulls it in. schemars also reads the attribute to mark the field
    /// non-required in the schema's `required[]`.
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
    /// For single-finding comments the value is a bare 16-char hex FNV-1a
    /// hash. For merged comments (multiple findings on the same path:line)
    /// the value is `merged:<16-char hex>` over the sorted constituent
    /// fingerprints, so the identity shifts whenever constituent findings
    /// change membership. Bundled wrappers and `fallow ci reconcile-review`
    /// dedupe on this primary fingerprint only; consumers wanting
    /// update-in-place reconciliation (preserving reviewer reply threads
    /// across content changes) implement their own identity tracking via
    /// `marker_regex`.
    pub fingerprint: String,
    /// True when [`Self::body`] was truncated to fit a downstream provider's
    /// note-size budget (today: 65,536 bytes). The body retains the closing
    /// fallow-fingerprint marker so reconciliation continues to work after
    /// truncation.
    ///
    /// Co-presence invariant: `truncated == true` always implies the body
    /// contains an inline `<!-- fallow-truncated -->` HTML marker and the
    /// `> Body truncated by fallow.` blockquote breadcrumb, and vice versa.
    /// All three signals are emitted together; consumers may use any one
    /// (the typed boolean is the authoritative machine-readable signal).
    #[serde(default, skip_serializing_if = "is_false")]
    pub truncated: bool,
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
    /// Stable fingerprint for the comment. See
    /// [`GitHubReviewComment::fingerprint`] for the single vs `merged:`
    /// shape contract; semantics are identical across providers.
    pub fingerprint: String,
    /// True when [`Self::body`] was truncated to fit GitLab's note-size
    /// budget. See [`GitHubReviewComment::truncated`] for the full
    /// co-presence invariant with the inline HTML marker and human
    /// blockquote breadcrumb.
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
    /// Envelope schema marker. v2 emit always tags
    /// `fallow-review-envelope/v2`; v1 is recognized on deserialize for
    /// backward-compat with historical envelopes captured before the v2
    /// migration.
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
    /// Operator-facing retry hint when apply stopped early.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub apply_hint: Option<String>,
    /// Errors collected during apply, one entry per failure.
    pub apply_errors: Vec<String>,
    /// Stale fingerprints whose provider mutation failed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub failed_fingerprints: Vec<String>,
    /// Stale fingerprints not fully applied after the fail-fast stop.
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
    /// Group by CODEOWNERS team.
    Owner,
    /// Group by top-level directory prefix.
    Directory,
    /// Group by workspace package name.
    Package,
    /// Group by GitLab CODEOWNERS `[Section]` header name.
    Section,
}

// ── list --boundaries --format json envelope ────────────────────────
//
// The runtime path builds the wire shape via `serde_json::json!` in
// `crates/cli/src/list.rs::boundary_data_to_json`; the typed structs below
// exist so the drift gate can lock the schema shape against Rust source.
// A follow-up that swaps the runtime builder over to typed construction
// can land independently (out of scope for issue #384 items 3a/3b/3c).

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
    /// The boundaries section. The list command can also emit `files`,
    /// `plugins`, `entry_points` siblings under additional flags; those
    /// shapes are not part of this envelope today.
    pub boundaries: BoundariesListing,
}

/// `boundaries` block carried by [`ListBoundariesOutput`].
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[allow(
    dead_code,
    reason = "schema-source-of-truth: see `ListBoundariesOutput`."
)]
pub struct BoundariesListing {
    /// `false` when the project has no `boundaries` configured; `true`
    /// otherwise. When `false` every array below is empty and every count
    /// is `0` (parity is enforced so consumers can read the counts without
    /// first branching on this flag).
    pub configured: bool,
    /// Length of [`Self::zones`]; emitted alongside the array for parity
    /// with `rule_count` / `logical_group_count`.
    pub zone_count: usize,
    /// Boundary zones after preset and `autoDiscover` expansion.
    pub zones: Vec<BoundariesListZone>,
    /// Length of [`Self::rules`].
    pub rule_count: usize,
    /// Boundary import rules, each `from -> allow[]`.
    pub rules: Vec<BoundariesListRule>,
    /// Length of [`Self::logical_groups`]. Always present (issue #373).
    pub logical_group_count: usize,
    /// Pre-expansion `autoDiscover` groups carrying the user-authored parent
    /// name and grouping intent (issue #373).
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
    /// Zone identifier as referenced in rules (e.g. `app`, `features/auth`).
    pub name: String,
    /// Compiled glob patterns. Children of an `autoDiscover` parent each
    /// carry a single pattern like `src/features/auth/**`.
    pub patterns: Vec<String>,
    /// Number of discovered files classified into this zone.
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
    /// Source zone the rule applies to.
    pub from: String,
    /// Target zones [`Self::from`] is allowed to import from. Self-imports
    /// are always allowed implicitly.
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
    /// Logical parent zone name as authored by the user.
    pub name: String,
    /// Discovered child zone names in stable directory-sorted order.
    pub children: Vec<String>,
    /// Verbatim `autoDiscover` strings from the user's config (not
    /// normalized) so round-trip tooling can match byte-for-byte.
    pub auto_discover: Vec<String>,
    /// Why [`Self::children`] is what it is.
    pub status: fallow_config::LogicalGroupStatus,
    /// Position of the parent zone in the user's pre-expansion `zones[]`.
    pub source_zone_index: usize,
    /// Sum of `file_count` across [`Self::children`] plus the fallback
    /// zone's `file_count` when present.
    pub file_count: usize,
    /// Pre-expansion rule keyed on the parent name, when the user wrote
    /// one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authored_rule: Option<fallow_config::AuthoredRule>,
    /// When the parent zone also carried explicit `patterns`, it stayed in
    /// [`BoundariesListing::zones`] as a fallback classifier; this is its
    /// name. Equal to [`Self::name`] when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_zone: Option<String>,
    /// Parent zone indices merged into this group when the user declared
    /// the same parent name multiple times.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merged_from: Option<Vec<usize>>,
    /// Echo of the parent zone's `root` (subtree scope) as the user wrote
    /// it. `None` when the parent had no `root` field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_zone_root: Option<String>,
    /// Parallel to [`Self::children`]: for child at index `i`, the index
    /// into [`Self::auto_discover`] of the path that produced it. Empty
    /// when only one path was authored (every child trivially maps to
    /// index 0). `serde(default)` keeps the schema's `required` array in
    /// step with the runtime's `skip_serializing_if` behavior.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub child_source_indices: Vec<usize>,
}

/// Typed root of every fallow `--format json` envelope shape that
/// serializes as a JSON object. The schema derived from this enum drives
/// the document-root `oneOf` in `docs/output-schema.json`, replacing the
/// previously hand-maintained block.
///
/// `#[serde(untagged)]` preserves wire compatibility: consumers see exactly
/// the same top-level keys today (`schema_version`, `version`, plus the
/// per-envelope shape). The schema's `oneOf` lets agents narrow by trying
/// variants in order; field sets differ enough that the first matching
/// variant is the correct one in practice. Note that [`HealthOutput`] and
/// [`DupesOutput`] flatten their inner body (`HealthReport` /
/// `DuplicationReport`) into top-level fields, so the actual
/// discriminators are nested-body keys such as `health_score` (health) and
/// `clone_groups` (dupes), NOT `report` or `groups`.
///
/// Variant order is **most-specific first**. Schemars 1 preserves
/// declaration order in the emitted `oneOf`, and validators that enforce
/// strict `oneOf` (and any future migration that adds `Deserialize`) will
/// try branches top-to-bottom. The required-field sets shrink as we move
/// down the list, with [`CombinedOutput`] last because its three required
/// fields (`schema_version`, `version`, `elapsed_ms`) are a strict subset
/// of every other variant's required set; placing it earlier would let a
/// `CheckOutput` payload silently match `CombinedOutput` first.
///
/// One envelope is intentionally NOT in this enum:
/// - `CodeClimateOutput` serializes as a bare JSON array
///   (`#[serde(transparent)]`) per the Code Climate / GitLab Code Quality
///   spec; `#[serde(tag = ...)]` cannot internally tag a non-object
///   variant and wrapping the array would break the spec. The root schema
///   carries it as a sibling `oneOf` branch alongside `FallowOutput`.
///
/// A future major release plans to switch this to
/// `#[serde(tag = "kind")]` for true O(1) discriminability on AI / agent
/// consumers, paired with a one-cycle `--legacy-envelope` opt-out flag.
/// Tracked under issue #384.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(
    feature = "schema",
    schemars(title = "fallow --format json (typed root)")
)]
#[serde(untagged)]
#[allow(
    dead_code,
    reason = "consumed at schema-emit time only; runtime code uses the per-variant envelope structs directly"
)]
pub enum FallowOutput {
    /// `fallow audit --format json`. Required `command: "audit"` singleton
    /// plus `verdict` and `summary`.
    Audit(AuditOutput),
    /// `fallow explain <issue-type> --format json`. Required `id`, `name`,
    /// `rationale`, `example`, `how_to_fix`, `docs`; no `schema_version`.
    Explain(ExplainOutput),
    /// `fallow --format review-github` / `--format review-gitlab`. Required
    /// `body`, `comments`, `meta`; no `schema_version`.
    ReviewEnvelope(ReviewEnvelopeOutput),
    /// `fallow ci reconcile-review --format json`. Required `schema`
    /// singleton plus `provider`, `comments`, and the various
    /// `*_fingerprints` arrays.
    ReviewReconcile(ReviewReconcileOutput),
    /// `fallow coverage setup --json`. Required `schema_version` singleton
    /// plus `framework_detected`, `members`, `commands`, `snippets`.
    CoverageSetup(CoverageSetupOutput),
    /// `fallow coverage analyze --format json`. Required
    /// `schema_version: "1"` singleton plus `version`, `elapsed_ms`,
    /// `runtime_coverage`. The `runtime_coverage` discriminator field is
    /// uniquely present here; ordered before broader variants so untagged
    /// narrowing matches `CoverageAnalyzeOutput` first.
    CoverageAnalyze(CoverageAnalyzeOutput),
    /// `fallow list --boundaries --format json`. Required `boundaries`
    /// sub-object; no `schema_version`.
    ListBoundaries(ListBoundariesOutput),
    /// `fallow health --format json`. Required `report: HealthReport`.
    Health(HealthOutput),
    /// `fallow dupes --format json`. Required `report: DupesReportPayload`
    /// (typed wrapper payload carrying `clone_groups[]: CloneGroupFinding`
    /// and `clone_families[]: CloneFamilyFinding`).
    Dupes(DupesOutput),
    /// `fallow check --format json --group-by <mode>`. Required `grouped_by`
    /// plus a `groups` array; ordered before [`Self::Check`] because the
    /// `grouped_by` discriminator field is uniquely present here.
    CheckGrouped(CheckGroupedOutput),
    /// `fallow impact --format json`. Required `enabled`, `record_count`,
    /// `containment_count`, `recent_containment`; no `schema_version`,
    /// `command`, `total_issues`, or `report`. Ordered before the broader
    /// variants because its `record_count` + `containment_count` discriminator
    /// pair is uniquely present here.
    Impact(crate::impact::ImpactReport),
    /// `fallow check --format json` / `fallow dead-code --format json`.
    /// Required `total_issues` plus `summary: CheckSummary`.
    Check(CheckOutput),
    /// Bare `fallow --format json` (combined dead-code + dupes + health).
    /// LAST because its required-field set (`schema_version`, `version`,
    /// `elapsed_ms`) is a strict subset of every other variant's required
    /// set; placing it earlier would let untagged narrowing match a
    /// `CheckOutput` payload against `CombinedOutput` first.
    Combined(CombinedOutput),
}
