#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests use unwrap and expect to keep fixture setup concise"
)]

//! Protocol-level coverage of the resource surface: the built `fallow-mcp`
//! binary is driven over stdio JSON-RPC through `initialize`,
//! `resources/list`, `resources/templates/list`, and `resources/read`, so the
//! capability declaration and the wire shape of every response are pinned
//! end to end rather than through the in-process free functions alone.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::Duration;

const RESPONSE_TIMEOUT: Duration = Duration::from_mins(1);

#[test]
fn resources_are_listed_and_read_over_stdio() {
    let mut server = McpServer::start();

    let capabilities = &server.initialize_result["capabilities"];
    assert!(
        capabilities["resources"].is_object(),
        "initialize must advertise resources: {capabilities}"
    );
    assert!(
        capabilities["resources"].get("subscribe").is_none()
            && capabilities["resources"].get("listChanged").is_none(),
        "static catalogue must not advertise subscribe or listChanged: {capabilities}"
    );

    let listed = server.request(2, "resources/list", &serde_json::json!({}));
    let resources = listed["result"]["resources"]
        .as_array()
        .unwrap_or_else(|| panic!("resources array: {listed}"));
    let uris: Vec<&str> = resources.iter().filter_map(|r| r["uri"].as_str()).collect();
    assert_eq!(
        uris,
        [
            "fallow://tools",
            "fallow://issue-types",
            "fallow://explain",
            "fallow://task-matrix",
            "fallow://schema/config",
            "fallow://schema/plugin",
            "fallow://schema/rule-pack",
        ]
    );
    for resource in resources {
        assert_eq!(resource["mimeType"], "application/json", "{resource}");
        assert!(
            resource["size"].as_u64().is_some_and(|s| s > 0),
            "{resource}"
        );
        assert_eq!(
            resource["annotations"]["audience"],
            serde_json::json!(["assistant"])
        );
    }

    let templates = server.request(3, "resources/templates/list", &serde_json::json!({}));
    let template_uris: Vec<&str> = templates["result"]["resourceTemplates"]
        .as_array()
        .unwrap_or_else(|| panic!("resourceTemplates array: {templates}"))
        .iter()
        .filter_map(|t| t["uriTemplate"].as_str())
        .collect();
    assert_eq!(template_uris, ["fallow://explain/{issue_type}"]);

    let task_matrix = server.read(4, "fallow://task-matrix");
    assert_eq!(task_matrix["fallow_version"], env!("CARGO_PKG_VERSION"));
    let rows = task_matrix["rows"].as_array().expect("rows");
    assert!(
        rows.iter()
            .any(|row| row["command"] == "fallow audit --base <ref>"),
        "task matrix must carry the audit row: {task_matrix}"
    );

    let explain = server.read(5, "fallow://explain/unused-export");
    assert_eq!(explain["kind"], "explain");
    assert_eq!(explain["id"], "fallow/unused-export");
    assert_eq!(explain["fallow_version"], env!("CARGO_PKG_VERSION"));

    let error = server.request(
        6,
        "resources/read",
        &serde_json::json!({ "uri": "fallow://explain/not-a-rule" }),
    );
    assert!(
        error["result"].is_null(),
        "unknown issue type must not succeed: {error}"
    );
    assert!(
        error["error"]["data"]["nearest_matches"].is_array(),
        "structured error must list nearest matches: {error}"
    );
    assert_eq!(error["error"]["data"]["index"], "fallow://explain");

    let unknown = server.request(
        7,
        "resources/read",
        &serde_json::json!({ "uri": "fallow://nope" }),
    );
    assert!(
        unknown["error"]["data"]["known_uris"].is_array(),
        "unknown uri error must list the catalogue: {unknown}"
    );
}

struct McpServer {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<String>,
    initialize_result: serde_json::Value,
}

impl Drop for McpServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl McpServer {
    fn start() -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_fallow-mcp"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn fallow-mcp");

        let stdin = child.stdin.take().expect("child stdin");
        let stdout = child.stdout.take().expect("child stdout");
        let (sender, lines) = mpsc::channel();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if sender.send(line).is_err() {
                    return;
                }
            }
        });

        let mut server = Self {
            child,
            stdin,
            lines,
            initialize_result: serde_json::Value::Null,
        };
        server.send(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"resources-test","version":"0"}}}"#,
        );
        let response = server.response(1);
        assert!(
            response["result"]["serverInfo"].is_object(),
            "initialize must return server info: {response}"
        );
        server.initialize_result = response["result"].clone();
        server.send(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#);
        server
    }

    fn request(&mut self, id: u64, method: &str, params: &serde_json::Value) -> serde_json::Value {
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        self.send(&serde_json::to_string(&request).expect("serialize request"));
        self.response(id)
    }

    /// Read a resource and parse its single JSON text content.
    fn read(&mut self, id: u64, uri: &str) -> serde_json::Value {
        let response = self.request(id, "resources/read", &serde_json::json!({ "uri": uri }));
        let contents = response["result"]["contents"]
            .as_array()
            .unwrap_or_else(|| panic!("contents array for {uri}: {response}"));
        assert_eq!(contents.len(), 1, "{uri} returns one content item");
        assert_eq!(contents[0]["uri"], uri);
        assert_eq!(contents[0]["mimeType"], "application/json");
        let text = contents[0]["text"]
            .as_str()
            .unwrap_or_else(|| panic!("text content for {uri}: {response}"));
        serde_json::from_str(text).unwrap_or_else(|err| panic!("{uri} payload is JSON: {err}"))
    }

    fn send(&mut self, message: &str) {
        writeln!(self.stdin, "{message}").expect("write message");
        self.stdin.flush().expect("flush message");
    }

    fn response(&self, id: u64) -> serde_json::Value {
        loop {
            let line = self
                .lines
                .recv_timeout(RESPONSE_TIMEOUT)
                .unwrap_or_else(|err| panic!("no response for id {id}: {err}"));
            let Ok(message) = serde_json::from_str::<serde_json::Value>(&line) else {
                continue;
            };
            if message["id"].as_u64() == Some(id) {
                return message;
            }
        }
    }
}
