//! Shared list command JSON output assembly.

use fallow_output::{
    ListEntryPointOutput, ListOutput, ListPluginOutput, RootEnvelopeMode, WorkspacesOutput,
};
use serde::Serialize;

/// Root envelope mode for a `fallow list --format json` payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListJsonEnvelope {
    /// Emit the historical plain object without a `kind` field.
    Plain,
    /// Wrap as `kind: "list-boundaries"`.
    Boundaries,
    /// Wrap as `kind: "list-workspaces"`.
    Workspaces,
}

/// Section data for serializing a `fallow list --format json` payload.
pub struct ListJsonOutputInput<Boundaries, Diagnostic> {
    pub plugins: Option<Vec<String>>,
    pub files: Option<Vec<String>>,
    pub entry_points: Option<Vec<ListEntryPointOutput>>,
    pub boundaries: Option<Boundaries>,
    pub workspaces: Option<WorkspacesOutput<Diagnostic>>,
}

/// Build the typed list output body before optional root wrapping.
#[must_use]
pub fn build_list_json_output<Boundaries, Diagnostic>(
    input: ListJsonOutputInput<Boundaries, Diagnostic>,
) -> ListOutput<Boundaries, Diagnostic> {
    let plugins = input.plugins.map(|plugins| {
        plugins
            .into_iter()
            .map(|name| ListPluginOutput { name })
            .collect()
    });
    let file_count = input.files.as_ref().map(Vec::len);
    let entry_point_count = input.entry_points.as_ref().map(Vec::len);
    let (workspace_count, workspaces, workspace_diagnostics) =
        input.workspaces.map_or((None, None, None), |workspaces| {
            (
                Some(workspaces.workspace_count),
                Some(workspaces.workspaces),
                Some(workspaces.workspace_diagnostics),
            )
        });

    ListOutput {
        plugins,
        file_count,
        files: input.files,
        entry_point_count,
        entry_points: input.entry_points,
        boundaries: input.boundaries,
        workspace_count,
        workspaces,
        workspace_diagnostics,
    }
}

/// Serialize a typed `fallow list --format json` payload.
///
/// # Errors
///
/// Returns a serde error when the selected list output cannot be converted to
/// JSON.
pub fn serialize_list_json_output<Boundaries, Diagnostic>(
    input: ListJsonOutputInput<Boundaries, Diagnostic>,
    mode: RootEnvelopeMode,
    envelope: ListJsonEnvelope,
) -> Result<serde_json::Value, serde_json::Error>
where
    Boundaries: Serialize,
    Diagnostic: Serialize,
{
    let output = build_list_json_output(input);
    match envelope {
        ListJsonEnvelope::Plain => serde_json::to_value(output),
        ListJsonEnvelope::Boundaries => {
            fallow_output::serialize_list_boundaries_json_output(output, mode)
        }
        ListJsonEnvelope::Workspaces => {
            fallow_output::serialize_list_workspaces_json_output(output, mode)
        }
    }
}

#[cfg(test)]
mod tests {
    use fallow_output::ListEntryPointOutput;
    use serde_json::json;

    use super::*;

    #[test]
    fn list_json_output_preserves_plain_legacy_body() {
        let value = serialize_list_json_output::<serde_json::Value, serde_json::Value>(
            ListJsonOutputInput {
                plugins: Some(vec!["react".to_string()]),
                files: Some(vec!["src/index.ts".to_string()]),
                entry_points: Some(vec![ListEntryPointOutput {
                    path: "src/index.ts".to_string(),
                    source: "package.json main".to_string(),
                }]),
                boundaries: None,
                workspaces: None,
            },
            RootEnvelopeMode::Tagged,
            ListJsonEnvelope::Plain,
        )
        .expect("list output should serialize");

        assert_eq!(value["plugins"][0]["name"], "react");
        assert_eq!(value["file_count"], 1);
        assert_eq!(value["files"], json!(["src/index.ts"]));
        assert_eq!(value["entry_point_count"], 1);
        assert!(value.get("kind").is_none());
    }

    #[test]
    fn list_json_output_wraps_boundary_payloads() {
        let value = serialize_list_json_output::<serde_json::Value, serde_json::Value>(
            ListJsonOutputInput {
                plugins: None,
                files: None,
                entry_points: None,
                boundaries: Some(json!({"configured": false})),
                workspaces: None,
            },
            RootEnvelopeMode::Tagged,
            ListJsonEnvelope::Boundaries,
        )
        .expect("list output should serialize");

        assert_eq!(value["kind"], "list-boundaries");
        assert_eq!(value["boundaries"]["configured"], false);
    }
}
