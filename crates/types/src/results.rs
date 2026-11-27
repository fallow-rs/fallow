//! Analysis result types for all issue categories.

use std::path::{Path, PathBuf};

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
    CircularDependencyFinding, DeprecatedExportInUseFinding, DevDependencyInProductionFinding,
    DuplicateExportAliasFinding, DuplicateExportFinding, DuplicatePropShapeFinding,
    DynamicSegmentNameConflictFinding, EmptyCatalogGroupFinding, InvalidClientExportFinding,
    MisconfiguredDependencyOverrideFinding, MisplacedDirectiveFinding,
    MixedClientServerBarrelFinding, NamespaceEscapeFinding, PackageCycleFinding,
    PolicyViolationFinding, PrivateTypeLeakFinding, PropDrillingChainFinding, ReExportCycleFinding,
    RouteCollisionFinding, TestOnlyDependencyFinding, TestOnlyExportFinding, ThinWrapperFinding,
    TypeOnlyDependencyFinding, UnlistedBinaryFinding, UnlistedDependencyFinding,
    UnprovidedInjectFinding, UnrenderedComponentFinding, UnresolvedCatalogReferenceFinding,
    UnresolvedImportFinding, UnusedCatalogEntryFinding, UnusedClassMemberFinding,
    UnusedComponentEmitFinding, UnusedComponentInputFinding, UnusedComponentOutputFinding,
    UnusedComponentPropFinding, UnusedDependencyFinding, UnusedDependencyOverrideFinding,
    UnusedDevDependencyFinding, UnusedEnumMemberFinding, UnusedExportFinding, UnusedFileFinding,
    UnusedLoadDataKeyFinding, UnusedNamespaceMemberFinding, UnusedOptionalDependencyFinding,
    UnusedProtectedClassMemberFinding, UnusedServerActionFinding, UnusedStoreMemberFinding,
    UnusedSvelteEventFinding, UnusedTypeFinding,
};
pub use crate::output_dead_code::{
    UnproducedCatchTagFinding, UnusedEffectLayerFinding, UnusedTaggedErrorFinding,
};
#[path = "results_boundary.rs"]
mod boundary;
#[path = "results_cycles.rs"]
mod cycles;
pub use cycles::{
    CircularDependency, CircularDependencyEdge, PackageCycle, PackageCycleEdge, ReExportCycle,
    ReExportCycleKind,
};
#[path = "results_namespace_escape.rs"]
mod namespace_escape;
pub use namespace_escape::{NamespaceEscape, NamespaceEscapeReason};
#[path = "results_security_finding.rs"]
mod security_finding;
pub use security_finding::SecurityFinding;
#[path = "results_test_only_export.rs"]
mod test_only_export;
pub use test_only_export::TestOnlyExport;
#[path = "results_trace_hop.rs"]
mod trace_hop;
pub use trace_hop::{TraceHop, TraceHopRole, TraceSecretRead};
#[path = "results_caller_review.rs"]
mod caller_review;
#[path = "results_component_health.rs"]
mod component_health;
pub use boundary::{BoundaryViolation, BoundaryViolationReason};
pub use caller_review::*;

pub use component_health::ERROR_ONLY_RESULT_KEYS;

use crate::serde_path;
#[path = "results_ignore_findings.rs"]
mod ignore_findings;
#[path = "results_stale_suppression_text.rs"]
mod stale_suppression_text;
use ignore_findings::classify_ignore_findings_fields;

#[path = "results_active_suppression.rs"]
mod active_suppression;
pub use active_suppression::ActiveSuppression;
#[path = "results_inventory_merge.rs"]
mod inventory_merge;
#[path = "results_merge_parts.rs"]
mod merge_parts;
use merge_parts::split_merge_parts;

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

