//! Catalogue, reader, and error contract of the MCP resource surface. The
//! exact URI strings are pinned as literals on purpose: `fallow://tools` and
//! `fallow:///tools` look identical in prose and differ on the wire.

use std::collections::BTreeSet;

use fallow_api::{
    CHECK_RULES, DUPES_RULES, FLAGS_RULES, HEALTH_RULES, SECURITY_RULES, bare_rule_id,
};
use fallow_types::mcp_manifest::MCP_RESOURCES;
use rmcp::model::{ErrorCode, ResourceContents, Role};

use crate::resources::{list_resource_templates, list_resources, read_resource};

const STATIC_URIS: [&str; 7] = [
    "fallow://tools",
    "fallow://issue-types",
    "fallow://explain",
    "fallow://task-matrix",
    "fallow://schema/config",
    "fallow://schema/plugin",
    "fallow://schema/rule-pack",
];

fn read_text(uri: &str) -> String {
    let result = read_resource(uri).unwrap_or_else(|err| panic!("{uri} must read: {err:?}"));
    let [content] = result.contents.as_slice() else {
        panic!("{uri} must return exactly one content item");
    };
    let ResourceContents::TextResourceContents {
        uri: content_uri,
        mime_type,
        text,
        ..
    } = content
    else {
        panic!("{uri} must return text contents");
    };
    assert_eq!(content_uri, uri, "content must echo the requested uri");
    assert_eq!(mime_type.as_deref(), Some("application/json"));
    text.clone()
}

fn read_json(uri: &str) -> serde_json::Value {
    let text = read_text(uri);
    serde_json::from_str(&text).unwrap_or_else(|err| panic!("{uri} must be valid JSON: {err}"))
}

#[test]
fn catalogue_order_and_uris_are_pinned() {
    let listed: Vec<String> = list_resources().iter().map(|r| r.uri.clone()).collect();
    assert_eq!(listed, STATIC_URIS);
    let templates: Vec<String> = list_resource_templates()
        .iter()
        .map(|t| t.uri_template.clone())
        .collect();
    assert_eq!(templates, ["fallow://explain/{issue_type}"]);
}

#[test]
fn static_resources_carry_size_mime_and_assistant_annotations() {
    for resource in list_resources() {
        let text = read_text(&resource.uri);
        assert_eq!(
            resource.size,
            Some(text.len() as u64),
            "{} size must match the rendered payload",
            resource.uri
        );
        assert_eq!(resource.mime_type.as_deref(), Some("application/json"));
        assert!(
            resource
                .description
                .as_deref()
                .is_some_and(|d| !d.is_empty()),
            "{} needs a description",
            resource.uri
        );
        let annotations = resource
            .annotations
            .as_ref()
            .unwrap_or_else(|| panic!("{} needs annotations", resource.uri));
        assert_eq!(annotations.audience, Some(vec![Role::Assistant]));
        assert!(
            annotations.last_modified.is_none(),
            "compiled-in data has no meaningful mtime"
        );
        let priority = annotations.priority.expect("priority");
        assert!((0.0..=1.0).contains(&priority));
    }
    let priority = |uri: &str| {
        list_resources()
            .into_iter()
            .find(|r| r.uri == uri)
            .and_then(|r| r.annotations)
            .and_then(|a| a.priority)
            .expect("priority")
    };
    assert!(priority("fallow://tools") > priority("fallow://schema/config"));
    assert!(priority("fallow://task-matrix") > priority("fallow://schema/rule-pack"));
}

#[test]
fn every_static_payload_is_json_with_fallow_version() {
    for uri in STATIC_URIS {
        let json = read_json(uri);
        assert_eq!(
            json["fallow_version"],
            env!("CARGO_PKG_VERSION"),
            "{uri} must carry fallow_version"
        );
    }
}

#[test]
fn tools_resource_mirrors_the_shared_manifest() {
    let json = read_json("fallow://tools");
    let tools = json["tools"].as_array().expect("tools array");
    assert_eq!(tools.len(), fallow_types::mcp_manifest::MCP_TOOLS.len());
    let analyze = tools
        .iter()
        .find(|t| t["name"] == "analyze")
        .expect("analyze row");
    assert_eq!(
        analyze["cli_command"],
        "fallow dead-code --format json --quiet"
    );
    assert_eq!(analyze["read_only"], true);
    assert_eq!(analyze["license"], "free");
}

