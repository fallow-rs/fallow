//! Analysis result types for all issue categories.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::extract::{
    MemberKind, SecurityControlKind, SecurityUrlShape, SkippedSecurityCalleeExpressionKind,
    SkippedSecurityCalleeReason,
};
use crate::output::{
    FixAction, FixActionType, IssueAction, SuppressLineAction, SuppressLineKind, SuppressLineScope,
};
use crate::output_dead_code::{
    BoundaryCallViolationFinding, BoundaryCoverageViolationFinding, BoundaryViolationFinding,
    CircularDependencyFinding, DuplicateExportFinding, DuplicatePropShapeFinding,
    DynamicSegmentNameConflictFinding, EmptyCatalogGroupFinding, InvalidClientExportFinding,
    MisconfiguredDependencyOverrideFinding, MisplacedDirectiveFinding,
    MixedClientServerBarrelFinding, PolicyViolationFinding, PrivateTypeLeakFinding,
    PropDrillingChainFinding, ReExportCycleFinding, RouteCollisionFinding,
    TestOnlyDependencyFinding, ThinWrapperFinding, TypeOnlyDependencyFinding,
    UnlistedDependencyFinding, UnprovidedInjectFinding, UnrenderedComponentFinding,
    UnresolvedCatalogReferenceFinding, UnresolvedImportFinding, UnusedCatalogEntryFinding,
    UnusedClassMemberFinding, UnusedComponentEmitFinding, UnusedComponentInputFinding,
    UnusedComponentOutputFinding, UnusedComponentPropFinding, UnusedDependencyFinding,
    UnusedDependencyOverrideFinding, UnusedDevDependencyFinding, UnusedEnumMemberFinding,
    UnusedExportFinding, UnusedFileFinding, UnusedLoadDataKeyFinding,
    UnusedOptionalDependencyFinding, UnusedServerActionFinding, UnusedStoreMemberFinding,
    UnusedSvelteEventFinding, UnusedTypeFinding,
};
use crate::serde_path;
use crate::suppress::{IssueKind, closest_known_kind_name};

/// Summary of detected entry points, grouped by discovery source.
///
/// Used to surface entry-point detection status in human and JSON output,
/// so library authors can verify that fallow found the right entry points.
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct EntryPointSummary {
    /// Total number of entry points detected.
    pub total: usize,
    /// Breakdown by source category (e.g., "package.json" -> 3, "plugin" -> 12).
    /// Sorted by key for deterministic output.
    pub by_source: Vec<(String, usize)>,
}

/// Per-component render fan-in counts plus the precomputed concentration
/// aggregates.
///
/// DESCRIPTIVE blast-radius signal (NOT a rule, finding, or threshold): the
/// component-graph analogue of module-level fan-in. Module fan-in counts
/// importing MODULES; render fan-in counts JSX render CALL SITES (a shared
/// `<Button>` is rendered in far more places than it is imported).
///
/// `per_component` is the internal carrier (keyed for hotspot path annotation),
/// `#[serde(skip)]` on [`AnalysisResults`] so it never appears under bare
/// `fallow` / `audit`; the aggregates feed the descriptive `VitalSigns` block
/// (`p95_render_fan_in` / `render_fan_in_high_pct` / `max_render_fan_in`).
///
/// UNDERCOUNT is the documented safe direction: a child rendered via a JSX
/// spread, a dynamic / `createElement(var)` form, or a member-expression tag
/// (`<Lib.Button/>`) is not resolved by the shared `ChildResolver` and so
/// increments no component's fan-in. A true high-fan-in component can only be
/// undersold, never falsely flagged. A rare name-collision over-credit is
/// possible via the default-import sole-component fallback (inherited verbatim
/// from the prop-drilling / thin-wrapper resolver); low-harm for a descriptive,
/// non-gating metric.
#[derive(Debug, Clone, Default)]
pub struct RenderFanInMetric {
    /// Per-component render-site + distinct-parent counts. Keyed by
    /// `(component file path, component name)` so the hotspot surface can map a
    /// file back to its top component's fan-in. Components rendered nowhere ARE
    /// included as a real `0` so the percentile distribution is not skewed.
    pub per_component: Vec<RenderFanInComponent>,
    /// 95th-percentile DISTINCT-PARENTS render fan-in across components (the
    /// per-component distribution analogue of the module-fan-in p95). `None` on
    /// an empty population. Mirrors `compute_coupling_concentration`.
    pub p95_distinct_parents: Option<u32>,
    /// Percentage of components whose distinct-parents render fan-in exceeds the
    /// `max(p95, 10)` threshold (the same floor coupling concentration uses).
    /// `None` on an empty population.
    pub high_pct: Option<f64>,
    /// The single highest DISTINCT-PARENTS count across all components (the
    /// headline blast-radius number: the most distinct render LOCATIONS any one
    /// component is rendered from, the honest edit-ripple count). `None` on an
    /// empty population. `render_sites` (incl. repeats) is secondary per-component
    /// context, never the headline.
    pub max_distinct_parents: Option<u32>,
}

/// One component's render fan-in detail: how many JSX render SITES target it and
/// how many DISTINCT parent components render it.
#[derive(Debug, Clone)]
pub struct RenderFanInComponent {
    /// Absolute path of the file declaring the component.
    pub file: PathBuf,
    /// The component name.
    pub component: String,
    /// Total JSX render SITES that resolve to this component across the project
    /// (each capitalized / member JSX tag is one site). SECONDARY context ("incl.
    /// repeats"): a single parent rendering one child five times is five sites but
    /// one distinct parent, so render_sites overcounts blast radius.
    pub render_sites: u32,
    /// Distinct `(parent_file, parent_component)` keys that render this
    /// component. The HEADLINE blast-radius axis: the honest count of distinct
    /// render LOCATIONS, the percentiled distribution analogue of "distinct
    /// importers".
    pub distinct_parents: u32,
}

/// Complete analysis results.
///
/// # Examples
///
/// ```
/// use fallow_types::output_dead_code::UnusedFileFinding;
/// use fallow_types::results::{AnalysisResults, UnusedFile};
/// use std::path::PathBuf;
///
/// let mut results = AnalysisResults::default();
/// assert_eq!(results.total_issues(), 0);
/// assert!(!results.has_issues());
///
/// results
///     .unused_files
///     .push(UnusedFileFinding::with_actions(UnusedFile {
///         path: PathBuf::from("src/dead.ts"),
///     }));
/// assert_eq!(results.total_issues(), 1);
/// assert!(results.has_issues());
/// ```
#[derive(Debug, Default, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct AnalysisResults {
    /// Files not reachable from any entry point. Wrapped in
    /// [`UnusedFileFinding`] so each entry carries a typed `actions` array
    /// natively, replacing the pre-2.76 post-pass injection.
    pub unused_files: Vec<UnusedFileFinding>,
    /// Exports never imported by other modules. Wrapped in
    /// [`UnusedExportFinding`] so each entry carries a typed `actions`
    /// array natively.
    pub unused_exports: Vec<UnusedExportFinding>,
    /// Type exports never imported by other modules. Wrapped in
    /// [`UnusedTypeFinding`]: the inner [`UnusedExport`] struct is shared
    /// with `unused_exports` but the wrapper emits a type-targeted fix
    /// description.
    pub unused_types: Vec<UnusedTypeFinding>,
    /// Exported symbols whose public signature references same-file private
    /// types. Wrapped in [`PrivateTypeLeakFinding`] so each entry carries a
    /// typed `actions` array natively.
    pub private_type_leaks: Vec<PrivateTypeLeakFinding>,
    /// Dependencies listed in package.json but never imported. Wrapped in
    /// [`UnusedDependencyFinding`] so each entry carries a typed `actions`
    /// array natively. The fix action swaps from `remove-dependency` to
    /// `move-dependency` when `used_in_workspaces` is non-empty.
    pub unused_dependencies: Vec<UnusedDependencyFinding>,
    /// Dev dependencies listed in package.json but never imported. Wrapped
    /// in [`UnusedDevDependencyFinding`]: same bare struct as
    /// `unused_dependencies` with a `devDependencies`-targeted fix
    /// description.
    pub unused_dev_dependencies: Vec<UnusedDevDependencyFinding>,
    /// Optional dependencies listed in package.json but never imported.
    /// Wrapped in [`UnusedOptionalDependencyFinding`] with an
    /// `optionalDependencies`-targeted fix description.
    pub unused_optional_dependencies: Vec<UnusedOptionalDependencyFinding>,
    /// Enum members never accessed. Wrapped in
    /// [`UnusedEnumMemberFinding`] so each entry carries a typed `actions`
    /// array natively.
    pub unused_enum_members: Vec<UnusedEnumMemberFinding>,
    /// Class members never accessed. Wrapped in
    /// [`UnusedClassMemberFinding`]: same inner [`UnusedMember`] struct as
    /// `unused_enum_members`, with a class-targeted fix description and the
    /// `auto_fixable: false` default to reflect dependency-injection
    /// patterns.
    pub unused_class_members: Vec<UnusedClassMemberFinding>,
    /// Store members (Pinia `state` / `getters` / `actions` key, or a
    /// setup-store returned key) declared but never accessed by any consumer
    /// project-wide. Wrapped in [`UnusedStoreMemberFinding`]: same inner
    /// [`UnusedMember`] struct as `unused_class_members`, with a
    /// store-targeted fix description. Cross-graph: the store binding is
    /// imported (the module is reachable) yet a specific member is dead.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unused_store_members: Vec<UnusedStoreMemberFinding>,
    /// Import specifiers that could not be resolved. Wrapped in
    /// [`UnresolvedImportFinding`] so each entry carries a typed `actions`
    /// array natively.
    pub unresolved_imports: Vec<UnresolvedImportFinding>,
    /// Dependencies used in code but not listed in package.json. Wrapped in
    /// [`UnlistedDependencyFinding`].
    pub unlisted_dependencies: Vec<UnlistedDependencyFinding>,
    /// Exports with the same name across multiple modules. Wrapped in
    /// [`DuplicateExportFinding`] so each entry carries a typed `actions`
    /// array natively, with the position-0 `add-to-config` `ignoreExports`
    /// snippet wired in at wrapper construction.
    pub duplicate_exports: Vec<DuplicateExportFinding>,
    /// Production dependencies only used via type-only imports (could be
    /// devDependencies). Only populated in production mode. Wrapped in
    /// [`TypeOnlyDependencyFinding`].
    pub type_only_dependencies: Vec<TypeOnlyDependencyFinding>,
    /// Production dependencies only imported by test files (could be
    /// devDependencies). Wrapped in [`TestOnlyDependencyFinding`].
    #[serde(default)]
    pub test_only_dependencies: Vec<TestOnlyDependencyFinding>,
    /// Circular dependency chains detected in the module graph. Wrapped in
    /// [`CircularDependencyFinding`] so each entry carries a typed `actions`
    /// array natively.
    pub circular_dependencies: Vec<CircularDependencyFinding>,
    /// Cycles or self-loops in the re-export edge subgraph (barrel files
    /// re-exporting from each other in a loop). Wrapped in
    /// [`ReExportCycleFinding`] so each entry carries a typed `actions`
    /// array natively (a `refactor-re-export-cycle` informational primary
    /// plus a `suppress-file` secondary; cycles are file-scoped so a single
    /// suppression breaks the cycle).
    #[serde(default)]
    pub re_export_cycles: Vec<ReExportCycleFinding>,
    /// Imports that cross architecture boundary rules. Wrapped in
    /// [`BoundaryViolationFinding`] so each entry carries a typed `actions`
    /// array natively.
    #[serde(default)]
    pub boundary_violations: Vec<BoundaryViolationFinding>,
    /// Files that matched no architecture boundary zone while
    /// `boundaries.coverage.requireAllFiles` was enabled.
    #[serde(default)]
    pub boundary_coverage_violations: Vec<BoundaryCoverageViolationFinding>,
    /// Calls from zoned files to callees forbidden for that zone via
    /// `boundaries.calls.forbidden`. Wrapped in
    /// [`BoundaryCallViolationFinding`] so each entry carries a typed
    /// `actions` array natively.
    #[serde(default)]
    pub boundary_call_violations: Vec<BoundaryCallViolationFinding>,
    /// Banned calls and banned imports matched by declarative rule packs
    /// (`rulePacks` config). Wrapped in [`PolicyViolationFinding`] so each
    /// entry carries a typed `actions` array natively. Each finding carries
    /// its effective per-rule severity.
    #[serde(default)]
    pub policy_violations: Vec<PolicyViolationFinding>,
    /// Suppression comments or JSDoc tags that no longer match any issue.
    #[serde(default)]
    pub stale_suppressions: Vec<StaleSuppression>,
    /// Entries in package manager catalog sections not referenced by any
    /// workspace package via the catalog: protocol. Supports
    /// `pnpm-workspace.yaml` catalogs and Bun root `package.json` catalogs.
    /// Wrapped in [`UnusedCatalogEntryFinding`] so each entry carries a typed
    /// `actions` array natively, with per-instance `auto_fixable` derived
    /// from `hardcoded_consumers` and the catalog source file.
    #[serde(default)]
    pub unused_catalog_entries: Vec<UnusedCatalogEntryFinding>,
    /// Named groups under package manager catalogs sections that declare no
    /// package entries. The top-level catalog: map is not reported. Wrapped in
    /// [`EmptyCatalogGroupFinding`].
    #[serde(default)]
    pub empty_catalog_groups: Vec<EmptyCatalogGroupFinding>,
    /// Workspace package.json references to catalogs (`catalog:` or
    /// `catalog:<name>`) that do not declare the consumed package. The package
    /// manager install will error until the named catalog grows to include the
    /// package or the reference is switched / removed. Wrapped in
    /// [`UnresolvedCatalogReferenceFinding`] with the discriminated
    /// `add-catalog-entry` / `update-catalog-reference` primary at position 0.
    #[serde(default)]
    pub unresolved_catalog_references: Vec<UnresolvedCatalogReferenceFinding>,
    /// Entries in pnpm-workspace.yaml's overrides: section, or package.json's
    /// pnpm.overrides block, whose target package is not declared by any
    /// workspace package and is not present in pnpm-lock.yaml. Default severity
    /// is warn because projects without a readable lockfile fall back to
    /// manifest-only checks; the hint field flags those conservative cases.
    /// Wrapped in [`UnusedDependencyOverrideFinding`].
    #[serde(default)]
    pub unused_dependency_overrides: Vec<UnusedDependencyOverrideFinding>,
    /// pnpm.overrides entries whose key or value does not parse as a valid
    /// override spec (empty key, empty value, malformed selector, unbalanced
    /// parent matcher). pnpm install will reject these. Default severity is
    /// error. Wrapped in [`MisconfiguredDependencyOverrideFinding`].
    #[serde(default)]
    pub misconfigured_dependency_overrides: Vec<MisconfiguredDependencyOverrideFinding>,
    /// `"use client"` files that export a Next.js server-only / route-segment
    /// config name (e.g. `metadata`, `revalidate`, `GET`). Next.js rejects this
    /// at build time. Wrapped in [`InvalidClientExportFinding`] so each entry
    /// carries a typed `actions` array natively. Default severity is `warn`.
    #[serde(default)]
    pub invalid_client_exports: Vec<InvalidClientExportFinding>,
    /// Barrel files that re-export BOTH a `"use client"` origin module AND a
    /// server-only origin module (the Next.js App Router footgun). Wrapped in
    /// [`MixedClientServerBarrelFinding`] so each entry carries a typed
    /// `actions` array natively. Default severity is `warn`.
    #[serde(default)]
    pub mixed_client_server_barrels: Vec<MixedClientServerBarrelFinding>,
    /// `"use client"` / `"use server"` directives written as expression
    /// statements after a non-directive statement, so the RSC bundler parses
    /// them as ordinary strings and silently ignores them. Wrapped in
    /// [`MisplacedDirectiveFinding`] so each entry carries a typed `actions`
    /// array natively. Default severity is `warn`.
    #[serde(default)]
    pub misplaced_directives: Vec<MisplacedDirectiveFinding>,
    /// Vue `inject(KEY)` / Svelte `getContext(KEY)` calls whose symbol KEY is
    /// provided nowhere in the project (the injected-never-provided dead-half).
    /// Wrapped in [`UnprovidedInjectFinding`] so each entry carries a typed
    /// `actions` array natively. Default severity is `warn`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unprovided_injects: Vec<UnprovidedInjectFinding>,
    /// Vue/Svelte single-file components that are reachable but rendered nowhere
    /// (the imported-but-never-rendered dead-half). Wrapped in
    /// [`UnrenderedComponentFinding`] so each entry carries a typed `actions`
    /// array natively. Default severity is `warn`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unrendered_components: Vec<UnrenderedComponentFinding>,
    /// Next.js App Router route files that resolve to the same URL within one
    /// app-root (a guaranteed `next build` failure). Wrapped in
    /// [`RouteCollisionFinding`] so each entry carries a typed `actions` array
    /// natively. One finding per colliding file. Default severity is `warn`.
    #[serde(default)]
    pub route_collisions: Vec<RouteCollisionFinding>,
    /// Sibling Next.js dynamic route segments at one tree position using
    /// different param spellings (a dev / runtime error; `next build` does NOT
    /// catch it). Wrapped in [`DynamicSegmentNameConflictFinding`] so each entry
    /// carries a typed `actions` array natively. Default severity is `warn`.
    #[serde(default)]
    pub dynamic_segment_name_conflicts: Vec<DynamicSegmentNameConflictFinding>,
    /// Vue `<script setup>` `defineProps`, Svelte 5 `$props()`, and React props
    /// referenced nowhere in their own component. Wrapped in
    /// [`UnusedComponentPropFinding`] so each entry carries a typed `actions`
    /// array natively. Default severity is `warn`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unused_component_props: Vec<UnusedComponentPropFinding>,
    /// Vue `<script setup>` `defineEmits` events emitted nowhere in their own SFC
    /// (no `emit('<name>')` call). Wrapped in [`UnusedComponentEmitFinding`] so
    /// each entry carries a typed `actions` array natively. Default severity is
    /// `warn`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unused_component_emits: Vec<UnusedComponentEmitFinding>,
    /// Angular `@Input()` / signal `input()` / `model()` inputs read nowhere in
    /// their own component (neither the template nor the class body). Wrapped in
    /// [`UnusedComponentInputFinding`] so each entry carries a typed `actions`
    /// array natively. Default severity is `warn`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unused_component_inputs: Vec<UnusedComponentInputFinding>,
    /// Angular `@Output()` / signal `output()` outputs emitted nowhere in their
    /// own component (no `this.<output>.emit(...)`). Wrapped in
    /// [`UnusedComponentOutputFinding`] so each entry carries a typed `actions`
    /// array natively. Default severity is `warn`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unused_component_outputs: Vec<UnusedComponentOutputFinding>,
    /// Svelte components dispatching a custom event via `createEventDispatcher()`
    /// whose event name is listened to nowhere project-wide (cross-file
    /// dead-output direction). Wrapped in [`UnusedSvelteEventFinding`] so each
    /// entry carries a typed `actions` array natively. Default severity is
    /// `warn`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unused_svelte_events: Vec<UnusedSvelteEventFinding>,
    /// Next.js Server Actions (exports of `"use server"` files) that no code in
    /// the project references. Reclassified out of `unused_exports` for
    /// `"use server"` files. Wrapped in [`UnusedServerActionFinding`] so each
    /// entry carries a typed `actions` array natively. Default severity is
    /// `warn`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unused_server_actions: Vec<UnusedServerActionFinding>,
    /// SvelteKit `+page.{ts,server.ts,js,server.js}` `load()` return-object keys
    /// read by no consumer. Wrapped in [`UnusedLoadDataKeyFinding`] so each entry
    /// carries a typed `actions` array natively. Default severity is `warn`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unused_load_data_keys: Vec<UnusedLoadDataKeyFinding>,
    /// `true` when the `unused-load-data-key` detector abstained project-wide
    /// because a whole-object use of `page.data` / `$page.data` was seen
    /// somewhere (S1 observability: an empty `unused_load_data_keys` with this
    /// flag set is NOT a clean bill, it means the rule could not run safely).
    /// Serialized only when `true` so the default JSON contract is unchanged.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub unused_load_data_keys_global_abstain: bool,
    /// React/Preact props forwarded unchanged through `>= N` intermediate
    /// pass-through components until a consumer (located per-chain records).
    /// Wrapped in [`PropDrillingChainFinding`] so each entry carries a typed
    /// `actions` array natively. Health signal: the rule defaults to `off`
    /// (opt-in), so this is dormant and populated ONLY when the user enables it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub prop_drilling_chains: Vec<PropDrillingChainFinding>,
    /// React/Preact components whose entire body is a single spread-forwarded
    /// child render (`return <Child {...props}/>`): pure structural indirection,
    /// a candidate for inlining at call sites. Wrapped in [`ThinWrapperFinding`]
    /// so each entry carries a typed `actions` array natively. Health signal: the
    /// rule defaults to `off` (opt-in), so this is dormant and populated ONLY
    /// when the user enables it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub thin_wrappers: Vec<ThinWrapperFinding>,
    /// React/Preact components that participate in a duplicate-prop-shape group:
    /// three or more components across two or more files whose statically-known
    /// prop NAME set is identical after stripping ubiquitous DOM / passthrough
    /// names (a missing shared `Props` type / base component). Wrapped in
    /// [`DuplicatePropShapeFinding`] so each entry carries a typed `actions`
    /// array and its sibling roster natively. Health signal: the rule defaults to
    /// `off` (opt-in), so this is dormant and populated ONLY when the user
    /// enables it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub duplicate_prop_shapes: Vec<DuplicatePropShapeFinding>,
    /// Number of suppression entries that matched an issue during analysis.
    /// Human output uses this for the suppression footer; it is skipped in
    /// machine output to avoid changing the public JSON issue contract.
    #[serde(skip)]
    pub suppression_count: usize,
    /// Suppression comments present in analyzed files this run (every present
    /// marker, all kinds, not only consumed ones). Internal: read in-process by
    /// `fallow impact` to distinguish a genuinely resolved finding from one
    /// silenced by a `fallow-ignore`. Skipped during serialization, like
    /// [`Self::suppression_count`], so the public JSON output contract is
    /// unchanged.
    #[serde(skip)]
    pub active_suppressions: Vec<ActiveSuppression>,
    /// Detected feature flag patterns. Advisory output, not included in issue counts.
    /// Skipped during default serialization: injected separately in JSON output when enabled.
    #[serde(skip)]
    pub feature_flags: Vec<FeatureFlag>,
    /// Local security candidates (e.g. `client-server-leak`). CANDIDATES for
    /// downstream agent verification, NOT verified vulnerabilities. Off by
    /// default; populated only when the corresponding `security_*` rule is
    /// enabled (forced on by `fallow security`). Excluded from `total_issues`
    /// and skipped during serialization so they never surface under bare
    /// `fallow` or the `audit` gate; the `fallow security` command reads this
    /// field and emits its own envelope. Mirrors [`Self::feature_flags`].
    #[serde(skip)]
    pub security_findings: Vec<SecurityFinding>,
    /// In-band blind-spot count: number of `"use client"` files whose transitive
    /// import cone contains a dynamic `import()` the reachability BFS cannot
    /// follow. Surfaced by `fallow security` so a leak hidden behind an
    /// unresolved edge is never silently reported as "clean". Skipped during
    /// serialization like [`Self::security_findings`].
    #[serde(skip)]
    pub security_unresolved_edge_files: usize,
    /// In-band blind-spot count: number of sink-shaped nodes the catalogue
    /// detector could not flatten to a static callee path (dynamic dispatch,
    /// computed members, aliased bindings). Surfaced by `fallow security` so an
    /// empty catalogue result with a non-zero count is not reported as "clean".
    /// Skipped during serialization like [`Self::security_findings`].
    #[serde(skip)]
    pub security_unresolved_callee_sites: usize,
    /// Location samples for sink-shaped nodes the catalogue detector could not
    /// flatten to a static callee path. Skipped during default serialization;
    /// `fallow security` summarizes this metadata in its own envelope.
    #[serde(skip)]
    pub security_unresolved_callee_diagnostics: Vec<SecurityUnresolvedCalleeDiagnostic>,
    /// Usage counts for all exports across the project. Used by the LSP for Code Lens.
    /// Not included in issue counts -- this is metadata, not an issue type.
    /// Skipped during serialization: this is internal LSP data, not part of the JSON output schema.
    #[serde(skip)]
    pub export_usages: Vec<ExportUsage>,
    /// Summary of detected entry points, grouped by discovery source.
    /// Not included in issue counts -- this is informational metadata.
    /// Skipped during serialization: rendered separately in JSON output.
    #[serde(skip)]
    pub entry_point_summary: Option<EntryPointSummary>,
    /// Per-component render fan-in (JSX render SITES + distinct parents) plus the
    /// precomputed concentration aggregates. DESCRIPTIVE blast-radius signal, not
    /// an issue type: the component-graph analogue of module fan-in. `None` on
    /// non-React projects (the dep gate fails and `render_edges` is empty).
    /// Skipped during serialization (internal carrier, like
    /// [`Self::export_usages`]); the public surface is the `VitalSigns`
    /// aggregate, so bare `fallow` / `audit` never serialize it. See
    /// [`RenderFanInMetric`].
    #[serde(skip)]
    pub render_fan_in: Option<RenderFanInMetric>,
}