#[path = "results_component_intel.rs"]
mod component_intel;
pub use component_intel::*;

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
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct AnalysisResults {
    /// Optional architecture inventory, excluded from issue totals and gates.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shared_state: Option<crate::shared_state::SharedStateInventory>,
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
    /// Exports marked `@deprecated` that still have at least one consumer in
    /// a reachable file. Wrapped in [`DeprecatedExportInUseFinding`]. Opt-in: the
    /// `deprecated-exports-in-use` rule defaults to `off`.
    #[serde(default)]
    pub deprecated_exports_in_use: Vec<DeprecatedExportInUseFinding>,
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
    /// Exported members of an exported TypeScript namespace that no code
    /// reads. Wrapped in [`UnusedNamespaceMemberFinding`]: same inner
    /// [`UnusedMember`] struct as `unused_enum_members`. `member_name` is the
    /// dotted path from the exported namespace (`Inner.x`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unused_namespace_members: Vec<UnusedNamespaceMemberFinding>,
    /// `protected` class members that neither the class nor a subclass
    /// reads. Wrapped in [`UnusedProtectedClassMemberFinding`]: same inner
    /// [`UnusedMember`] struct as `unused_class_members`. The rule is `off`
    /// by default, so this array is empty unless a config turns it on.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unused_protected_class_members: Vec<UnusedProtectedClassMemberFinding>,
    /// Namespace bindings that the code uses as a whole value, so every
    /// export of the target module gets credit and stays unchecked. The rule
    /// is `off` by default, so this array is empty unless a config turns it on.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub namespace_escapes: Vec<NamespaceEscapeFinding>,
    /// Exported values that only test-side files use. Default-scope runs
    /// only: a production run reports such an export as an unused export.
    /// The rule is `off` by default, so this array is empty unless a config
    /// turns it on.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub test_only_exports: Vec<TestOnlyExportFinding>,
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
    /// Binaries that package.json scripts or process calls run, but that no
    /// installed or listed package provides. Wrapped in
    /// [`UnlistedBinaryFinding`].
    #[serde(default)]
    pub unlisted_binaries: Vec<UnlistedBinaryFinding>,
    /// Exports with the same name across multiple modules. Wrapped in
    /// [`DuplicateExportFinding`] so each entry carries a typed `actions`
    /// array natively, with the position-0 `add-to-config` `ignoreExports`
    /// snippet wired in at wrapper construction.
    pub duplicate_exports: Vec<DuplicateExportFinding>,
    /// One module that exports one binding under two or more names. Wrapped
    /// in [`DuplicateExportAliasFinding`]. Default severity is `warn`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub duplicate_export_aliases: Vec<DuplicateExportAliasFinding>,
    /// Production dependencies only used via type-only imports (could be
    /// devDependencies). Only populated in production mode. Wrapped in
    /// [`TypeOnlyDependencyFinding`].
    pub type_only_dependencies: Vec<TypeOnlyDependencyFinding>,
    /// Production dependencies only imported by test files (could be
    /// devDependencies). Wrapped in [`TestOnlyDependencyFinding`].
    #[serde(default)]
    pub test_only_dependencies: Vec<TestOnlyDependencyFinding>,
    /// devDependencies imported by production (non-test, non-config) source code
    /// via a runtime/value import; they should be promoted to dependencies.
    /// The promote-side mirror of [`TestOnlyDependencyFinding`]. Wrapped in
    /// [`DevDependencyInProductionFinding`].
    #[serde(default)]
    pub dev_dependencies_in_production: Vec<DevDependencyInProductionFinding>,
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
    /// Dependency cycles between workspace packages, built from resolved
    /// cross-package imports. Wrapped in [`PackageCycleFinding`] so each
    /// entry carries a typed `actions` array natively.
    #[serde(default)]
    pub package_cycles: Vec<PackageCycleFinding>,
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
    /// Banned calls, imports, and catalogue-derived effects matched by
    /// declarative rule packs
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
    /// Entries in pnpm-workspace.yaml's overrides section, package.json's
    /// pnpm.overrides block, npm or Bun's top-level overrides object, or Bun's
    /// top-level resolutions object,
    /// whose target package is not declared by any workspace package and is
    /// not present in pnpm-lock.yaml, package-lock.json, npm-shrinkwrap.json,
    /// or bun.lock. Default severity is warn because projects without a
    /// readable lockfile fall back to manifest-only checks; the hint field
    /// flags those conservative cases. When the only lockfile is bun's binary
    /// bun.lockb, resolution cannot be read and the check emits nothing.
    /// Wrapped in [`UnusedDependencyOverrideFinding`].
    #[serde(default)]
    pub unused_dependency_overrides: Vec<UnusedDependencyOverrideFinding>,
    /// Package-manager override or resolution entries whose key or value does
    /// not parse in the declaration source's grammar (empty key, empty value,
    /// malformed selector, unbalanced parent matcher). The package manager may
    /// reject or ignore these at install time. Default severity is error. Wrapped in
    /// [`MisconfiguredDependencyOverrideFinding`].
    #[serde(default)]
    pub misconfigured_dependency_overrides: Vec<MisconfiguredDependencyOverrideFinding>,
    /// `"use client"` files that export a Next.js server-only / route-segment
    /// config name (e.g. `metadata`, `revalidate`, `GET`). Next.js rejects this
    /// at build time. Wrapped in [`InvalidClientExportFinding`] so each entry
    /// carries a typed `actions` array natively. Default severity is `warn`.
    #[serde(default)]
    pub invalid_client_exports: Vec<InvalidClientExportFinding>,
    /// Barrel files that re-export both a `"use client"` origin module and a
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
    /// Used optional component inputs absent from inspected reachable callers. Off by default.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub absent_component_props: Vec<AbsentComponentPropFinding>,
    /// Parameters that each visible call passes in one way. Off by default.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub constant_parameters: Vec<ConstantParameterFinding>,
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
    /// Effect layers composed only into other layers that nothing uses.
    /// Gated on the project declaring `effect`. Default severity is `warn`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unused_effect_layers: Vec<UnusedEffectLayerFinding>,
    /// Effect tagged error classes that only type positions or `catchTag`
    /// handlers reference. Gated on the project declaring `effect`. Default
    /// severity is `warn`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unused_tagged_errors: Vec<UnusedTaggedErrorFinding>,
    /// `catchTag` / `catchTags` handlers for a project tag that no code
    /// produces and that no `unused_tagged_errors` entry covers. Gated on the
    /// project declaring `effect`. Default severity is `warn`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unproduced_catch_tags: Vec<UnproducedCatchTagFinding>,
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
    /// (opt-in), so this is dormant and populated only when the user enables it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub prop_drilling_chains: Vec<PropDrillingChainFinding>,
    /// React/Preact components whose entire body is a single spread-forwarded
    /// child render (`return <Child {...props}/>`): pure structural indirection,
    /// a candidate for inlining at call sites. Wrapped in [`ThinWrapperFinding`]
    /// so each entry carries a typed `actions` array natively. Health signal: the
    /// rule defaults to `off` (opt-in), so this is dormant and populated only
    /// when the user enables it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub thin_wrappers: Vec<ThinWrapperFinding>,
    /// React/Preact components that participate in a duplicate-prop-shape group:
    /// three or more components across two or more files whose statically-known
    /// prop NAME set is identical after stripping ubiquitous DOM / passthrough
    /// names (a missing shared `Props` type / base component). Wrapped in
    /// [`DuplicatePropShapeFinding`] so each entry carries a typed `actions`
    /// array and its sibling roster natively. Health signal: the rule defaults to
    /// `off` (opt-in), so this is dormant and populated only when the user
    /// enables it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub duplicate_prop_shapes: Vec<DuplicatePropShapeFinding>,
    /// Number of `unused_exports`, `unused_types`, `unused_class_members` and
    /// `unused_enum_members` findings that this run did not report because
    /// their file is an unused file. Deleting the file removes them, so they
    /// add no information. `--show-cascade` or the `showCascade` config key
    /// keeps them in the report, and then this count is zero. The count is
    /// taken after the scope and the rules and before a baseline. Serialized
    /// only when not zero, so a run without such findings keeps its JSON.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub cascade_hidden: usize,
    /// In-process state of the unused-file cascade filter: the unused-file
    /// candidates of the detector and the findings that the filter removed.
    /// Skipped in machine output; [`Self::cascade_hidden`] is the public count.
    #[serde(skip)]
    #[cfg_attr(feature = "schema", schemars(skip))]
    pub cascade: CascadeState,
    /// Number of suppression entries that matched an issue during analysis.
    /// Human output uses this for the suppression footer; it is skipped in
    /// machine output to avoid changing the public JSON issue contract.
    #[serde(skip)]
    pub suppression_count: usize,
    /// Number of component props exempted from `unused-component-props` this run
    /// because their local destructure binding name matched
    /// `unusedComponentProps.ignorePattern`. Drives a human-output note so a
    /// mistyped pattern (matching nothing) is not a silent no-op; skipped in
    /// machine output, like [`Self::suppression_count`].
    #[serde(skip)]
    pub unused_component_props_exempted: usize,
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
    /// Security candidates to verify, not verified vulnerabilities. Set only when a `security_*`
    /// rule is on (`fallow security` turns them on). Skipped in serialization and not counted in
    /// `total_issues`, so bare `fallow` never shows them; `fallow security` has its own envelope.
    #[serde(skip)]
    pub security_findings: Vec<SecurityFinding>,
    /// Inline-suppressed security candidates. The stable id ordinals count them; then cleared.
    #[serde(skip)]
    pub suppressed_security_findings: Vec<SecurityFinding>,
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
    /// The type exports that `unused-type` keeps because a same-file
    /// signature uses them. Internal: the trace reads it to give the same
    /// reason as the report. Skipped in machine output.
    #[serde(skip)]
    #[cfg_attr(feature = "schema", schemars(skip))]
    pub signature_backed_types: Vec<crate::trace::SignatureBackedType>,
    /// Summary of detected entry points, grouped by discovery source.
    /// Not included in issue counts -- this is informational metadata.
    /// Skipped during serialization: rendered separately in JSON output.
    #[serde(skip)]
    pub entry_point_summary: Option<EntryPointSummary>,
    /// Internal component render metrics, exposed through health aggregates.
    #[serde(skip)]
    pub render_fan_in: Option<RenderFanInMetric>,
    /// Internal component intelligence used by editor overlays.
    #[serde(skip)]
    pub react_component_intel: Vec<ReactComponentIntel>,
    /// Reachable files in the discovery scope, captured before report filters.
    /// Empty or deserialized scope cannot grant semantic usage credit.
    #[serde(skip)]
    #[cfg_attr(feature = "schema", schemars(skip))]
    pub semantic_reachable_files: rustc_hash::FxHashSet<PathBuf>,
    /// Plugin-owned framework contracts carried only into the optional
    /// semantic reconciliation pass.
    #[serde(skip)]
    #[cfg_attr(feature = "schema", schemars(skip))]
    pub semantic_framework_contracts: Vec<crate::semantic::SemanticFrameworkContract>,
    /// `constant-parameter` candidates for the optional semantic pass.
    #[serde(skip)]
    #[cfg_attr(feature = "schema", schemars(skip))]
    pub constant_parameter_candidates: ConstantParameterCandidates,
}

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde `skip_serializing_if` passes the field by reference"
)]
const fn is_zero(count: &usize) -> bool {
    *count == 0
}

