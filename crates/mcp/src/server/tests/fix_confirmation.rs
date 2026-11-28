//! `fix_apply` asks for confirmation through form elicitation before it
//! writes, on both protocol routes, and writes nothing without a yes.
//!
//! The tests speak raw JSON-RPC over an in-memory stream, so they pin the
//! wire shape that a client sees, and run the real `fallow` binary.

use std::path::{Path, PathBuf};
use std::time::Duration;

use rmcp::ServiceExt;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, ReadHalf, WriteHalf};

use super::super::FallowMcp;
use super::e2e::fallow_binary;
use crate::tools::{CONFIRM_INPUT_KEY, CONFIRM_TIMEOUT};

const GIVE_UP: Duration = Duration::from_secs(120);
const UNUSED_EXPORT: &str = "export const unusedHelper";
const LIBRARY: &str =
    "export const used = (): number => 1;\n\nexport const unusedHelper = (): number => 2;\n";

struct Client {
    reader: BufReader<ReadHalf<tokio::io::DuplexStream>>,
    writer: WriteHalf<tokio::io::DuplexStream>,
    server: tokio::task::JoinHandle<()>,
}

impl Client {
    fn start() -> Self {
        Self::start_with_timeout(CONFIRM_TIMEOUT)
    }

    fn start_with_timeout(confirm_timeout: Duration) -> Self {
        let server = FallowMcp {
            binary: fallow_binary(),
            cargo_fallow: "cargo-fallow".to_string(),
            tool_router: FallowMcp::tool_router(),
            confirm_timeout,
            tasks: crate::tasks::TaskStore::new(),
            code_mode_worker: None,
        };
        let (client, server_io) = tokio::io::duplex(1024 * 1024);
        let (server_read, server_write) = tokio::io::split(server_io);
        let server = tokio::spawn(async move {
            let service = server
                .serve((server_read, server_write))
                .await
                .expect("the server starts");
            let _ = service.waiting().await;
        });
        let (reader, writer) = tokio::io::split(client);
        Self {
            reader: BufReader::new(reader),
            writer,
            server,
        }
    }

    async fn send(&mut self, message: Value) {
        let mut line = message.to_string();
        line.push('\n');
        self.writer
            .write_all(line.as_bytes())
            .await
            .expect("write a message to the server");
    }

    /// Read server messages until one matches `wanted`.
    async fn read_until(&mut self, wanted: impl Fn(&Value) -> bool) -> Value {
        let mut line = String::new();
        loop {
            line.clear();
            let read = tokio::time::timeout(GIVE_UP, self.reader.read_line(&mut line))
                .await
                .expect("the server answers in time")
                .expect("read a server message");
            assert!(read > 0, "the server closed the stream");
            let message: Value = serde_json::from_str(&line).expect("server messages are JSON");
            if wanted(&message) {
                return message;
            }
        }
    }

    /// The next server request, or the response with `id`.
    async fn next(&mut self, id: u64) -> Value {
        self.read_until(|message| {
            let request = message.get("method").is_some() && message["id"].is_number();
            let response = message.get("method").is_none() && message["id"] == id;
            request || response
        })
        .await
    }

    /// Open a session on protocol 2025-11-25 with `capabilities`.
    async fn initialize(&mut self, capabilities: Value) {
        self.send(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-11-25",
                "capabilities": capabilities,
                "clientInfo": { "name": "fix-confirmation-test", "version": "0" }
            }
        }))
        .await;
        let response = self.next(1).await;
        assert!(response.get("result").is_some(), "{response}");
        self.send(json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }))
            .await;
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        self.server.abort();
    }
}

/// A project whose `src/lib.ts` is `library`.
fn project_with(library: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("temporary project");
    let root = dir.path().join("project");
    std::fs::create_dir_all(root.join("src")).expect("create project");
    std::fs::write(
        root.join("package.json"),
        r#"{ "name": "fix-confirmation", "version": "1.0.0", "private": true, "main": "src/index.ts" }"#,
    )
    .expect("write manifest");
    std::fs::write(root.join("src/lib.ts"), library).expect("write library");
    std::fs::write(
        root.join("src/index.ts"),
        "import { used } from \"./lib\";\n\nexport const run = (): number => used();\n",
    )
    .expect("write entry");
    (dir, root)
}

/// A project with one unused export in `src/lib.ts`.
fn project() -> (tempfile::TempDir, PathBuf) {
    project_with(LIBRARY)
}