impl AnalysisResults {
    /// Total number of issues found.
    ///
    /// Sums across all issue categories (unused files, exports, types,
    /// dependencies, members, unresolved imports, unlisted deps, duplicates,
    /// type-only deps, circular deps, and boundary violations).
    ///
    /// # Examples
    ///
    /// ```
    /// use fallow_types::output_dead_code::{UnresolvedImportFinding, UnusedFileFinding};
    /// use fallow_types::results::{AnalysisResults, UnresolvedImport, UnusedFile};
    /// use std::path::PathBuf;
    ///
    /// let mut results = AnalysisResults::default();
    /// results
    ///     .unused_files
    ///     .push(UnusedFileFinding::with_actions(UnusedFile {
    ///         path: PathBuf::from("a.ts"),
    ///     }));
    /// results
    ///     .unresolved_imports
    ///     .push(UnresolvedImportFinding::with_actions(UnresolvedImport {
    ///         path: PathBuf::from("b.ts"),
    ///         specifier: "./missing".to_string(),
    ///         line: 1,
    ///         col: 0,
    ///         specifier_col: 0,
    ///     }));
    /// assert_eq!(results.total_issues(), 2);
    /// ```
    #[must_use]
    pub const fn total_issues(&self) -> usize {
        self.unused_files.len()
            + self.unused_exports.len()
            + self.unused_types.len()
            + self.private_type_leaks.len()
            + self.unused_dependencies.len()
            + self.unused_dev_dependencies.len()
            + self.unused_optional_dependencies.len()
            + self.unused_enum_members.len()
            + self.unused_class_members.len()
            + self.unused_store_members.len()
            + self.unresolved_imports.len()
            + self.unlisted_dependencies.len()
            + self.duplicate_exports.len()
            + self.type_only_dependencies.len()
            + self.test_only_dependencies.len()
            + self.circular_dependencies.len()
            + self.re_export_cycles.len()
            + self.boundary_violations.len()
            + self.boundary_coverage_violations.len()
            + self.boundary_call_violations.len()
            + self.policy_violations.len()
            + self.stale_suppressions.len()
            + self.unused_catalog_entries.len()
            + self.empty_catalog_groups.len()
            + self.unresolved_catalog_references.len()
            + self.unused_dependency_overrides.len()
            + self.misconfigured_dependency_overrides.len()
            + self.invalid_client_exports.len()
            + self.mixed_client_server_barrels.len()
            + self.misplaced_directives.len()
            + self.unprovided_injects.len()
            + self.unrendered_components.len()
            + self.route_collisions.len()
            + self.dynamic_segment_name_conflicts.len()
            + self.unused_component_props.len()
            + self.unused_component_emits.len()
            + self.unused_component_inputs.len()
            + self.unused_component_outputs.len()
            + self.unused_svelte_events.len()
            + self.unused_server_actions.len()
            + self.unused_load_data_keys.len()
    }

    /// Whether any issues were found.
    #[must_use]
    pub const fn has_issues(&self) -> bool {
        self.total_issues() > 0
    }

    /// Merge `other` into `self`, taking the union of every field.
    ///
    /// This is the single canonical way to combine two [`AnalysisResults`]
    /// (the LSP merges per-project-root results through it). The method
    /// exhaustively destructures `Self`, so adding a field to the struct
    /// becomes a compile error here instead of a silently-dropped field. See
    /// issue #444.
    ///
    /// Every `Vec` field is appended (callers dedup downstream where needed,
    /// e.g. the LSP's identity-keyed `dedup_results`). `suppression_count`
    /// sums; `entry_point_summary` keeps `self`'s value when present and
    /// otherwise adopts `other`'s.
    pub fn merge_into(&mut self, other: Self) {
        let Self {
            unused_files,
            unused_exports,
            unused_types,
            private_type_leaks,
            unused_dependencies,
            unused_dev_dependencies,
            unused_optional_dependencies,
            unused_enum_members,
            unused_class_members,
            unused_store_members,
            unresolved_imports,
            unlisted_dependencies,
            duplicate_exports,
            type_only_dependencies,
            test_only_dependencies,
            circular_dependencies,
            re_export_cycles,
            boundary_violations,
            boundary_coverage_violations,
            boundary_call_violations,
            policy_violations,
            stale_suppressions,
            unused_catalog_entries,
            empty_catalog_groups,
            unresolved_catalog_references,
            unused_dependency_overrides,
            misconfigured_dependency_overrides,
            invalid_client_exports,
            mixed_client_server_barrels,
            misplaced_directives,
            unprovided_injects,
            unrendered_components,
            route_collisions,
            dynamic_segment_name_conflicts,
            unused_component_props,
            unused_component_emits,
            unused_component_inputs,
            unused_component_outputs,
            unused_svelte_events,
            unused_server_actions,
            unused_load_data_keys,
            unused_load_data_keys_global_abstain,
            prop_drilling_chains,
            thin_wrappers,
            duplicate_prop_shapes,
            suppression_count,
            active_suppressions,
            feature_flags,
            security_findings,
            security_unresolved_edge_files,
            security_unresolved_callee_sites,
            security_unresolved_callee_diagnostics,
            export_usages,
            entry_point_summary,
            render_fan_in,
        } = other;

        self.unused_files.extend(unused_files);
        self.unused_exports.extend(unused_exports);
        self.unused_types.extend(unused_types);
        self.private_type_leaks.extend(private_type_leaks);
        self.unused_dependencies.extend(unused_dependencies);
        self.unused_dev_dependencies.extend(unused_dev_dependencies);
        self.unused_optional_dependencies
            .extend(unused_optional_dependencies);
        self.unused_enum_members.extend(unused_enum_members);
        self.unused_class_members.extend(unused_class_members);
        self.unused_store_members.extend(unused_store_members);
        self.unresolved_imports.extend(unresolved_imports);
        self.unlisted_dependencies.extend(unlisted_dependencies);
        self.duplicate_exports.extend(duplicate_exports);
        self.type_only_dependencies.extend(type_only_dependencies);
        self.test_only_dependencies.extend(test_only_dependencies);
        self.circular_dependencies.extend(circular_dependencies);
        self.re_export_cycles.extend(re_export_cycles);
        self.boundary_violations.extend(boundary_violations);
        self.boundary_coverage_violations
            .extend(boundary_coverage_violations);
        self.boundary_call_violations
            .extend(boundary_call_violations);
        self.policy_violations.extend(policy_violations);
        self.stale_suppressions.extend(stale_suppressions);
        self.unused_catalog_entries.extend(unused_catalog_entries);
        self.empty_catalog_groups.extend(empty_catalog_groups);
        self.unresolved_catalog_references
            .extend(unresolved_catalog_references);
        self.unused_dependency_overrides
            .extend(unused_dependency_overrides);
        self.misconfigured_dependency_overrides
            .extend(misconfigured_dependency_overrides);
        self.invalid_client_exports.extend(invalid_client_exports);
        self.mixed_client_server_barrels
            .extend(mixed_client_server_barrels);
        self.misplaced_directives.extend(misplaced_directives);
        self.unprovided_injects.extend(unprovided_injects);
        self.unrendered_components.extend(unrendered_components);
        self.route_collisions.extend(route_collisions);
        self.dynamic_segment_name_conflicts
            .extend(dynamic_segment_name_conflicts);
        self.unused_component_props.extend(unused_component_props);
        self.unused_component_emits.extend(unused_component_emits);
        self.unused_component_inputs.extend(unused_component_inputs);
        self.unused_component_outputs
            .extend(unused_component_outputs);
        self.unused_svelte_events.extend(unused_svelte_events);
        self.unused_server_actions.extend(unused_server_actions);
        self.unused_load_data_keys.extend(unused_load_data_keys);
        self.unused_load_data_keys_global_abstain |= unused_load_data_keys_global_abstain;
        self.prop_drilling_chains.extend(prop_drilling_chains);
        self.thin_wrappers.extend(thin_wrappers);
        self.duplicate_prop_shapes.extend(duplicate_prop_shapes);
        self.feature_flags.extend(feature_flags);
        self.security_findings.extend(security_findings);
        self.security_unresolved_edge_files += security_unresolved_edge_files;
        self.security_unresolved_callee_sites += security_unresolved_callee_sites;
        self.security_unresolved_callee_diagnostics
            .extend(security_unresolved_callee_diagnostics);
        self.export_usages.extend(export_usages);
        self.active_suppressions.extend(active_suppressions);
        self.suppression_count += suppression_count;
        if self.entry_point_summary.is_none() {
            self.entry_point_summary = entry_point_summary;
        }
        if self.render_fan_in.is_none() {
            self.render_fan_in = render_fan_in;
        }
    }

    /// Sort all result arrays for deterministic output ordering.
    ///
    /// Parallel collection (rayon, `FxHashMap` iteration) does not guarantee
    /// insertion order, so the same project can produce different orderings
    /// across runs. This method canonicalises every result list by sorting on
    /// (path, line, col, name) so that JSON/SARIF/human output is stable.
    pub fn sort(&mut self) {
        self.sort_core_findings();
        self.sort_dependency_findings();
        self.sort_graph_findings();
        self.sort_catalog_findings();
        self.sort_metadata_findings();
        self.sort_export_usages();
    }

    fn sort_core_findings(&mut self) {
        self.sort_core_declaration_findings();
        self.sort_core_member_findings();
        self.sort_core_framework_findings();
        self.sort_core_route_and_load_findings();
    }

    fn sort_core_declaration_findings(&mut self) {
        self.unused_files
            .sort_by(|a, b| a.file.path.cmp(&b.file.path));

        self.unused_exports.sort_by(|a, b| {
            a.export
                .path
                .cmp(&b.export.path)
                .then(a.export.line.cmp(&b.export.line))
                .then(a.export.export_name.cmp(&b.export.export_name))
        });

        self.unused_types.sort_by(|a, b| {
            a.export
                .path
                .cmp(&b.export.path)
                .then(a.export.line.cmp(&b.export.line))
                .then(a.export.export_name.cmp(&b.export.export_name))
        });

        self.private_type_leaks.sort_by(|a, b| {
            a.leak
                .path
                .cmp(&b.leak.path)
                .then(a.leak.line.cmp(&b.leak.line))
                .then(a.leak.export_name.cmp(&b.leak.export_name))
                .then(a.leak.type_name.cmp(&b.leak.type_name))
        });

        self.unused_dependencies.sort_by(|a, b| {
            a.dep
                .path
                .cmp(&b.dep.path)
                .then(a.dep.line.cmp(&b.dep.line))
                .then(a.dep.package_name.cmp(&b.dep.package_name))
        });

        self.unused_dev_dependencies.sort_by(|a, b| {
            a.dep
                .path
                .cmp(&b.dep.path)
                .then(a.dep.line.cmp(&b.dep.line))
                .then(a.dep.package_name.cmp(&b.dep.package_name))
        });

        self.unused_optional_dependencies.sort_by(|a, b| {
            a.dep
                .path
                .cmp(&b.dep.path)
                .then(a.dep.line.cmp(&b.dep.line))
                .then(a.dep.package_name.cmp(&b.dep.package_name))
        });
    }

    fn sort_core_member_findings(&mut self) {
        self.unused_enum_members.sort_by(|a, b| {
            a.member
                .path
                .cmp(&b.member.path)
                .then(a.member.line.cmp(&b.member.line))
                .then(a.member.parent_name.cmp(&b.member.parent_name))
                .then(a.member.member_name.cmp(&b.member.member_name))
        });

        self.unused_class_members.sort_by(|a, b| {
            a.member
                .path
                .cmp(&b.member.path)
                .then(a.member.line.cmp(&b.member.line))
                .then(a.member.parent_name.cmp(&b.member.parent_name))
                .then(a.member.member_name.cmp(&b.member.member_name))
        });

        self.unused_store_members.sort_by(|a, b| {
            a.member
                .path
                .cmp(&b.member.path)
                .then(a.member.line.cmp(&b.member.line))
                .then(a.member.parent_name.cmp(&b.member.parent_name))
                .then(a.member.member_name.cmp(&b.member.member_name))
        });

        self.unresolved_imports.sort_by(|a, b| {
            a.import
                .path
                .cmp(&b.import.path)
                .then(a.import.line.cmp(&b.import.line))
                .then(a.import.col.cmp(&b.import.col))
                .then(a.import.specifier.cmp(&b.import.specifier))
        });
    }

    fn sort_core_framework_findings(&mut self) {
        self.invalid_client_exports.sort_by(|a, b| {
            a.export
                .path
                .cmp(&b.export.path)
                .then(a.export.line.cmp(&b.export.line))
                .then(a.export.export_name.cmp(&b.export.export_name))
        });

        self.mixed_client_server_barrels.sort_by(|a, b| {
            a.barrel
                .path
                .cmp(&b.barrel.path)
                .then(a.barrel.line.cmp(&b.barrel.line))
                .then(a.barrel.client_origin.cmp(&b.barrel.client_origin))
                .then(a.barrel.server_origin.cmp(&b.barrel.server_origin))
        });

        self.misplaced_directives.sort_by(|a, b| {
            a.directive_site
                .path
                .cmp(&b.directive_site.path)
                .then(a.directive_site.line.cmp(&b.directive_site.line))
                .then(a.directive_site.col.cmp(&b.directive_site.col))
                .then(a.directive_site.directive.cmp(&b.directive_site.directive))
        });

        self.unprovided_injects.sort_by(|a, b| {
            a.inject
                .path
                .cmp(&b.inject.path)
                .then(a.inject.line.cmp(&b.inject.line))
                .then(a.inject.col.cmp(&b.inject.col))
                .then(a.inject.key_name.cmp(&b.inject.key_name))
        });

        self.unrendered_components.sort_by(|a, b| {
            a.component
                .path
                .cmp(&b.component.path)
                .then(a.component.line.cmp(&b.component.line))
                .then(a.component.col.cmp(&b.component.col))
                .then(a.component.component_name.cmp(&b.component.component_name))
        });
    }

