//! Drift tests keeping the shared MCP tool manifest in `fallow-types`
//! (`fallow_types::mcp_manifest::MCP_TOOLS`) in sync with the live rmcp
//! tool router. The manifest feeds `fallow schema` (the agent capability
//! manifest) and the telemetry tool-name allowlist, so a silent divergence
//! would mislead every agent that introspects fallow.
//!
//! Known accepted gaps, by design:
//! - `key_params` is a deliberate SUBSET of each tool's input schema; a
//!   newly added param can lawfully be absent from the manifest. The test
//!   only rejects manifest params that do not exist on the live schema.
//! - Manifest descriptions are concise introspection summaries, not copies of
//!   the full rmcp wire descriptions. `tool_descriptions` locks the wire prose
//!   byte for byte; the test here keeps the two roles distinct.

use std::collections::BTreeSet;

use fallow_types::mcp_manifest::MCP_TOOLS;
use fallow_types::suppress::DEAD_CODE_FILTER_FLAGS;

use super::super::FallowMcp;
use crate::tools::ISSUE_TYPE_FLAGS;

#[test]
fn manifest_names_match_live_tool_router_both_directions() {
    let server = FallowMcp::new();
    let tools = server.tool_router.list_all();
    let live: BTreeSet<String> = tools.iter().map(|t| t.name.to_string()).collect();
    let manifest: BTreeSet<String> = MCP_TOOLS.iter().map(|t| t.name.to_string()).collect();
    assert_eq!(
        live, manifest,
        "fallow_types::mcp_manifest::MCP_TOOLS must list exactly the tools the MCP server \
         registers; update the shared manifest when adding, renaming, or removing a tool"
    );
}

#[test]
fn manifest_descriptions_are_concise_summaries_not_wire_copies() {
    let server = FallowMcp::new();
    let tools = server.tool_router.list_all();

    for entry in MCP_TOOLS {
        let tool = tools
            .iter()
            .find(|tool| tool.name.as_ref() == entry.name)
            .unwrap_or_else(|| panic!("manifest tool {} is not registered", entry.name));
        let wire_description = tool
            .description
            .as_deref()
            .unwrap_or_else(|| panic!("tool {} has no wire description", entry.name));

        assert_ne!(
            entry.description, wire_description,
            "manifest description for {} must remain a distinct capability summary",
            entry.name
        );
        assert!(
            entry.description.len() < wire_description.len(),
            "manifest description for {} must stay shorter than its full wire description",
            entry.name
        );
    }
}

#[test]
fn manifest_key_params_exist_on_live_input_schemas() {
    let server = FallowMcp::new();
    let tools = server.tool_router.list_all();
    for entry in MCP_TOOLS {
        let tool = tools
            .iter()
            .find(|t| t.name.as_ref() == entry.name)
            .unwrap_or_else(|| panic!("manifest tool {} is not registered", entry.name));
        let properties = tool
            .input_schema
            .get("properties")
            .and_then(serde_json::Value::as_object)
            .unwrap_or_else(|| panic!("tool {} input schema has no properties object", entry.name));
        for param in entry.key_params {
            assert!(
                properties.contains_key(*param),
                "manifest key_param '{param}' does not exist on the live input schema of \
                 tool {}; fix the manifest entry in fallow-types",
                entry.name
            );
        }
    }
}

/// The manifest's `read_only` flag must match the live rmcp
/// `read_only_hint` annotation, so a future destructive tool cannot ship
/// with a manifest that still advertises it as read-only.
#[test]
fn manifest_read_only_matches_live_annotations() {
    let server = FallowMcp::new();
    let tools = server.tool_router.list_all();
    for entry in MCP_TOOLS {
        let tool = tools
            .iter()
            .find(|t| t.name.as_ref() == entry.name)
            .unwrap_or_else(|| panic!("manifest tool {} is not registered", entry.name));
        let live_read_only = tool
            .annotations
            .as_ref()
            .and_then(|a| a.read_only_hint)
            .unwrap_or(false);
        assert_eq!(
            entry.read_only, live_read_only,
            "manifest read_only for {} diverges from the live read_only_hint annotation",
            entry.name
        );
    }
}

#[test]
fn issue_type_flags_match_shared_filter_flag_list() {
    let mcp_flags: BTreeSet<&str> = ISSUE_TYPE_FLAGS.iter().map(|(_, flag)| *flag).collect();
    let shared: BTreeSet<&str> = DEAD_CODE_FILTER_FLAGS.iter().copied().collect();
    assert_eq!(
        mcp_flags, shared,
        "crates/mcp ISSUE_TYPE_FLAGS and fallow_types::suppress::DEAD_CODE_FILTER_FLAGS \
         must carry the same dead-code filter flags; update both when adding an issue type"
    );
}
