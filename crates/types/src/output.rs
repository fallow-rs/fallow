//! Types that describe fallow's JSON output contract.
//!
//! Today the JSON serialization layer (`crates/cli/src/report/json.rs`) builds
//! its output via `serde_json::json!` macros. The types defined here are the
//! schema-side counterpart of that output: they document, with Rust's type
//! system, the augmentations the JSON layer adds to each per-finding struct
//! (the `actions` array on every finding, the optional `introduced` flag in
//! audit-mode sub-results).
//!
//! The `schema-emit` binary derives `JsonSchema` for these types (gated by the
//! `schema` cargo feature) so the public `docs/output-schema.json` stays in
//! sync with the Rust source of truth. A future refactor will route the JSON
//! emission path through these types directly, eliminating the drift class
//! between the augmentation list here and the `serde_json::json!` builders.

use serde::{Deserialize, Serialize};

/// A suggested action attached to a finding in the JSON output. Each finding
/// carries an `actions` array; consumers (agents, IDE clients, CI bots) can
/// dispatch on the `type` discriminant to choose the right remediation.
///
/// The discriminator is `type` (snake_case `type` field), the payload uses the
/// matching kebab-case identifier per variant.
///
/// ## `auto_fixable` is per-finding, not per action type
///
/// Every action variant carries an `auto_fixable: bool` field. The value is
/// evaluated PER FINDING, not per action type: the same action type may
/// appear with `auto_fixable: true` on one finding and `auto_fixable: false`
/// on another, depending on per-instance guards in the `fallow fix` applier.
/// Agents that filter on `auto_fixable: true` must branch on the bool of
/// each individual finding's action, not on the action `type` alone.
///
/// Current per-instance flips:
///
/// - `remove-catalog-entry` (`unused-catalog-entries`): `true` only when the
///   finding's `hardcoded_consumers` array is empty and the source is
///   `pnpm-workspace.yaml`. When a workspace package still pins a hardcoded
///   version of the same package, `fallow fix` skips the entry to avoid
///   breaking `pnpm install`. Bun `package.json` catalog entries are also
///   emitted with `auto_fixable: false` because the current fixer is
///   YAML-only.
/// - `remove-dependency` vs `move-dependency` (dependency findings): when the
///   finding's `used_in_workspaces` array is non-empty, the primary action
///   flips to `move-dependency` with `auto_fixable: false` (`fallow fix` will
///   not remove a dependency that another workspace imports). On findings
///   without cross-workspace consumers the action stays `remove-dependency`
///   with `auto_fixable: true`.
/// - `add-to-config` for `ignoreExports` (`duplicate-exports`): `true` when
///   `fallow fix` can safely apply the action without further user setup.
///   That is: a fallow config file exists on disk, OR no config exists AND
///   the working directory is NOT inside a monorepo subpackage (in which
///   case the applier creates `.fallowrc.json` from `fallow init`'s
///   framework-aware scaffolding and layers the new rules on top).
///   `false` inside a monorepo subpackage with no workspace-root config
///   (the applier refuses to fragment per-package configs across the
///   monorepo and points at the workspace root instead).
/// - `update-catalog-reference` (`unresolved-catalog-references`): always
///   `false` today (the catalog-switching applier is not wired in yet); the
///   field is non-singleton so that future enablement does not require a
///   schema change.
///
/// All `suppress-line` and `suppress-file` actions are uniformly
/// `auto_fixable: false`. The field is non-singleton on the wire so that a
/// future auto-applier (e.g. an LLM-driven suppression writer) can promote
/// individual variants without a schema bump.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(untagged)]
pub enum IssueAction {
    /// A code-change fix the user can apply (auto-fixable by `fallow fix` for
    /// some variants, manual for others).
    Fix(FixAction),
    /// Place a `// fallow-ignore-next-line ...` comment above the offending
    /// line. Always manual.
    SuppressLine(SuppressLineAction),
    /// Place a `// fallow-ignore-file ...` comment at the top of the file.
    /// Always manual.
    SuppressFile(SuppressFileAction),
    /// Add the offending finding to the fallow config (e.g.
    /// `ignoreDependencies: ["lodash"]`). Auto-fixable for the array-shaped
    /// `ignoreExports` variant when `fallow fix` can safely apply the
    /// action (config file exists, or no config exists and the working
    /// directory is not inside a monorepo subpackage); manual otherwise.
    AddToConfig(AddToConfigAction),
}

