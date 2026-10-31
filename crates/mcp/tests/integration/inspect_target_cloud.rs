#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests use unwrap and expect to keep fixture setup concise"
)]

//! `inspect_target` with `cloud: true`, end to end, against a stubbed cloud.
//!
//! The tool runs `fallow inspect --file <path> --cloud`, so the parts to pin
//! are the ones a unit test on the argument builder cannot reach: the call
//! with a key reaches the file-context endpoint and returns the `cloud`
//! section, and a call without a key returns the `no_key` status and does not
//! fail. The compact format returns the plain text of the CLI.

use std::path::Path;

use crate::cloud_runtime_context::{McpServer, serve_once, tool_payload};

/// A file-context body for `src/api.ts`: `loadUser` is called in production.
/// Its line differs from the local line, so the name ordinal pairs it.
const FILE_CONTEXT_BODY: &str = r#"{"data":{
  "schema_version":"fallow-cloud-file-context-v1",
  "repo":"acme/app","path":"src/api.ts","state":"observed",
  "latest_sha":"abc123","period_days":30,
  "generated_at":"2026-10-29T10:00:00.000Z",
  "actionable":true,"actionability_reason":null,"never_called_withheld":false,
  "summary":{"functions":1,"called":1,"never_called":0,"untracked":0,
    "calls":4120,"error_groups":0,"error_events":0,
    "text":"1 function: 1 called."},
  "functions":[
    {"name":"loadUser","line":9,"end_line":9,"stable_id":"fn:moved",
      "state":"called","period_state":"called","calls":4120,
      "deployments_observed":2,"last_seen_at":null,
      "errors":null,"cpu":null,"browser":null,"branches":null,
      "static_callers":null,"production_callers":null,
      "summary":"loadUser ran 4120 times."}
  ],
  "warnings":[],"context_only":true}}"#;

fn write_project(root: &Path) {
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"inspect-target-cloud","type":"module"}"#,
    )
    .unwrap();
    std::fs::write(root.join(".fallowrc.json"), r#"{"entry":["src/index.ts"]}"#).unwrap();
    std::fs::write(
        root.join("src/index.ts"),
        "import { loadUser } from './api';\nexport const boot = () => loadUser('1');\n",
    )
    .unwrap();
    std::fs::write(
        root.join("src/api.ts"),
        "export function loadUser(id: string) { return id; }\n",
    )
    .unwrap();
}

fn arguments(root: &Path, extra: &serde_json::Value) -> serde_json::Value {
    let mut arguments = serde_json::json!({
        "root": root.display().to_string(),
        "target": { "type": "file", "file": "src/api.ts" },
        "cloud": true,
        "repo": "acme/app",
        "no_cache": true
    });
    if let (Some(base), Some(extra)) = (arguments.as_object_mut(), extra.as_object()) {
        base.extend(extra.clone());
    }
    arguments
}

#[test]
fn inspect_target_with_a_key_returns_the_cloud_section() {
    let project = tempfile::tempdir().expect("project dir");
    write_project(project.path());
    let (endpoint, request, handle) = serve_once(FILE_CONTEXT_BODY);
    let mut server = McpServer::start(Some(&endpoint));
    let result = server.call_tool(
        "inspect_target",
        &arguments(project.path(), &serde_json::json!({})),
    );

    // Assert before the join: a call that never reaches the stub must fail
    // here, not wait on the stub forever.
    assert_ne!(result["isError"], true, "call must succeed: {result}");
    let payload = tool_payload(&result);
    let cloud = &payload["evidence"]["cloud"];
    assert_eq!(cloud["status"], "ok", "{cloud}");
    assert_eq!(cloud["repo"], "acme/app");
    assert_eq!(cloud["functions"][0]["name"], "loadUser");
    assert_eq!(cloud["functions"][0]["line"], 1);
    assert_eq!(cloud["functions"][0]["calls"], 4120);
    handle.join().expect("stub server joins");

    let request = request.lock().expect("request lock").clone();
    assert!(
        request.starts_with("GET /v1/coverage/acme%2Fapp/file-context?path=src%2Fapi.ts"),
        "the tool must reach the file-context endpoint: {request}"
    );
    assert!(
        request
            .to_ascii_lowercase()
            .contains("authorization: bearer fallow_live_test"),
        "the request must carry the key of the server environment: {request}"
    );
}

#[test]
fn inspect_target_without_a_key_reports_no_key_and_does_not_fail() {
    let project = tempfile::tempdir().expect("project dir");
    write_project(project.path());
    let mut server = McpServer::start(None);
    let result = server.call_tool(
        "inspect_target",
        &arguments(project.path(), &serde_json::json!({})),
    );

    assert_ne!(result["isError"], true, "no_key must not fail: {result}");
    let payload = tool_payload(&result);
    let cloud = &payload["evidence"]["cloud"];
    assert_eq!(cloud["status"], "no_key", "{cloud}");
    assert!(
        cloud["message"]
            .as_str()
            .is_some_and(|message| message.contains("fallow cloud login")),
        "{cloud}"
    );
}

#[test]
fn inspect_target_compact_returns_the_plain_text() {
    let project = tempfile::tempdir().expect("project dir");
    write_project(project.path());
    let (endpoint, _request, handle) = serve_once(FILE_CONTEXT_BODY);
    let mut server = McpServer::start(Some(&endpoint));
    let result = server.call_tool(
        "inspect_target",
        &arguments(project.path(), &serde_json::json!({ "format": "compact" })),
    );

    assert_ne!(result["isError"], true, "call must succeed: {result}");
    let text = result["content"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("text content: {result}"));
    assert!(text.starts_with("fallow src/api.ts\n"), "{text}");
    assert!(
        text.contains("\nprod loadUser:1 called 4120 calls"),
        "{text}"
    );
    assert!(
        serde_json::from_str::<serde_json::Value>(text).is_err(),
        "compact is plain text, not JSON: {text}"
    );
    handle.join().expect("stub server joins");
}
