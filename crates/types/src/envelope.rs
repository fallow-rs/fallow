//! Typed envelope and utility-shape structs for the JSON output contract.
//!
//! Today the JSON serialization layer (`crates/cli/src/report/json.rs`) builds
//! its envelopes (`CheckOutput`, `HealthOutput`, ...) via `serde_json::json!`
//! macros and ad-hoc map merging. The types in this module are the schema-side
//! counterpart of those envelopes plus a small set of utility shapes
//! (`SchemaVersion`, `Meta`, `BaselineDeltas`, ...) that the envelopes
//! reference.
//!
//! Gated on the `schema` cargo feature so consumers that do not need the
//! `schemars::JsonSchema` derive (every crate except `fallow-cli` with
//! `--features schema-emit`) skip the schemars compile cost.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::semantic::{
    ApiSurfaceResult, SemanticAnalysisIdentity, SemanticCandidateDecision, SemanticGapReason,
    SemanticQuerySummary, SemanticSymbolImpact, SemanticSymbolTrace, TypeCouplingReport,
};

/// Schema version for this output format (independent of tool version). Bump
/// policy: ADDITIVE changes (new optional top-level fields, new optional struct
/// fields, new array entries, new MCP tools, new CLI flags that map to new
/// optional fields) do NOT bump the version; consumers receive new fields
/// without breaking. BREAKING changes (renamed fields, removed fields, type
/// changes, enum-variant removals, semantic changes to existing fields) DO
/// bump. To detect newly-added fields without a bump, check field presence via
/// JSON-key existence rather than gating on the version. v4 was introduced
/// alongside fallow-cov-protocol 0.2 (per-finding verdict, stable IDs, evidence
/// block, renamed summary fields); v5 introduced health_score formula_version 2
/// with scale-invariant scoring semantics; v6 widened `AddToConfigAction.value`
/// from a scalar string to `oneOf: [string, array]` so the new `ignoreExports`
/// action can carry a paste-ready array of `{ file, exports }` rule objects
/// (the legacy `ignoreDependencies` etc. variants still emit strings, so
/// consumers that switch on `config_key` keep working unchanged). v8 added the
/// required duplication `spread` field and changed `duplicated_tokens` to count
/// redundant copies, excluding the retained copy in each group. The
/// runtime-coverage block is extended additively as the protocol evolves
/// (currently 0.3, which adds an optional capture_quality summary field). Other
/// additive examples: dupes --group-by adds optional grouped_by, total_issues,
/// groups fields without bumping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(transparent)]
pub struct SchemaVersion(pub u32);

/// Fallow CLI version that produced this envelope. Renders to the JSON wire as
/// a bare string (e.g. `"2.74.0"`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(transparent)]
pub struct ToolVersion(pub String);

/// Analysis duration in milliseconds. Renders to the JSON wire as a bare
/// integer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(transparent)]
pub struct ElapsedMs(pub u64);

/// Audit-mode marker emitted on each finding when `fallow audit --format json`
/// runs with a base ref. `true` means the finding's structural key was not
/// present at the base ref (introduced by the current changeset); `false`
/// means it was inherited.
///
/// Outside of audit sub-results the field is omitted, so call sites typically
/// hold `Option<AuditIntroduced>`. Renders to the JSON wire as a bare boolean.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(transparent)]
pub struct AuditIntroduced(pub bool);

/// Entry-point detection summary embedded in `CheckOutput` and the combined
/// envelope.
#[derive(Debug, Clone, Default, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct EntryPoints {
    /// Total number of detected entry points.
    pub total: usize,
    /// Breakdown of entry points by detection source (e.g., `"package.json"`,
    /// `"next.js"`, `"config entry"`). Underscored keys so dashboards can
    /// drill into individual sources.
    pub sources: BTreeMap<String, usize>,
}