    fn sort_core_route_and_load_findings(&mut self) {
        self.route_collisions.sort_by(|a, b| {
            a.collision
                .path
                .cmp(&b.collision.path)
                .then(a.collision.url.cmp(&b.collision.url))
        });

        self.dynamic_segment_name_conflicts.sort_by(|a, b| {
            a.conflict
                .path
                .cmp(&b.conflict.path)
                .then(a.conflict.position.cmp(&b.conflict.position))
        });

        self.unused_component_props.sort_by(|a, b| {
            a.prop
                .path
                .cmp(&b.prop.path)
                .then(a.prop.line.cmp(&b.prop.line))
                .then(a.prop.prop_name.cmp(&b.prop.prop_name))
        });

        self.unused_component_emits.sort_by(|a, b| {
            a.emit
                .path
                .cmp(&b.emit.path)
                .then(a.emit.line.cmp(&b.emit.line))
                .then(a.emit.emit_name.cmp(&b.emit.emit_name))
        });

        self.unused_component_inputs.sort_by(|a, b| {
            a.input
                .path
                .cmp(&b.input.path)
                .then(a.input.line.cmp(&b.input.line))
                .then(a.input.input_name.cmp(&b.input.input_name))
        });

        self.unused_component_outputs.sort_by(|a, b| {
            a.output
                .path
                .cmp(&b.output.path)
                .then(a.output.line.cmp(&b.output.line))
                .then(a.output.output_name.cmp(&b.output.output_name))
        });

        self.unused_svelte_events.sort_by(|a, b| {
            a.event
                .path
                .cmp(&b.event.path)
                .then(a.event.line.cmp(&b.event.line))
                .then(a.event.event_name.cmp(&b.event.event_name))
        });

        self.unused_server_actions.sort_by(|a, b| {
            a.action
                .path
                .cmp(&b.action.path)
                .then(a.action.line.cmp(&b.action.line))
                .then(a.action.col.cmp(&b.action.col))
                .then(a.action.action_name.cmp(&b.action.action_name))
        });

        self.unused_load_data_keys.sort_by(|a, b| {
            a.key
                .path
                .cmp(&b.key.path)
                .then(a.key.line.cmp(&b.key.line))
                .then(a.key.col.cmp(&b.key.col))
                .then(a.key.key_name.cmp(&b.key.key_name))
        });
    }

    /// Sort prop-drilling chains by their source hop (first hop): file, line,
    /// prop, depth, for deterministic output. Split out of `sort_core_findings`
    /// to keep that function under the unit-size ceiling.
    fn sort_prop_drilling_chains(&mut self) {
        self.prop_drilling_chains.sort_by(|a, b| {
            let a_src = a.chain.hops.first();
            let b_src = b.chain.hops.first();
            let a_file = a_src.map(|h| &h.file);
            let b_file = b_src.map(|h| &h.file);
            a_file
                .cmp(&b_file)
                .then_with(|| a_src.map(|h| h.line).cmp(&b_src.map(|h| h.line)))
                .then(a.chain.prop.cmp(&b.chain.prop))
                .then(a.chain.depth.cmp(&b.chain.depth))
        });
    }

    /// Sort thin-wrapper findings by file, line, then component for
    /// deterministic output.
    fn sort_thin_wrappers(&mut self) {
        self.thin_wrappers.sort_by(|a, b| {
            a.wrapper
                .file
                .cmp(&b.wrapper.file)
                .then(a.wrapper.line.cmp(&b.wrapper.line))
                .then(a.wrapper.component.cmp(&b.wrapper.component))
        });
    }

    /// Sort duplicate-prop-shape findings by the shared shape first (so a
    /// group's members stay adjacent), then file, line, and component, for
    /// deterministic output.
    fn sort_duplicate_prop_shapes(&mut self) {
        self.duplicate_prop_shapes.sort_by(|a, b| {
            a.shape
                .shape
                .cmp(&b.shape.shape)
                .then(a.shape.file.cmp(&b.shape.file))
                .then(a.shape.line.cmp(&b.shape.line))
                .then(a.shape.component.cmp(&b.shape.component))
        });
    }

    fn sort_dependency_findings(&mut self) {
        self.unlisted_dependencies
            .sort_by(|a, b| a.dep.package_name.cmp(&b.dep.package_name));
        for dep in &mut self.unlisted_dependencies {
            dep.dep
                .imported_from
                .sort_by(|a, b| a.path.cmp(&b.path).then(a.line.cmp(&b.line)));
        }

        self.duplicate_exports
            .sort_by(|a, b| a.export.export_name.cmp(&b.export.export_name));
        for dup in &mut self.duplicate_exports {
            dup.export
                .locations
                .sort_by(|a, b| a.path.cmp(&b.path).then(a.line.cmp(&b.line)));
        }

        self.type_only_dependencies.sort_by(|a, b| {
            a.dep
                .path
                .cmp(&b.dep.path)
                .then(a.dep.line.cmp(&b.dep.line))
                .then(a.dep.package_name.cmp(&b.dep.package_name))
        });

        self.test_only_dependencies.sort_by(|a, b| {
            a.dep
                .path
                .cmp(&b.dep.path)
                .then(a.dep.line.cmp(&b.dep.line))
                .then(a.dep.package_name.cmp(&b.dep.package_name))
        });
    }

    fn sort_graph_findings(&mut self) {
        self.circular_dependencies.sort_by(|a, b| {
            a.cycle
                .files
                .cmp(&b.cycle.files)
                .then(a.cycle.length.cmp(&b.cycle.length))
        });

        self.re_export_cycles
            .sort_by(|a, b| a.cycle.files.cmp(&b.cycle.files));

        self.boundary_violations.sort_by(|a, b| {
            a.violation
                .from_path
                .cmp(&b.violation.from_path)
                .then(a.violation.line.cmp(&b.violation.line))
                .then(a.violation.col.cmp(&b.violation.col))
                .then(a.violation.to_path.cmp(&b.violation.to_path))
        });

        self.boundary_coverage_violations.sort_by(|a, b| {
            a.violation
                .path
                .cmp(&b.violation.path)
                .then(a.violation.line.cmp(&b.violation.line))
                .then(a.violation.col.cmp(&b.violation.col))
        });

        self.boundary_call_violations.sort_by(|a, b| {
            a.violation
                .path
                .cmp(&b.violation.path)
                .then(a.violation.line.cmp(&b.violation.line))
                .then(a.violation.col.cmp(&b.violation.col))
                .then(a.violation.callee.cmp(&b.violation.callee))
        });

        self.policy_violations.sort_by(|a, b| {
            a.violation
                .path
                .cmp(&b.violation.path)
                .then(a.violation.line.cmp(&b.violation.line))
                .then(a.violation.col.cmp(&b.violation.col))
                .then(a.violation.rule_id.cmp(&b.violation.rule_id))
        });
    }

    fn sort_catalog_findings(&mut self) {
        self.stale_suppressions.sort_by(|a, b| {
            a.path
                .cmp(&b.path)
                .then(a.line.cmp(&b.line))
                .then(a.col.cmp(&b.col))
        });

        self.unused_catalog_entries.sort_by(|a, b| {
            a.entry
                .path
                .cmp(&b.entry.path)
                .then_with(|| {
                    catalog_sort_key(&a.entry.catalog_name)
                        .cmp(&catalog_sort_key(&b.entry.catalog_name))
                })
                .then(a.entry.catalog_name.cmp(&b.entry.catalog_name))
                .then(a.entry.entry_name.cmp(&b.entry.entry_name))
        });
        for finding in &mut self.unused_catalog_entries {
            finding.entry.hardcoded_consumers.sort();
            finding.entry.hardcoded_consumers.dedup();
        }

        self.empty_catalog_groups.sort_by(|a, b| {
            a.group
                .path
                .cmp(&b.group.path)
                .then_with(|| {
                    catalog_sort_key(&a.group.catalog_name)
                        .cmp(&catalog_sort_key(&b.group.catalog_name))
                })
                .then(a.group.catalog_name.cmp(&b.group.catalog_name))
                .then(a.group.line.cmp(&b.group.line))
        });

        self.unresolved_catalog_references.sort_by(|a, b| {
            a.reference
                .path
                .cmp(&b.reference.path)
                .then(a.reference.line.cmp(&b.reference.line))
                .then_with(|| {
                    catalog_sort_key(&a.reference.catalog_name)
                        .cmp(&catalog_sort_key(&b.reference.catalog_name))
                })
                .then(a.reference.catalog_name.cmp(&b.reference.catalog_name))
                .then(a.reference.entry_name.cmp(&b.reference.entry_name))
        });
        for finding in &mut self.unresolved_catalog_references {
            finding.reference.available_in_catalogs.sort();
            finding.reference.available_in_catalogs.dedup();
        }

        self.unused_dependency_overrides.sort_by(|a, b| {
            a.entry
                .path
                .cmp(&b.entry.path)
                .then(a.entry.line.cmp(&b.entry.line))
                .then(a.entry.raw_key.cmp(&b.entry.raw_key))
        });
    }

    fn sort_metadata_findings(&mut self) {
        self.sort_prop_drilling_chains();
        self.sort_thin_wrappers();
        self.sort_duplicate_prop_shapes();

        self.misconfigured_dependency_overrides.sort_by(|a, b| {
            a.entry
                .path
                .cmp(&b.entry.path)
                .then(a.entry.line.cmp(&b.entry.line))
                .then(a.entry.raw_key.cmp(&b.entry.raw_key))
        });

        self.feature_flags.sort_by(|a, b| {
            a.path
                .cmp(&b.path)
                .then(a.line.cmp(&b.line))
                .then(a.flag_name.cmp(&b.flag_name))
        });

        self.security_unresolved_callee_diagnostics.sort_by(|a, b| {
            a.path
                .cmp(&b.path)
                .then(a.line.cmp(&b.line))
                .then(a.col.cmp(&b.col))
                .then(a.reason.cmp(&b.reason))
                .then(a.expression_kind.cmp(&b.expression_kind))
        });
    }

    fn sort_export_usages(&mut self) {
        for usage in &mut self.export_usages {
            usage.reference_locations.sort_by(|a, b| {
                a.path
                    .cmp(&b.path)
                    .then(a.line.cmp(&b.line))
                    .then(a.col.cmp(&b.col))
            });
        }
        self.export_usages.sort_by(|a, b| {
            a.path
                .cmp(&b.path)
                .then(a.line.cmp(&b.line))
                .then(a.export_name.cmp(&b.export_name))
        });
    }
}

/// Sort key for catalog names: the default catalog ("default") sorts before any named catalog.
fn catalog_sort_key(name: &str) -> (u8, &str) {
    if name == "default" {
        (0, name)
    } else {
        (1, name)
    }
}

/// A file that is not reachable from any entry point.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct UnusedFile {
    /// Absolute path to the unused file.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
}

/// An export that is never imported by other modules.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct UnusedExport {
    /// File containing the unused export.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// Name of the unused export.
    pub export_name: String,
    /// Whether this is a type-only export.
    pub is_type_only: bool,
    /// 1-based line number of the export.
    pub line: u32,
    /// 0-based byte column offset.
    pub col: u32,
    /// Byte offset into the source file (used by the fix command).
    pub span_start: u32,
    /// Whether this finding comes from a barrel/index re-export rather than the source definition.
    pub is_re_export: bool,
}

/// A public export signature that references a same-file private type.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct PrivateTypeLeak {
    /// File containing the exported symbol.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// Export whose public signature leaks the private type.
    pub export_name: String,
    /// Private type referenced by the public signature.
    pub type_name: String,
    /// 1-based line number of the leaking type reference.
    pub line: u32,
    /// 0-based byte column offset.
    pub col: u32,
    /// Byte offset of the type reference.
    pub span_start: u32,
}

/// A `"use client"` file that exports a Next.js server-only / route-segment
/// config name. Next.js rejects this combination at build time; fallow catches
/// it statically before the build runs.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct InvalidClientExport {
    /// File carrying the `"use client"` directive and the illegal export.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// Name of the server-only / route-config export that is illegal in a
    /// client file (e.g. `metadata`, `generateMetadata`, `revalidate`, `GET`).
    pub export_name: String,
    /// The file-level directive that makes the export illegal. Always
    /// `"use client"` today; carried so the message can name it verbatim.
    pub directive: String,
    /// 1-based line number of the export.
    pub line: u32,
    /// 0-based byte column offset of the export.
    pub col: u32,
}

/// A barrel file that re-exports BOTH a `"use client"` origin module AND a
/// server-only origin module. Importing one name from such a barrel drags the
/// other's directive context across the React Server Components boundary (the
/// Next.js App Router footgun); fallow catches it statically.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MixedClientServerBarrel {
    /// The barrel file re-exporting both a client and a server-only origin.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// The `"use client"` origin's relative path or specifier as written in the
    /// barrel's offending re-export.
    pub client_origin: String,
    /// The server-only origin's relative path or specifier as written in the
    /// barrel's offending re-export.
    pub server_origin: String,
    /// 1-based line number of the barrel's first offending re-export.
    pub line: u32,
    /// 0-based byte column offset of the barrel's first offending re-export.
    pub col: u32,
}

/// A `"use client"` / `"use server"` directive written as an expression
/// statement after a non-directive statement (an import, a const). The RSC
/// bundler only honors a directive in the leading prologue, so once any
/// statement precedes it the string is parsed as an ordinary expression and
/// silently ignored: the intended client/server boundary never takes effect.
/// The fix is to move the directive to the very top of the file.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MisplacedDirective {
    /// The file carrying the misplaced directive.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// The directive string as written, either `"use client"` or
    /// `"use server"` (without the surrounding quotes).
    pub directive: String,
    /// 1-based line number of the misplaced directive statement.
    pub line: u32,
    /// 0-based byte column offset of the misplaced directive statement.
    pub col: u32,
}

/// A Vue `inject(KEY)` or Svelte `getContext(KEY)` whose symbol KEY is
/// `provide`/`setContext`'d nowhere in the analyzed project. The key is a
/// symbol with cross-file identity, so an unmatched key is a real dead-half DI
/// link: at runtime the inject returns `undefined`, surfaced only at render.
/// The fix is binary: provide the key somewhere, or remove the dead inject.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct UnprovidedInject {
    /// The file carrying the orphan inject / getContext call.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// The injected key identifier as written at the call site.
    pub key_name: String,
    /// Which framework's DI API this came from: `"vue"` or `"svelte"`.
    pub framework: String,
    /// 1-based line number of the inject / getContext call.
    pub line: u32,
    /// 0-based byte column offset of the inject / getContext call.
    pub col: u32,
}

/// A Next.js Server Action (an export of a `"use server"` file) that no code in
/// the analyzed project references: no import-and-call, no `action={fn}` JSX
/// binding, no `<form action={fn}>`. This is the cross-graph "declared but zero
/// consumers" direction, reclassified out of `unused-export` for `"use server"`
/// files so the finding carries the action-specific signal. It does NOT mean the
/// endpoint is unreachable: Next still registers the action id, so it stays
/// POST-able. It means no project code calls it (likely forgotten / dead, and a
/// candidate for removal to shrink surface area).
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct UnusedServerAction {
    /// The `"use server"` file that exports the unreferenced action.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// The exported action name as written, or `"default"` for a default export.
    pub action_name: String,
    /// 1-based line number of the export.
    pub line: u32,
    /// 0-based byte column offset of the export.
    pub col: u32,
}

/// A SvelteKit `+page.{ts,server.ts,js,server.js}` `load()` return-object key
/// read by no consumer: not off the sibling `+page.svelte`'s `data.<key>`, nor
/// project-wide via `page.data.<key>` / `$page.data.<key>`. A dead load key runs
/// a real server/DB fetch cost on every request for data nothing renders. The
/// fix is a human call (delete the key, or wire a consumer): a load fetch may
/// have side effects, so there is no safe auto-fix.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct UnusedLoadDataKey {
    /// The producer `+page.{ts,server.ts,js,server.js}` file declaring the key.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// The returned-object key name read by no consumer.
    pub key_name: String,
    /// 1-based line number of the key in the return object.
    pub line: u32,
    /// 0-based byte column offset of the key.
    pub col: u32,
    /// The route directory relative to the project root (`src/routes/blog`), for
    /// agent remediation and per-route trend aggregation. `None` when not
    /// determinable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route_dir: Option<String>,
}

/// A Vue/Svelte single-file component (the default export of a `.vue`/`.svelte`
/// file) that is reachable in the module graph but rendered NOWHERE in the
/// project: no `<Tag>`, no `:is`/`this=` binding, no `components`/`app.component`
/// registration, no `h()`/auto-import use, and no script value-read. It survives
/// `unused-file` (a barrel re-export keeps it reachable) and `unused-export`
/// (the re-export counts as a use), yet no file actually instantiates it.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct UnrenderedComponent {
    /// The component file that is reachable but rendered nowhere.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// The component name (the `.vue`/`.svelte` file stem, PascalCase).
    pub component_name: String,
    /// Which framework this component belongs to: `"vue"` or `"svelte"`.
    pub framework: String,
    /// A barrel/file that re-exports this component, kept for the remediation
    /// trace ("reachable via X, rendered nowhere"). Absolute in memory,
    /// serialized workspace-relative (like `path`); `None` when not determinable.
    #[serde(
        serialize_with = "serde_path::serialize_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub reachable_via: Option<PathBuf>,
    /// 1-based line number of the component (the file head; SFCs have no explicit
    /// default-export statement).
    pub line: u32,
    /// 0-based byte column offset.
    pub col: u32,
}

/// A Vue `<script setup>` `defineProps`, Svelte 5 `$props()`, or React declared
/// prop that is referenced NOWHERE inside its own component. Single-component
/// finding, zero-FP doctrine: the component abstains on any opaque public or
/// fallthrough signal.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct UnusedComponentProp {
    /// The component file declaring the unused prop.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// The component name.
    pub component_name: String,
    /// The declared prop name that is never referenced.
    pub prop_name: String,
    /// 1-based line number of the prop declaration.
    pub line: u32,
    /// 0-based byte column offset of the prop declaration.
    pub col: u32,
}

/// A Vue `<script setup>` `defineEmits` declared event that is EMITTED nowhere
/// inside its own single-file component (no `emit('<name>')` call). Single-file
/// finding, zero-FP doctrine: the whole file abstains on any
/// unharvestable / dynamic-emit / whole-object-use / `defineModel` signal.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct UnusedComponentEmit {
    /// The `.vue` SFC declaring the unused emit.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// The component name (the `.vue` file stem).
    pub component_name: String,
    /// The declared emit event name that is never emitted.
    pub emit_name: String,
    /// 1-based line number of the emit declaration.
    pub line: u32,
    /// 0-based byte column offset of the emit declaration.
    pub col: u32,
}

