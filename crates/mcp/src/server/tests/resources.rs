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

const STATIC_URIS: [&str; 8] = [
    "fallow://tools",
    "fallow://issue-types",
    "fallow://explain",
    "fallow://task-matrix",
    "fallow://schema/config",
    "fallow://schema/plugin",
    "fallow://schema/rule-pack",
    "fallow://schema/similar-code-snapshot",
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
    assert_eq!(
        templates,
        ["fallow://tools/{name}", "fallow://explain/{issue_type}"]
    );
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
fn every_read_carries_fallow_version_in_meta_not_in_the_payload() {
    for uri in STATIC_URIS
        .iter()
        .copied()
        .chain(std::iter::once("fallow://explain/unused-export"))
    {
        let result = serde_json::to_value(read_resource(uri).expect("readable"))
            .expect("serializable result");
        assert_eq!(
            result["contents"][0]["_meta"]["fallow_version"],
            env!("CARGO_PKG_VERSION"),
            "{uri} must carry fallow_version in _meta"
        );
        assert!(
            read_json(uri).get("fallow_version").is_none(),
            "{uri} payload must not carry fallow_version"
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
    assert_eq!(analyze["read_only"], false);
    assert_eq!(analyze["license"], "free");

    for row in tools {
        let name = row["name"].as_str().expect("tool name");
        let manifest = fallow_types::mcp_manifest::MCP_TOOLS
            .iter()
            .find(|entry| entry.name == name)
            .unwrap_or_else(|| panic!("{name} in the shared manifest"));
        assert_eq!(
            row["code_mode_alias"].as_str(),
            manifest.code_mode_alias,
            "the fallow://tools resource must carry {name}'s Code Mode reachability so agents \
             never have to parse the code_execute description"
        );
    }
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
        (
            "fallow://schema/similar-code-snapshot",
            fallow_api::schemas::similar_code_snapshot_schema(),
        ),
    ] {
        let json = read_json(uri);
        assert_eq!(json, expected, "{uri} must be the CLI schema document");
    }
}

/// The guide template carries the per-flag prose the wire description no
/// longer does, so every section that moved must still be reachable.
#[test]
fn tool_guide_template_serves_the_prose_kept_out_of_tools_list() {
    let json = read_json("fallow://tools/check_health");
    assert_eq!(json["tool"], "check_health");
    let sections = json["sections"].as_array().expect("sections array");
    let topics: BTreeSet<&str> = sections
        .iter()
        .filter_map(|section| section["topic"].as_str())
        .collect();
    for topic in [
        "css",
        "complexity_breakdown",
        "react_hook_profile",
        "vital_signs.render_fan_in",
        "threshold_overrides",
    ] {
        assert!(topics.contains(topic), "guide missing {topic}: {topics:?}");
    }
    // The guide exists because the detail did not fit on the wire beside the
    // summary. Assert that relationship rather than a character count, which a
    // wording pass moves without changing what the resource is for.
    for section in sections {
        let summary = section["summary"].as_str().expect("section summary");
        let detail = section["detail"].as_str().expect("section detail");
        assert!(
            detail.len() > summary.len(),
            "a guide section must say more than the summary it expands: {section}"
        );
    }
}

/// The shortest run of guide words whose verbatim appearance in the terse
/// catalogue is evidence of a paste rather than of two texts describing the
/// same subject. Short runs collide honestly ("the fallow config file"); this
/// many words in a row, in this order, do not.
const GUIDE_PROSE_RUN_WORDS: usize = 8;

/// Every string leaf of a resource payload, whitespace-normalized. The
/// catalogue is JSON, so a pasted sentence lands inside a field with its
/// quotes escaped; comparing against the decoded leaves rather than the raw
/// document is what makes the paste visible.
fn normalized_string_leaves(value: &serde_json::Value) -> Vec<String> {
    let mut leaves = Vec::new();
    let mut stack = vec![value];
    while let Some(node) = stack.pop() {
        match node {
            serde_json::Value::String(text) => {
                leaves.push(text.split_whitespace().collect::<Vec<_>>().join(" "));
            }
            serde_json::Value::Array(items) => stack.extend(items),
            serde_json::Value::Object(map) => stack.extend(map.values()),
            _ => {}
        }
    }
    leaves
}

/// The first run of `GUIDE_PROSE_RUN_WORDS` consecutive words of `prose` that
/// appears verbatim in one of `haystack`, or `None` when none does.
fn leaked_prose_run(haystack: &[String], prose: &str) -> Option<String> {
    let words: Vec<&str> = prose.split_whitespace().collect();
    words
        .windows(GUIDE_PROSE_RUN_WORDS)
        .map(|run| run.join(" "))
        .find(|run| haystack.iter().any(|line| line.contains(run.as_str())))
}

/// The terse `fallow://tools` catalogue and the long-form guide are different
/// channels; reading the catalogue must not start returning guide prose.
///
/// Every section of every guide is checked, and by word run rather than by
/// whole-string equality: asserting `!contains(detail)` on one section of one
/// guide rejected exactly one byte-exact paste of exactly one paragraph, so a
/// catalogue line could absorb most of a guide section and stay green.
#[test]
fn tool_guide_prose_stays_out_of_the_tools_catalogue() {
    let catalogue = normalized_string_leaves(&read_json("fallow://tools"));
    for guide in crate::tool_guides::TOOL_GUIDES {
        let json = read_json(&format!("fallow://tools/{}", guide.tool));
        for section in json["sections"].as_array().expect("sections array") {
            let topic = section["topic"].as_str().expect("section topic");
            for channel in ["summary", "detail"] {
                let prose = section[channel].as_str().expect("section prose");
                assert_eq!(
                    leaked_prose_run(&catalogue, prose),
                    None,
                    "fallow://tools repeats {}'s {topic} {channel}; it must stay the \
                     one-line-per-tool catalogue, with long prose in the guide",
                    guide.tool
                );
            }
        }
    }
}

/// The guard above is only worth having if it fires. A catalogue line that
/// absorbed a slice of guide prose (not the whole section, which is what the
/// earlier whole-string check demanded) must be caught.
#[test]
fn a_partial_guide_paste_is_caught_in_the_catalogue() {
    let guide = read_json("fallow://tools/check_health");
    let detail = guide["sections"][0]["detail"]
        .as_str()
        .expect("first section detail");
    let pasted: String = detail.chars().take(200).collect();
    assert!(
        pasted.split_whitespace().count() > GUIDE_PROSE_RUN_WORDS,
        "the reproduction needs more words than one run: {pasted}"
    );

    let catalogue = vec![format!("check_health. {pasted} Reports project health.")];

    assert!(
        leaked_prose_run(&catalogue, detail).is_some(),
        "200 bytes of guide prose pasted into a catalogue line must be caught"
    );
    assert_eq!(
        leaked_prose_run(&catalogue, "an unrelated one-line catalogue summary"),
        None,
        "prose that was never pasted must not be reported as a leak"
    );
}

#[test]
fn misspelled_tool_guide_suggests_the_nearest_documented_tool() {
    let error = read_resource("fallow://tools/check_helth").expect_err("unknown tool guide");
    let data = error.data.expect("structured error data");
    assert_eq!(data["code"], "unknown_tool");
    assert_eq!(data["registered_tool"], false);
    assert_eq!(
        data["nearest_matches"],
        serde_json::json!(["fallow://tools/check_health"])
    );
}

/// A typo and a real tool that simply has no guide are different problems with
/// different fixes. They used to return byte-identical bodies, so a caller
/// could not tell "correct the name" from "stop looking, the description is
/// the whole contract".
#[test]
fn a_typo_and_an_undocumented_tool_are_distinguishable() {
    let undocumented =
        read_resource("fallow://tools/fix_apply").expect_err("fix_apply has no guide");
    let typo = read_resource("fallow://tools/totally_made_up").expect_err("not a tool");

    let undocumented_data = undocumented.data.expect("structured error data");
    let typo_data = typo.data.expect("structured error data");

    assert_eq!(undocumented_data["code"], "no_tool_guide");
    assert_eq!(undocumented_data["registered_tool"], true);
    assert_eq!(typo_data["code"], "unknown_tool");
    assert_eq!(typo_data["registered_tool"], false);
    assert_ne!(
        undocumented.message, typo.message,
        "a registered tool with no guide must not read like a hallucinated name"
    );
}

#[test]
fn every_registered_rule_resolves_through_the_explain_template() {
    for rule in fallow_api::all_rules() {
        for token in [bare_rule_id(rule), rule.id] {
            let uri = format!("fallow://explain/{token}");
            let json = read_json(&uri);
            assert_eq!(json["kind"], "explain", "{uri}");
            assert_eq!(json["id"], rule.id, "{uri}");
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
    let resource = read_json("fallow://explain/unused-export");
    let tool = fallow_api::serialize_explain_programmatic_json("unused-export", None)
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
            serde_json::json!(["fallow://tools/{name}", "fallow://explain/{issue_type}"])
        );
    }
}

#[test]
fn near_miss_uri_names_the_resource_the_caller_meant() {
    let error = read_resource("fallow://task-matrx").expect_err("unknown uri must fail");
    assert!(
        error
            .message
            .contains("did you mean 'fallow://task-matrix'?"),
        "near-miss uri should be named: {}",
        error.message
    );
}

#[test]
fn novel_uri_stays_silent_rather_than_guessing() {
    let error = read_resource("file:///etc/passwd").expect_err("unknown uri must fail");
    assert!(
        !error.message.contains("did you mean"),
        "a completely novel uri must not get a misleading suggestion: {}",
        error.message
    );
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

/// The shared scorer answers an unknown issue type, and the only shaping this
/// resource still does for it is the namespace a caller may have copied off a
/// printed rule id or a CLI flag. `/` is not a word separator the scorer
/// splits on, so `security/sql-injction` would otherwise align its first word
/// against `security/sql` and match nothing; `_` and `-` it splits on itself,
/// which is why no case folding is left here.
#[test]
fn an_issue_type_typo_resolves_through_any_spelling_of_the_id() {
    for (token, expected) in [
        ("security/sql-injction", "fallow://explain/sql-injection"),
        ("--unused-exprt", "fallow://explain/unused-export"),
        ("unused_exprt", "fallow://explain/unused-export"),
    ] {
        let error =
            read_resource(&format!("fallow://explain/{token}")).expect_err("unknown issue type");
        let data = error.data.expect("structured data");
        let nearest = data["nearest_matches"].as_array().expect("nearest array");
        assert!(
            nearest.iter().any(|uri| uri == expected),
            "{token} must reach {expected}: {nearest:?}"
        );
    }
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