/// Per-category issue counts for dead-code analysis. Always present in
/// `CheckOutput`; when `--summary` is used the individual issue arrays are
/// omitted but this object stays populated.
#[derive(Debug, Clone, Default, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CheckSummary {
    /// Total number of issues across all categories.
    pub total_issues: usize,
    /// Unused source files.
    pub unused_files: usize,
    /// Unused value exports.
    pub unused_exports: usize,
    /// Unused type exports.
    pub unused_types: usize,
    /// Public exports whose signature references same-file private types.
    pub private_type_leaks: usize,
    /// Combined count of unused entries across `dependencies`,
    /// `devDependencies`, and `optionalDependencies`. The per-section
    /// breakdown lives in the individual issue arrays on `CheckOutput`.
    pub unused_dependencies: usize,
    /// Unused enum members.
    pub unused_enum_members: usize,
    /// Unused class members.
    pub unused_class_members: usize,
    /// Unused store members.
    #[serde(default)]
    pub unused_store_members: usize,
    /// Vue/Svelte injects whose key is provided nowhere in the project.
    #[serde(default)]
    pub unprovided_injects: usize,
    /// Vue/Svelte components reachable but rendered nowhere in the project.
    #[serde(default)]
    pub unrendered_components: usize,
    /// Vue, Svelte, or React props referenced nowhere inside their own component.
    #[serde(default)]
    pub unused_component_props: usize,
    /// Vue `<script setup>` emits emitted nowhere inside their own SFC.
    #[serde(default)]
    pub unused_component_emits: usize,
    /// Angular `@Input()` bindings referenced nowhere inside their own component.
    #[serde(default)]
    pub unused_component_inputs: usize,
    /// Angular `@Output()` bindings emitted nowhere inside their own component.
    #[serde(default)]
    pub unused_component_outputs: usize,
    /// Svelte components dispatching a custom event via `createEventDispatcher`
    /// whose name is listened to nowhere in the project.
    #[serde(default)]
    pub unused_svelte_events: usize,
    /// Next.js Server Actions (exports of `"use server"` files) referenced by no
    /// code in the project.
    #[serde(default)]
    pub unused_server_actions: usize,
    /// SvelteKit `load()` return-object keys read by no consumer.
    #[serde(default)]
    pub unused_load_data_keys: usize,
    /// Imports that could not be resolved against the project's module graph.
    pub unresolved_imports: usize,
    /// Dependencies imported but absent from `package.json`.
    pub unlisted_dependencies: usize,
    /// Same-named exports declared in more than one module.
    pub duplicate_exports: usize,
    /// Production dependencies only used via type-only imports (could be
    /// devDependencies). Only populated in production mode.
    pub type_only_dependencies: usize,
    /// Production dependencies only imported by test files (could be
    /// devDependencies).
    pub test_only_dependencies: usize,
    /// devDependencies imported by production source code with a runtime/value
    /// import (should be promoted to dependencies).
    pub dev_dependencies_in_production: usize,
    /// Cycles detected in the import graph.
    pub circular_dependencies: usize,
    /// Cycles or self-loops in the re-export edge subgraph (barrel files
    /// re-exporting from each other in a loop).
    #[serde(default)]
    pub re_export_cycles: usize,
    /// Imports that cross architecture boundary rules.
    pub boundary_violations: usize,
    /// Files that match no architecture boundary zone.
    #[serde(default)]
    pub boundary_coverage_violations: usize,
    /// Calls from zoned files to callees forbidden for that zone.
    #[serde(default)]
    pub boundary_call_violations: usize,
    /// Banned calls, imports, and catalogue-derived effects matched by
    /// declarative rule packs.
    #[serde(default)]
    pub policy_violations: usize,
    /// Suppression comments that no longer match a finding.
    pub stale_suppressions: usize,
    /// Unused pnpm-workspace catalog entries.
    pub unused_catalog_entries: usize,
    /// Empty named catalog groups.
    pub empty_catalog_groups: usize,
    /// Workspace package.json catalog references the workspace catalogs
    /// do not declare.
    pub unresolved_catalog_references: usize,
    /// Pnpm `overrides:` entries whose target package is not declared by any
    /// workspace package and not present in the lockfile.
    pub unused_dependency_overrides: usize,
    /// Pnpm `overrides:` entries whose key or value cannot be parsed.
    pub misconfigured_dependency_overrides: usize,
    /// `"use client"` files that export a Next.js server-only / route-config name.
    #[serde(default)]
    pub invalid_client_exports: usize,
    /// Barrel files that re-export both a `"use client"` origin and a
    /// server-only origin.
    #[serde(default)]
    pub mixed_client_server_barrels: usize,
    /// Misplaced `"use client"` / `"use server"` directives written as
    /// expression statements after a non-directive statement.
    #[serde(default)]
    pub misplaced_directives: usize,
    /// Next.js App Router route files that resolve to the same URL within one
    /// app-root.
    #[serde(default)]
    pub route_collisions: usize,
    /// Sibling Next.js dynamic route segments at one position using different
    /// param spellings.
    #[serde(default)]
    pub dynamic_segment_name_conflicts: usize,
}

