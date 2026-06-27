//! List command output envelopes.

use crate::root_envelopes::{RootEnvelopeMode, serialize_named_json_output};
use serde::Serialize;

/// Plain body emitted by `fallow list --format json` before an optional
/// command-specific root envelope is attached.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ListOutput<Boundaries, Diagnostic> {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugins: Option<Vec<ListPluginOutput>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_count: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry_point_count: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry_points: Option<Vec<ListEntryPointOutput>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub boundaries: Option<Boundaries>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_count: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspaces: Option<Vec<WorkspaceInfo>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_diagnostics: Option<Vec<Diagnostic>>,
}

/// One active plugin in `fallow list --plugins --format json`.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ListPluginOutput {
    pub name: String,
}

/// One entry point in `fallow list --entry-points --format json`.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ListEntryPointOutput {
    pub path: String,
    pub source: String,
}

/// Envelope emitted by `fallow list --boundaries --format json`. Surfaces
/// the architecture boundary zones, rules, and the user's pre-expansion
/// `autoDiscover` logical groups so consumers can render grouping intent that
/// expansion would otherwise flatten out of `zones[]`.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(
    feature = "schema",
    schemars(title = "fallow list --boundaries --format json")
)]
pub struct ListBoundariesOutput<Status, Rule> {
    pub boundaries: BoundariesListing<Status, Rule>,
}

/// `fallow workspaces --format json` envelope.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(
    feature = "schema",
    schemars(title = "fallow workspaces --format json")
)]
pub struct WorkspacesOutput<Diagnostic> {
    /// Number of workspace package entries in `workspaces`.
    pub workspace_count: usize,
    /// Workspace packages discovered from package manager and tsconfig workspace
    /// declarations. Paths are project-root-relative and use forward slashes.
    pub workspaces: Vec<WorkspaceInfo>,
    /// Workspace discovery diagnostics produced while reading workspace
    /// declarations. Present for compatibility with the current wire contract,
    /// even when empty.
    pub workspace_diagnostics: Vec<Diagnostic>,
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
pub struct BoundariesListing<Status, Rule> {
    pub configured: bool,
    pub zone_count: usize,
    pub zones: Vec<BoundariesListZone>,
    pub rule_count: usize,
    pub rules: Vec<BoundariesListRule>,
    pub logical_group_count: usize,
    pub logical_groups: Vec<BoundariesListLogicalGroup<Status, Rule>>,
}

/// A boundary zone after preset and `autoDiscover` expansion. Each entry
/// classifies files into a single zone via glob patterns.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
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
pub struct BoundariesListRule {
    pub from: String,
    pub allow: Vec<String>,
}

/// A pre-expansion `autoDiscover` logical group surfaced for observability.
/// Captured during expansion so consumers can see the user-authored parent
/// name and grouping intent after expansion would otherwise flatten it out of
/// [`BoundariesListing::zones`].
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct BoundariesListLogicalGroup<Status, Rule> {
    pub name: String,
    pub children: Vec<String>,
    pub auto_discover: Vec<String>,
    pub status: Status,
    pub source_zone_index: usize,
    pub file_count: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authored_rule: Option<Rule>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_zone: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merged_from: Option<Vec<usize>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_zone_root: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub child_source_indices: Vec<usize>,
}

/// Serialize `fallow list --boundaries --format json`.
///
/// # Errors
///
/// Returns a serde error when the list output cannot be converted to JSON.
pub fn serialize_list_boundaries_json_output<T: Serialize>(
    output: T,
    mode: RootEnvelopeMode,
) -> Result<serde_json::Value, serde_json::Error> {
    serialize_named_json_output(output, "list-boundaries", mode)
}

/// Serialize `fallow list --workspaces --format json`.
///
/// # Errors
///
/// Returns a serde error when the list output cannot be converted to JSON.
pub fn serialize_list_workspaces_json_output<T: Serialize>(
    output: T,
    mode: RootEnvelopeMode,
) -> Result<serde_json::Value, serde_json::Error> {
    serialize_named_json_output(output, "list-workspaces", mode)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn list_boundaries_json_output_uses_output_owned_root_contract() {
        let value = serialize_list_boundaries_json_output(
            json!({"boundaries": {}}),
            RootEnvelopeMode::Tagged,
        )
        .expect("list boundaries output should serialize");

        assert_eq!(value["kind"], "list-boundaries");
    }

    #[test]
    fn list_workspaces_json_output_uses_output_owned_root_contract() {
        let value = serialize_list_workspaces_json_output(
            json!({"workspace_count": 0, "workspaces": []}),
            RootEnvelopeMode::Tagged,
        )
        .expect("list workspaces output should serialize");

        assert_eq!(value["kind"], "list-workspaces");
    }
}