/// In-process state of the unused-file cascade filter.
///
/// An unused file in the report makes every export and member finding in it
/// redundant: deleting the file removes them. The filter needs the files that
/// a suppression comment removed from the report, and it keeps the findings
/// that it removed, because a baseline entry for such a finding still
/// describes the project and must not count as stale.
#[derive(Debug, Default, Clone)]
pub struct CascadeState {
    /// Unused files whose finding an inline `fallow-ignore-file unused-file`
    /// comment suppressed. The cascade filter treats them as reported, so a
    /// suppression comment never makes a hidden finding visible again. An
    /// issue-type selection without unused files clears this list.
    pub suppressed_unused_files: Vec<PathBuf>,
    /// The findings that the filter removed from the report.
    pub hidden: CascadeHiddenFindings,
}

/// Findings that the unused-file cascade filter removed from the report.
#[derive(Debug, Default, Clone)]
pub struct CascadeHiddenFindings {
    /// Hidden `unused_exports` findings.
    pub unused_exports: Vec<UnusedExportFinding>,
    /// Hidden `unused_types` findings.
    pub unused_types: Vec<UnusedTypeFinding>,
    /// Hidden `unused_enum_members` findings.
    pub unused_enum_members: Vec<UnusedEnumMemberFinding>,
    /// Hidden `unused_class_members` findings.
    pub unused_class_members: Vec<UnusedClassMemberFinding>,
}

impl CascadeHiddenFindings {
    /// Number of hidden findings.
    #[must_use]
    pub fn len(&self) -> usize {
        self.unused_exports.len()
            + self.unused_types.len()
            + self.unused_enum_members.len()
            + self.unused_class_members.len()
    }

    /// Whether the filter removed no finding.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The hidden findings as a result set, for a baseline comparison.
    #[must_use]
    pub fn to_results(&self) -> AnalysisResults {
        AnalysisResults {
            unused_exports: self.unused_exports.clone(),
            unused_types: self.unused_types.clone(),
            unused_enum_members: self.unused_enum_members.clone(),
            unused_class_members: self.unused_class_members.clone(),
            ..AnalysisResults::default()
        }
    }

    fn extend(&mut self, other: Self) {
        self.unused_exports.extend(other.unused_exports);
        self.unused_types.extend(other.unused_types);
        self.unused_enum_members.extend(other.unused_enum_members);
        self.unused_class_members.extend(other.unused_class_members);
    }
}

