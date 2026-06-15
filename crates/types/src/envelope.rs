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

use serde::Serialize;

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
/// consumers that switch on `config_key` keep working unchanged). The
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
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
    /// Vue `<script setup>` props referenced nowhere inside their own SFC.
    #[serde(default)]
    pub unused_component_props: usize,
    /// Vue `<script setup>` emits emitted nowhere inside their own SFC.
    #[serde(default)]
    pub unused_component_emits: usize,
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
    /// Banned calls and banned imports matched by declarative rule packs.
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