/// A code-change fix. `type` is one of the kebab-case identifiers in
/// [`FixActionType`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct FixAction {
    /// Kebab-case identifier for the fix action.
    #[serde(rename = "type")]
    pub kind: FixActionType,
    /// Whether `fallow fix` can apply this fix automatically. Evaluated PER
    /// FINDING, not per action type: the same `type` may carry
    /// `auto_fixable: true` on one finding and `auto_fixable: false` on
    /// another when per-instance guards in the applier discriminate (e.g.
    /// `remove-catalog-entry` flips on `hardcoded_consumers` and catalog
    /// source file, the primary dependency action flips between
    /// `remove-dependency` / `move-dependency` on `used_in_workspaces`).
    /// Filter on this bool of each individual action, not on `type`. See the
    /// [`IssueAction`] enum-level docs for the full list of per-instance
    /// flips.
    pub auto_fixable: bool,
    /// Human-readable description of the fix.
    pub description: String,
    /// Optional context note. Present on non-auto-fixable actions, and on
    /// auto-fixable re-export findings to warn about public API surface.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// Only present on `update-catalog-reference` actions: catalogs in the
    /// same workspace that DO declare the package, sorted lexicographically.
    /// Lets agents pick the catalog to switch to without re-reading the
    /// source.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub available_in_catalogs: Option<Vec<String>>,
    /// Only present on `update-catalog-reference` actions when exactly one
    /// alternative catalog declares the package: the unambiguous switch
    /// target. Lets deterministic (non-LLM) agents land the edit without
    /// picking from a list. Absent when `available_in_catalogs` has zero
    /// or more than one entry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggested_target: Option<String>,
}

/// Discriminant string for [`FixAction`]. Kebab-case per the JSON output
/// contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum FixActionType {
    /// Remove an export declaration from a source file.
    RemoveExport,
    /// Delete an entire unused file.
    DeleteFile,
    /// Remove an entry from `dependencies` / `devDependencies` in
    /// `package.json`.
    RemoveDependency,
    /// Move an entry between `dependencies` and `devDependencies`.
    MoveDependency,
    /// Remove an enum member from a TypeScript enum.
    RemoveEnumMember,
    /// Remove a class member (method or property).
    RemoveClassMember,
    /// Resolve an unresolved import (manual).
    ResolveImport,
    /// Install a missing dependency.
    InstallDependency,
    /// Remove a duplicate export (the canonical action for
    /// `duplicate-exports`).
    RemoveDuplicate,
    /// Move a production dependency to `devDependencies`
    /// (used by type-only-dependency and test-only-dependency findings).
    MoveToDev,
    /// Move a `devDependencies` entry to `dependencies`
    /// (used by dev-dependency-in-production findings; the promote-side mirror
    /// of [`FixActionType::MoveToDev`]).
    MoveToProd,
    /// Break a circular dependency by refactoring imports.
    RefactorCycle,
    /// Break a re-export cycle by removing an `export * from` (or
    /// `export { ... } from`) statement on any one member file. Re-export
    /// cycles are structurally always bugs (chain propagation through the
    /// loop is a no-op), so there is no auto-fix; the action is manual.
    RefactorReExportCycle,
    /// Resolve a boundary violation by refactoring the import.
    RefactorBoundary,
    /// Convert an import statement to a type-only import (used by
    /// private-type-leak findings).
    ExportType,
    /// Remove an unused catalog entry. Auto-fix only supports `pnpm-workspace.yaml`;
    /// Bun `package.json` catalogs are manual.
    RemoveCatalogEntry,
    /// Remove an empty named catalog group. Auto-fix only supports
    /// `pnpm-workspace.yaml`; Bun `package.json` catalogs are manual.
    RemoveEmptyCatalogGroup,
    /// Update an existing `catalog:` reference in a workspace `package.json`
    /// to point at a different (declared) catalog.
    UpdateCatalogReference,
    /// Add the missing entry to the referenced catalog.
    AddCatalogEntry,
    /// Remove the catalog reference from the workspace `package.json` and
    /// replace it with a hardcoded version.
    RemoveCatalogReference,
    /// Remove an unused dependency override entry.
    RemoveDependencyOverride,
    /// Fix a misconfigured dependency override entry (unparsable key or empty
    /// value).
    FixDependencyOverride,
    /// Replace a banned call or banned import flagged by a rule-pack rule
    /// (manual; the rule's message usually names the sanctioned alternative).
    ResolvePolicyViolation,
    /// Move a server-only export out of a `"use client"` file into a
    /// non-client module (manual; used by invalid-client-export findings).
    MoveToServerModule,
    /// Split a barrel that re-exports both client and server-only modules
    /// into separate client and server barrels (manual; used by
    /// mixed-client-server-barrel findings).
    SplitMixedBarrel,
    /// Hoist a misplaced `"use client"` / `"use server"` directive to the
    /// leading prologue of the file (manual; used by misplaced-directive
    /// findings).
    HoistDirective,
    /// Wire a server action to a project consumer or remove the unused action
    /// export (manual; used by unused-server-action findings).
    WireServerAction,
    /// Add a provider for an injected key or remove the dead inject call
    /// (manual; used by unprovided-inject findings).
    ProvideInject,
    /// Use a SvelteKit load-data key from the route UI or remove the unused
    /// returned key (manual; used by unused-load-data-key findings).
    UseLoadData,
    /// Render a reachable component from project code or remove the component
    /// (manual; used by unrendered-component findings).
    RenderComponent,
    /// Use a declared component prop or remove it from the component API
    /// (manual; used by unused-component-prop findings).
    UseComponentProp,
    /// Emit a declared component event or remove it from the component API
    /// (manual; used by unused-component-emit findings).
    EmitComponentEvent,
    /// Add or forward a Svelte custom-event listener, or remove the dispatch
    /// (manual; used by unused-svelte-event findings).
    WireSvelteEvent,
    /// Resolve a Next.js App Router route collision by moving or merging one of
    /// the files that own the same URL (manual; suppressing a guaranteed build
    /// error is never the right fix, so this is the primary action).
    ResolveRouteCollision,
    /// Resolve a Next.js dynamic-segment name conflict by renaming the dynamic
    /// segments at the conflicting position to a single consistent slug name
    /// (manual).
    ResolveDynamicSegmentNameConflict,
    /// Add a human-authored reason to a suppression that requires one.
    AddSuppressionReason,
    /// Remove or update a suppression that no longer matches a finding.
    RemoveStaleSuppression,
}