struct AnalysisResultsCoreMergeParts {
    unused_files: Vec<UnusedFileFinding>,
    unused_exports: Vec<UnusedExportFinding>,
    unused_types: Vec<UnusedTypeFinding>,
    private_type_leaks: Vec<PrivateTypeLeakFinding>,
    deprecated_exports_in_use: Vec<DeprecatedExportInUseFinding>,
    unused_enum_members: Vec<UnusedEnumMemberFinding>,
    unused_class_members: Vec<UnusedClassMemberFinding>,
    unused_namespace_members: Vec<UnusedNamespaceMemberFinding>,
    unused_protected_class_members: Vec<UnusedProtectedClassMemberFinding>,
    namespace_escapes: Vec<NamespaceEscapeFinding>,
    test_only_exports: Vec<TestOnlyExportFinding>,
    unused_store_members: Vec<UnusedStoreMemberFinding>,
    unresolved_imports: Vec<UnresolvedImportFinding>,
    boundary_violations: Vec<BoundaryViolationFinding>,
    boundary_coverage_violations: Vec<BoundaryCoverageViolationFinding>,
    boundary_call_violations: Vec<BoundaryCallViolationFinding>,
    policy_violations: Vec<PolicyViolationFinding>,
    stale_suppressions: Vec<StaleSuppression>,
}

struct AnalysisResultsGraphMergeParts {
    unused_dependencies: Vec<UnusedDependencyFinding>,
    unused_dev_dependencies: Vec<UnusedDevDependencyFinding>,
    unused_optional_dependencies: Vec<UnusedOptionalDependencyFinding>,
    unlisted_dependencies: Vec<UnlistedDependencyFinding>,
    unlisted_binaries: Vec<UnlistedBinaryFinding>,
    duplicate_exports: Vec<DuplicateExportFinding>,
    duplicate_export_aliases: Vec<DuplicateExportAliasFinding>,
    type_only_dependencies: Vec<TypeOnlyDependencyFinding>,
    test_only_dependencies: Vec<TestOnlyDependencyFinding>,
    dev_dependencies_in_production: Vec<DevDependencyInProductionFinding>,
    circular_dependencies: Vec<CircularDependencyFinding>,
    re_export_cycles: Vec<ReExportCycleFinding>,
    package_cycles: Vec<PackageCycleFinding>,
}

struct AnalysisResultsWorkspaceMergeParts {
    unused_catalog_entries: Vec<UnusedCatalogEntryFinding>,
    empty_catalog_groups: Vec<EmptyCatalogGroupFinding>,
    unresolved_catalog_references: Vec<UnresolvedCatalogReferenceFinding>,
    unused_dependency_overrides: Vec<UnusedDependencyOverrideFinding>,
    misconfigured_dependency_overrides: Vec<MisconfiguredDependencyOverrideFinding>,
}

struct AnalysisResultsFrameworkMergeParts {
    invalid_client_exports: Vec<InvalidClientExportFinding>,
    mixed_client_server_barrels: Vec<MixedClientServerBarrelFinding>,
    misplaced_directives: Vec<MisplacedDirectiveFinding>,
    unprovided_injects: Vec<UnprovidedInjectFinding>,
    unrendered_components: Vec<UnrenderedComponentFinding>,
    route_collisions: Vec<RouteCollisionFinding>,
    dynamic_segment_name_conflicts: Vec<DynamicSegmentNameConflictFinding>,
    unused_component_props: Vec<UnusedComponentPropFinding>,
    absent_component_props: Vec<AbsentComponentPropFinding>,
    constant_parameters: Vec<ConstantParameterFinding>,
    unused_component_emits: Vec<UnusedComponentEmitFinding>,
    unused_component_inputs: Vec<UnusedComponentInputFinding>,
    unused_component_outputs: Vec<UnusedComponentOutputFinding>,
    unused_svelte_events: Vec<UnusedSvelteEventFinding>,
    unused_effect_layers: Vec<UnusedEffectLayerFinding>,
    unused_tagged_errors: Vec<UnusedTaggedErrorFinding>,
    unproduced_catch_tags: Vec<UnproducedCatchTagFinding>,
    unused_server_actions: Vec<UnusedServerActionFinding>,
    unused_load_data_keys: Vec<UnusedLoadDataKeyFinding>,
    unused_load_data_keys_global_abstain: bool,
    prop_drilling_chains: Vec<PropDrillingChainFinding>,
    thin_wrappers: Vec<ThinWrapperFinding>,
    duplicate_prop_shapes: Vec<DuplicatePropShapeFinding>,
}

struct AnalysisResultsMetadataMergeParts {
    shared_state: Option<crate::shared_state::SharedStateInventory>,
    cascade_hidden: usize,
    cascade: CascadeState,
    suppression_count: usize,
    unused_component_props_exempted: usize,
    active_suppressions: Vec<ActiveSuppression>,
    feature_flags: Vec<FeatureFlag>,
    security_findings: Vec<SecurityFinding>,
    suppressed_security_findings: Vec<SecurityFinding>,
    security_unresolved_edge_files: usize,
    security_unresolved_callee_sites: usize,
    security_unresolved_callee_diagnostics: Vec<SecurityUnresolvedCalleeDiagnostic>,
    export_usages: Vec<ExportUsage>,
    signature_backed_types: Vec<crate::trace::SignatureBackedType>,
    entry_point_summary: Option<EntryPointSummary>,
    render_fan_in: Option<RenderFanInMetric>,
    react_component_intel: Vec<ReactComponentIntel>,
    semantic_reachable_files: rustc_hash::FxHashSet<PathBuf>,
    semantic_framework_contracts: Vec<crate::semantic::SemanticFrameworkContract>,
    constant_parameter_candidates: ConstantParameterCandidates,
}