/// Per-category delta comparison against a saved baseline. Only present in
/// `CheckOutput` when `--baseline` is used.
#[derive(Debug, Clone, Default, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct BaselineDeltas {
    /// Net change in total issues vs baseline (positive = more issues).
    pub total_delta: i64,
    /// Per-category breakdown of current, baseline, and delta counts.
    pub per_category: BTreeMap<String, BaselineCategoryDelta>,
}

/// Single-category baseline delta entry inside [`BaselineDeltas::per_category`].
#[derive(Debug, Clone, Copy, Default, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct BaselineCategoryDelta {
    /// Current issue count for this category.
    pub current: usize,
    /// Baseline issue count for this category.
    pub baseline: usize,
    /// Change from baseline (current - baseline).
    pub delta: i64,
}

/// Baseline match statistics. Shows how many baseline entries existed and how
/// many matched current issues. Useful for detecting stale baselines
/// programmatically. Only present in `CheckOutput` when `--baseline` is used.
#[derive(Debug, Clone, Copy, Default, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct BaselineMatch {
    /// Total number of entries in the loaded baseline file.
    pub entries: usize,
    /// Number of baseline entries that matched current issues and were
    /// filtered.
    pub matched: usize,
}

/// Result of regression detection (`--fail-on-regression`). Compares current
/// issue counts against a baseline from config or an explicit file.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct RegressionResult {
    /// Outcome of the regression check.
    pub status: RegressionStatus,
    /// Baseline total before the change. Absent when status is `skipped`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_total: Option<i64>,
    /// Current total after the change. Absent when status is `skipped`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_total: Option<i64>,
    /// Difference current - baseline. Absent when status is `skipped`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delta: Option<i64>,
    /// Configured tolerance, interpreted per [`RegressionToleranceKind`].
    /// Absent when status is `skipped`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tolerance: Option<f64>,
    /// Interpretation of the tolerance value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tolerance_kind: Option<RegressionToleranceKind>,
    /// Whether the regression exceeded the tolerance.
    pub exceeded: bool,
    /// Only present when status is `skipped`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Status of a regression-check pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum RegressionStatus {
    /// Issue count within tolerance.
    Pass,
    /// Issue count exceeded tolerance.
    Exceeded,
    /// Regression check did not run (missing baseline, etc.).
    Skipped,
}

/// Interpretation of [`RegressionResult::tolerance`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum RegressionToleranceKind {
    /// Tolerance is interpreted as an absolute issue-count delta.
    Absolute,
    /// Tolerance is interpreted as a percentage of the baseline total.
    Percentage,
}

/// Metric and rule definitions emitted under `_meta` when `--explain` is
/// passed (always present in MCP responses). Helps AI agents and CI systems
/// interpret metric values without re-reading the docs site.
#[derive(Debug, Clone, Default, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Meta {
    /// URL to the documentation page for this command.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub docs: Option<String>,
    /// Local telemetry correlation metadata for agent follow-up runs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub telemetry: Option<TelemetryMeta>,
    /// Provenance for the opt-in TypeScript semantic analysis pass.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub type_aware: Option<TypeAwareMeta>,
    /// Per-field definitions for envelope fields and action payload fields.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub field_definitions: BTreeMap<String, String>,
    /// Per-metric definitions: name, description, range, interpretation.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metrics: BTreeMap<String, MetaMetric>,
    /// Per-rule definitions for check command output.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub rules: BTreeMap<String, MetaRule>,
}