/// A Svelte component dispatching a custom event via `createEventDispatcher()`
/// whose event name is listened to NOWHERE in the analyzed project. Cross-file
/// dead-output direction: the component fires an event nothing handles.
/// Zero-FP doctrine: the whole component abstains on any dynamic-dispatch or
/// whole-`dispatch`-value signal, and a listener on ANY component anywhere
/// credits the event name (the liberal over-credit direction).
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct UnusedSvelteEvent {
    /// The `.svelte` component dispatching the unlistened event.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// The component name (the `.svelte` file stem).
    pub component_name: String,
    /// The dispatched event name that is listened to nowhere.
    pub event_name: String,
    /// 1-based line number of the `dispatch('<name>')` call.
    pub line: u32,
    /// 0-based byte column offset of the `dispatch('<name>')` call.
    pub col: u32,
}

/// One hop in a prop-drilling chain: a component that received the prop and
/// passed it along (or, at the chain ends, the source that owns it and the
/// consumer that substantively reads it).
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct PropDrillHop {
    /// The file containing this hop's component.
    #[serde(serialize_with = "serde_path::serialize")]
    pub file: PathBuf,
    /// 1-based line of the component definition (or the prop declaration at the
    /// source hop). Anchors a jump-to-source for the agent.
    pub line: u32,
    /// The component name at this hop.
    pub component: String,
}

/// A located prop-drilling chain: a received prop forwarded unchanged through
/// `>= N` intermediate pass-through components, each of which only re-passes it,
/// until a component that substantively consumes it. The high-confidence signal
/// is "the received identifier is used ONLY as the root of forwarded child-JSX
/// attribute values", not the attribute name matching. Health signal (rule
/// defaults to `off`, opt-in): a small capped penalty plus a `health --hotspots`
/// surface, and located per-chain records so CI / an agent can act ("colocate or
/// lift to context at hop B"). Zero-FP doctrine: any spread / `cloneElement` /
/// element-as-prop / render-prop / context-provider / dynamic shape in the path
/// abstains the whole chain.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct PropDrillingChain {
    /// The drilled prop name as declared at the chain SOURCE.
    pub prop: String,
    /// The chain depth = the number of components the prop is forwarded THROUGH
    /// (source + intermediates + consumer = `hops.len()`). Always `>= N`.
    pub depth: u32,
    /// The ordered hop trail from source to consumer. The first hop owns the
    /// prop, the middle hops are pass-throughs, the last hop consumes it. The
    /// finding anchor is the first hop (`path` / `line` for suppression + CI).
    pub hops: Vec<PropDrillHop>,
}

/// A located thin-wrapper / passthrough component: a React/Preact component
/// whose entire body is `return <Child {...props}/>` (a single spread-forwarded
/// child render, no host wrapper, no own value-add). It is pure structural
/// indirection, a CANDIDATE for inlining at call sites or deleting. Health
/// signal (rule defaults to `off`, opt-in): never a correctness error. Zero-FP
/// doctrine: `forwardRef` / `memo` / exported / context-provider /
/// `cloneElement` / render-prop / named-attr / unresolved-child wrappers all
/// abstain (each is an intentional indirection or unprovable shape).
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ThinWrapper {
    /// The file containing the wrapper component.
    #[serde(serialize_with = "serde_path::serialize")]
    pub file: PathBuf,
    /// 1-based line of the wrapper component definition (the finding anchor for
    /// jump-to-source and line-level suppression).
    pub line: u32,
    /// The wrapper component name.
    pub component: String,
    /// The single child component the wrapper forwards its props to (as written
    /// at the render site).
    pub child_component: String,
}

/// One member of a duplicate-prop-shape group: the OTHER components that share
/// the same significant prop-name set, listed in each member's
/// `sharing_components`. Path-sorted for stable output. A located reference (no
/// `shape`, which is carried once on the owning [`DuplicatePropShape`]).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct DuplicatePropShapeMember {
    /// The file containing the sibling component.
    #[serde(serialize_with = "serde_path::serialize")]
    pub file: PathBuf,
    /// 1-based line of the sibling component definition.
    pub line: u32,
    /// The sibling component name.
    pub component: String,
}

/// A React/Preact component that participates in a duplicate-prop-shape GROUP:
/// three or more distinct components across two or more files whose
/// statically-harvested, fully-known prop NAME set is byte-for-byte IDENTICAL
/// after excluding a fixed denylist of ubiquitous DOM / render-passthrough prop
/// names, with the REMAINING significant set holding four or more members. This
/// is a structural-refactor health signal (extract a shared `Props` type or a
/// base component), never a correctness error and never an auto-fix. One finding
/// is emitted per participating component; `sharing_components` lists the other
/// members of the same group. Health signal: the rule defaults to `off`
/// (opt-in), so this is dormant until enabled. Exact full-set identity only: a
/// superset / subset relationship does NOT group (so the finding always fits one
/// extracted shared type).
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct DuplicatePropShape {
    /// The file containing this component.
    #[serde(serialize_with = "serde_path::serialize")]
    pub file: PathBuf,
    /// 1-based line of this component definition (the finding anchor for
    /// jump-to-source and line-level suppression).
    pub line: u32,
    /// This component name.
    pub component: String,
    /// The shared SIGNIFICANT prop-name set (sorted, denylist-stripped). The
    /// unit being grouped; identical across every member of the group.
    pub shape: Vec<String>,
    /// The total number of components in this group (this one plus every
    /// sibling).
    pub group_size: u32,
    /// The OTHER components sharing this exact prop shape (path-sorted). A
    /// file-level-suppressed member drops from its own finding but still appears
    /// here, because the group is real regardless of suppression.
    pub sharing_components: Vec<DuplicatePropShapeMember>,
}

/// An Angular `@Input()` / signal `input()` / `model()` declared input that is
/// read NOWHERE inside its own component (neither the inline/external template
/// nor the class body). Single-file dead-input direction; the Angular analogue
/// of [`UnusedComponentProp`]. The whole component abstains on an unresolved
/// `extends` heritage clause (a base class in another file may read `this.foo`).
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct UnusedComponentInput {
    /// The Angular component/directive `.ts` file declaring the unused input.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// The component name (the `.ts` file stem).
    pub component_name: String,
    /// The declared input name that is never read.
    pub input_name: String,
    /// 1-based line number of the input declaration.
    pub line: u32,
    /// 0-based byte column offset of the input declaration.
    pub col: u32,
}

/// An Angular `@Output()` / signal `output()` declared output that is EMITTED
/// nowhere inside its own component (no `this.<output>.emit(...)`). Single-file
/// dead-output direction; the Angular analogue of [`UnusedComponentEmit`]. A
/// `model()` is recorded as an input only, so its framework-driven `update:`
/// emit is never flagged here. The whole component abstains on an unresolved
/// `extends` heritage clause.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct UnusedComponentOutput {
    /// The Angular component/directive `.ts` file declaring the unused output.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// The component name (the `.ts` file stem).
    pub component_name: String,
    /// The declared output name that is never emitted.
    pub output_name: String,
    /// 1-based line number of the output declaration.
    pub line: u32,
    /// 0-based byte column offset of the output declaration.
    pub col: u32,
}

/// Two or more Next.js App Router route files that resolve to the SAME URL
/// within one app-root. Next.js fails the build ("You cannot have two parallel
/// pages that resolve to the same path"); fallow catches it statically and
/// names every colliding file at once. One finding is emitted per colliding
/// file; `conflicting_paths` lists the sibling files that share the URL.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct RouteCollision {
    /// This colliding route file (a `page` or `route` leaf).
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// The URL pathname this file resolves to within its app-root, after
    /// stripping route groups `(x)` and parallel-slot `@slot` prefixes (e.g.
    /// `/about`, `/api/health`, `/blog/:slug`).
    pub url: String,
    /// The other route files that resolve to the same URL within the same
    /// app-root. Path-sorted for stable output / fingerprints.
    #[serde(serialize_with = "serde_path::serialize_vec")]
    pub conflicting_paths: Vec<PathBuf>,
    /// 1-based line number (file-level finding, always 1).
    pub line: u32,
    /// 0-based byte column offset (file-level finding, always 0).
    pub col: u32,
}

/// Two or more sibling dynamic route segments at the SAME App Router tree
/// position using different param spellings (`[id]` vs `[slug]`, or `[...x]`
/// vs `[[...x]]`). Next.js throws "You cannot use different slug names for the
/// same dynamic path" at dev / production RUNTIME when the position is hit;
/// `next build` does NOT catch it, so fallow's static catch surfaces a route
/// that would otherwise pass CI and crash at request time. One finding is
/// emitted per involved file.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct DynamicSegmentNameConflict {
    /// This route file living under one of the conflicting dynamic segments.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// The tree position (parent URL after group/slot normalization) where the
    /// dynamic segments conflict, e.g. `/shop` for `/shop/[id]` vs
    /// `/shop/[slug]`. The app-root prefix is stripped.
    pub position: String,
    /// The distinct conflicting dynamic-segment spellings at this position, as
    /// written (e.g. `["[id]", "[slug]"]`). Sorted for stable output.
    pub conflicting_segments: Vec<String>,
    /// The other route files at the same position under a conflicting dynamic
    /// segment. Path-sorted for stable output / fingerprints.
    #[serde(serialize_with = "serde_path::serialize_vec")]
    pub conflicting_paths: Vec<PathBuf>,
    /// 1-based line number (file-level finding, always 1).
    pub line: u32,
    /// 0-based byte column offset (file-level finding, always 0).
    pub col: u32,
}

/// A dependency that is listed in package.json but never imported.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct UnusedDependency {
    /// Package name, including internal workspace package names.
    pub package_name: String,
    /// Whether this is in `dependencies`, `devDependencies`, or `optionalDependencies`.
    pub location: DependencyLocation,
    /// Path to the package.json where this dependency is listed.
    /// For root deps this is `<root>/package.json`, for workspace deps it is `<ws>/package.json`.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// 1-based line number of the dependency entry in package.json.
    pub line: u32,
    /// Workspace roots that import this package even though the declaring workspace does not.
    #[serde(
        serialize_with = "serde_path::serialize_vec",
        skip_serializing_if = "Vec::is_empty"
    )]
    #[cfg_attr(feature = "schema", schemars(default))]
    pub used_in_workspaces: Vec<PathBuf>,
}

/// Where in package.json a dependency is listed.
///
/// # Examples
///
/// ```
/// use fallow_types::results::DependencyLocation;
///
/// // All three variants are constructible
/// let loc = DependencyLocation::Dependencies;
/// let dev = DependencyLocation::DevDependencies;
/// let opt = DependencyLocation::OptionalDependencies;
/// // Debug output includes the variant name
/// assert!(format!("{loc:?}").contains("Dependencies"));
/// assert!(format!("{dev:?}").contains("DevDependencies"));
/// assert!(format!("{opt:?}").contains("OptionalDependencies"));
/// ```
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub enum DependencyLocation {
    /// Listed in `dependencies`.
    Dependencies,
    /// Listed in `devDependencies`.
    DevDependencies,
    /// Listed in `optionalDependencies`.
    OptionalDependencies,
}

/// An unused enum or class member.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct UnusedMember {
    /// File containing the unused member.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// Name of the parent enum or class.
    pub parent_name: String,
    /// Name of the unused member.
    pub member_name: String,
    /// Whether this is an enum member, class method, or class property.
    pub kind: MemberKind,
    /// 1-based line number.
    pub line: u32,
    /// 0-based byte column offset.
    pub col: u32,
}

/// An import that could not be resolved.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct UnresolvedImport {
    /// File containing the unresolved import.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// The import specifier that could not be resolved.
    pub specifier: String,
    /// 1-based line number.
    pub line: u32,
    /// 0-based byte column offset of the import statement.
    pub col: u32,
    /// 0-based byte column offset of the source string literal (the specifier in quotes).
    /// Used by the LSP to underline just the specifier, not the entire import line.
    pub specifier_col: u32,
}

/// A dependency used in code but not listed in package.json.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct UnlistedDependency {
    /// Package name, including internal workspace package names, that is
    /// imported but not listed in package.json.
    pub package_name: String,
    /// Import sites where this unlisted dependency is used (file path, line, column).
    pub imported_from: Vec<ImportSite>,
}

/// A location where an import occurs.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ImportSite {
    /// File containing the import.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// 1-based line number.
    pub line: u32,
    /// 0-based byte column offset.
    pub col: u32,
}

/// An export that appears multiple times across the project.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct DuplicateExport {
    /// The duplicated export name.
    pub export_name: String,
    /// Locations where this export name appears.
    pub locations: Vec<DuplicateLocation>,
}

/// A location where a duplicate export appears.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct DuplicateLocation {
    /// File containing the duplicate export.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// 1-based line number.
    pub line: u32,
    /// 0-based byte column offset.
    pub col: u32,
}

/// A production dependency that is only used via type-only imports.
/// In production builds, type imports are erased, so this dependency
/// is not needed at runtime and could be moved to devDependencies.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct TypeOnlyDependency {
    /// Production dependency that is only used via type-only imports.
    pub package_name: String,
    /// Path to the package.json where the dependency is listed.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// 1-based line number of the dependency entry in package.json.
    pub line: u32,
}

/// The kind of security candidate. Findings are CANDIDATES for downstream agent
/// verification, NOT verified vulnerabilities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum SecurityFindingKind {
    /// A `"use client"` file transitively imports a module that reads a
    /// non-public `process.env` secret (graph-structural; bespoke, not catalogue).
    ClientServerLeak,
    /// A syntactic sink site matched against the data-driven catalogue
    /// (`security_matchers.toml`). Serializes `"tainted-sink"`; the CWE class is
    /// carried in `category` + `cwe`. ONE variant covers all catalogue categories.
    TaintedSink,
}

/// The role a hop plays in a security finding's structural import trace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum TraceHopRole {
    /// The `"use client"` boundary file the finding is anchored on.
    ClientBoundary,
    /// A module that reads an untrusted input source such as request data,
    /// where the candidate's sink argument actually traces back to that read in
    /// the same statement (arg-level, the strong intra-module association).
    UntrustedSource,
    /// A module that merely CONTAINS an untrusted-input source somewhere and is
    /// import-reachable to the sink module (module-level, issue #885). This is a
    /// reachability signal, NOT a proven value path: the specific source value
    /// is not shown to reach the sink argument. Labeled distinctly from
    /// `UntrustedSource` so a consumer never reads a module-level hop as a
    /// value-flow proof.
    ModuleSource,
    /// An intermediate module on the transitive import path.
    Intermediate,
    /// The module that reads the secret.
    SecretSource,
    /// The syntactic sink site of a catalogue-driven `tainted-sink` candidate
    /// (the single hop the `tainted_sink` detector emits). Distinct from
    /// `SecretSource`, which is specific to the `client-server-leak` rule.
    Sink,
}

/// One hop in a security finding's structural trace. Stored as an absolute path
/// internally; JSON serialization strips the project root via
/// `serde_path::serialize`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct TraceHop {
    /// File on this hop of the import chain.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// 1-based line number. Import-chain hops point at the import site; the
    /// terminal secret-source hop points at the source module when extraction
    /// does not carry a more precise member-access span.
    pub line: u32,
    /// 0-based byte column offset.
    pub col: u32,
    /// Role of this hop in the chain.
    pub role: TraceHopRole,
}

/// How strongly the untrusted-source signal is associated with the sink, a
/// structured discriminator so a consumer can tier candidates without parsing
/// the human `evidence` prose. Present only when
/// [`SecurityReachability::reachable_from_untrusted_source`] is true. Neither
/// value proves exploitability; both are ranking signals (issue #885 doctrine:
/// rank, never gate).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum TaintConfidence {
    /// The sink's argument traces back to a known untrusted-source read in the
    /// SAME statement / module (the intra-module back-trace, issue #859). The
    /// strong, high-value candidate: a specific source expression is implicated.
    ArgLevel,
    /// The sink merely lives in a module that is import-reachable from a module
    /// containing an untrusted source (issue #885). The weak candidate: only the
    /// module is implicated, not a specific value path to the sink argument.
    ModuleLevel,
}

/// Graph-derived reachability ranking signal for a security candidate. Computed
/// from the existing module graph after detection, never proven exploitable.
/// Used to surface candidates that sit on a request/runtime-reachable surface,
/// receive same-module source evidence, or are import-reachable from an
/// untrusted-source module above isolated helpers or scripts.
///
/// This is a relative-ordering signal, NOT a `confidence` or `signal_strength`
/// score: fallow does not prove the path is exploitable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SecurityReachability {
    /// Whether the anchor module is reachable from a runtime/application entry
    /// point (route handlers, server entry, framework runtime roots), the
    /// closest graph proxy for an external/request input surface. Code reachable
    /// only from test entry points does not count.
    pub reachable_from_entry: bool,
    /// Whether the anchor module is reachable over value imports from a module
    /// that reads a known untrusted input source. Module-level only: this does
    /// not prove a specific source value reaches the sink argument.
    #[serde(default)]
    pub reachable_from_untrusted_source: bool,
    /// Structured tier of the untrusted-source association: `arg-level` when the
    /// sink argument traces to a same-module source read (strong), `module-level`
    /// when only the module is import-reachable from a source (weak). Present
    /// exactly when `reachable_from_untrusted_source` is true, so a consumer can
    /// separate strong from weak candidates from this field alone without parsing
    /// the `evidence` string. Not an exploitability proof.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub taint_confidence: Option<TaintConfidence>,
    /// Number of value-import hops from the untrusted-source module to the sink
    /// module when `reachable_from_untrusted_source` is true.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub untrusted_source_hop_count: Option<u32>,
    /// Module-level import path from the untrusted-source module to the sink
    /// anchor. Empty when no source module reaches this candidate. The path is a
    /// ranking explanation, not a value-flow proof.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub untrusted_source_trace: Vec<TraceHop>,
    /// Number of distinct modules that transitively depend on the anchor module
    /// (fan-in via the graph's reverse-dependency index). A higher value means a
    /// wider surface: more call sites could route untrusted input into the sink.
    pub blast_radius: u32,
    /// Whether the anchor module participates in an architecture-boundary
    /// violation found in the same run (as the importing or imported file).
    /// Optional pairing: a candidate that also crosses a declared boundary is a
    /// stronger review target.
    pub crosses_boundary: bool,
}

/// Dead-code cross-link attached to a security candidate when fallow's dead-code
/// pass reports the same anchor as removable code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SecurityDeadCodeContext {
    /// Dead-code issue kind that matched the security candidate.
    pub kind: SecurityDeadCodeKind,
    /// Unused export name when `kind` is `unused-export`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub export_name: Option<String>,
    /// Dead-code finding line when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    /// Agent-facing guidance for deciding between deletion and hardening.
    pub guidance: String,
}

/// Dead-code issue kind linked to a security candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum SecurityDeadCodeKind {
    /// The candidate's anchor file is also reported as an unused file.
    UnusedFile,
    /// The candidate's anchor sits on an unused export declaration.
    UnusedExport,
}

/// Internal row for a security sink-shaped callee that extraction could not
/// flatten to a static catalogue path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SecurityUnresolvedCalleeDiagnostic {
    /// File containing the skipped callee. Absolute internally.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// 1-based line of the skipped callee.
    pub line: u32,
    /// 0-based byte column of the skipped callee.
    pub col: u32,
    /// Why the callee could not be flattened.
    pub reason: SkippedSecurityCalleeReason,
    /// Compact syntax shape of the skipped callee.
    pub expression_kind: SkippedSecurityCalleeExpressionKind,
}

