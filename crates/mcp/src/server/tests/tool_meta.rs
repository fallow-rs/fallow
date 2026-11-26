use std::collections::BTreeSet;

use super::super::FallowMcp;

/// Claude Code saves a text result over this many characters to a file,
/// unless the tool declares `anthropic/maxResultSizeChars`.
const HOST_DEFAULT_RESULT_CHARS: u64 = 50_000;

/// Claude Code ignores a declared result limit above this value.
const HOST_MAX_RESULT_CHARS: u64 = 500_000;

/// The tools whose result passed 50,000 characters on a large public
/// project. Measure a tool on such a project before you add it here.
const LARGE_RESULT_TOOLS: [&str; 13] = [
    "analyze",
    "audit",
    "check_architecture",
    "check_changed",
    "check_health",
    "code_execute",
    "find_dupes",
    "fix_preview",
    "get_token_blast_radius",
    "impact_all",
    "project_info",
    "security_candidates",
    "trace_file",
];

/// The `_meta` objects as a client receives them in `tools/list`.
fn declared_result_limits() -> Vec<(String, Option<serde_json::Value>)> {
    FallowMcp::new()
        .tool_router
        .list_all()
        .iter()
        .map(|tool| {
            let wire = serde_json::to_value(tool).expect("a tool serializes");
            let limit = wire
                .get("_meta")
                .and_then(|meta| meta.get("anthropic/maxResultSizeChars"))
                .cloned();
            (tool.name.to_string(), limit)
        })
        .collect()
}

#[test]
fn only_the_large_result_tools_declare_a_result_limit() {
    let declared: BTreeSet<String> = declared_result_limits()
        .into_iter()
        .filter_map(|(name, limit)| limit.map(|_| name))
        .collect();
    let expected: BTreeSet<String> = LARGE_RESULT_TOOLS.iter().map(ToString::to_string).collect();
    assert_eq!(declared, expected);
}

#[test]
fn each_declared_result_limit_is_an_integer_the_host_accepts() {
    for (name, limit) in declared_result_limits() {
        let Some(limit) = limit else { continue };
        let chars = limit.as_u64().unwrap_or_else(|| {
            panic!("{name}: maxResultSizeChars must be an integer, got {limit}")
        });
        assert!(
            chars > HOST_DEFAULT_RESULT_CHARS && chars <= HOST_MAX_RESULT_CHARS,
            "{name}: maxResultSizeChars {chars} must be above the host default \
             {HOST_DEFAULT_RESULT_CHARS} and at most the host ceiling {HOST_MAX_RESULT_CHARS}"
        );
    }
}

/// An upfront tool puts its description and its schema in every session, and
/// Claude Code cuts that description at 4,096 characters. The server
/// instructions already route agents to the entry points.
#[test]
fn no_tool_asks_to_load_upfront() {
    let upfront: Vec<String> = FallowMcp::new()
        .tool_router
        .list_all()
        .iter()
        .filter(|tool| {
            let wire = serde_json::to_value(tool).expect("a tool serializes");
            wire.get("_meta")
                .is_some_and(|meta| meta.get("anthropic/alwaysLoad").is_some())
        })
        .map(|tool| tool.name.to_string())
        .collect();
    assert!(
        upfront.is_empty(),
        "tools with anthropic/alwaysLoad: {upfront:?}"
    );
}