/// Inline-comment suppression for a single finding line.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SuppressLineAction {
    /// Action type identifier.
    #[serde(rename = "type")]
    pub kind: SuppressLineKind,
    /// Always false for suppress actions.
    pub auto_fixable: bool,
    /// Human-readable description of the suppression.
    pub description: String,
    /// The inline comment to place above the line (e.g.,
    /// `// fallow-ignore-next-line unused-export`). When multiple
    /// suppressible findings share the same path and line, this may contain a
    /// comma-separated issue-kind list such as
    /// `// fallow-ignore-next-line unused-export, complexity`.
    pub comment: String,
    /// Present on multi-location issue types (e.g., `duplicate_exports`) to
    /// indicate the comment must be applied at each location.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<SuppressLineScope>,
}

/// Singleton discriminant for [`SuppressLineAction`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum SuppressLineKind {
    /// `// fallow-ignore-next-line <kind>` directive.
    SuppressLine,
}

/// Scope marker for line suppressions that span multiple locations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum SuppressLineScope {
    /// Apply the suppression comment at each location of the multi-location
    /// finding (e.g., every `duplicate_exports` site).
    PerLocation,
}

/// File-wide suppression placed at the top of the source file.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SuppressFileAction {
    /// Action type identifier.
    #[serde(rename = "type")]
    pub kind: SuppressFileKind,
    /// Always false for suppress actions.
    pub auto_fixable: bool,
    /// Human-readable description of the suppression.
    pub description: String,
    /// The file-level comment to place at the top of the file (e.g.,
    /// `// fallow-ignore-file unused-file`).
    pub comment: String,
}

/// Singleton discriminant for [`SuppressFileAction`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum SuppressFileKind {
    /// `// fallow-ignore-file <kind>` directive.
    SuppressFile,
}

