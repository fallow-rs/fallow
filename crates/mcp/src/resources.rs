//! MCP resources: the read-only, cacheable reference channel of the server.
//!
//! Every resource is compile-time reference material rendered in-process
//! (no subprocess, no project root): the tool manifest, the issue-type
//! registry, the explain index and per-issue-type explain documents, the
//! agent task matrix, and the three JSON Schemas. The catalogue is the shared
//! `fallow_types::mcp_manifest::MCP_RESOURCES` manifest, so `fallow schema`,
//! the generated skill reference, and the live server agree on URIs, names,
//! and MIME types; drift tests in `server/tests` pin that.
//!
//! The logic lives in free functions because rmcp's `RequestContext` cannot
//! be constructed outside a running service; the `ServerHandler` methods in
//! `server/mod.rs` are one-line delegators.

use std::sync::LazyLock;

use fallow_api::{
    RootEnvelopeMode, RuleDef, all_rules, bare_rule_id, explain_issue_type, rule_command,
    rule_docs_url, rule_severity_key, serialize_explain_programmatic_json,
};
use fallow_types::issue_meta::{issue_is_fixable, issue_meta_by_code};
use fallow_types::mcp_manifest::{
    MCP_EXPLAIN_RESOURCE_TEMPLATE, MCP_RESOURCES, MCP_TOOLS, MCP_TOOLS_KEY_PARAMS_NOTE,
    McpResourceInfo,
};
use fallow_types::task_matrix::{MUTATING_COMMANDS, TASK_MATRIX};
use rmcp::ErrorData as McpError;
use rmcp::model::{
    Annotations, ReadResourceResult, Resource, ResourceContents, ResourceTemplate, Role,
};
use serde_json::{Map, Value};

const FALLOW_VERSION: &str = env!("CARGO_PKG_VERSION");
const EXPLAIN_URI_PREFIX: &str = "fallow://explain/";
const EXPLAIN_INDEX_URI: &str = "fallow://explain";
const MAX_NEAREST_MATCHES: usize = 5;

/// Priority hints for `annotations.priority`: the tool manifest and the task
/// matrix are the entries an agent should read first, the schemas last.
const PRIORITY_PRIMARY: f32 = 0.9;
const PRIORITY_REGISTRY: f32 = 0.7;
const PRIORITY_EXPLAIN: f32 = 0.6;
const PRIORITY_SCHEMA: f32 = 0.4;

struct StaticResource {
    info: &'static McpResourceInfo,
    text: String,
}

/// Rendered static payloads, computed once per process. Every input is
/// compile-time data, so the render is deterministic and the `size` reported
/// by `resources/list` is exact.
static STATIC_RESOURCES: LazyLock<Vec<StaticResource>> = LazyLock::new(|| {
    MCP_RESOURCES
        .iter()
        .filter(|info| !info.template)
        .map(|info| StaticResource {
            info,
            text: render_static_payload(info.uri).to_string(),
        })
        .collect()
});

fn render_static_payload(uri: &str) -> Value {
    match uri {
        "fallow://tools" => tools_payload(),
        "fallow://issue-types" => issue_types_payload(),
        EXPLAIN_INDEX_URI => explain_index_payload(),
        "fallow://task-matrix" => task_matrix_payload(),
        "fallow://schema/config" => fallow_api::schemas::config_schema(),
        "fallow://schema/plugin" => fallow_api::schemas::plugin_schema(),
        "fallow://schema/rule-pack" => fallow_api::schemas::rule_pack_schema(),
        other => unreachable!(
            "MCP_RESOURCES lists {other} but crates/mcp/src/resources.rs has no renderer for it"
        ),
    }
}

/// `_meta` attached to every content item: the server version, so a cached
/// copy is self-describing without the payload itself carrying extra keys
/// (the schema documents must stay valid strict JSON Schema).
fn version_meta() -> rmcp::model::MetaObject {
    let mut meta = Map::new();
    meta.insert(
        "fallow_version".to_string(),
        Value::String(FALLOW_VERSION.to_string()),
    );
    rmcp::model::MetaObject(meta)
}

fn tools_payload() -> Value {
    serde_json::json!({
        "server": "fallow-mcp",
        "note": MCP_TOOLS_KEY_PARAMS_NOTE,
        "tools": MCP_TOOLS.iter().map(fallow_types::mcp_manifest::McpToolInfo::to_json).collect::<Vec<_>>(),
    })
}