#[test]
fn issue_types_resource_carries_severity_fixable_and_explain_uri() {
    let json = read_json("fallow://issue-types");
    let rows = json["issue_types"].as_array().expect("issue_types array");
    let expected: usize = [
        CHECK_RULES,
        HEALTH_RULES,
        DUPES_RULES,
        FLAGS_RULES,
        SECURITY_RULES,
    ]
    .iter()
    .map(|rules| rules.len())
    .sum();
    assert_eq!(rows.len(), expected);
    for row in rows {
        for key in [
            "id",
            "rule_id",
            "command",
            "category",
            "name",
            "summary",
            "config_key",
            "default_severity",
            "opt_in",
            "fixable",
            "docs_url",
            "explain_uri",
        ] {
            assert!(
                row.get(key).is_some(),
                "issue type {} missing key {key}",
                row["id"]
            );
        }
        assert!(
            row["docs_url"]
                .as_str()
                .is_some_and(|url| url.starts_with("https://docs.fallow.tools/")),
            "{} docs_url must be a docs site URL",
            row["id"]
        );
    }
    let unused_export = rows
        .iter()
        .find(|r| r["id"] == "unused-export")
        .expect("unused-export row");
    assert_eq!(unused_export["command"], "dead-code");
    assert_eq!(unused_export["config_key"], "unused-exports");
    assert_eq!(unused_export["default_severity"], "error");
    assert_eq!(unused_export["opt_in"], false);
    assert_eq!(unused_export["fixable"], true);
    assert_eq!(
        unused_export["explain_uri"],
        "fallow://explain/unused-export"
    );
    let sql_injection = rows
        .iter()
        .find(|r| r["id"] == "sql-injection")
        .expect("sql-injection row");
    assert_eq!(sql_injection["command"], "security");
    assert_eq!(sql_injection["default_severity"], "off");
    assert_eq!(sql_injection["opt_in"], true);
    assert_eq!(sql_injection["fixable"], false);
    let complexity = rows
        .iter()
        .find(|r| r["id"] == "high-cyclomatic-complexity")
        .expect("complexity row");
    assert!(complexity["default_severity"].is_null());
    assert!(complexity["opt_in"].is_null());
}

#[test]
fn explain_index_lists_every_rule_with_its_template_uri() {
    let json = read_json("fallow://explain");
    assert_eq!(json["template"], "fallow://explain/{issue_type}");
    let rows = json["issue_types"].as_array().expect("issue_types array");
    let listed: BTreeSet<&str> = rows.iter().filter_map(|r| r["uri"].as_str()).collect();
    for rule in fallow_api::all_rules() {
        let uri = format!("fallow://explain/{}", bare_rule_id(rule));
        assert!(listed.contains(uri.as_str()), "index missing {uri}");
    }
}

#[test]
fn task_matrix_resource_projects_rows_without_probe() {
    let json = read_json("fallow://task-matrix");
    let rows = json["rows"].as_array().expect("rows array");
    assert_eq!(rows.len(), fallow_types::task_matrix::TASK_MATRIX.len());
    for (row, source) in rows.iter().zip(fallow_types::task_matrix::TASK_MATRIX) {
        assert_eq!(row["task"], source.task);
        assert_eq!(row["command"], source.command);
        assert!(row.get("note").is_some(), "note key must always be present");
        assert!(row.get("probe").is_none(), "probe is test-only data");
    }
    assert_eq!(
        json["excluded_commands"],
        serde_json::json!(fallow_types::task_matrix::MUTATING_COMMANDS)
    );
}

#[test]
fn schema_resources_equal_the_cli_schema_documents_plus_version() {
    for (uri, expected) in [
        (
            "fallow://schema/config",
            fallow_api::schemas::config_schema(),
        ),
        (
            "fallow://schema/plugin",
            fallow_api::schemas::plugin_schema(),
        ),
        (
            "fallow://schema/rule-pack",
            fallow_api::schemas::rule_pack_schema(),
        ),
    ] {
        let mut json = read_json(uri);
        let object = json.as_object_mut().expect("schema object");
        assert!(object.shift_remove("fallow_version").is_some());
        assert_eq!(json, expected, "{uri} must be the CLI schema document");
    }
}