/// Edit a fallow config file (`.fallowrc.json`, `fallow.toml`, etc.) to
/// add the offending value to an `ignore*` rule.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct AddToConfigAction {
    /// Action type identifier.
    #[serde(rename = "type")]
    pub kind: AddToConfigKind,
    /// True when `fallow fix` can apply this config action automatically.
    /// Evaluated PER FINDING, not per action type: `ignoreExports`
    /// duplicate-export actions are auto-fixable when `fallow fix` can
    /// safely write the rule, which today means EITHER a fallow config
    /// file already exists OR no config exists and the working directory
    /// is NOT inside a monorepo subpackage (in which case the applier
    /// creates `.fallowrc.json` from `fallow init`'s framework-aware
    /// scaffolding). The action is `false` inside a monorepo subpackage
    /// with no workspace-root config because the applier refuses to
    /// fragment per-package configs across the monorepo. Older scalar
    /// config-ignore actions (e.g. `ignoreDependencies` on dependency
    /// findings) are always manual today. Filter on this bool of each
    /// individual action, not on the `type` alone. See the [`IssueAction`]
    /// enum-level docs for the full list of per-instance flips.
    pub auto_fixable: bool,
    /// Human-readable description of the config change.
    pub description: String,
    /// The fallow config key to add the value to (e.g.,
    /// `ignoreDependencies`).
    pub config_key: String,
    /// Value to add to the config key. Shape depends on `config_key`. For
    /// scalar config keys (`ignoreDependencies`, others) this is a string
    /// such as `"lodash"`. For `ignoreExports` this is an array of
    /// `{ file, exports }` rule objects so the snippet can be merged into
    /// the user's config verbatim. For `ignoreCatalogReferences` and
    /// `ignoreDependencyOverrides` this is an object whose shape matches the
    /// rule entry users add to their fallow config.
    pub value: AddToConfigValue,
    /// Optional URL pointing at a stable JSON Schema fragment that describes
    /// the shape of `value`. Agents that intend to validate `value` before
    /// writing it into a user's config can fetch the linked schema and run
    /// it against `value`. The URL is a JSON Pointer fragment into fallow's
    /// main config schema (e.g.
    /// `schema.json#/properties/ignoreExports` for the ignoreExports
    /// action, or `schema.json#/properties/ignoreDependencies/items` for
    /// the per-package ignoreDependencies action). Strictly additive:
    /// consumers that ignore the field keep working unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value_schema: Option<String>,
}

/// Singleton discriminant for [`AddToConfigAction`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum AddToConfigKind {
    /// Append a value into a fallow config `ignore*` list.
    AddToConfig,
}

/// Value payload for [`AddToConfigAction::value`]. The variants line up with
/// the documented per-`config_key` shapes; deserialization is untagged so
/// downstream consumers can switch on the JSON value's type.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(untagged)]
pub enum AddToConfigValue {
    /// Scalar string value (e.g., a package name for
    /// `ignoreDependencies: ["lodash"]`).
    Scalar(String),
    /// Array of file+export rule objects for `ignoreExports`.
    ExportsRules(Vec<IgnoreExportsRule>),
    /// Free-form object for rule-shaped keys like
    /// `ignoreCatalogReferences` / `ignoreDependencyOverrides`. The shape
    /// matches the rule entry users add to their fallow config; consumers
    /// validate against the per-key schema referenced by `value_schema`.
    RuleObject(serde_json::Map<String, serde_json::Value>),
}

/// Single `ignoreExports` rule entry. The fallow config accepts an array of
/// these under the `ignoreExports` key.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct IgnoreExportsRule {
    /// File path (forward slashes, relative to project root) to which this
    /// rule applies. Globs are accepted.
    pub file: String,
    /// Names of exports inside `file` to silently treat as used.
    pub exports: Vec<String>,
}

/// A read-only follow-up command fallow surfaces from the current findings,
/// emitted as the top-level `next_steps` array on each command's JSON envelope.
///
/// `next_steps` exists to point agents and humans sideways to fallow's adjacent
/// verification capabilities (trace, complexity breakdown, audit, workspace
/// scoping) that telemetry shows agents rarely discover, because they act on the
/// output in front of them rather than on reference docs.
///
/// ## Two hard contracts
///
/// 1. **Read-only.** A `next_step` NEVER suggests `fallow fix` or any mutating
///    command. Fallow surfaces evidence and verification paths; deciding and
///    applying the remediation is the agent's job.
/// 2. **Runnable, placeholder-free.** `command` is always runnable as-is. It
///    never contains an angle-bracket placeholder (`<...>`); finding-derived
///    values are filled in from a real, deterministically-selected finding, and
///    any environment- or user-specific value that cannot be made concrete lives
///    in `reason` instead. An agent can copy `command` and run it without edits.
///
/// Both contracts are enforced by unit tests in
/// `crates/cli/src/report/suggestions.rs`.
///
/// Note: a SEPARATE, unrelated `next_steps` field exists on the
/// `coverage setup` envelope (`CoverageSetupOutput.next_steps`) as a plain
/// `Vec<String>` of human onboarding steps. Consumers that read multiple
/// envelope kinds must route on the envelope's `kind` before interpreting a
/// `next_steps` field: on analysis envelopes it is `Vec<NextStep>` objects, on
/// `coverage setup` it is `Vec<String>`.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct NextStep {
    /// Stable kebab-case key for machine dispatch and de-duplication
    /// (for example `"trace-unused-export"`). Identity is stable across runs;
    /// the `command` and `reason` strings may vary with the findings.
    pub id: String,
    /// A runnable, read-only command string. Placeholder-free by contract.
    pub command: String,
    /// One short phrase explaining why this helps. Carries any value that
    /// cannot be made concrete in `command`.
    pub reason: String,
}
