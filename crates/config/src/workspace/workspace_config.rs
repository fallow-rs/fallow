//! The `workspaces` config section.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Workspace configuration for monorepo support.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct WorkspaceConfig {
    /// Additional workspace patterns (beyond what's in root package.json).
    ///
    /// `packages` is accepted as a back-compat alias: an older `fallow init --toml`
    /// wrote `[workspaces]` with a `packages` key, and reading it as `patterns`
    /// keeps those existing configs scoping correctly. schemars omits serde
    /// aliases, so `schema.json` documents only `patterns`.
    #[serde(default, alias = "packages")]
    pub patterns: Vec<String>,
    /// Git baseline refs keyed by workspace roots, written relative to the project root as `fallow list --workspaces` prints them. Scopes `check`, `dead-code`, `dupes` and editor diagnostics; `health`, `security` and `audit` ignore it. A global changed-since request takes precedence. Unlisted workspaces and root files remain in full scope. A key that names no workspace, or a ref that Git cannot resolve, leaves every package in full scope with a warning.
    #[serde(
        default,
        rename = "changedSince",
        skip_serializing_if = "BTreeMap::is_empty"
    )]
    pub changed_since: BTreeMap<String, String>,
    /// Settings that apply only to the workspace packages that a key selects. A key is a workspace root relative to the project root, as `fallow list --workspaces` prints it, or a glob such as `packages/*`. A single `*` stays within one path segment. Each value can hold `ignoreDependencies` and `includeEntryExports`. A workspace gets the global `ignoreDependencies` plus the lists of every key that selects it. For `includeEntryExports`, an exact root wins over a glob, a glob with more literal segments wins over a shorter glob, and a workspace that no key sets uses the global value. The root package uses only the global keys. A key that selects no workspace gives a warning. An older fallow version ignores this map, so pair it with `minimumVersion`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub settings: BTreeMap<String, WorkspaceSettings>,
    /// Keys under `workspaces` that fallow does not know. Config load warns
    /// about each one, so that a misspelled key does not do nothing in silence.
    #[serde(flatten, default, skip_serializing)]
    #[schemars(skip)]
    pub unknown_keys: BTreeMap<String, serde_json::Value>,
}

/// The settings of one `workspaces.settings` key.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct WorkspaceSettings {
    /// Package names or package-name globs that unused-dependency and unlisted-dependency detection ignore in the selected workspaces only, in addition to the global `ignoreDependencies`. An unused dependency uses the workspace of its `package.json`. An unlisted dependency uses the deepest workspace of the file that imports it. The syntax is the same as the global `ignoreDependencies`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ignore_dependencies: Vec<String>,
    /// When set, replaces the global `includeEntryExports` value for the files of the selected workspaces. A file belongs to its deepest workspace. The `--include-entry-exports` CLI flag turns the check on in every workspace and wins over this value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include_entry_exports: Option<bool>,
}