#[test]
fn every_registered_rule_resolves_through_the_explain_template() {
    for rule in fallow_api::all_rules() {
        for token in [bare_rule_id(rule), rule.id] {
            let uri = format!("fallow://explain/{token}");
            let json = read_json(&uri);
            assert_eq!(json["kind"], "explain", "{uri}");
            assert_eq!(json["id"], rule.id, "{uri}");
            assert_eq!(json["fallow_version"], env!("CARGO_PKG_VERSION"));
            for key in [
                "name",
                "summary",
                "rationale",
                "example",
                "how_to_fix",
                "docs",
            ] {
                assert!(json[key].is_string(), "{uri} missing {key}");
            }
        }
    }
}

#[test]
fn explain_template_matches_the_fallow_explain_tool_payload() {
    let mut resource = read_json("fallow://explain/unused-export");
    resource
        .as_object_mut()
        .expect("object")
        .shift_remove("fallow_version");
    let tool = fallow_api::serialize_explain_programmatic_json(
        "unused-export",
        fallow_api::RootEnvelopeMode::Tagged,
        None,
    )
    .expect("tool payload");
    assert_eq!(resource, tool);
}

#[test]
fn explain_template_accepts_percent_encoded_namespaced_ids() {
    let json = read_json("fallow://explain/security%2Fsql-injection");
    assert_eq!(json["id"], "security/sql-injection");
    let plain = read_json("fallow://explain/security/sql-injection");
    assert_eq!(plain["id"], "security/sql-injection");
}

#[test]
fn unknown_uri_is_a_structured_resource_not_found_error() {
    for uri in ["fallow:///tools", "fallow://nope", "file:///etc/passwd", ""] {
        let error = read_resource(uri).expect_err("unknown uri must fail");
        assert_eq!(error.code, ErrorCode::RESOURCE_NOT_FOUND, "{uri}");
        let data = error.data.expect("structured data");
        assert_eq!(data["uri"], uri);
        assert_eq!(data["known_uris"], serde_json::json!(STATIC_URIS));
        assert_eq!(
            data["templates"],
            serde_json::json!(["fallow://explain/{issue_type}"])
        );
    }
}

#[test]
fn unknown_issue_type_lists_nearest_matches() {
    let error = read_resource("fallow://explain/unused-exprt").expect_err("unknown issue type");
    assert_eq!(error.code, ErrorCode::RESOURCE_NOT_FOUND);
    let data = error.data.expect("structured data");
    assert_eq!(data["code"], "unknown_issue_type");
    assert_eq!(data["issue_type"], "unused-exprt");
    assert_eq!(data["index"], "fallow://explain");
    let nearest = data["nearest_matches"].as_array().expect("nearest array");
    assert!(!nearest.is_empty() && nearest.len() <= 5);
    assert!(
        nearest.iter().all(|u| u
            .as_str()
            .is_some_and(|u| u.starts_with("fallow://explain/"))),
        "nearest matches are explain URIs: {nearest:?}"
    );
    assert!(
        nearest
            .iter()
            .any(|u| u == "fallow://explain/unused-export"),
        "shared kebab words rank unused-export first: {nearest:?}"
    );
    let empty = read_resource("fallow://explain/").expect_err("empty issue type");
    assert_eq!(empty.code, ErrorCode::RESOURCE_NOT_FOUND);
}

#[test]
fn manifest_and_live_catalogue_agree_both_directions() {
    let manifest_static: BTreeSet<&str> = MCP_RESOURCES
        .iter()
        .filter(|r| !r.template)
        .map(|r| r.uri)
        .collect();
    let live_static: BTreeSet<String> = list_resources().into_iter().map(|r| r.uri).collect();
    assert_eq!(
        live_static
            .iter()
            .map(String::as_str)
            .collect::<BTreeSet<_>>(),
        manifest_static
    );
    for uri in manifest_static {
        read_text(uri);
    }
}
