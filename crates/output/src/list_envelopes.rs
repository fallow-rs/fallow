//! List command output envelopes.

use crate::root_envelopes::{RootEnvelopeMode, serialize_named_json_output};
use serde::Serialize;

/// Plain body emitted by `fallow list --format json` before an optional
/// command-specific root envelope is attached.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ListOutput<Boundaries, Diagnostic> {
    /// Active plugins; present for `--plugins`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugins: Option<Vec<ListPluginOutput>>,
    /// Number of analyzable files; present for `--files`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_count: Option<usize>,
    /// Analyzable file paths relative to the root; present for `--files`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files: Option<Vec<String>>,
    /// Number of entry points; present for `--entry-points`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry_point_count: Option<usize>,
    /// Detected entry points; present for `--entry-points`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry_points: Option<Vec<ListEntryPointOutput>>,
    /// Boundary listing; present for `--boundaries`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub boundaries: Option<Boundaries>,
    /// Number of workspace packages; present for `--workspaces`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_count: Option<usize>,
    /// Workspace packages; present for `--workspaces`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspaces: Option<Vec<WorkspaceInfo>>,
    /// Workspace-discovery diagnostics; present for `--workspaces`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_diagnostics: Option<Vec<Diagnostic>>,
}

/// One active plugin in `fallow list --plugins --format json`.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ListPluginOutput {
    /// Plugin name, e.g. `nextjs`.
    pub name: String,
}

/// One entry point in `fallow list --entry-points --format json`.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ListEntryPointOutput {
    /// File path relative to the analysed root.
    pub path: String,
    /// What declared the entry point, e.g. a plugin or config pattern.
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
    /// Boundary zones, rules, and pre-expansion logical groups.
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
    /// declarations. Paths are project-root-relative and use forward slashes,
    /// like `workspaces[].path` and like the `workspace_diagnostics[]` array on
    /// the analysis envelopes. Present for compatibility with the current wire
    /// contract, even when empty.
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
    /// Whether the project configures architecture boundaries at all.
    pub configured: bool,
    /// Number of entries in `zones`.
    pub zone_count: usize,
    /// Boundary zones after preset and `autoDiscover` expansion.
    pub zones: Vec<BoundariesListZone>,
    /// Number of entries in `rules`.
    pub rule_count: usize,
    /// Import rules operating on expanded zone names.
    pub rules: Vec<BoundariesListRule>,
    /// Number of entries in `logical_groups`.
    pub logical_group_count: usize,
    /// Pre-expansion `autoDiscover` logical groups.
    pub logical_groups: Vec<BoundariesListLogicalGroup<Status, Rule>>,
}

/// A boundary zone after preset and `autoDiscover` expansion. Each entry
/// classifies files into a single zone via glob patterns.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct BoundariesListZone {
    /// Zone name referenced by rules.
    pub name: String,
    /// Glob patterns that classify files into the zone.
    pub patterns: Vec<String>,
    /// Number of analyzable files the zone matched.
    pub file_count: usize,
}

/// A boundary import rule, expanded to operate on concrete child zone
/// names after `autoDiscover` flattening. The user's pre-expansion rule
/// (keyed on the logical parent name, if any) is preserved on the
/// corresponding [`BoundariesListLogicalGroup::authored_rule`].
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct BoundariesListRule {
    /// Zone the rule constrains imports from.
    pub from: String,
    /// Zone names the `from` zone may import.
    pub allow: Vec<String>,
}

/// A pre-expansion `autoDiscover` logical group surfaced for observability.
/// Captured during expansion so consumers can see the user-authored parent
/// name and grouping intent after expansion would otherwise flatten it out of
/// [`BoundariesListing::zones`].
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct BoundariesListLogicalGroup<Status, Rule> {
    /// User-authored parent zone name.
    pub name: String,
    /// Child zone names produced by discovery.
    pub children: Vec<String>,
    /// Authored `autoDiscover` paths.
    pub auto_discover: Vec<String>,
    /// Discovery outcome (ok / empty / invalid path).
    pub status: Status,
    /// Index of the authored entry in the pre-expansion `zones[]` config.
    pub source_zone_index: usize,
    /// Files matched across the group's zones.
    pub file_count: usize,
    /// User's pre-expansion rule keyed on the parent name, when authored.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authored_rule: Option<Rule>,
    /// Zone that keeps the parent's own patterns when the parent kept any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_zone: Option<String>,
    /// `zones[]` indices of duplicate parents merged into this group.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merged_from: Option<Vec<usize>>,
    /// Authored parent `root`, when one was declared.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_zone_root: Option<String>,
    /// Per-child indices into the pre-expansion `zones[]` config.
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
