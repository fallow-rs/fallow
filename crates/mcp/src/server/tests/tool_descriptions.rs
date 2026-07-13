use std::collections::BTreeMap;

use super::super::FallowMcp;

const DESCRIPTION_FIXTURE: &str = include_str!("fixtures/tool-descriptions.json");
const SERVER_SOURCE: &str = include_str!("../mod.rs");

fn live_tool_descriptions() -> BTreeMap<String, String> {
    let server = FallowMcp::new();
    server
        .tool_router
        .list_all()
        .iter()
        .map(|tool| {
            (
                tool.name.to_string(),
                tool.description.as_deref().unwrap_or_default().to_owned(),
            )
        })
        .collect()
}

fn descriptions_match(fixture: &str, live: &BTreeMap<String, String>) -> bool {
    serde_json::from_str::<BTreeMap<String, String>>(fixture)
        .is_ok_and(|expected| expected.eq(live))
}

#[test]
fn live_tool_descriptions_match_the_checked_fixture() {
    let live = live_tool_descriptions();
    assert!(
        descriptions_match(DESCRIPTION_FIXTURE, &live),
        "live MCP tool descriptions changed; update the checked fixture only for an intentional wire-contract change"
    );
}

#[test]
fn description_contract_detects_punctuation_and_whitespace_drift() {
    let live = BTreeMap::from([("example".to_owned(), "alpha beta".to_owned())]);

    assert!(!descriptions_match(r#"{"example":"alpha beta."}"#, &live));
    assert!(!descriptions_match(r#"{"example":"alpha  beta"}"#, &live));
}

#[test]
fn tool_attributes_take_descriptions_from_method_docs() {
    assert!(
        !SERVER_SOURCE.contains("description ="),
        "tool descriptions must live in method docs, not #[tool] arguments"
    );
}
