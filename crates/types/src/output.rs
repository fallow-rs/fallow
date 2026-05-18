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

use serde::Serialize;

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
///   finding's `hardcoded_consumers` array is empty. When a workspace
///   package still pins a hardcoded version of the same package, `fallow fix`
///   skips the entry to avoid breaking `pnpm install`, and the action is
///   emitted with `auto_fixable: false`.
/// - `remove-dependency` vs `move-dependency` (dependency findings): when the
///   finding's `used_in_workspaces` array is non-empty, the primary action
///   flips to `move-dependency` with `auto_fixable: false` (`fallow fix` will
///   not remove a dependency that another workspace imports). On findings
///   without cross-workspace consumers the action stays `remove-dependency`
///   with `auto_fixable: true`.
/// - `add-to-config` for `ignoreExports` (`duplicate-exports`): `true` when
///   a fallow config file exists on disk; `false` otherwise (the applier
///   declines to materialize a config file from scratch).
/// - `update-catalog-reference` (`unresolved-catalog-references`): always
///   `false` today (the catalog-switching applier is not wired in yet); the
///   field is non-singleton so that future enablement does not require a
///   schema change.
///
/// All `suppress-line` and `suppress-file` actions are uniformly
/// `auto_fixable: false`. The field is non-singleton on the wire so that a
/// future auto-applier (e.g. an LLM-driven suppression writer) can promote
/// individual variants without a schema bump.
#[derive(Debug, Clone, Serialize)]
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
    /// `ignoreExports` variant when a config file exists; manual otherwise.
    AddToConfig(AddToConfigAction),
}

/// A code-change fix. `type` is one of the kebab-case identifiers in
/// [`FixActionType`].
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct FixAction {
    /// Kebab-case identifier for the fix action.
    #[serde(rename = "type")]
    pub kind: FixActionType,
    /// Whether `fallow fix` can apply this fix automatically. Evaluated PER
    /// FINDING, not per action type: the same `type` may carry
    /// `auto_fixable: true` on one finding and `auto_fixable: false` on
    /// another when per-instance guards in the applier discriminate (e.g.
    /// `remove-catalog-entry` flips on `hardcoded_consumers`, the primary
    /// dependency action flips between `remove-dependency` /
    /// `move-dependency` on `used_in_workspaces`). Filter on this bool of
    /// each individual action, not on `type`. See the [`IssueAction`]
    /// enum-level docs for the full list of per-instance flips.
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
}

/// Discriminant string for [`FixAction`]. Kebab-case per the JSON output
/// contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
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
    /// Break a circular dependency by refactoring imports.
    RefactorCycle,
    /// Resolve a boundary violation by refactoring the import.
    RefactorBoundary,
    /// Convert an import statement to a type-only import (used by
    /// private-type-leak findings).
    ExportType,
    /// Remove an unused catalog entry from `pnpm-workspace.yaml`.
    RemoveCatalogEntry,
    /// Remove an empty named catalog group from `pnpm-workspace.yaml`.
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
}

/// Inline-comment suppression for a single finding line.
#[derive(Debug, Clone, Serialize)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum SuppressLineKind {
    /// `// fallow-ignore-next-line <kind>` directive.
    SuppressLine,
}

/// Scope marker for line suppressions that span multiple locations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum SuppressLineScope {
    /// Apply the suppression comment at each location of the multi-location
    /// finding (e.g., every `duplicate_exports` site).
    PerLocation,
}

/// File-wide suppression placed at the top of the source file.
#[derive(Debug, Clone, Serialize)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum SuppressFileKind {
    /// `// fallow-ignore-file <kind>` directive.
    SuppressFile,
}

/// Edit a fallow config file (`.fallowrc.json`, `fallow.toml`, etc.) to
/// add the offending value to an `ignore*` rule.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct AddToConfigAction {
    /// Action type identifier.
    #[serde(rename = "type")]
    pub kind: AddToConfigKind,
    /// True when `fallow fix` can apply this config action automatically.
    /// Evaluated PER FINDING, not per action type: `ignoreExports`
    /// duplicate-export actions are auto-fixable when a fallow config file
    /// exists and manual when one does not. Older scalar config-ignore
    /// actions (e.g. `ignoreDependencies` on dependency findings) are always
    /// manual today. Filter on this bool of each individual action, not on
    /// the `type` alone. See the [`IssueAction`] enum-level docs for the
    /// full list of per-instance flips.
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum AddToConfigKind {
    /// Append a value into a fallow config `ignore*` list.
    AddToConfig,
}

/// Value payload for [`AddToConfigAction::value`]. The variants line up with
/// the documented per-`config_key` shapes; deserialization is untagged so
/// downstream consumers can switch on the JSON value's type.
#[derive(Debug, Clone, Serialize)]
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
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct IgnoreExportsRule {
    /// File path (forward slashes, relative to project root) to which this
    /// rule applies. Globs are accepted.
    pub file: String,
    /// Names of exports inside `file` to silently treat as used.
    pub exports: Vec<String>,
}