/// The sink slot of a [`SecurityCandidate`]: a self-contained description of the
/// matched sink site. Echoes the finding's own span (`path`/`line`/`col`) plus
/// the catalogue `category`/`cwe` and the captured `callee`, so an agent can act
/// on `candidate.sink` in isolation (e.g. after fanning a finding out to a
/// sub-agent) without reading the parent finding.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SecurityCandidateSink {
    /// File of the sink site. Absolute internally; JSON strips the project root
    /// via `serde_path::serialize`.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// 1-based line of the sink site.
    pub line: u32,
    /// 0-based byte column of the sink site.
    pub col: u32,
    /// Catalogue category id of the sink (e.g. `"dangerous-html"`). For
    /// `client-server-leak` this is `None` for the secret-leak finding, and
    /// `Some("server-only-import")` when a `"use client"` cone reaches
    /// server-only code.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    /// CWE number declared by the catalogue entry. `None` for
    /// `client-server-leak`; never fabricated beyond the catalogue's value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwe: Option<u32>,
    /// The sink callee (the dangerous function or member path, e.g.
    /// `"el.innerHTML"`, `"child_process.exec"`) captured by the catalogue match.
    /// `None` for `client-server-leak` and matches that name no callee.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub callee: Option<String>,
    /// URL construction shape for SSRF and open-redirect style candidates when
    /// fallow can classify whether the origin is fixed or dynamic. Absent for
    /// non-URL sinks and unclassified URL expressions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url_shape: Option<SecurityUrlShape>,
}

/// A declared architecture-zone crossing, recovered by correlating a finding's
/// anchor against the run's architecture-boundary violations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SecurityZoneCrossing {
    /// Zone the importing side belongs to.
    pub from: String,
    /// Zone the imported side belongs to.
    pub to: String,
}

/// The boundary slot of a [`SecurityCandidate`]: which structural boundaries the
/// candidate's flow crosses. A flow that crosses a client/server or module
/// boundary is a stronger review target than a self-contained one; the boundary
/// is fallow's structural signal over a pure source-sink match.
///
/// Two further boundary kinds are RESERVED for a follow-up and are deliberately
/// absent here rather than emitted as always-false: `export_visibility` (is the
/// sink on a publicly-exported symbol?) and a package boundary (does the flow
/// cross an npm-package edge?). Both need new graph derivation that does not
/// exist today; emitting them as `false` would misreport "we checked and it does
/// not cross" when fallow has not checked at all.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SecurityCandidateBoundary {
    /// Whether the finding crosses a client/server boundary (a `"use client"`
    /// file appears in the trace). True only for `client-server-leak` today;
    /// `tainted-sink` candidates carry no client/server marker.
    pub client_server: bool,
    /// Whether an untrusted source reaches the sink across one or more
    /// value-import (module) hops. Derived from the reachability hop count.
    pub cross_module: bool,
    /// The architecture-zone crossing when the anchor participates in a declared
    /// boundary-rule violation in the same run. `None` when it crosses no
    /// declared zone boundary.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub architecture_zone: Option<SecurityZoneCrossing>,
}

/// Network-destination context for a `secret-to-network` candidate (#890): where
/// the secret-bearing network call sends its data. Present only on
/// network-category candidates. A consuming agent uses it to triage exfil
/// (dynamic / untrusted destination) from intended auth (a literal provider
/// host) without re-reading source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SecurityNetworkContext {
    /// The network call's destination as a static URL string literal, or absent
    /// when the destination is DYNAMIC (not a literal). A dynamic destination is
    /// the higher-signal exfil case; a literal provider host is usually intended
    /// auth.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub destination: Option<String>,
}

/// An agent-actionable candidate record on a [`SecurityFinding`]. fallow fills
/// `source_kind`, `sink`, and `boundary`. The exploitability IMPACT is
/// deliberately NOT a field: `severity` on the parent finding is only a
/// review-priority tier, while deciding exploitability remains the consuming
/// agent's job. A perpetually-null `impact` key would only train consumers to
/// ignore it. The agent reads this record, then writes its own impact verdict
/// downstream.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SecurityCandidate {
    /// The kind of untrusted input that reaches the sink, as a stable catalogue
    /// source id (`"http-request-input"`, `"process-env"`, `"process-argv"`,
    /// `"message-event-data"`, `"location-input"`, ...). `None`/absent when no
    /// untrusted source was matched (always `None` for `client-server-leak`).
    /// This is an OPEN string set, driven by the data-driven source catalogue; a
    /// consumer should treat an unknown id as "untrusted source of unknown kind"
    /// and never drop the candidate on that basis.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_kind: Option<String>,
    /// The sink the candidate fires on, self-contained so the record is
    /// actionable without reading the parent finding.
    pub sink: SecurityCandidateSink,
    /// The structural boundary the flow crosses.
    pub boundary: SecurityCandidateBoundary,
    /// Network-destination context, present only on `secret-to-network` (#890)
    /// candidates: the host the secret-bearing call targets, so an agent can
    /// triage exfil from intended auth. Absent for every other category.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network: Option<SecurityNetworkContext>,
}

/// One endpoint (source or sink node) of a [`SecurityTaintFlow`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct TaintEndpoint {
    /// File of the endpoint. Absolute internally; JSON strips the project root.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// 1-based line of the endpoint.
    pub line: u32,
    /// 0-based byte column of the endpoint.
    pub col: u32,
}

/// Compact taint-flow path shape. The ordered per-hop trace is NOT duplicated
/// here: it lives on [`SecurityReachability::untrusted_source_trace`]. This
/// carries only the flow's structural summary (intra-module flow plus the
/// cross-module hop count) so consumers do not parse two copies of the hops.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct TaintPath {
    /// Whether the source and sink sit in the same module (no import hop between
    /// them); the source-to-sink association is intra-module.
    pub intra_module: bool,
    /// Number of value-import hops from the untrusted-source module to the sink
    /// module. Zero for an intra-module flow.
    pub cross_module_hops: u32,
}

/// A source-to-sink taint-flow triple, emitted only when an untrusted source is
/// import-reachable to the sink (`reachability.reachable_from_untrusted_source`).
/// The `{ source, sink, path }` shape matches the model agent SAST tooling
/// expects (cf. Semgrep `taint_source` / `taint_sink`, SARIF `threadFlows`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SecurityTaintFlow {
    /// The untrusted-source endpoint (first hop of the reachability trace).
    pub source: TaintEndpoint,
    /// The sink endpoint (terminal hop of the reachability trace / the anchor).
    pub sink: TaintEndpoint,
    /// Compact flow shape: same-module flag plus module hop count. The full
    /// ordered path is `reachability.untrusted_source_trace`.
    pub path: TaintPath,
}

/// Runtime coverage state for the function enclosing a security sink.
/// This is production-observation evidence, not an exploitability verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum SecurityRuntimeState {
    /// The sink sits inside a runtime hot path.
    RuntimeHot,
    /// The sink sits inside a tracked function with zero production invocations.
    RuntimeCold,
    /// The sink sits inside a tracked function the runtime layer marked as safe
    /// to delete because it was never executed.
    NeverExecuted,
    /// The sink sits inside a function that executed, but below the low-traffic
    /// threshold.
    LowTraffic,
    /// Runtime coverage could not classify the enclosing function.
    CoverageUnavailable,
    /// A static enclosing function was found, but the runtime report carried no
    /// matching evidence for it.
    RuntimeUnknown,
}

/// Runtime coverage context attached to a security candidate when
/// `fallow security --runtime-coverage` is supplied.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SecurityRuntimeContext {
    /// Runtime state for the enclosing function.
    pub state: SecurityRuntimeState,
    /// Enclosing function name from static extraction.
    pub function: String,
    /// 1-based line where the enclosing function starts.
    pub line: u32,
    /// Observed invocation count when the runtime report provides it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invocations: Option<u64>,
    /// Runtime coverage stable function id, when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stable_id: Option<String>,
    /// Short candidate-framed explanation of the runtime evidence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence: Option<String>,
}

/// Verification-priority tier for a security candidate. This is ranking, not an
/// exploitability verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum SecuritySeverity {
    /// Highest-priority candidate based on reachability, boundary, or runtime-hot signals.
    High,
    /// Candidate has source-reachability evidence but no high-priority signal.
    Medium,
    /// Candidate has no source-reachability or boundary signal.
    Low,
}

/// Defensive control found on an attack-surface path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SecurityDefensiveControl {
    /// Control family.
    pub kind: SecurityControlKind,
    /// File of the control site. Absolute internally; JSON strips the project root.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// 1-based line of the control site.
    pub line: u32,
    /// 0-based byte column of the control site.
    pub col: u32,
    /// Flattened callee path or a stable synthetic guard name.
    pub callee: String,
}

/// Agent-facing defensive-boundary verification context for one surface path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SecurityDefensiveBoundary {
    /// Known controls detected along this path.
    pub controls: Vec<SecurityDefensiveControl>,
    /// Verification question for the consuming agent. It is a prompt, not a
    /// missing-guard verdict.
    pub verification_prompt: String,
}

/// One untrusted entry to reachable sink path for `fallow security --surface`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SecurityAttackSurfaceEntry {
    /// The untrusted-source endpoint.
    pub source: TaintEndpoint,
    /// The reachable sink endpoint and catalogue metadata.
    pub sink: SecurityCandidateSink,
    /// Ordered source to sink path. Same shape as the reachability trace so
    /// consumers can reuse existing path handling.
    pub path: Vec<TraceHop>,
    /// Defensive-boundary context detected on this path.
    pub defensive_boundary: SecurityDefensiveBoundary,
}

/// A local security CANDIDATE for downstream agent verification, NOT a verified
/// vulnerability. Emitted only by `fallow security`, never under bare `fallow`
/// or the `audit` gate. There is deliberately no `confidence` or
/// `signal_strength` field: fallow does not prove exploitability, so the trace
/// (its hops and length) is the only honest signal.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SecurityFinding {
    /// Stable per-finding correlation id, identical across runs for the same
    /// rule + anchor path + line. An autonomous agent that triaged this
    /// candidate on a prior run uses it to correlate the candidate after a
    /// rebase. Equal to the SARIF `partialFingerprints` value for the same
    /// finding (one shared helper computes both).
    pub finding_id: String,
    /// The rule that produced this candidate.
    pub kind: SecurityFindingKind,
    /// The catalogue category id (e.g. `"dangerous-html"`). `Some` for
    /// `TaintedSink`. For `ClientServerLeak` this is `None` for the secret-leak
    /// finding, and `Some("server-only-import")` when a `"use client"` cone
    /// reaches server-only code.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    /// The CWE number declared by the matched catalogue entry. `None` for
    /// `ClientServerLeak`; never fabricated beyond the catalogue's value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwe: Option<u32>,
    /// File the finding is anchored on (the client boundary). Absolute
    /// internally; JSON strips the project root via `serde_path::serialize`.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// 1-based line number of the anchor.
    pub line: u32,
    /// 0-based byte column offset of the anchor.
    pub col: u32,
    /// Agent/human-readable evidence (e.g. the named env var the chain reaches).
    pub evidence: String,
    /// Whether the sink argument was associated with a known untrusted source by
    /// the intra-module source-to-sink back-trace (issue #859): a local binding
    /// referenced in the argument was sourced from a catalogue source path
    /// (`req.query`, `process.argv`, message-event `data`, etc.). `true` ranks
    /// the candidate higher and annotates the evidence; `false` does NOT
    /// suppress the finding (the association is conservative, never a proof, and
    /// fallow prefers false-negatives over false-positives). Always `false` for
    /// `ClientServerLeak`. Skipped from JSON when `false` for output stability.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub source_backed: bool,
    /// Internal cross-pass carrier (NEVER serialized): the (1-based line, 0-based
    /// col) of the arg-level source read, resolved by the detector when
    /// `source_backed` is true and a concrete read span was captured. The ranking
    /// pass uses it to anchor the taint trace's source node at the real read
    /// instead of the module import line. `None` for module-level findings and
    /// for arg-level findings with no concrete read span (synthetic
    /// framework-param / helper-return sources), where the trace falls back to
    /// the sink site.
    #[serde(skip)]
    pub source_read: Option<(u32, u32)>,
    /// Verification-priority tier derived from existing reachability, boundary,
    /// source-backed, and runtime signals. Candidate-only: this does not prove
    /// exploitability and does not change gates.
    pub severity: SecuritySeverity,
    /// Structural import-hop trace from the client boundary to the secret source.
    /// The hop count is the uncalibrated signal; fallow does not prove the path
    /// is exploitable.
    pub trace: Vec<TraceHop>,
    /// Machine-actionable next steps. Always emitted (possibly empty for
    /// forward-compat). For security candidates this is a single file-level
    /// suppress hint (`auto_fixable: false`); there is no auto-fix because
    /// verification is the agent's job, not fallow's.
    pub actions: Vec<IssueAction>,
    /// Dead-code cross-link when the same sink candidate sits in code fallow also
    /// reports as removable. Agents should verify the dead-code finding and delete
    /// the code instead of hardening the sink when deletion is safe.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dead_code: Option<SecurityDeadCodeContext>,
    /// Graph-derived reachability ranking signal (issues #860 and #885). `None`
    /// until the post-detection ranking pass fills it; additive on the wire
    /// (skipped when absent). Drives the order findings are emitted in:
    /// runtime-reachable candidates sort first, followed by source-backed and
    /// source-reachable candidates, then wider blast radius.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reachability: Option<SecurityReachability>,
    /// Agent-actionable candidate record: the untrusted input kind, the sink,
    /// and the boundary the flow crosses. fallow fills these three slots; the
    /// exploitability verdict is the agent's job and is not a field here. Always
    /// present.
    pub candidate: SecurityCandidate,
    /// Source-to-sink taint-flow triple, present only when an untrusted source
    /// is import-reachable to this sink. Absent (skipped) otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub taint_flow: Option<SecurityTaintFlow>,
    /// Production runtime coverage context for the function enclosing this
    /// security sink. Present only when `fallow security --runtime-coverage`
    /// runs and the candidate is a `tainted-sink`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime: Option<SecurityRuntimeContext>,
    /// Internal projection used by `fallow security --surface`. The CLI strips
    /// this from per-finding JSON and promotes it to the top-level
    /// `attack_surface` field only when requested.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attack_surface: Option<SecurityAttackSurfaceEntry>,
}

/// A package manager catalog entry that no workspace package references via
/// the `catalog:` protocol.
///
/// The default catalog uses `catalog_name: "default"`. Named catalogs
/// (`catalogs.<name>`) use their declared name. The source file is
/// `pnpm-workspace.yaml` for pnpm catalogs or root `package.json` for Bun
/// catalogs.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct UnusedCatalogEntry {
    /// Package name declared in the catalog (e.g. `"react"`, `"@scope/lib"`).
    pub entry_name: String,
    /// Catalog group: `"default"` for the default catalog map, or the named
    /// catalog key for entries declared under `catalogs.<name>`.
    pub catalog_name: String,
    /// Path to the catalog source file, relative to the analyzed root.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// 1-based line number of the catalog entry within the source file.
    pub line: u32,
    /// Workspace `package.json` files that declare the same package with a
    /// hardcoded version range instead of `catalog:`. Empty when no consumer
    /// uses a hardcoded version. Sorted lexicographically for deterministic
    /// output.
    #[serde(
        default,
        serialize_with = "serde_path::serialize_vec",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub hardcoded_consumers: Vec<PathBuf>,
}

/// A named `catalogs.<name>` group with no package entries.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct EmptyCatalogGroup {
    /// Catalog group name declared under the `catalogs` map.
    pub catalog_name: String,
    /// Path to the catalog source file, relative to the analyzed root.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// 1-based line number of the empty group header within the source file.
    pub line: u32,
}

/// A workspace package.json reference (`catalog:` or `catalog:<name>`) that points
/// at a catalog which does not declare the consumed package.
///
/// Package manager installs error when this happens. fallow surfaces it
/// statically so the failure is caught at `fallow dead-code` time, before any
/// install.
///
/// The default catalog (bare `catalog:`) uses `catalog_name: "default"`.
/// Named catalogs (`catalog:react17`) use the declared catalog name.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct UnresolvedCatalogReference {
    /// Package name being referenced via the catalog protocol (e.g. `"react"`).
    pub entry_name: String,
    /// Catalog group the reference points at: `"default"` for bare `catalog:` references,
    /// or the named catalog key for `catalog:<name>` references.
    pub catalog_name: String,
    /// Absolute path to the consumer `package.json`. Matches the storage
    /// convention used by every path-anchored finding type (`UnusedFile`,
    /// `UnresolvedImport`, `UnusedExport`, etc.) so the shared filtering
    /// pipelines (`filter_results_by_changed_files`, per-file overrides,
    /// audit attribution) work without a separate root-join pass. JSON
    /// output strips the project-root prefix via `serde_path::serialize`.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// 1-based line number of the dependency entry in the consumer `package.json`.
    pub line: u32,
    /// Other catalogs in the same catalog source that DO declare this package.
    /// Empty when no catalog has the package. Sorted lexicographically. Lets
    /// agents and humans decide whether to switch the reference to a different
    /// catalog or to add the entry to the named catalog.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub available_in_catalogs: Vec<String>,
}

/// Where an override entry was declared. Serialized as the filename label
/// (`"pnpm-workspace.yaml"` or `"package.json"`) so the value in JSON output
/// matches the value users write in `ignoreDependencyOverrides[].source`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum DependencyOverrideSource {
    /// Top-level `overrides:` key in `pnpm-workspace.yaml`.
    #[serde(rename = "pnpm-workspace.yaml")]
    PnpmWorkspaceYaml,
    /// `pnpm.overrides` in a root `package.json`.
    #[serde(rename = "package.json")]
    PnpmPackageJson,
}

impl DependencyOverrideSource {
    /// Stable string label matching the serde rename. Used in baseline keys,
    /// audit keys, jq comparisons, and `ignoreDependencyOverrides[].source`.
    #[must_use]
    pub const fn as_label(&self) -> &'static str {
        match self {
            Self::PnpmWorkspaceYaml => "pnpm-workspace.yaml",
            Self::PnpmPackageJson => "package.json",
        }
    }
}

impl std::fmt::Display for DependencyOverrideSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_label())
    }
}

/// An entry in pnpm's `overrides:` map (or the legacy `pnpm.overrides` in
/// `package.json`) whose target package is not declared in any workspace
/// `package.json` and is not present in `pnpm-lock.yaml`. Projects without a
/// readable lockfile fall back to package manifest checks; the `hint` field
/// flags that conservative mode.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct UnusedDependencyOverride {
    /// The full original override key as written in the source (e.g.
    /// `"react>react-dom"`, `"@types/react@<18"`). Preserved for round-trip
    /// reporting so agents see the unmodified spelling.
    pub raw_key: String,
    /// The target package the override rewrites (e.g. `"react-dom"` for
    /// `"react>react-dom"`, `"@types/react"` for `"@types/react@<18"`).
    pub target_package: String,
    /// Optional parent package (left side of `>`). `None` for bare-target keys.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_package: Option<String>,
    /// Optional version selector on the target (e.g. `Some("<18")` for
    /// `"@types/react@<18"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version_constraint: Option<String>,
    /// The right-hand side of the entry: the version pnpm should force.
    pub version_range: String,
    /// File the override was declared in. Matches the value users write in
    /// `ignoreDependencyOverrides[].source`.
    pub source: DependencyOverrideSource,
    /// Path to the source file. `pnpm-workspace.yaml` or a `package.json`,
    /// stored as an absolute filesystem path so `--changed-since` and
    /// per-file `overrides.rules` can compare directly against the analyzer's
    /// changed-set / per-path rule lookups. JSON serialization strips the
    /// project root via `serde_path::serialize`, matching the
    /// `UnresolvedCatalogReference` convention.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// 1-based line number of the entry within the source file.
    pub line: u32,
    /// Soft hint reminding consumers to verify the override before removal.
    /// Emitted on every unused-override finding (both bare-target and
    /// parent-chain shapes) because projects without a readable lockfile still
    /// use the conservative package-manifest fallback.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