macro_rules! counted_analysis_result_fields {
    ($callback:ident $(, $arg:expr)? ) => {
        $callback! {
            $($arg,)?
            unused_files => "unused_files",
            unused_exports => "unused_exports",
            unused_types => "unused_types",
            private_type_leaks => "private_type_leaks",
            deprecated_exports_in_use => "deprecated_exports_in_use",
            unused_dependencies => "unused_dependencies",
            unused_dev_dependencies => "unused_dev_dependencies",
            unused_optional_dependencies => "unused_optional_dependencies",
            unused_enum_members => "unused_enum_members",
            unused_class_members => "unused_class_members",
            unused_namespace_members => "unused_namespace_members",
            unused_protected_class_members => "unused_protected_class_members",
            namespace_escapes => "namespace_escapes",
            test_only_exports => "test_only_exports",
            unused_store_members => "unused_store_members",
            unresolved_imports => "unresolved_imports",
            unlisted_dependencies => "unlisted_dependencies",
            unlisted_binaries => "unlisted_binaries",
            duplicate_exports => "duplicate_exports",
            duplicate_export_aliases => "duplicate_export_aliases",
            type_only_dependencies => "type_only_dependencies",
            test_only_dependencies => "test_only_dependencies",
            dev_dependencies_in_production => "dev_dependencies_in_production",
            circular_dependencies => "circular_dependencies",
            re_export_cycles => "re_export_cycles",
            package_cycles => "package_cycles",
            boundary_violations => "boundary_violations",
            boundary_coverage_violations => "boundary_coverage_violations",
            boundary_call_violations => "boundary_call_violations",
            policy_violations => "policy_violations",
            stale_suppressions => "stale_suppressions",
            unused_catalog_entries => "unused_catalog_entries",
            empty_catalog_groups => "empty_catalog_groups",
            unresolved_catalog_references => "unresolved_catalog_references",
            unused_dependency_overrides => "unused_dependency_overrides",
            misconfigured_dependency_overrides => "misconfigured_dependency_overrides",
            invalid_client_exports => "invalid_client_exports",
            mixed_client_server_barrels => "mixed_client_server_barrels",
            misplaced_directives => "misplaced_directives",
            unprovided_injects => "unprovided_injects",
            unrendered_components => "unrendered_components",
            route_collisions => "route_collisions",
            dynamic_segment_name_conflicts => "dynamic_segment_name_conflicts",
            unused_component_props => "unused_component_props",
            absent_component_props => "absent_component_props",
            constant_parameters => "constant_parameters",
            unused_component_emits => "unused_component_emits",
            unused_component_inputs => "unused_component_inputs",
            unused_component_outputs => "unused_component_outputs",
            unused_svelte_events => "unused_svelte_events",
            unused_effect_layers => "unused_effect_layers",
            unused_tagged_errors => "unused_tagged_errors",
            unproduced_catch_tags => "unproduced_catch_tags",
            unused_server_actions => "unused_server_actions",
            unused_load_data_keys => "unused_load_data_keys",
        }
    };
}

trait FindingIgnorePolicy {
    fn should_ignore(&self, predicate: &mut impl FnMut(&Path) -> bool) -> bool;
}

macro_rules! impl_single_source_dead_code {
    ($($finding:ty => $($field:ident).+),+ $(,)?) => {
        $(
            impl FindingIgnorePolicy for $finding {
                fn should_ignore(
                    &self,
                    predicate: &mut impl FnMut(&Path) -> bool,
                ) -> bool {
                    predicate(&self.$($field).+)
                }
            }
        )+
    };
}

impl_single_source_dead_code! {
    UnusedFileFinding => file.path,
    UnusedExportFinding => export.path,
    UnusedTypeFinding => export.path,
    PrivateTypeLeakFinding => leak.path,
    DeprecatedExportInUseFinding => export.path,
    UnusedEnumMemberFinding => member.path,
    UnusedClassMemberFinding => member.path,
    UnusedNamespaceMemberFinding => member.path,
    UnusedProtectedClassMemberFinding => member.path,
    NamespaceEscapeFinding => escape.path,
    TestOnlyExportFinding => export.path,
    UnusedStoreMemberFinding => member.path,
    UnresolvedImportFinding => import.path,
    UnprovidedInjectFinding => inject.path,
    UnrenderedComponentFinding => component.path,
    UnusedComponentPropFinding => prop.path,
    AbsentComponentPropFinding => prop.path,
    ConstantParameterFinding => parameter.path,
    UnusedComponentEmitFinding => emit.path,
    UnusedComponentInputFinding => input.path,
    UnusedComponentOutputFinding => output.path,
    UnusedSvelteEventFinding => event.path,
    UnusedEffectLayerFinding => layer.path,
    DuplicateExportAliasFinding => alias.path,
    UnusedTaggedErrorFinding => error.path,
    UnproducedCatchTagFinding => catch_tag.path,
    UnusedServerActionFinding => action.path,
    UnusedLoadDataKeyFinding => key.path,
    ThinWrapperFinding => wrapper.file,
    DuplicatePropShapeFinding => shape.file,
}

macro_rules! impl_never_ignored_finding {
    ($($finding:ty),+ $(,)?) => {
        $(
            impl FindingIgnorePolicy for $finding {
                fn should_ignore(
                    &self,
                    _predicate: &mut impl FnMut(&Path) -> bool,
                ) -> bool {
                    false
                }
            }
        )+
    };
}

impl_never_ignored_finding! {
    UnusedDependencyFinding,
    UnusedDevDependencyFinding,
    UnusedOptionalDependencyFinding,
    TypeOnlyDependencyFinding,
    TestOnlyDependencyFinding,
    DevDependencyInProductionFinding,
    UnusedCatalogEntryFinding,
    EmptyCatalogGroupFinding,
    UnresolvedCatalogReferenceFinding,
    UnusedDependencyOverrideFinding,
    MisconfiguredDependencyOverrideFinding,
    BoundaryViolationFinding,
    BoundaryCoverageViolationFinding,
    BoundaryCallViolationFinding,
    PolicyViolationFinding,
    StaleSuppression,
    InvalidClientExportFinding,
    MixedClientServerBarrelFinding,
    MisplacedDirectiveFinding,
    RouteCollisionFinding,
    DynamicSegmentNameConflictFinding,
}

impl FindingIgnorePolicy for UnlistedDependencyFinding {
    fn should_ignore(&self, predicate: &mut impl FnMut(&Path) -> bool) -> bool {
        all_nonempty_paths_match(
            self.dep.imported_from.iter().map(|site| &site.path),
            predicate,
        )
    }
}