fn explain_uri(rule: &RuleDef) -> String {
    format!("{EXPLAIN_URI_PREFIX}{}", bare_rule_id(rule))
}

fn issue_types_payload() -> Value {
    let defaults = fallow_api::schemas::default_rule_severities();
    let rows: Vec<Value> = all_rules()
        .map(|rule| {
            let bare = bare_rule_id(rule);
            let default_severity = rule_severity_key(rule)
                .and_then(|key| defaults.get(key))
                .and_then(Value::as_str);
            let (name, summary) = explain_issue_type(rule.id).map_or_else(
                |_| (rule.name.to_string(), rule.short.to_string()),
                |output| (output.name, output.summary),
            );
            serde_json::json!({
                "id": bare,
                "rule_id": rule.id,
                "command": rule_command(rule),
                "category": rule.category,
                "name": name,
                "summary": summary,
                "config_key": issue_meta_by_code(bare).and_then(|meta| meta.config_key),
                "default_severity": default_severity,
                "opt_in": default_severity.map(|severity| severity == "off"),
                "fixable": issue_is_fixable(bare),
                "docs_url": rule_docs_url(rule),
                "explain_uri": explain_uri(rule),
            })
        })
        .collect();
    serde_json::json!({
        "note": "default_severity is the zero-config rules.* severity of the rule that gates the finding (null when no rules.* key gates it); opt_in is true when that default is off. fallow schema issue_types carries the full projection (filter flags, suppression comments, result keys, frameworks)",
        "issue_types": rows,
    })
}

fn explain_index_payload() -> Value {
    let rows: Vec<Value> = all_rules()
        .map(|rule| {
            let (name, summary) = explain_issue_type(rule.id).map_or_else(
                |_| (rule.name.to_string(), rule.short.to_string()),
                |output| (output.name, output.summary),
            );
            serde_json::json!({
                "id": bare_rule_id(rule),
                "rule_id": rule.id,
                "name": name,
                "summary": summary,
                "uri": explain_uri(rule),
            })
        })
        .collect();
    serde_json::json!({
        "template": MCP_EXPLAIN_RESOURCE_TEMPLATE,
        "note": "Read uri for the full explain document (rationale, example, how to fix, docs URL); issue_type accepts the bare id, the namespaced rule id, and the CLI filter spelling",
        "issue_types": rows,
    })
}

fn task_matrix_payload() -> Value {
    serde_json::json!({
        "note": "Read-only evidence commands to run before the listed task; command may contain <placeholder> tokens. The matrix never names a mutating command",
        "excluded_commands": MUTATING_COMMANDS,
        "rows": TASK_MATRIX.iter().map(fallow_types::task_matrix::TaskRow::to_json).collect::<Vec<_>>(),
    })
}

fn priority_for(uri: &str) -> f32 {
    match uri {
        "fallow://tools" | "fallow://task-matrix" => PRIORITY_PRIMARY,
        "fallow://issue-types" => PRIORITY_REGISTRY,
        uri if uri.starts_with("fallow://schema/") => PRIORITY_SCHEMA,
        _ => PRIORITY_EXPLAIN,
    }
}

fn annotations_for(uri: &str) -> Annotations {
    Annotations::default()
        .with_audience(vec![Role::Assistant])
        .with_priority(priority_for(uri))
}

/// The concrete resource catalogue, in manifest order.
#[must_use]
pub fn list_resources() -> Vec<Resource> {
    STATIC_RESOURCES
        .iter()
        .map(|resource| {
            Resource::new(resource.info.uri, resource.info.name)
                .with_title(resource.info.title)
                .with_description(resource.info.description)
                .with_mime_type(resource.info.mime_type)
                .with_size(resource.text.len() as u64)
                .with_annotations(annotations_for(resource.info.uri))
        })
        .collect()
}

/// The resource templates, in manifest order.
#[must_use]
pub fn list_resource_templates() -> Vec<ResourceTemplate> {
    MCP_RESOURCES
        .iter()
        .filter(|info| info.template)
        .map(|info| {
            ResourceTemplate::new(info.uri, info.name)
                .with_title(info.title)
                .with_description(info.description)
                .with_mime_type(info.mime_type)
                .with_annotations(annotations_for(info.uri))
        })
        .collect()
}