/// Why a dependency-override entry is misconfigured. `pnpm install` would
/// either fail at install time or silently no-op on these entries; surfacing
/// them statically catches the issue before pnpm does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum DependencyOverrideMisconfigReason {
    /// The override key could not be parsed into a recognised pnpm shape
    /// (e.g. dangling `>`, missing target, garbage characters).
    UnparsableKey,
    /// The override value is missing, empty, or contains line breaks.
    EmptyValue,
}

impl DependencyOverrideMisconfigReason {
    /// Human-readable summary of the reason.
    #[must_use]
    pub const fn describe(self) -> &'static str {
        match self {
            Self::UnparsableKey => "override key cannot be parsed",
            Self::EmptyValue => "override value is missing or empty",
        }
    }
}

/// An override entry whose key or value is malformed. Default severity is
/// `error` because pnpm refuses to install (or silently produces a no-op
/// override) when it encounters these shapes.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MisconfiguredDependencyOverride {
    /// The full original override key as written in the source.
    pub raw_key: String,
    /// Parsed target package name when the key was syntactically valid (the
    /// `EmptyValue` reason path). `None` for `UnparsableKey` findings whose
    /// key could not be parsed at all. Used by JSON `add-to-config` actions to
    /// emit a paste-ready `ignoreDependencyOverrides` value that matches the
    /// suppression matcher (which also keys on `target_package`); avoids the
    /// pitfall where `raw_key` like `"react@<18"` would not match the rule
    /// that targets package `"react"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_package: Option<String>,
    /// The right-hand side of the entry, exactly as written. Empty when the
    /// value was missing.
    pub raw_value: String,
    /// Classifier for the misconfiguration. 'unparsable-key' = the key is not a
    /// valid pnpm shape; 'empty-value' = the value is missing, empty, or
    /// contains line breaks.
    pub reason: DependencyOverrideMisconfigReason,
    /// Where the override entry was declared.
    pub source: DependencyOverrideSource,
    /// Path to the source file. Stored as an absolute filesystem path so
    /// `--changed-since` and per-file `overrides.rules` can compare directly.
    /// JSON serialization strips the project root via `serde_path::serialize`.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// 1-based line number of the entry within the source file.
    pub line: u32,
}

/// A production dependency that is only imported by test files.
/// Since it is never used in production code, it could be moved to devDependencies.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct TestOnlyDependency {
    /// Production dependency that is only imported by test files, consider
    /// moving to devDependencies.
    pub package_name: String,
    /// Path to the package.json where the dependency is listed.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// 1-based line number of the dependency entry in package.json.
    pub line: u32,
}

/// One import hop in a circular dependency: the file containing the import
/// and where that import statement sits.
///
/// `edges[i]` is the import IN `path` (the hop SOURCE, equal to the cycle's
/// `files[i]`) that points to the NEXT file in the cycle
/// (`files[(i + 1) % files.len()]`); the target is not repeated here to keep
/// the wire compact. Enables a per-file diagnostic squiggly anchored under
/// the offending import rather than a single squiggly on the first file.
///
/// `col` is a 0-based BYTE column, matching the cycle's top-level `col`;
/// converting it to a UTF-16 code-unit column for LSP clients is a tracked
/// follow-up shared with the existing field.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CircularDependencyEdge {
    /// The file containing the import (the hop SOURCE; equal to `files[i]`).
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// 1-based line number of the import statement pointing to the next file.
    pub line: u32,
    /// 0-based byte column offset of the import statement.
    pub col: u32,
}

/// A circular dependency chain detected in the module graph.
///
/// The `line` and `col` fields carry `#[serde(default)]` so callers reading
/// historical baseline JSON without these fields can still deserialize the
/// struct, but the JSON output layer always emits them (u32 always
/// serializes, never via `skip_serializing_if`). The schemars derive sees
/// the serde defaults and marks both fields optional in the generated
/// schema; the explicit `extend("required" = ...)` override here keeps the
/// schema's `required` array honest about what the JSON output actually
/// contains.
///
/// `edges` is deliberately kept OUT of the `required` extend: it is
/// `#[serde(default)]` (so historical baseline JSON without it still
/// deserializes) and the output layer always emits it, but listing it in
/// `required` would make pre-upgrade JSON fail validation against the new
/// schema. It is a normal additive field: always present in current output,
/// optional for backward compatibility.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "schema", schemars(extend("required" = ["files", "length", "line", "col"])))]
pub struct CircularDependency {
    /// Files forming the cycle, in import order.
    #[serde(serialize_with = "serde_path::serialize_vec")]
    pub files: Vec<PathBuf>,
    /// Number of files in the cycle.
    pub length: usize,
    /// 1-based line number of the import that starts the cycle (in the first file).
    #[serde(default)]
    pub line: u32,
    /// 0-based byte column offset of the import that starts the cycle.
    #[serde(default)]
    pub col: u32,
    /// Per-file import anchors, one entry per hop in cycle order: `edges[i]`
    /// is the import in `files[i]` pointing to `files[(i + 1) % len]`. Always
    /// the same length as `files`. Drives the per-file LSP diagnostic
    /// squiggly. `#[serde(default)]` so pre-`edges` baselines deserialize;
    /// always emitted on output but intentionally not in the schema's
    /// `required` set (see the struct doc).
    #[serde(default)]
    pub edges: Vec<CircularDependencyEdge>,
    /// Whether this cycle crosses workspace package boundaries.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub is_cross_package: bool,
}

/// A cycle or self-loop in the re-export edge subgraph.
///
/// Detected by Tarjan SCC over `(barrel, source)` re-export edges in
/// `crates/graph/src/graph/re_exports/`. A multi-node cycle is a strongly
/// connected component of size >= 2; a self-loop is a barrel that re-exports
/// from itself (often a rename leftover or accidental `export * from './'`).
/// Both are structural bugs because chain propagation through the loop is a
/// no-op: any symbol consumers think they are re-exporting through the cycle
/// silently fails to resolve.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ReExportCycle {
    /// Files participating in the cycle, sorted lexicographically. For a
    /// self-loop, exactly one entry.
    #[serde(serialize_with = "serde_path::serialize_vec")]
    pub files: Vec<PathBuf>,
    /// Which structural shape this finding describes.
    pub kind: ReExportCycleKind,
}

/// Discriminator for [`ReExportCycle`]: which structural shape was detected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum ReExportCycleKind {
    /// Two or more barrel files re-export from each other in a loop
    /// (SCC of size >= 2).
    MultiNode,
    /// A single barrel file re-exports from itself.
    SelfLoop,
}

/// An import that crosses an architecture boundary rule.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct BoundaryViolation {
    /// The file making the disallowed import.
    #[serde(serialize_with = "serde_path::serialize")]
    pub from_path: PathBuf,
    /// The file being imported that violates the boundary.
    #[serde(serialize_with = "serde_path::serialize")]
    pub to_path: PathBuf,
    /// The zone the importing file belongs to.
    pub from_zone: String,
    /// The zone the imported file belongs to.
    pub to_zone: String,
    /// The raw import specifier from the source file.
    pub import_specifier: String,
    /// 1-based line number of the import statement in the source file.
    pub line: u32,
    /// 0-based byte column offset of the import statement.
    pub col: u32,
}

/// A source file that does not match any configured architecture boundary zone.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct BoundaryCoverageViolation {
    /// The unmatched source file.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// 1-based line number used for diagnostics.
    pub line: u32,
    /// 0-based byte column offset used for diagnostics.
    pub col: u32,
}

/// A call from a zoned file to a callee forbidden for that zone via
/// `boundaries.calls.forbidden`. One finding is reported per unique callee
/// path per file (first occurrence wins).
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct BoundaryCallViolation {
    /// The zoned source file making the forbidden call.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// 1-based line number of the call site.
    pub line: u32,
    /// 0-based byte column offset of the call site.
    pub col: u32,
    /// The zone the calling file is classified into.
    pub zone: String,
    /// The callee path as written at the call site (e.g. `cp.exec`).
    pub callee: String,
    /// The configured pattern that matched (e.g. `child_process.*`), so
    /// consumers can see both the written path and the rule that fired.
    pub pattern: String,
}

/// Which rule-pack rule kind produced a [`PolicyViolation`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum PolicyRuleKind {
    /// A call site matched a `banned-call` rule's callee patterns.
    BannedCall,
    /// An import or re-export specifier matched a `banned-import` rule.
    BannedImport,
}

/// Effective severity of a single [`PolicyViolation`]. Per-rule `severity`
/// overrides the `rules."policy-violation"` master; `off` rules emit nothing,
/// so only `error` and `warn` appear on the wire. The exit-code gate inspects
/// this per-finding value, not the master severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum PolicyViolationSeverity {
    /// Fails CI (non-zero exit code).
    Error,
    /// Reported without failing CI.
    Warn,
}

/// A banned call or banned import matched by a declarative rule pack
/// (`rulePacks` config). Banned-call findings report one entry per unique
/// callee path per file (first occurrence wins, matching
/// `boundary_call_violations`); banned-import findings anchor at each
/// matching import or re-export declaration.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct PolicyViolation {
    /// The source file containing the banned call or import.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// 1-based line number of the call site or import declaration.
    pub line: u32,
    /// 0-based byte column offset of the call site or import declaration.
    pub col: u32,
    /// Name of the rule pack that declared the matching rule.
    pub pack: String,
    /// Id of the matching rule inside the pack. `pack` plus `rule_id` is the
    /// finding's policy identity.
    pub rule_id: String,
    /// Which rule kind matched.
    pub kind: PolicyRuleKind,
    /// What matched: the written callee path for `banned-call` (e.g.
    /// `cp.exec`), or the raw import specifier for `banned-import` (e.g.
    /// `moment/locale/nl`).
    pub matched: String,
    /// Effective severity for this finding (per-rule `severity`, else the
    /// `rules."policy-violation"` master).
    pub severity: PolicyViolationSeverity,
    /// The rule's author-provided message, when set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// The origin of a stale suppression: inline comment or JSDoc tag.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum SuppressionOrigin {
    /// A `// fallow-ignore-next-line` or `// fallow-ignore-file` comment.
    Comment {
        /// The issue kind token from the comment (e.g., "unused-exports"), or None for blanket.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        issue_kind: Option<String>,
        /// Human-authored reason after `--`, when present.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
        /// Whether this was a file-level suppression.
        is_file_level: bool,
        /// Whether `issue_kind` parses to a known `IssueKind`. False when the
        /// token is a typo or refers to a kind that was renamed or removed in
        /// a newer fallow release. JSON consumers (CI annotations, MCP agents,
        /// VS Code) branch on this to choose the right next-step text.
        /// Omitted from the wire when `true` so producers that have not yet
        /// adopted the field stay byte-compatible. See issue #449.
        #[serde(default = "default_true", skip_serializing_if = "is_true")]
        kind_known: bool,
    },
    /// An `@expected-unused` JSDoc tag on an export.
    JsdocTag {
        /// The name of the export that was tagged.
        export_name: String,
        /// Human-authored reason after `--`, when present.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
}

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde skip_serializing_if takes a reference by contract"
)]
const fn is_true(b: &bool) -> bool {
    *b
}

/// Default for `SuppressionOrigin::Comment.kind_known` when the field is
/// absent from a deserialized payload, paired with `skip_serializing_if = is_true`
/// so schemars marks the field non-required in the generated JSON Schema AND
/// the absent case round-trips to the recognized-kind interpretation.
/// Referenced by the always-emitted `#[serde(default = "default_true")]`
/// attribute. Today `SuppressionOrigin` derives only `Serialize`, so serde
/// itself never calls this; schemars (under the `schema` feature) reads the
/// attribute textually to mark `kind_known` non-required. The `cfg_attr`
/// applies `#[expect(dead_code)]` only on builds WITHOUT the `schema` feature
/// (where the function is genuinely dead): under the feature schemars
/// references it, the lint does not fire, and an unconditional `#[expect]`
/// would be unfulfilled. The function stays un-gated so a future
/// `Deserialize` derive on `SuppressionOrigin` does not produce a missing-
/// function compile error on non-`schema` builds.
#[cfg_attr(
    not(feature = "schema"),
    expect(
        dead_code,
        reason = "referenced via #[serde(default = ...)]; only consumed by schemars under the `schema` feature, dead on default builds today"
    )
)]
const fn default_true() -> bool {
    true
}

/// A suppression comment or JSDoc tag that no longer matches any issue.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct StaleSuppression {
    /// File containing the stale suppression.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// 1-based line number of the suppression comment or tag.
    pub line: u32,
    /// 0-based byte column offset.
    pub col: u32,
    /// The origin and details of the stale suppression.
    pub origin: SuppressionOrigin,
    /// True when `rules.require-suppression-reason` reported a suppression
    /// comment or tag that has no reason.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub missing_reason: bool,
    /// Suggested next steps. Always emitted.
    pub actions: Vec<IssueAction>,
}

impl StaleSuppression {
    /// Build the typed action list for this suppression finding.
    #[must_use]
    pub fn actions_for(missing_reason: bool) -> Vec<IssueAction> {
        let (kind, description) = if missing_reason {
            (
                FixActionType::AddSuppressionReason,
                "Add a human-authored reason after `--` on the suppression",
            )
        } else {
            (
                FixActionType::RemoveStaleSuppression,
                "Remove or update the stale suppression",
            )
        };
        let mut actions = vec![IssueAction::Fix(FixAction {
            kind,
            auto_fixable: false,
            description: description.to_string(),
            note: None,
            available_in_catalogs: None,
            suggested_target: None,
        })];
        if !missing_reason {
            actions.push(IssueAction::SuppressLine(SuppressLineAction {
                kind: SuppressLineKind::SuppressLine,
                auto_fixable: false,
                description:
                    "Suppress this stale suppression finding with a comment above the suppression"
                        .to_string(),
                comment: "// fallow-ignore-next-line stale-suppression".to_string(),
                scope: Some(SuppressLineScope::PerLocation),
            }));
        }
        actions
    }

    /// Produce a human-readable description of this stale suppression.
    #[must_use]
    pub fn description(&self) -> String {
        match &self.origin {
            SuppressionOrigin::Comment {
                issue_kind,
                reason,
                is_file_level,
                ..
            } => {
                let directive = if *is_file_level {
                    "fallow-ignore-file"
                } else {
                    "fallow-ignore-next-line"
                };
                match issue_kind {
                    Some(kind) => match reason {
                        Some(reason) => format!("// {directive} {kind} -- {reason}"),
                        None => format!("// {directive} {kind}"),
                    },
                    None => match reason {
                        Some(reason) => format!("// {directive} -- {reason}"),
                        None => format!("// {directive}"),
                    },
                }
            }
            SuppressionOrigin::JsdocTag {
                export_name,
                reason,
            } => match reason {
                Some(reason) => format!("@expected-unused on {export_name} -- {reason}"),
                None => format!("@expected-unused on {export_name}"),
            },
        }
    }

    /// Produce an explanation of why this suppression is stale.
    ///
    /// For comment suppressions where `kind_known == false`, surfaces the
    /// unknown token plus a Levenshtein "did you mean?" hint when one is
    /// within edit distance 2. Other tokens on the same comment line still
    /// apply normally (see issue #449).
    #[must_use]
    pub fn explanation(&self) -> String {
        match &self.origin {
            SuppressionOrigin::Comment {
                issue_kind,
                is_file_level,
                kind_known,
                ..
            } => {
                if self.missing_reason {
                    return "suppression is missing a reason".to_string();
                }
                let scope = if *is_file_level {
                    "in this file"
                } else {
                    "on the next line"
                };
                match issue_kind {
                    Some(kind) if !*kind_known => match closest_known_kind_name(kind) {
                        Some(suggestion) => format!(
                            "'{kind}' is not a recognized fallow issue kind. Did you mean '{suggestion}'? Other tokens on this line still apply."
                        ),
                        None => format!(
                            "'{kind}' is not a recognized fallow issue kind. Other tokens on this line still apply."
                        ),
                    },
                    Some(kind) => format!("no {kind} issue found {scope}"),
                    None => format!("no issues found {scope}"),
                }
            }
            SuppressionOrigin::JsdocTag { export_name, .. } => {
                if self.missing_reason {
                    return "suppression is missing a reason".to_string();
                }
                format!("{export_name} is now used")
            }
        }
    }

    /// The suppressed `IssueKind`, if this was a comment suppression with a specific known kind.
    ///
    /// Returns `None` for unknown-kind comments (`kind_known == false`) and
    /// for JSDoc tags.
    #[must_use]
    pub fn suppressed_kind(&self) -> Option<IssueKind> {
        match &self.origin {
            SuppressionOrigin::Comment {
                issue_kind,
                kind_known: true,
                ..
            } => issue_kind.as_deref().and_then(IssueKind::parse),
            SuppressionOrigin::Comment { .. } | SuppressionOrigin::JsdocTag { .. } => None,
        }
    }

    /// Per-format display message combining `description()` and `explanation()`
    /// for the unknown-kind case so SARIF, CodeClimate, and compact consumers
    /// surface the typo-fix copy and Levenshtein hint without needing to
    /// branch on `origin.kind_known` themselves. Stale-but-known and JSDoc
    /// origins keep the bare `description()` so existing wire bytes stay
    /// unchanged. See issue #449.
    #[must_use]
    pub fn display_message(&self) -> String {
        match &self.origin {
            SuppressionOrigin::Comment {
                kind_known: false, ..
            } => format!("{} ({})", self.description(), self.explanation()),
            SuppressionOrigin::Comment { .. } | SuppressionOrigin::JsdocTag { .. }
                if self.missing_reason =>
            {
                format!("{} ({})", self.description(), self.explanation())
            }
            SuppressionOrigin::Comment { .. } | SuppressionOrigin::JsdocTag { .. } => {
                self.description()
            }
        }
    }
}

/// A suppression comment present in an analyzed file this run.
///
/// This is the "active-suppression state" the Fallow Impact value report needs
/// to tell a genuinely resolved finding (the code was fixed) from one merely
/// silenced by a newly-added `fallow-ignore`. It captures every PRESENT marker,
/// not only the ones a detector consumed: complexity and code-duplication
/// suppressions are consumed in the CLI layer rather than the core suppression
/// context, so presence is the single uniform signal that covers all impact
/// categories. A present-but-stale marker is harmless because impact keys on a
/// suppression that newly appeared between two recorded runs. It is internal:
/// never serialized into the public JSON output schema (the field on
/// [`AnalysisResults`] is `#[serde(skip)]`), only read in-process by
/// `fallow impact`.
#[derive(Debug, Clone)]
pub struct ActiveSuppression {
    /// Absolute path to the file carrying the suppression comment.
    pub path: PathBuf,
    /// The suppressed issue kind in kebab-case (e.g. `"unused-export"`), or
    /// `None` for a blanket marker that suppresses every kind on its target.
    pub kind: Option<String>,
    /// Whether this is a `fallow-ignore-file` (file-level) marker rather than a
    /// `fallow-ignore-next-line` marker.
    pub is_file_level: bool,
    /// Human-authored reason after `--`, when present.
    pub reason: Option<String>,
}