fn library(root: &Path) -> String {
    std::fs::read_to_string(root.join("src/lib.ts")).expect("read library")
}

fn arguments(root: &Path) -> Value {
    json!({
        "root": root.to_string_lossy(),
        "no_cache": true,
        "no_create_config": true
    })
}

fn call(id: u64, root: &Path) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "tools/call",
        "params": { "name": "fix_apply", "arguments": arguments(root) }
    })
}

fn body(response: &Value) -> Value {
    let text = response["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("a text result: {response}"));
    serde_json::from_str(text).unwrap_or_else(|e| panic!("JSON text: {e}\n{text}"))
}

fn accept() -> Value {
    json!({ "action": "accept", "content": { "apply": true } })
}

/// Start a legacy session with form elicitation, call `fix_apply`, and
/// return the `elicitation/create` request after checking that no file
/// changed yet.
async fn legacy_form(client: &mut Client, root: &Path) -> Value {
    client
        .initialize(json!({ "elicitation": { "form": {} } }))
        .await;
    client.send(call(2, root)).await;
    let request = client.next(2).await;
    assert_eq!(request["method"], "elicitation/create", "{request}");
    assert_eq!(request["params"]["mode"], "form", "{request}");
    assert!(
        library(root).contains(UNUSED_EXPORT),
        "fix_apply wrote before the answer"
    );
    request
}

/// Run one call on the legacy route and answer its form with `answer`.
async fn legacy_call_with_answer(answer: Value) -> (PathBuf, Value, tempfile::TempDir) {
    let (dir, root) = project();
    let mut client = Client::start();
    let request = legacy_form(&mut client, &root).await;
    let message = request["params"]["message"].as_str().unwrap_or_default();
    assert!(message.contains("1 fix in 1 file"), "{message}");
    assert!(message.contains("remove export"), "{message}");
    client
        .send(json!({ "jsonrpc": "2.0", "id": request["id"], "result": answer }))
        .await;
    let response = client.next(2).await;
    (root, response, dir)
}

#[tokio::test]
async fn legacy_decline_writes_nothing() {
    let (root, response, _dir) = legacy_call_with_answer(json!({ "action": "decline" })).await;
    let body = body(&response);
    assert_eq!(body["applied"], false, "{body}");
    assert_eq!(body["confirmation"], "declined", "{body}");
    assert_eq!(body["planned_fixes"], 1, "{body}");
    assert_ne!(response["result"]["isError"], true, "{response}");
    assert!(library(&root).contains(UNUSED_EXPORT));
}

#[tokio::test]
async fn legacy_cancel_writes_nothing() {
    let (root, response, _dir) = legacy_call_with_answer(json!({ "action": "cancel" })).await;
    assert_eq!(body(&response)["confirmation"], "cancelled", "{response}");
    assert!(library(&root).contains(UNUSED_EXPORT));
}

#[tokio::test]
async fn legacy_accept_with_cleared_box_writes_nothing() {
    let (root, response, _dir) =
        legacy_call_with_answer(json!({ "action": "accept", "content": { "apply": false } })).await;
    assert_eq!(body(&response)["confirmation"], "declined", "{response}");
    assert!(library(&root).contains(UNUSED_EXPORT));
}

#[tokio::test]
async fn legacy_accept_applies_the_fix() {
    let (root, response, _dir) = legacy_call_with_answer(accept()).await;
    let body = body(&response);
    assert_eq!(body["dry_run"], false, "{body}");
    assert_eq!(body["total_fixed"], 1, "{body}");
    assert!(!library(&root).contains(UNUSED_EXPORT));
}

#[tokio::test]
async fn legacy_client_error_writes_nothing() {
    let (_dir, root) = project();
    let mut client = Client::start();
    let request = legacy_form(&mut client, &root).await;
    client
        .send(json!({
            "jsonrpc": "2.0",
            "id": request["id"],
            "error": { "code": -32603, "message": "no dialog" }
        }))
        .await;
    let response = client.next(2).await;
    assert_eq!(body(&response)["confirmation"], "failed", "{response}");
    assert!(library(&root).contains(UNUSED_EXPORT));
}

#[tokio::test]
async fn legacy_timeout_writes_nothing() {
    let (_dir, root) = project();
    let mut client = Client::start_with_timeout(Duration::from_millis(200));
    legacy_form(&mut client, &root).await;
    let response = client.next(2).await;
    assert_eq!(body(&response)["confirmation"], "timed_out", "{response}");
    assert!(library(&root).contains(UNUSED_EXPORT));
}

/// A cancel of the tool call while the form is open closes the form in the
/// client and writes nothing.
#[tokio::test]
async fn legacy_tool_cancel_cancels_the_form() {
    let (_dir, root) = project();
    let mut client = Client::start();
    let request = legacy_form(&mut client, &root).await;
    client
        .send(json!({
            "jsonrpc": "2.0",
            "method": "notifications/cancelled",
            "params": { "requestId": 2, "reason": "test" }
        }))
        .await;
    let cancel = client
        .read_until(|message| message["method"] == "notifications/cancelled")
        .await;
    assert_eq!(cancel["params"]["requestId"], request["id"], "{cancel}");
    client
        .send(json!({ "jsonrpc": "2.0", "id": request["id"], "result": accept() }))
        .await;
    assert!(library(&root).contains(UNUSED_EXPORT));
}

/// An accept for a plan that changed while the form was open does not
/// apply the new plan.
#[tokio::test]
async fn legacy_accept_for_a_changed_plan_writes_nothing() {
    let (_dir, root) = project();
    let mut client = Client::start();
    let request = legacy_form(&mut client, &root).await;
    let changed = format!("{LIBRARY}\nexport const unusedOther = (): number => 3;\n");
    std::fs::write(root.join("src/lib.ts"), &changed).expect("change the library");
    client
        .send(json!({ "jsonrpc": "2.0", "id": request["id"], "result": accept() }))
        .await;
    let response = client.next(2).await;
    assert_eq!(
        body(&response)["confirmation"],
        "plan_changed",
        "{response}"
    );
    assert_eq!(library(&root), changed);
}

/// A plan with a skip that the dry run reports with exit 2 still reaches the
/// form, and the form names the skip.
#[tokio::test]
async fn legacy_form_shows_a_plan_with_a_skipped_file() {
    let (_dir, root) = project();
    std::fs::write(
        root.join("src/mixed.ts"),
        "export const usedMixed = 1;\r\nexport const unusedMixed = 2;\n",
    )
    .expect("write a file with mixed line endings");
    std::fs::write(
        root.join("src/index.ts"),
        "import { used } from \"./lib\";\nimport { usedMixed } from \"./mixed\";\n\nexport const run = (): number => used() + usedMixed;\n",
    )
    .expect("write entry");
    let mut client = Client::start();
    let request = legacy_form(&mut client, &root).await;
    let message = request["params"]["message"].as_str().unwrap_or_default();
    assert!(message.contains("1 fix in 1 file"), "{message}");
    assert!(message.contains("skips 1 more fix"), "{message}");
    client
        .send(json!({ "jsonrpc": "2.0", "id": request["id"], "result": { "action": "decline" } }))
        .await;
    let response = client.next(2).await;
    assert_eq!(body(&response)["confirmation"], "declined", "{response}");
}

/// Call `fix_apply` once and require a result with no form request first.
async fn apply_without_a_form(client: &mut Client, request: Value) -> Value {
    client.send(request).await;
    let response = client.next(2).await;
    assert!(
        response.get("method").is_none(),
        "unexpected request: {response}"
    );
    assert_ne!(
        response["result"]["resultType"], "input_required",
        "{response}"
    );
    response
}

#[tokio::test]
async fn a_client_without_elicitation_applies_without_a_question() {
    let (_dir, root) = project();
    let mut client = Client::start();
    client.initialize(json!({})).await;
    let response = apply_without_a_form(&mut client, call(2, &root)).await;
    assert_eq!(body(&response)["total_fixed"], 1, "{response}");
    assert!(!library(&root).contains(UNUSED_EXPORT));
}

#[tokio::test]
async fn a_client_with_only_url_elicitation_applies_without_a_question() {
    let (_dir, root) = project();
    let mut client = Client::start();
    client
        .initialize(json!({ "elicitation": { "url": {} } }))
        .await;
    let response = apply_without_a_form(&mut client, call(2, &root)).await;
    assert_eq!(body(&response)["total_fixed"], 1, "{response}");
}

#[tokio::test]
async fn an_empty_plan_applies_without_a_question() {
    let (_dir, root) = project_with("export const used = (): number => 1;\n");
    let mut client = Client::start();
    client
        .initialize(json!({ "elicitation": { "form": {} } }))
        .await;
    let response = apply_without_a_form(&mut client, call(2, &root)).await;
    assert_eq!(body(&response)["total_fixed"], 0, "{response}");
}

/// A `tools/call` on protocol 2026-07-28: no initialize, inline metadata.
fn inline_call(id: u64, root: &Path, capabilities: &Value, retry: Option<(Value, &str)>) -> Value {
    let mut params = json!({
        "name": "fix_apply",
        "arguments": arguments(root),
        "_meta": {
            "io.modelcontextprotocol/protocolVersion": "2026-07-28",
            "io.modelcontextprotocol/clientCapabilities": capabilities,
            "io.modelcontextprotocol/clientInfo": { "name": "fix-confirmation-test", "version": "0" }
        }
    });
    if let Some((answer, state)) = retry {
        params["inputResponses"] = json!({ CONFIRM_INPUT_KEY: answer });
        params["requestState"] = json!(state);
    }
    json!({ "jsonrpc": "2.0", "id": id, "method": "tools/call", "params": params })
}

fn form_capabilities() -> Value {
    json!({ "elicitation": { "form": {} } })
}

/// The first round returns the form. Returns the request state.
async fn first_round(client: &mut Client, root: &Path) -> String {
    client
        .send(inline_call(2, root, &form_capabilities(), None))
        .await;
    let response = client.next(2).await;
    let result = &response["result"];
    assert_eq!(result["resultType"], "input_required", "{response}");
    let form = &result["inputRequests"][CONFIRM_INPUT_KEY];
    assert_eq!(form["method"], "elicitation/create", "{response}");
    assert_eq!(form["params"]["mode"], "form", "{response}");
    assert!(
        library(root).contains(UNUSED_EXPORT),
        "fix_apply wrote before the answer"
    );
    result["requestState"]
        .as_str()
        .unwrap_or_else(|| panic!("a request state: {response}"))
        .to_string()
}

async fn retry(client: &mut Client, root: &Path, answer: Value, state: &str) -> Value {
    client
        .send(inline_call(
            3,
            root,
            &form_capabilities(),
            Some((answer, state)),
        ))
        .await;
    client.next(3).await
}

#[tokio::test]
async fn input_required_accept_applies_the_fix() {
    let (_dir, root) = project();
    let mut client = Client::start();
    let state = first_round(&mut client, &root).await;
    let response = retry(&mut client, &root, accept(), &state).await;
    assert_eq!(body(&response)["total_fixed"], 1, "{response}");
    assert!(!library(&root).contains(UNUSED_EXPORT));
}

#[tokio::test]
async fn input_required_decline_writes_nothing() {
    let (_dir, root) = project();
    let mut client = Client::start();
    let state = first_round(&mut client, &root).await;
    let response = retry(&mut client, &root, json!({ "action": "decline" }), &state).await;
    assert_eq!(body(&response)["confirmation"], "declined", "{response}");
    assert!(library(&root).contains(UNUSED_EXPORT));
}

#[tokio::test]
async fn input_required_cancel_writes_nothing() {
    let (_dir, root) = project();
    let mut client = Client::start();
    let state = first_round(&mut client, &root).await;
    let response = retry(&mut client, &root, json!({ "action": "cancel" }), &state).await;
    assert_eq!(body(&response)["confirmation"], "cancelled", "{response}");
    assert!(library(&root).contains(UNUSED_EXPORT));
}

/// An answer to an older plan does not apply a newer one.
#[tokio::test]
async fn input_required_answer_for_a_changed_plan_asks_again() {
    let (_dir, root) = project();
    let mut client = Client::start();
    let state = first_round(&mut client, &root).await;
    let changed = format!("{LIBRARY}\nexport const unusedOther = (): number => 3;\n");
    std::fs::write(root.join("src/lib.ts"), &changed).expect("change the library");
    let response = retry(&mut client, &root, accept(), &state).await;
    assert_eq!(
        response["result"]["resultType"], "input_required",
        "{response}"
    );
    assert_ne!(
        response["result"]["requestState"],
        json!(state),
        "{response}"
    );
    assert_eq!(library(&root), changed);
}

#[tokio::test]
async fn input_required_route_without_elicitation_applies_without_a_question() {
    let (_dir, root) = project();
    let mut client = Client::start();
    let response = apply_without_a_form(&mut client, inline_call(2, &root, &json!({}), None)).await;
    assert_eq!(body(&response)["total_fixed"], 1, "{response}");
    assert!(!library(&root).contains(UNUSED_EXPORT));
}