impl FindingIgnorePolicy for UnlistedBinaryFinding {
    fn should_ignore(&self, predicate: &mut impl FnMut(&Path) -> bool) -> bool {
        all_nonempty_paths_match(self.binary.used_in.iter().map(|site| &site.path), predicate)
    }
}

impl FindingIgnorePolicy for DuplicateExportFinding {
    fn should_ignore(&self, predicate: &mut impl FnMut(&Path) -> bool) -> bool {
        all_nonempty_paths_match(
            self.export.locations.iter().map(|location| &location.path),
            predicate,
        )
    }
}

impl FindingIgnorePolicy for CircularDependencyFinding {
    fn should_ignore(&self, predicate: &mut impl FnMut(&Path) -> bool) -> bool {
        all_nonempty_paths_match(self.cycle.files.iter(), predicate)
    }
}

impl FindingIgnorePolicy for ReExportCycleFinding {
    fn should_ignore(&self, predicate: &mut impl FnMut(&Path) -> bool) -> bool {
        all_nonempty_paths_match(self.cycle.files.iter(), predicate)
    }
}

impl FindingIgnorePolicy for PackageCycleFinding {
    fn should_ignore(&self, predicate: &mut impl FnMut(&Path) -> bool) -> bool {
        all_nonempty_paths_match(self.cycle.edges.iter().map(|edge| &edge.path), predicate)
    }
}

impl FindingIgnorePolicy for PropDrillingChainFinding {
    fn should_ignore(&self, predicate: &mut impl FnMut(&Path) -> bool) -> bool {
        all_nonempty_paths_match(self.chain.hops.iter().map(|hop| &hop.file), predicate)
    }
}

/// Source-owned health families hidden by `ignoreFindings`.
///
/// These live outside the unconditional counted table: only explicit error
/// findings contribute to `total_issues`; warnings remain advisory.
macro_rules! uncounted_source_owned_result_fields {
    ($callback:ident $(, $arg:expr)? ) => {
        $callback! {
            $($arg,)?
            prop_drilling_chains => "prop_drilling_chains",
            thin_wrappers => "thin_wrappers",
            duplicate_prop_shapes => "duplicate_prop_shapes",
        }
    };
}

macro_rules! remove_configured_ignored_findings {
    ($state:expr, $($field:ident => $key:literal,)+) => {{
        let (results, predicate) = $state;
        $(
            results.$field.retain(|issue| {
                !issue.should_ignore(&mut *predicate)
            });
        )+
    }};
}

macro_rules! counted_result_key_slice {
    ($($field:ident => $key:literal,)+) => {
        &[$($key),+, "prop_drilling_chains", "thin_wrappers", "duplicate_prop_shapes"]
    };
}

macro_rules! counted_result_field_sum {
    ($results:expr, $($field:ident => $key:literal,)+) => {
        0 $(+ ($results).$field.len())+
    };
}
#[path = "results_key_findings.rs"]
mod result_key_findings;

/// Serialized `AnalysisResults` arrays that contribute to [`AnalysisResults::total_issues`].
pub const TOTAL_ISSUE_RESULT_KEYS: &[&str] =
    counted_analysis_result_fields!(counted_result_key_slice);

impl AnalysisResults {
    /// Remove the export and member findings whose file `covered` selects.
    ///
    /// Only `unused_exports`, `unused_types`, `unused_enum_members` and
    /// `unused_class_members` change. A removed finding moves to
    /// [`CascadeState::hidden`] and [`Self::cascade_hidden`] counts it. A second
    /// call removes only findings that a later stage added, so the pass is
    /// idempotent.
    pub fn hide_cascade_findings(&mut self, covered: &dyn Fn(&Path) -> bool) {
        let mut hidden = CascadeHiddenFindings::default();
        move_covered(
            &mut self.unused_exports,
            &mut hidden.unused_exports,
            |finding| finding.export.path.as_path(),
            covered,
        );
        move_covered(
            &mut self.unused_types,
            &mut hidden.unused_types,
            |finding| finding.export.path.as_path(),
            covered,
        );
        move_covered(
            &mut self.unused_enum_members,
            &mut hidden.unused_enum_members,
            |finding| finding.member.path.as_path(),
            covered,
        );
        move_covered(
            &mut self.unused_class_members,
            &mut hidden.unused_class_members,
            |finding| finding.member.path.as_path(),
            covered,
        );
        self.cascade_hidden += hidden.len();
        self.cascade.hidden.extend(hidden);
    }

    /// Move the findings that [`Self::hide_cascade_findings`] removed back
    /// into the report and reset [`Self::cascade_hidden`].
    ///
    /// Metrics that measure the code, such as the vital signs, read the
    /// result set after this call, so their value does not depend on
    /// `showCascade`.
    pub fn restore_cascade_findings(&mut self) {
        let hidden = std::mem::take(&mut self.cascade.hidden);
        self.unused_exports.extend(hidden.unused_exports);
        self.unused_types.extend(hidden.unused_types);
        self.unused_enum_members.extend(hidden.unused_enum_members);
        self.unused_class_members
            .extend(hidden.unused_class_members);
        self.cascade_hidden = 0;
    }