/// Read one resource by URI: a static catalogue entry or an expansion of the
/// `fallow://explain/{issue_type}` template.
///
/// # Errors
///
/// Returns a structured `resource_not_found` error carrying the known URIs for
/// an unknown URI, or the nearest issue types for an unknown template
/// expansion.
pub fn read_resource(uri: &str) -> Result<ReadResourceResult, McpError> {
    if let Some(resource) = STATIC_RESOURCES.iter().find(|r| r.info.uri == uri) {
        return Ok(json_result(
            uri,
            resource.text.clone(),
            resource.info.mime_type,
        ));
    }
    if let Some(raw_issue_type) = uri.strip_prefix(EXPLAIN_URI_PREFIX)
        && !raw_issue_type.is_empty()
    {
        let issue_type = percent_decode(raw_issue_type);
        return read_explain(uri, &issue_type);
    }
    Err(unknown_uri_error(uri))
}

fn read_explain(uri: &str, issue_type: &str) -> Result<ReadResourceResult, McpError> {
    match serialize_explain_programmatic_json(issue_type, RootEnvelopeMode::Tagged, None) {
        Ok(value) => Ok(json_result(uri, value.to_string(), "application/json")),
        Err(error) => Err(McpError::resource_not_found(
            error.message,
            Some(serde_json::json!({
                "uri": uri,
                "issue_type": issue_type,
                "code": error.code,
                "nearest_matches": nearest_explain_uris(issue_type),
                "index": EXPLAIN_INDEX_URI,
            })),
        )),
    }
}

fn json_result(uri: &str, text: String, mime_type: &str) -> ReadResourceResult {
    ReadResourceResult::new(vec![
        ResourceContents::text(text, uri)
            .with_mime_type(mime_type)
            .with_meta(version_meta()),
    ])
}

fn unknown_uri_error(uri: &str) -> McpError {
    let known: Vec<&str> = MCP_RESOURCES
        .iter()
        .filter(|info| !info.template)
        .map(|info| info.uri)
        .collect();
    let templates: Vec<&str> = MCP_RESOURCES
        .iter()
        .filter(|info| info.template)
        .map(|info| info.uri)
        .collect();
    McpError::resource_not_found(
        format!("unknown fallow resource '{uri}'"),
        Some(serde_json::json!({
            "uri": uri,
            "known_uris": known,
            "templates": templates,
        })),
    )
}

/// Decode `%XX` escapes so a client that applied RFC 6570 simple expansion to
/// `security/sql-injection` (`security%2Fsql-injection`) still resolves.
fn percent_decode(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && index + 2 < bytes.len()
            && let Some(decoded) = hex_pair(bytes[index + 1], bytes[index + 2])
        {
            out.push(decoded);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(out).unwrap_or_else(|_| raw.to_string())
}

fn hex_pair(high: u8, low: u8) -> Option<u8> {
    let digit = |byte: u8| char::from(byte).to_digit(16);
    u8::try_from(digit(high)? * 16 + digit(low)?).ok()
}

/// Explain URIs for the issue types closest to an unknown token: shared
/// kebab-case words first, then substring overlap, then common prefix.
fn nearest_explain_uris(token: &str) -> Vec<String> {
    let normalized = token.trim().to_ascii_lowercase().replace('_', "-");
    let normalized = normalized
        .strip_prefix("fallow/")
        .or_else(|| normalized.strip_prefix("security/"))
        .unwrap_or(&normalized)
        .trim_start_matches("--");
    let words: Vec<&str> = normalized.split('-').filter(|w| !w.is_empty()).collect();
    let mut scored: Vec<(usize, &'static RuleDef)> = all_rules()
        .filter_map(|rule| {
            let bare = bare_rule_id(rule);
            let shared_words = bare.split('-').filter(|word| words.contains(word)).count();
            let substring = usize::from(
                !normalized.is_empty() && (bare.contains(normalized) || normalized.contains(bare)),
            );
            let prefix = bare
                .bytes()
                .zip(normalized.bytes())
                .take_while(|(left, right)| left == right)
                .count();
            let score = shared_words * 8 + substring * 4 + prefix;
            (score > 0).then_some((score, rule))
        })
        .collect();
    scored.sort_by(|left, right| {
        right
            .0
            .cmp(&left.0)
            .then_with(|| bare_rule_id(left.1).cmp(bare_rule_id(right.1)))
    });
    scored
        .into_iter()
        .take(MAX_NEAREST_MATCHES)
        .map(|(_, rule)| explain_uri(rule))
        .collect()
}