/// The detection method used to identify a feature flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum FlagKind {
    /// Environment variable check (e.g., `process.env.FEATURE_X`).
    EnvironmentVariable,
    /// Feature flag SDK call (e.g., `useFlag('name')`, `variation('name', false)`).
    SdkCall,
    /// Config object property access (e.g., `config.features.newCheckout`).
    ConfigObject,
}

/// Detection confidence for a feature flag finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum FlagConfidence {
    /// Low confidence: heuristic match (config object patterns).
    Low,
    /// Medium confidence: pattern match with some ambiguity.
    Medium,
    /// High confidence: unambiguous pattern (env vars, direct SDK calls).
    High,
}

/// A detected feature flag use site.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct FeatureFlag {
    /// File containing the feature flag usage.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// Name or identifier of the flag (e.g., `ENABLE_NEW_CHECKOUT`, `new-checkout`).
    pub flag_name: String,
    /// How the flag was detected.
    pub kind: FlagKind,
    /// Detection confidence level.
    pub confidence: FlagConfidence,
    /// 1-based line number.
    pub line: u32,
    /// 0-based byte column offset.
    pub col: u32,
    /// Start byte offset of the guarded code block (if-branch span), if detected.
    #[serde(skip)]
    pub guard_span_start: Option<u32>,
    /// End byte offset of the guarded code block (if-branch span), if detected.
    #[serde(skip)]
    pub guard_span_end: Option<u32>,
    /// SDK or provider name (e.g., "LaunchDarkly", "Statsig"), if detected from SDK call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sdk_name: Option<String>,
    /// Line range of the guarded code block (derived from guard_span + line_offsets).
    /// Used for cross-reference with dead code findings.
    #[serde(skip)]
    pub guard_line_start: Option<u32>,
    /// End line of the guarded code block.
    #[serde(skip)]
    pub guard_line_end: Option<u32>,
    /// Unused exports found within the guarded code block.
    /// Populated by cross-reference with dead code analysis.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub guarded_dead_exports: Vec<String>,
}

// Size assertion: FeatureFlag is stored in a Vec per analysis run.
const _: () = assert!(std::mem::size_of::<FeatureFlag>() <= 160);

/// Usage count for an export symbol. Used by the LSP Code Lens to show
/// reference counts above each export declaration.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ExportUsage {
    /// File containing the export.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// Name of the exported symbol.
    pub export_name: String,
    /// 1-based line number.
    pub line: u32,
    /// 0-based byte column offset.
    pub col: u32,
    /// Number of files that reference this export.
    pub reference_count: usize,
    /// Locations where this export is referenced. Used by the LSP Code Lens
    /// to enable click-to-navigate via `editor.action.showReferences`.
    pub reference_locations: Vec<ReferenceLocation>,
}