/// Bounded provenance emitted when the opt-in type-aware pass runs.
#[derive(Debug, Clone, Default, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct TypeAwareMeta {
    /// Compatibility identity used by audit, baselines, snapshots, and stores.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity: Option<SemanticAnalysisIdentity>,
    /// Effective CLI or repository policy for incomplete semantic evidence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required_completeness: Option<crate::semantic::SemanticCompletenessRequirement>,
    /// Compact status for every requested semantic query.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub queries: Vec<SemanticQuerySummary>,
    /// Bounded decision and evidence for each semantic dead-code candidate.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub candidate_decisions: Vec<SemanticCandidateDecision>,
    /// Checker-backed trace evidence requested by focused symbol queries.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub symbol_traces: Vec<SemanticSymbolTrace>,
    /// Package-public surface and confirmed private type leaks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_surface: Option<ApiSurfaceResult>,
    /// Exact-symbol blast radius and targeted-test recommendations.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub symbol_impacts: Vec<SemanticSymbolImpact>,
    /// Advisory project-local public-signature coupling.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub type_coupling: Option<TypeCouplingReport>,
    /// Whether the semantic companion executed at least one query.
    pub executed: bool,
    /// Version of Fallow's backend-neutral sidecar protocol.
    pub protocol_version: u32,
    /// Version of the sidecar package that executed the query.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sidecar_version: Option<String>,
    /// Semantic backend capability identifier.
    pub backend: String,
    /// Backend compiler or engine version that executed the query.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend_version: Option<String>,
    /// TypeScript project configs selected for candidate files.
    pub selected_tsconfigs: Vec<String>,
    /// Number of candidate findings sent to the sidecar.
    pub candidate_count: usize,
    /// Number of candidates confirmed as used and removed.
    pub confirmed_used_count: usize,
    /// Number of candidates preserved because they implement or override a contract.
    pub contract_preserved_count: usize,
    /// Number of candidates with complete, closed-world no-static-reference evidence.
    pub no_static_references_count: usize,
    /// Number of retained class members eligible for a guarded type-aware fix.
    pub fix_eligible_count: usize,
    /// Number of candidates retained because semantic use was unresolved.
    pub unresolved_count: usize,
    /// Number of candidates retained because semantic analysis abstained.
    pub abstained_count: usize,
    /// Stable abstention reason counts for automation and diagnostics.
    pub abstention_reasons: TypeAwareAbstentionCounts,
    /// Per-project semantic refinement status and evidence.
    pub projects: Vec<TypeAwareProjectMeta>,
    /// Number of bounded warnings returned by the sidecar.
    pub warning_count: usize,
    /// Bounded semantic warnings. Findings mentioned here were retained.
    pub warnings: Vec<String>,
    /// Semantic pass duration as reported by the sidecar.
    pub elapsed_ms: u64,
    /// Bounded semantic phase timings reported by the sidecar.
    pub phase_timings_ms: TypeAwarePhaseTimings,
}

/// Closed set of reasons for retaining a candidate without semantic scanning.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum TypeAwareAbstentionReason {
    /// No selected TypeScript project contains the candidate file.
    #[default]
    NoProject,
    /// Multiple explicit TypeScript projects contain the candidate file.
    AmbiguousProject,
    /// Structural TypeScript diagnostics make exact matching unsafe.
    BlockingDiagnostics,
}

/// Closed abstention reason counts for stable machine consumption.
#[derive(Debug, Clone, Default, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct TypeAwareAbstentionCounts {
    /// Candidates not contained by a selected TypeScript project.
    pub no_project: usize,
    /// Candidates contained by more than one explicit TypeScript project.
    pub ambiguous_project: usize,
    /// Candidates retained because structural diagnostics block scanning.
    pub blocking_diagnostics: usize,
    /// Candidates whose exact declaration identity could not be resolved.
    pub unknown_symbol: usize,
    /// Candidates using declaration syntax unsupported by the semantic backend.
    pub unsupported_syntax: usize,
    /// Candidates retained because the bounded semantic request reached capacity.
    pub capacity: usize,
}

/// How a TypeScript project was selected for semantic refinement.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum TypeAwareProjectSource {
    /// Fallow selected the nearest discovered project automatically.
    #[default]
    Auto,
    /// The project was supplied with `--type-aware-project`.
    Explicit,
}