    /// Remove dead-code findings whose complete, non-empty source-owner set
    /// matches `is_ignored`.
    ///
    /// Architecture, policy, suppression-hygiene, framework-correctness,
    /// security, and package/project findings are retained. Context paths
    /// embedded in a dead-code finding are not owners.
    #[doc(hidden)]
    pub fn remove_ignored_dead_code_findings(&mut self, mut is_ignored: impl FnMut(&Path) -> bool) {
        classify_ignore_findings_fields(self);
        counted_analysis_result_fields!(
            remove_configured_ignored_findings,
            (&mut *self, &mut is_ignored)
        );
        uncounted_source_owned_result_fields!(
            remove_configured_ignored_findings,
            (&mut *self, &mut is_ignored)
        );
    }

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
        counted_analysis_result_fields!(counted_result_field_sum, self)
            + self.enforced_prop_drilling_count()
            + self.enforced_thin_wrapper_count()
            + self.enforced_duplicate_prop_shape_count()
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
        let (core, graph, workspace, framework, metadata) = split_merge_parts(other);
        self.merge_core_findings(core);
        self.merge_dependency_and_graph_findings(graph);
        self.merge_workspace_findings(workspace);
        self.merge_framework_findings(framework);
        self.merge_metadata_and_security(metadata);
    }

    fn merge_core_findings(&mut self, parts: AnalysisResultsCoreMergeParts) {
        self.unused_files.extend(parts.unused_files);
        self.unused_exports.extend(parts.unused_exports);
        self.unused_types.extend(parts.unused_types);
        self.private_type_leaks.extend(parts.private_type_leaks);
        self.deprecated_exports_in_use
            .extend(parts.deprecated_exports_in_use);
        self.unused_enum_members.extend(parts.unused_enum_members);
        self.unused_class_members.extend(parts.unused_class_members);
        self.unused_namespace_members
            .extend(parts.unused_namespace_members);
        self.unused_protected_class_members
            .extend(parts.unused_protected_class_members);
        self.namespace_escapes.extend(parts.namespace_escapes);
        self.test_only_exports.extend(parts.test_only_exports);
        self.unused_store_members.extend(parts.unused_store_members);
        self.unresolved_imports.extend(parts.unresolved_imports);
        self.boundary_violations.extend(parts.boundary_violations);
        self.boundary_coverage_violations
            .extend(parts.boundary_coverage_violations);
        self.boundary_call_violations
            .extend(parts.boundary_call_violations);
        self.policy_violations.extend(parts.policy_violations);
        self.stale_suppressions.extend(parts.stale_suppressions);
    }

    fn merge_dependency_and_graph_findings(&mut self, parts: AnalysisResultsGraphMergeParts) {
        self.unused_dependencies.extend(parts.unused_dependencies);
        self.unused_dev_dependencies
            .extend(parts.unused_dev_dependencies);
        self.unused_optional_dependencies
            .extend(parts.unused_optional_dependencies);
        self.unlisted_dependencies
            .extend(parts.unlisted_dependencies);
        self.unlisted_binaries.extend(parts.unlisted_binaries);
        self.duplicate_exports.extend(parts.duplicate_exports);
        self.duplicate_export_aliases
            .extend(parts.duplicate_export_aliases);
        self.type_only_dependencies
            .extend(parts.type_only_dependencies);
        self.test_only_dependencies
            .extend(parts.test_only_dependencies);
        self.dev_dependencies_in_production
            .extend(parts.dev_dependencies_in_production);
        self.circular_dependencies
            .extend(parts.circular_dependencies);
        self.re_export_cycles.extend(parts.re_export_cycles);
        self.package_cycles.extend(parts.package_cycles);
    }

    fn merge_workspace_findings(&mut self, parts: AnalysisResultsWorkspaceMergeParts) {
        self.unused_catalog_entries
            .extend(parts.unused_catalog_entries);
        self.empty_catalog_groups.extend(parts.empty_catalog_groups);
        self.unresolved_catalog_references
            .extend(parts.unresolved_catalog_references);
        self.unused_dependency_overrides
            .extend(parts.unused_dependency_overrides);
        self.misconfigured_dependency_overrides
            .extend(parts.misconfigured_dependency_overrides);
    }

    fn merge_framework_findings(&mut self, parts: AnalysisResultsFrameworkMergeParts) {
        self.invalid_client_exports
            .extend(parts.invalid_client_exports);
        self.mixed_client_server_barrels
            .extend(parts.mixed_client_server_barrels);
        self.misplaced_directives.extend(parts.misplaced_directives);
        self.unprovided_injects.extend(parts.unprovided_injects);
        self.unrendered_components
            .extend(parts.unrendered_components);
        self.route_collisions.extend(parts.route_collisions);
        self.dynamic_segment_name_conflicts
            .extend(parts.dynamic_segment_name_conflicts);
        self.unused_component_props
            .extend(parts.unused_component_props);
        self.absent_component_props
            .extend(parts.absent_component_props);
        self.constant_parameters.extend(parts.constant_parameters);
        self.unused_component_emits
            .extend(parts.unused_component_emits);
        self.unused_component_inputs
            .extend(parts.unused_component_inputs);
        self.unused_component_outputs
            .extend(parts.unused_component_outputs);
        self.unused_svelte_events.extend(parts.unused_svelte_events);
        self.unused_effect_layers.extend(parts.unused_effect_layers);
        self.unused_tagged_errors.extend(parts.unused_tagged_errors);
        self.unproduced_catch_tags
            .extend(parts.unproduced_catch_tags);
        self.unused_server_actions
            .extend(parts.unused_server_actions);
        self.unused_load_data_keys
            .extend(parts.unused_load_data_keys);
        self.unused_load_data_keys_global_abstain |= parts.unused_load_data_keys_global_abstain;
        self.prop_drilling_chains.extend(parts.prop_drilling_chains);
        self.thin_wrappers.extend(parts.thin_wrappers);
        self.duplicate_prop_shapes
            .extend(parts.duplicate_prop_shapes);
    }

    /// Sort all result arrays for deterministic output ordering.
    ///
    /// Parallel collection (rayon, `FxHashMap` iteration) does not guarantee
    /// insertion order, so the same project can produce different orderings
    /// across runs. This method canonicalises every result list by sorting on
    /// (path, line, col, name) so that JSON/SARIF/human output is stable.
    pub fn sort(&mut self) {
        self.semantic_framework_contracts.sort();
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

        self.deprecated_exports_in_use.sort_by(|a, b| {
            a.export
                .path
                .cmp(&b.export.path)
                .then(a.export.line.cmp(&b.export.line))
                .then(a.export.export_name.cmp(&b.export.export_name))
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

        self.namespace_escapes.sort_by(|a, b| {
            (
                &a.escape.path,
                a.escape.line,
                a.escape.col,
                &a.escape.local_name,
            )
                .cmp(&(
                    &b.escape.path,
                    b.escape.line,
                    b.escape.col,
                    &b.escape.local_name,
                ))
        });

        self.test_only_exports.sort_by(|a, b| {
            (
                &a.export.path,
                a.export.line,
                a.export.col,
                &a.export.export_name,
            )
                .cmp(&(
                    &b.export.path,
                    b.export.line,
                    b.export.col,
                    &b.export.export_name,
                ))
        });

        self.unused_protected_class_members.sort_by(|a, b| {
            a.member
                .path
                .cmp(&b.member.path)
                .then(a.member.line.cmp(&b.member.line))
                .then(a.member.parent_name.cmp(&b.member.parent_name))
                .then(a.member.member_name.cmp(&b.member.member_name))
        });

        self.unused_namespace_members.sort_by(|a, b| {
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
        self.sort_core_route_findings();
        self.sort_core_component_prop_and_emit_findings();
        self.sort_core_component_io_findings();
        self.sort_core_server_load_findings();
    }

    fn sort_core_route_findings(&mut self) {
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
    }

    fn sort_core_component_prop_and_emit_findings(&mut self) {
        sort_constant_parameters(&mut self.constant_parameters);
        self.absent_component_props.sort_by(|a, b| {
            a.prop
                .path
                .cmp(&b.prop.path)
                .then(a.prop.line.cmp(&b.prop.line))
                .then(a.prop.prop_name.cmp(&b.prop.prop_name))
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

        self.unused_svelte_events.sort_by(|a, b| {
            a.event
                .path
                .cmp(&b.event.path)
                .then(a.event.line.cmp(&b.event.line))
                .then(a.event.event_name.cmp(&b.event.event_name))
        });
        self.sort_effect_findings();
    }

    fn sort_effect_findings(&mut self) {
        self.unused_effect_layers.sort_by(|a, b| {
            a.layer
                .path
                .cmp(&b.layer.path)
                .then(a.layer.line.cmp(&b.layer.line))
                .then(a.layer.name.cmp(&b.layer.name))
        });
        self.unused_tagged_errors.sort_by(|a, b| {
            a.error
                .path
                .cmp(&b.error.path)
                .then(a.error.line.cmp(&b.error.line))
                .then(a.error.name.cmp(&b.error.name))
        });
        self.unproduced_catch_tags.sort_by(|a, b| {
            a.catch_tag
                .path
                .cmp(&b.catch_tag.path)
                .then(a.catch_tag.line.cmp(&b.catch_tag.line))
                .then(a.catch_tag.col.cmp(&b.catch_tag.col))
                .then(a.catch_tag.tag.cmp(&b.catch_tag.tag))
        });
    }

    fn sort_core_component_io_findings(&mut self) {
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
    }

    fn sort_core_server_load_findings(&mut self) {
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

        self.unlisted_binaries
            .sort_by(|a, b| a.binary.binary_name.cmp(&b.binary.binary_name));
        for binary in &mut self.unlisted_binaries {
            binary.binary.used_in.sort_by(|a, b| {
                a.path
                    .cmp(&b.path)
                    .then(a.line.cmp(&b.line))
                    .then(a.col.cmp(&b.col))
            });
        }

        self.duplicate_exports
            .sort_by(|a, b| a.export.export_name.cmp(&b.export.export_name));
        for dup in &mut self.duplicate_exports {
            dup.export
                .locations
                .sort_by(|a, b| a.path.cmp(&b.path).then(a.line.cmp(&b.line)));
        }
        self.duplicate_export_aliases.sort_by(|a, b| {
            a.alias
                .path
                .cmp(&b.alias.path)
                .then(a.alias.line.cmp(&b.alias.line))
                .then(a.alias.col.cmp(&b.alias.col))
                .then_with(|| a.alias.export_names.cmp(&b.alias.export_names))
        });

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

        self.dev_dependencies_in_production.sort_by(|a, b| {
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

        self.package_cycles.sort_by(|a, b| {
            a.cycle
                .length
                .cmp(&b.cycle.length)
                .then_with(|| a.cycle.packages.cmp(&b.cycle.packages))
        });

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
        self.sort_stale_suppressions();
        self.sort_unused_catalog_entries();
        self.sort_empty_catalog_groups();
        self.sort_unresolved_catalog_references();
        self.sort_unused_dependency_overrides();
    }

    fn sort_stale_suppressions(&mut self) {
        self.stale_suppressions.sort_by(|a, b| {
            a.path
                .cmp(&b.path)
                .then(a.line.cmp(&b.line))
                .then(a.col.cmp(&b.col))
        });
    }

    fn sort_unused_catalog_entries(&mut self) {
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
    }

    fn sort_empty_catalog_groups(&mut self) {
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
    }

    fn sort_unresolved_catalog_references(&mut self) {
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
    }

    fn sort_unused_dependency_overrides(&mut self) {
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

#[path = "results_export_findings.rs"]
mod export_findings;
pub use export_findings::*;

#[path = "results_framework_findings.rs"]
mod framework_findings;
pub use framework_findings::*;

#[path = "results_dependency_findings.rs"]
mod dependency_findings;
pub use dependency_findings::*;

#[path = "results_security_types.rs"]
mod security_types;
pub use security_types::*;

#[path = "results_catalog_findings.rs"]
mod catalog_findings;
pub use catalog_findings::*;

#[path = "results_policy_findings.rs"]
mod policy_findings;
pub use policy_findings::*;

#[path = "results_stale_suppression.rs"]
mod stale_suppression;
pub use stale_suppression::*;

const _: () = assert!(std::mem::size_of::<FeatureFlag>() <= 160);
#[path = "results_feature_flags.rs"]
mod feature_flags;
pub use feature_flags::*;

#[path = "results_helpers.rs"]
mod results_helpers;
use results_helpers::{all_nonempty_paths_match, catalog_sort_key, move_covered};

#[cfg(test)]
#[path = "results_tests.rs"]
mod tests;