/// A location where an export is referenced (import site in another file).
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ReferenceLocation {
    /// File containing the import that references the export.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// 1-based line number.
    pub line: u32,
    /// 0-based byte column offset.
    pub col: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output_dead_code::{
        BoundaryViolationFinding, CircularDependencyFinding, UnresolvedImportFinding,
        UnusedClassMemberFinding, UnusedEnumMemberFinding, UnusedExportFinding, UnusedFileFinding,
        UnusedTypeFinding,
    };

    #[test]
    fn empty_results_no_issues() {
        let results = AnalysisResults::default();
        assert_eq!(results.total_issues(), 0);
        assert!(!results.has_issues());
    }

    #[test]
    fn results_with_unused_file() {
        let mut results = AnalysisResults::default();
        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: PathBuf::from("test.ts"),
            }));
        assert_eq!(results.total_issues(), 1);
        assert!(results.has_issues());
    }

    #[test]
    fn results_with_unused_export() {
        let mut results = AnalysisResults::default();
        results
            .unused_exports
            .push(UnusedExportFinding::with_actions(UnusedExport {
                path: PathBuf::from("test.ts"),
                export_name: "foo".to_string(),
                is_type_only: false,
                line: 1,
                col: 0,
                span_start: 0,
                is_re_export: false,
            }));
        assert_eq!(results.total_issues(), 1);
        assert!(results.has_issues());
    }

    fn test_unused_export(path: &str, export_name: &str, is_type_only: bool) -> UnusedExport {
        UnusedExport {
            path: PathBuf::from(path),
            export_name: export_name.to_string(),
            is_type_only,
            line: 1,
            col: 0,
            span_start: 0,
            is_re_export: false,
        }
    }

    fn test_unused_dependency(
        package_name: &str,
        location: DependencyLocation,
    ) -> UnusedDependency {
        UnusedDependency {
            package_name: package_name.to_string(),
            location,
            path: PathBuf::from("package.json"),
            line: 5,
            used_in_workspaces: Vec::new(),
        }
    }

    fn test_unused_member(member_name: &str, kind: MemberKind) -> UnusedMember {
        UnusedMember {
            path: PathBuf::from("members.ts"),
            parent_name: "Parent".to_string(),
            member_name: member_name.to_string(),
            kind,
            line: 1,
            col: 0,
        }
    }

    #[test]
    fn results_total_counts_all_types() {
        let results = AnalysisResults {
            unused_files: vec![UnusedFileFinding::with_actions(UnusedFile {
                path: PathBuf::from("a.ts"),
            })],
            unused_exports: vec![UnusedExportFinding::with_actions(test_unused_export(
                "b.ts", "x", false,
            ))],
            unused_types: vec![UnusedTypeFinding::with_actions(test_unused_export(
                "c.ts", "T", true,
            ))],
            unused_dependencies: vec![UnusedDependencyFinding::with_actions(
                test_unused_dependency("dep", DependencyLocation::Dependencies),
            )],
            unused_dev_dependencies: vec![UnusedDevDependencyFinding::with_actions(
                test_unused_dependency("dev", DependencyLocation::DevDependencies),
            )],
            unused_enum_members: vec![UnusedEnumMemberFinding::with_actions(test_unused_member(
                "A",
                MemberKind::EnumMember,
            ))],
            unused_class_members: vec![UnusedClassMemberFinding::with_actions(test_unused_member(
                "m",
                MemberKind::ClassMethod,
            ))],
            unresolved_imports: vec![UnresolvedImportFinding::with_actions(UnresolvedImport {
                path: PathBuf::from("f.ts"),
                specifier: "./missing".to_string(),
                line: 1,
                col: 0,
                specifier_col: 0,
            })],
            unlisted_dependencies: vec![UnlistedDependencyFinding::with_actions(
                UnlistedDependency {
                    package_name: "unlisted".to_string(),
                    imported_from: vec![ImportSite {
                        path: PathBuf::from("g.ts"),
                        line: 1,
                        col: 0,
                    }],
                },
            )],
            duplicate_exports: vec![DuplicateExportFinding::with_actions(DuplicateExport {
                export_name: "dup".to_string(),
                locations: vec![
                    DuplicateLocation {
                        path: PathBuf::from("h.ts"),
                        line: 15,
                        col: 0,
                    },
                    DuplicateLocation {
                        path: PathBuf::from("i.ts"),
                        line: 30,
                        col: 0,
                    },
                ],
            })],
            unused_optional_dependencies: vec![UnusedOptionalDependencyFinding::with_actions(
                test_unused_dependency("optional", DependencyLocation::OptionalDependencies),
            )],
            type_only_dependencies: vec![TypeOnlyDependencyFinding::with_actions(
                TypeOnlyDependency {
                    package_name: "type-only".to_string(),
                    path: PathBuf::from("package.json"),
                    line: 8,
                },
            )],
            test_only_dependencies: vec![TestOnlyDependencyFinding::with_actions(
                TestOnlyDependency {
                    package_name: "test-only".to_string(),
                    path: PathBuf::from("package.json"),
                    line: 9,
                },
            )],
            circular_dependencies: vec![CircularDependencyFinding::with_actions(
                CircularDependency {
                    files: vec![PathBuf::from("a.ts"), PathBuf::from("b.ts")],
                    length: 2,
                    line: 3,
                    col: 0,
                    edges: Vec::new(),
                    is_cross_package: false,
                },
            )],
            boundary_violations: vec![BoundaryViolationFinding::with_actions(BoundaryViolation {
                from_path: PathBuf::from("src/ui/Button.tsx"),
                to_path: PathBuf::from("src/db/queries.ts"),
                from_zone: "ui".to_string(),
                to_zone: "database".to_string(),
                import_specifier: "../db/queries".to_string(),
                line: 3,
                col: 0,
            })],
            ..Default::default()
        };

        // 15 categories, one of each
        assert_eq!(results.total_issues(), 15);
        assert!(results.has_issues());
    }

    // ── total_issues / has_issues consistency ──────────────────

    #[test]
    fn total_issues_and_has_issues_are_consistent() {
        let results = AnalysisResults::default();
        assert_eq!(results.total_issues(), 0);
        assert!(!results.has_issues());
        assert_eq!(results.total_issues() > 0, results.has_issues());
    }

    // ── total_issues counts each category independently ─────────

    #[test]
    fn total_issues_sums_all_categories_independently() {
        let mut results = AnalysisResults::default();
        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: PathBuf::from("a.ts"),
            }));
        assert_eq!(results.total_issues(), 1);

        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: PathBuf::from("b.ts"),
            }));
        assert_eq!(results.total_issues(), 2);

        results
            .unresolved_imports
            .push(UnresolvedImportFinding::with_actions(UnresolvedImport {
                path: PathBuf::from("c.ts"),
                specifier: "./missing".to_string(),
                line: 1,
                col: 0,
                specifier_col: 0,
            }));
        assert_eq!(results.total_issues(), 3);
    }

    // ── default is truly empty ──────────────────────────────────

    #[test]
    fn default_results_all_fields_empty() {
        let r = AnalysisResults::default();
        assert!(r.unused_files.is_empty());
        assert!(r.unused_exports.is_empty());
        assert!(r.unused_types.is_empty());
        assert!(r.unused_dependencies.is_empty());
        assert!(r.unused_dev_dependencies.is_empty());
        assert!(r.unused_optional_dependencies.is_empty());
        assert!(r.unused_enum_members.is_empty());
        assert!(r.unused_class_members.is_empty());
        assert!(r.unresolved_imports.is_empty());
        assert!(r.unlisted_dependencies.is_empty());
        assert!(r.duplicate_exports.is_empty());
        assert!(r.type_only_dependencies.is_empty());
        assert!(r.test_only_dependencies.is_empty());
        assert!(r.circular_dependencies.is_empty());
        assert!(r.boundary_violations.is_empty());
        assert!(r.unused_catalog_entries.is_empty());
        assert!(r.unresolved_catalog_references.is_empty());
        assert!(r.export_usages.is_empty());
    }

    // ── EntryPointSummary ────────────────────────────────────────

    #[test]
    fn entry_point_summary_default() {
        let summary = EntryPointSummary::default();
        assert_eq!(summary.total, 0);
        assert!(summary.by_source.is_empty());
    }

    #[test]
    fn entry_point_summary_not_in_default_results() {
        let r = AnalysisResults::default();
        assert!(r.entry_point_summary.is_none());
    }

    #[test]
    fn entry_point_summary_some_preserves_data() {
        let r = AnalysisResults {
            entry_point_summary: Some(EntryPointSummary {
                total: 5,
                by_source: vec![("package.json".to_string(), 2), ("plugin".to_string(), 3)],
            }),
            ..AnalysisResults::default()
        };
        let summary = r.entry_point_summary.as_ref().unwrap();
        assert_eq!(summary.total, 5);
        assert_eq!(summary.by_source.len(), 2);
        assert_eq!(summary.by_source[0], ("package.json".to_string(), 2));
    }

    // ── sort: unused_files by path ──────────────────────────────

    #[test]
    fn sort_unused_files_by_path() {
        let mut r = AnalysisResults::default();
        r.unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: PathBuf::from("z.ts"),
            }));
        r.unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: PathBuf::from("a.ts"),
            }));
        r.unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: PathBuf::from("m.ts"),
            }));
        r.sort();
        let paths: Vec<_> = r
            .unused_files
            .iter()
            .map(|f| f.file.path.to_string_lossy().to_string())
            .collect();
        assert_eq!(paths, vec!["a.ts", "m.ts", "z.ts"]);
    }

    // ── sort: unused_exports by path, line, name ────────────────

    #[test]
    fn sort_unused_exports_by_path_line_name() {
        let mut r = AnalysisResults::default();
        let mk = |path: &str, line: u32, name: &str| {
            UnusedExportFinding::with_actions(UnusedExport {
                path: PathBuf::from(path),
                export_name: name.to_string(),
                is_type_only: false,
                line,
                col: 0,
                span_start: 0,
                is_re_export: false,
            })
        };
        r.unused_exports.push(mk("b.ts", 5, "beta"));
        r.unused_exports.push(mk("a.ts", 10, "zeta"));
        r.unused_exports.push(mk("a.ts", 10, "alpha"));
        r.unused_exports.push(mk("a.ts", 1, "gamma"));
        r.sort();
        let keys: Vec<_> = r
            .unused_exports
            .iter()
            .map(|e| {
                format!(
                    "{}:{}:{}",
                    e.export.path.to_string_lossy(),
                    e.export.line,
                    e.export.export_name
                )
            })
            .collect();
        assert_eq!(
            keys,
            vec![
                "a.ts:1:gamma",
                "a.ts:10:alpha",
                "a.ts:10:zeta",
                "b.ts:5:beta"
            ]
        );
    }

    // ── sort: unused_types (same sort as unused_exports) ────────

    #[test]
    fn sort_unused_types_by_path_line_name() {
        let mut r = AnalysisResults::default();
        let mk = |path: &str, line: u32, name: &str| {
            UnusedTypeFinding::with_actions(UnusedExport {
                path: PathBuf::from(path),
                export_name: name.to_string(),
                is_type_only: true,
                line,
                col: 0,
                span_start: 0,
                is_re_export: false,
            })
        };
        r.unused_types.push(mk("z.ts", 1, "Z"));
        r.unused_types.push(mk("a.ts", 1, "A"));
        r.sort();
        assert_eq!(r.unused_types[0].export.path, PathBuf::from("a.ts"));
        assert_eq!(r.unused_types[1].export.path, PathBuf::from("z.ts"));
    }

    // ── sort: unused_dependencies by path, line, name ───────────

    #[test]
    fn sort_unused_dependencies_by_path_line_name() {
        let mut r = AnalysisResults::default();
        let mk = |path: &str, line: u32, name: &str| {
            UnusedDependencyFinding::with_actions(UnusedDependency {
                package_name: name.to_string(),
                location: DependencyLocation::Dependencies,
                path: PathBuf::from(path),
                line,
                used_in_workspaces: Vec::new(),
            })
        };
        r.unused_dependencies.push(mk("b/package.json", 3, "zlib"));
        r.unused_dependencies.push(mk("a/package.json", 5, "react"));
        r.unused_dependencies.push(mk("a/package.json", 5, "axios"));
        r.sort();
        let names: Vec<_> = r
            .unused_dependencies
            .iter()
            .map(|d| d.dep.package_name.as_str())
            .collect();
        assert_eq!(names, vec!["axios", "react", "zlib"]);
    }

    // ── sort: unused_dev_dependencies ───────────────────────────

    #[test]
    fn sort_unused_dev_dependencies() {
        let mut r = AnalysisResults::default();
        r.unused_dev_dependencies
            .push(UnusedDevDependencyFinding::with_actions(UnusedDependency {
                package_name: "vitest".to_string(),
                location: DependencyLocation::DevDependencies,
                path: PathBuf::from("package.json"),
                line: 10,
                used_in_workspaces: Vec::new(),
            }));
        r.unused_dev_dependencies
            .push(UnusedDevDependencyFinding::with_actions(UnusedDependency {
                package_name: "jest".to_string(),
                location: DependencyLocation::DevDependencies,
                path: PathBuf::from("package.json"),
                line: 5,
                used_in_workspaces: Vec::new(),
            }));
        r.sort();
        assert_eq!(r.unused_dev_dependencies[0].dep.package_name, "jest");
        assert_eq!(r.unused_dev_dependencies[1].dep.package_name, "vitest");
    }

    // ── sort: unused_optional_dependencies ──────────────────────

    #[test]
    fn sort_unused_optional_dependencies() {
        let mut r = AnalysisResults::default();
        r.unused_optional_dependencies
            .push(UnusedOptionalDependencyFinding::with_actions(
                UnusedDependency {
                    package_name: "zod".to_string(),
                    location: DependencyLocation::OptionalDependencies,
                    path: PathBuf::from("package.json"),
                    line: 3,
                    used_in_workspaces: Vec::new(),
                },
            ));
        r.unused_optional_dependencies
            .push(UnusedOptionalDependencyFinding::with_actions(
                UnusedDependency {
                    package_name: "ajv".to_string(),
                    location: DependencyLocation::OptionalDependencies,
                    path: PathBuf::from("package.json"),
                    line: 2,
                    used_in_workspaces: Vec::new(),
                },
            ));
        r.sort();
        assert_eq!(r.unused_optional_dependencies[0].dep.package_name, "ajv");
        assert_eq!(r.unused_optional_dependencies[1].dep.package_name, "zod");
    }

    // ── sort: unused_enum_members by path, line, parent, member ─

    #[test]
    fn sort_unused_enum_members_by_path_line_parent_member() {
        let mut r = AnalysisResults::default();
        let mk = |path: &str, line: u32, parent: &str, member: &str| {
            UnusedEnumMemberFinding::with_actions(UnusedMember {
                path: PathBuf::from(path),
                parent_name: parent.to_string(),
                member_name: member.to_string(),
                kind: MemberKind::EnumMember,
                line,
                col: 0,
            })
        };
        r.unused_enum_members.push(mk("a.ts", 5, "Status", "Z"));
        r.unused_enum_members.push(mk("a.ts", 5, "Status", "A"));
        r.unused_enum_members.push(mk("a.ts", 1, "Direction", "Up"));
        r.sort();
        let keys: Vec<_> = r
            .unused_enum_members
            .iter()
            .map(|m| format!("{}:{}", m.member.parent_name, m.member.member_name))
            .collect();
        assert_eq!(keys, vec!["Direction:Up", "Status:A", "Status:Z"]);
    }

    // ── sort: unused_class_members by path, line, parent, member

    #[test]
    fn sort_unused_class_members() {
        let mut r = AnalysisResults::default();
        let mk = |path: &str, line: u32, parent: &str, member: &str| {
            UnusedClassMemberFinding::with_actions(UnusedMember {
                path: PathBuf::from(path),
                parent_name: parent.to_string(),
                member_name: member.to_string(),
                kind: MemberKind::ClassMethod,
                line,
                col: 0,
            })
        };
        r.unused_class_members.push(mk("b.ts", 1, "Foo", "z"));
        r.unused_class_members.push(mk("a.ts", 1, "Bar", "a"));
        r.sort();
        assert_eq!(r.unused_class_members[0].member.path, PathBuf::from("a.ts"));
        assert_eq!(r.unused_class_members[1].member.path, PathBuf::from("b.ts"));
    }

    // ── sort: unresolved_imports by path, line, col, specifier ──

    #[test]
    fn sort_unresolved_imports_by_path_line_col_specifier() {
        let mut r = AnalysisResults::default();
        let mk = |path: &str, line: u32, col: u32, spec: &str| {
            UnresolvedImportFinding::with_actions(UnresolvedImport {
                path: PathBuf::from(path),
                specifier: spec.to_string(),
                line,
                col,
                specifier_col: 0,
            })
        };
        r.unresolved_imports.push(mk("a.ts", 5, 0, "./z"));
        r.unresolved_imports.push(mk("a.ts", 5, 0, "./a"));
        r.unresolved_imports.push(mk("a.ts", 1, 0, "./m"));
        r.sort();
        let specs: Vec<_> = r
            .unresolved_imports
            .iter()
            .map(|i| i.import.specifier.as_str())
            .collect();
        assert_eq!(specs, vec!["./m", "./a", "./z"]);
    }

    // ── sort: unlisted_dependencies + inner imported_from ───────

    #[test]
    fn sort_unlisted_dependencies_by_name_and_inner_sites() {
        let mut r = AnalysisResults::default();
        r.unlisted_dependencies
            .push(UnlistedDependencyFinding::with_actions(
                UnlistedDependency {
                    package_name: "zod".to_string(),
                    imported_from: vec![
                        ImportSite {
                            path: PathBuf::from("b.ts"),
                            line: 10,
                            col: 0,
                        },
                        ImportSite {
                            path: PathBuf::from("a.ts"),
                            line: 1,
                            col: 0,
                        },
                    ],
                },
            ));
        r.unlisted_dependencies
            .push(UnlistedDependencyFinding::with_actions(
                UnlistedDependency {
                    package_name: "axios".to_string(),
                    imported_from: vec![ImportSite {
                        path: PathBuf::from("c.ts"),
                        line: 1,
                        col: 0,
                    }],
                },
            ));
        r.sort();

        // Outer sort: by package_name
        assert_eq!(r.unlisted_dependencies[0].dep.package_name, "axios");
        assert_eq!(r.unlisted_dependencies[1].dep.package_name, "zod");

        // Inner sort: imported_from sorted by path, then line
        let zod_sites: Vec<_> = r.unlisted_dependencies[1]
            .dep
            .imported_from
            .iter()
            .map(|s| s.path.to_string_lossy().to_string())
            .collect();
        assert_eq!(zod_sites, vec!["a.ts", "b.ts"]);
    }

    // ── sort: duplicate_exports + inner locations ───────────────

    #[test]
    fn sort_duplicate_exports_by_name_and_inner_locations() {
        let mut r = AnalysisResults::default();
        r.duplicate_exports
            .push(DuplicateExportFinding::with_actions(DuplicateExport {
                export_name: "z".to_string(),
                locations: vec![
                    DuplicateLocation {
                        path: PathBuf::from("c.ts"),
                        line: 1,
                        col: 0,
                    },
                    DuplicateLocation {
                        path: PathBuf::from("a.ts"),
                        line: 5,
                        col: 0,
                    },
                ],
            }));
        r.duplicate_exports
            .push(DuplicateExportFinding::with_actions(DuplicateExport {
                export_name: "a".to_string(),
                locations: vec![DuplicateLocation {
                    path: PathBuf::from("b.ts"),
                    line: 1,
                    col: 0,
                }],
            }));
        r.sort();

        // Outer sort: by export_name
        assert_eq!(r.duplicate_exports[0].export.export_name, "a");
        assert_eq!(r.duplicate_exports[1].export.export_name, "z");

        // Inner sort: locations sorted by path, then line
        let z_locs: Vec<_> = r.duplicate_exports[1]
            .export
            .locations
            .iter()
            .map(|l| l.path.to_string_lossy().to_string())
            .collect();
        assert_eq!(z_locs, vec!["a.ts", "c.ts"]);
    }

    // ── sort: type_only_dependencies ────────────────────────────

    #[test]
    fn sort_type_only_dependencies() {
        let mut r = AnalysisResults::default();
        r.type_only_dependencies
            .push(TypeOnlyDependencyFinding::with_actions(
                TypeOnlyDependency {
                    package_name: "zod".to_string(),
                    path: PathBuf::from("package.json"),
                    line: 10,
                },
            ));
        r.type_only_dependencies
            .push(TypeOnlyDependencyFinding::with_actions(
                TypeOnlyDependency {
                    package_name: "ajv".to_string(),
                    path: PathBuf::from("package.json"),
                    line: 5,
                },
            ));
        r.sort();
        assert_eq!(r.type_only_dependencies[0].dep.package_name, "ajv");
        assert_eq!(r.type_only_dependencies[1].dep.package_name, "zod");
    }

    // ── sort: test_only_dependencies ────────────────────────────

    #[test]
    fn sort_test_only_dependencies() {
        let mut r = AnalysisResults::default();
        r.test_only_dependencies
            .push(TestOnlyDependencyFinding::with_actions(
                TestOnlyDependency {
                    package_name: "vitest".to_string(),
                    path: PathBuf::from("package.json"),
                    line: 15,
                },
            ));
        r.test_only_dependencies
            .push(TestOnlyDependencyFinding::with_actions(
                TestOnlyDependency {
                    package_name: "jest".to_string(),
                    path: PathBuf::from("package.json"),
                    line: 10,
                },
            ));
        r.sort();
        assert_eq!(r.test_only_dependencies[0].dep.package_name, "jest");
        assert_eq!(r.test_only_dependencies[1].dep.package_name, "vitest");
    }

    // ── sort: circular_dependencies by files, then length ───────

    #[test]
    fn sort_circular_dependencies_by_files_then_length() {
        let mut r = AnalysisResults::default();
        r.circular_dependencies
            .push(CircularDependencyFinding::with_actions(
                CircularDependency {
                    files: vec![PathBuf::from("b.ts"), PathBuf::from("c.ts")],
                    length: 2,
                    line: 1,
                    col: 0,
                    edges: Vec::new(),
                    is_cross_package: false,
                },
            ));
        r.circular_dependencies
            .push(CircularDependencyFinding::with_actions(
                CircularDependency {
                    files: vec![PathBuf::from("a.ts"), PathBuf::from("b.ts")],
                    length: 2,
                    line: 1,
                    col: 0,
                    edges: Vec::new(),
                    is_cross_package: true,
                },
            ));
        r.sort();
        assert_eq!(
            r.circular_dependencies[0].cycle.files[0],
            PathBuf::from("a.ts")
        );
        assert_eq!(
            r.circular_dependencies[1].cycle.files[0],
            PathBuf::from("b.ts")
        );
    }

    // ── sort: boundary_violations by from_path, line, col, to_path

    #[test]
    fn sort_boundary_violations() {
        let mut r = AnalysisResults::default();
        let mk = |from: &str, line: u32, col: u32, to: &str| {
            BoundaryViolationFinding::with_actions(BoundaryViolation {
                from_path: PathBuf::from(from),
                to_path: PathBuf::from(to),
                from_zone: "a".to_string(),
                to_zone: "b".to_string(),
                import_specifier: to.to_string(),
                line,
                col,
            })
        };
        r.boundary_violations.push(mk("z.ts", 1, 0, "a.ts"));
        r.boundary_violations.push(mk("a.ts", 5, 0, "b.ts"));
        r.boundary_violations.push(mk("a.ts", 1, 0, "c.ts"));
        r.sort();
        let from_paths: Vec<_> = r
            .boundary_violations
            .iter()
            .map(|v| {
                format!(
                    "{}:{}",
                    v.violation.from_path.to_string_lossy(),
                    v.violation.line
                )
            })
            .collect();
        assert_eq!(from_paths, vec!["a.ts:1", "a.ts:5", "z.ts:1"]);
    }

    // ── sort: export_usages + inner reference_locations ─────────

    #[test]
    fn sort_export_usages_and_inner_reference_locations() {
        let mut r = AnalysisResults::default();
        r.export_usages.push(ExportUsage {
            path: PathBuf::from("z.ts"),
            export_name: "foo".to_string(),
            line: 1,
            col: 0,
            reference_count: 2,
            reference_locations: vec![
                ReferenceLocation {
                    path: PathBuf::from("c.ts"),
                    line: 10,
                    col: 0,
                },
                ReferenceLocation {
                    path: PathBuf::from("a.ts"),
                    line: 5,
                    col: 0,
                },
            ],
        });
        r.export_usages.push(ExportUsage {
            path: PathBuf::from("a.ts"),
            export_name: "bar".to_string(),
            line: 1,
            col: 0,
            reference_count: 1,
            reference_locations: vec![ReferenceLocation {
                path: PathBuf::from("b.ts"),
                line: 1,
                col: 0,
            }],
        });
        r.sort();

        // Outer sort: by path, then line, then export_name
        assert_eq!(r.export_usages[0].path, PathBuf::from("a.ts"));
        assert_eq!(r.export_usages[1].path, PathBuf::from("z.ts"));

        // Inner sort: reference_locations sorted by path, line, col
        let refs: Vec<_> = r.export_usages[1]
            .reference_locations
            .iter()
            .map(|l| l.path.to_string_lossy().to_string())
            .collect();
        assert_eq!(refs, vec!["a.ts", "c.ts"]);
    }

    // ── sort: empty results does not panic ──────────────────────

    #[test]
    fn sort_empty_results_is_noop() {
        let mut r = AnalysisResults::default();
        r.sort(); // should not panic
        assert_eq!(r.total_issues(), 0);
    }

    // ── sort: single-element lists remain stable ────────────────

    #[test]
    fn sort_single_element_lists_stable() {
        let mut r = AnalysisResults::default();
        r.unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: PathBuf::from("only.ts"),
            }));
        r.sort();
        assert_eq!(r.unused_files[0].file.path, PathBuf::from("only.ts"));
    }

    // ── serialization ──────────────────────────────────────────

    #[test]
    fn serialize_empty_results() {
        let r = AnalysisResults::default();
        let json = serde_json::to_value(&r).unwrap();

        // All arrays should be present and empty
        assert!(json["unused_files"].as_array().unwrap().is_empty());
        assert!(json["unused_exports"].as_array().unwrap().is_empty());
        assert!(json["circular_dependencies"].as_array().unwrap().is_empty());

        // Skipped fields should be absent
        assert!(json.get("export_usages").is_none());
        assert!(json.get("entry_point_summary").is_none());
    }

    #[test]
    fn serialize_unused_file_path() {
        let r = UnusedFile {
            path: PathBuf::from("src/utils/index.ts"),
        };
        let json = serde_json::to_value(&r).unwrap();
        assert_eq!(json["path"], "src/utils/index.ts");
    }

    #[test]
    fn serialize_dependency_location_camel_case() {
        let dep = UnusedDependency {
            package_name: "react".to_string(),
            location: DependencyLocation::DevDependencies,
            path: PathBuf::from("package.json"),
            line: 5,
            used_in_workspaces: Vec::new(),
        };
        let json = serde_json::to_value(&dep).unwrap();
        assert_eq!(json["location"], "devDependencies");

        let dep2 = UnusedDependency {
            package_name: "react".to_string(),
            location: DependencyLocation::Dependencies,
            path: PathBuf::from("package.json"),
            line: 3,
            used_in_workspaces: Vec::new(),
        };
        let json2 = serde_json::to_value(&dep2).unwrap();
        assert_eq!(json2["location"], "dependencies");

        let dep3 = UnusedDependency {
            package_name: "fsevents".to_string(),
            location: DependencyLocation::OptionalDependencies,
            path: PathBuf::from("package.json"),
            line: 7,
            used_in_workspaces: Vec::new(),
        };
        let json3 = serde_json::to_value(&dep3).unwrap();
        assert_eq!(json3["location"], "optionalDependencies");
    }

    #[test]
    fn serialize_circular_dependency_skips_false_cross_package() {
        let cd = CircularDependency {
            files: vec![PathBuf::from("a.ts"), PathBuf::from("b.ts")],
            length: 2,
            line: 1,
            col: 0,
            edges: Vec::new(),
            is_cross_package: false,
        };
        let json = serde_json::to_value(&cd).unwrap();
        // skip_serializing_if = "std::ops::Not::not" means false is skipped
        assert!(json.get("is_cross_package").is_none());
    }

    #[test]
    fn serialize_circular_dependency_includes_true_cross_package() {
        let cd = CircularDependency {
            files: vec![PathBuf::from("a.ts"), PathBuf::from("b.ts")],
            length: 2,
            line: 1,
            col: 0,
            edges: Vec::new(),
            is_cross_package: true,
        };
        let json = serde_json::to_value(&cd).unwrap();
        assert_eq!(json["is_cross_package"], true);
    }

    #[test]
    fn serialize_unused_export_fields() {
        let e = UnusedExport {
            path: PathBuf::from("src/mod.ts"),
            export_name: "helper".to_string(),
            is_type_only: true,
            line: 42,
            col: 7,
            span_start: 100,
            is_re_export: true,
        };
        let json = serde_json::to_value(&e).unwrap();
        assert_eq!(json["path"], "src/mod.ts");
        assert_eq!(json["export_name"], "helper");
        assert_eq!(json["is_type_only"], true);
        assert_eq!(json["line"], 42);
        assert_eq!(json["col"], 7);
        assert_eq!(json["span_start"], 100);
        assert_eq!(json["is_re_export"], true);
    }

    #[test]
    fn serialize_boundary_violation_fields() {
        let v = BoundaryViolation {
            from_path: PathBuf::from("src/ui/button.tsx"),
            to_path: PathBuf::from("src/db/queries.ts"),
            from_zone: "ui".to_string(),
            to_zone: "db".to_string(),
            import_specifier: "../db/queries".to_string(),
            line: 3,
            col: 0,
        };
        let json = serde_json::to_value(&v).unwrap();
        assert_eq!(json["from_path"], "src/ui/button.tsx");
        assert_eq!(json["to_path"], "src/db/queries.ts");
        assert_eq!(json["from_zone"], "ui");
        assert_eq!(json["to_zone"], "db");
        assert_eq!(json["import_specifier"], "../db/queries");
    }

    #[test]
    fn serialize_unlisted_dependency_with_import_sites() {
        let d = UnlistedDependency {
            package_name: "chalk".to_string(),
            imported_from: vec![
                ImportSite {
                    path: PathBuf::from("a.ts"),
                    line: 1,
                    col: 0,
                },
                ImportSite {
                    path: PathBuf::from("b.ts"),
                    line: 5,
                    col: 3,
                },
            ],
        };
        let json = serde_json::to_value(&d).unwrap();
        assert_eq!(json["package_name"], "chalk");
        let sites = json["imported_from"].as_array().unwrap();
        assert_eq!(sites.len(), 2);
        assert_eq!(sites[0]["path"], "a.ts");
        assert_eq!(sites[1]["line"], 5);
    }

    #[test]
    fn serialize_duplicate_export_with_locations() {
        let d = DuplicateExport {
            export_name: "Button".to_string(),
            locations: vec![
                DuplicateLocation {
                    path: PathBuf::from("src/a.ts"),
                    line: 10,
                    col: 0,
                },
                DuplicateLocation {
                    path: PathBuf::from("src/b.ts"),
                    line: 20,
                    col: 5,
                },
            ],
        };
        let json = serde_json::to_value(&d).unwrap();
        assert_eq!(json["export_name"], "Button");
        let locs = json["locations"].as_array().unwrap();
        assert_eq!(locs.len(), 2);
        assert_eq!(locs[0]["line"], 10);
        assert_eq!(locs[1]["col"], 5);
    }

    #[test]
    fn serialize_type_only_dependency() {
        let d = TypeOnlyDependency {
            package_name: "@types/react".to_string(),
            path: PathBuf::from("package.json"),
            line: 12,
        };
        let json = serde_json::to_value(&d).unwrap();
        assert_eq!(json["package_name"], "@types/react");
        assert_eq!(json["line"], 12);
    }

    #[test]
    fn serialize_test_only_dependency() {
        let d = TestOnlyDependency {
            package_name: "vitest".to_string(),
            path: PathBuf::from("package.json"),
            line: 8,
        };
        let json = serde_json::to_value(&d).unwrap();
        assert_eq!(json["package_name"], "vitest");
        assert_eq!(json["line"], 8);
    }

    #[test]
    fn serialize_unused_member() {
        let m = UnusedMember {
            path: PathBuf::from("enums.ts"),
            parent_name: "Status".to_string(),
            member_name: "Pending".to_string(),
            kind: MemberKind::EnumMember,
            line: 3,
            col: 4,
        };
        let json = serde_json::to_value(&m).unwrap();
        assert_eq!(json["parent_name"], "Status");
        assert_eq!(json["member_name"], "Pending");
        assert_eq!(json["line"], 3);
    }

    #[test]
    fn serialize_unresolved_import() {
        let i = UnresolvedImport {
            path: PathBuf::from("app.ts"),
            specifier: "./missing-module".to_string(),
            line: 7,
            col: 0,
            specifier_col: 21,
        };
        let json = serde_json::to_value(&i).unwrap();
        assert_eq!(json["specifier"], "./missing-module");
        assert_eq!(json["specifier_col"], 21);
    }

    // ── deserialize: CircularDependency serde(default) fields ──

    #[test]
    fn deserialize_circular_dependency_with_defaults() {
        // CircularDependency derives Deserialize; line/col/is_cross_package have #[serde(default)]
        let json = r#"{"files":["a.ts","b.ts"],"length":2}"#;
        let cd: CircularDependency = serde_json::from_str(json).unwrap();
        assert_eq!(cd.files.len(), 2);
        assert_eq!(cd.length, 2);
        assert_eq!(cd.line, 0);
        assert_eq!(cd.col, 0);
        assert!(!cd.is_cross_package);
    }

    #[test]
    fn deserialize_circular_dependency_with_all_fields() {
        let json =
            r#"{"files":["a.ts","b.ts"],"length":2,"line":5,"col":10,"is_cross_package":true}"#;
        let cd: CircularDependency = serde_json::from_str(json).unwrap();
        assert_eq!(cd.line, 5);
        assert_eq!(cd.col, 10);
        assert!(cd.is_cross_package);
    }

    // ── clone produces independent copies ───────────────────────

    #[test]
    fn clone_results_are_independent() {
        let mut r = AnalysisResults::default();
        r.unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: PathBuf::from("a.ts"),
            }));
        let mut cloned = r.clone();
        cloned
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: PathBuf::from("b.ts"),
            }));
        assert_eq!(r.total_issues(), 1);
        assert_eq!(cloned.total_issues(), 2);
    }

    // ── export_usages not counted in total_issues ───────────────

    #[test]
    fn export_usages_not_counted_in_total_issues() {
        let mut r = AnalysisResults::default();
        r.export_usages.push(ExportUsage {
            path: PathBuf::from("mod.ts"),
            export_name: "foo".to_string(),
            line: 1,
            col: 0,
            reference_count: 3,
            reference_locations: vec![],
        });
        // export_usages is metadata, not an issue type
        assert_eq!(r.total_issues(), 0);
        assert!(!r.has_issues());
    }

    // ── entry_point_summary not counted in total_issues ─────────

    #[test]
    fn entry_point_summary_not_counted_in_total_issues() {
        let r = AnalysisResults {
            entry_point_summary: Some(EntryPointSummary {
                total: 10,
                by_source: vec![("config".to_string(), 10)],
            }),
            ..AnalysisResults::default()
        };
        assert_eq!(r.total_issues(), 0);
        assert!(!r.has_issues());
    }
}