/// Outcome of semantic refinement for one TypeScript project.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum TypeAwareProjectStatus {
    /// The project was structurally safe and its candidates were scanned.
    #[default]
    Refined,
    /// Structural diagnostics prevented candidate scanning.
    Abstained,
    /// All semantic queries assigned to this Program completed.
    Complete,
    /// The Program could not answer its assigned semantic queries safely.
    Unavailable,
}

/// How a persistent semantic snapshot was refreshed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum TypeAwareInvalidationKind {
    /// The backend rebuilt project state from a clean snapshot.
    Full,
    /// The backend applied an explicit source-file change set.
    Incremental,
    /// No filesystem change was reported between compatible requests.
    None,
}

/// Semantic sidecar timings, separated from Fallow's syntactic pipeline.
#[derive(Debug, Clone, Default, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct TypeAwarePhaseTimings {
    /// TypeScript API construction and project snapshot selection.
    pub project_setup: u64,
    /// TypeScript diagnostics collected before any candidate refinement.
    pub diagnostics: u64,
    /// Batched symbol lookup and exact declaration matching.
    pub symbol_scan: u64,
}

/// Bounded provenance for one TypeScript project handled by the sidecar.
#[derive(Debug, Clone, Default, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct TypeAwareProjectMeta {
    /// Project config relative to the analysis root, or `<inferred>`.
    pub config: String,
    /// How the project was selected: `auto` or `explicit`.
    pub source: TypeAwareProjectSource,
    /// Project result: `refined`, `abstained`, `complete`, or `unavailable`.
    pub status: TypeAwareProjectStatus,
    /// Candidates assigned to this project.
    pub candidate_count: usize,
    /// Candidates confirmed as used and removed.
    pub confirmed_used_count: usize,
    /// Candidates retained because they implement or override a contract.
    pub contract_preserved_count: usize,
    /// Candidates with complete no-static-reference evidence.
    pub no_static_references_count: usize,
    /// Candidates eligible for a guarded class-member fix.
    pub fix_eligible_count: usize,
    /// Candidates whose exact semantic outcome remained unresolved.
    pub unresolved_count: usize,
    /// Candidates retained without scanning because the project was unsafe.
    pub abstained_count: usize,
    /// Config, program, syntactic, and bind diagnostics that block scanning.
    pub blocking_diagnostic_count: usize,
    /// Source files loaded into this TypeScript program.
    pub source_file_count: usize,
    /// Whether this Program served more than one semantic query in the batch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub program_reused: Option<bool>,
    /// Whether this Program served more than one query in the current batch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub program_shared_across_queries: Option<bool>,
    /// Whether the root-bound semantic session reused the prior snapshot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub program_reused_from_previous_snapshot: Option<bool>,
    /// Monotonic revision within the root-bound semantic session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot_revision: Option<u64>,
    /// Full, incremental, or no invalidation before this query.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invalidation_kind: Option<TypeAwareInvalidationKind>,
    /// Stable project-level gap reason.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason_code: Option<SemanticGapReason>,
    /// Stable reason code when `status` is `abstained`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schema", schemars(with = "TypeAwareAbstentionReason"))]
    pub abstain_reason: Option<TypeAwareAbstentionReason>,
}

/// Privacy-safe local run metadata emitted for JSON consumers.
#[derive(Debug, Clone, Default, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct TelemetryMeta {
    /// Ephemeral local token that may be passed to the hidden `--parent-run`
    /// flag on a later command. It is not derived from repository, path, user,
    /// machine, project, or cloud data.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub analysis_run_id: Option<String>,
}

/// Single-metric definition inside [`Meta::metrics`].
#[derive(Debug, Clone, Default, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MetaMetric {
    /// Human-readable metric name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// What this metric measures and how it is computed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Valid value range (e.g., `"[0, 100]"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range: Option<String>,
    /// How to read the value (e.g., `"lower is better"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interpretation: Option<String>,
}

/// Single-rule definition inside [`Meta::rules`].
#[derive(Debug, Clone, Default, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MetaRule {
    /// Human-readable rule name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// What this rule detects.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// URL to the rule documentation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub docs: Option<String>,
}
