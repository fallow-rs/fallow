#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests use unwrap and expect to keep fixture setup concise"
)]

//! `get_cloud_runtime_context` end to end, against a stubbed cloud.
//!
//! The tool is the only one in the runtime-coverage family whose evidence
//! comes over the network, so the parts worth pinning are the ones a unit test
//! on the argument builder cannot reach: that a `tools/call` with a key
//! actually reaches the runtime-context endpoint and comes back as the same
//! `runtime_coverage` block the local tools return, and that a server started
//! without a key refuses the call with a typed body instead of spawning
//! anything. Both run the built `fallow-mcp` binary over stdio with a
//! throwaway HTTP server standing in for fallow cloud.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

const RESPONSE_TIMEOUT: Duration = Duration::from_mins(3);

/// A runtime-context payload for the `coverage-gaps` fixture: one function the
/// cloud saw called often enough to be a hot path, and one it tracked but
/// never saw called.
const RUNTIME_CONTEXT_BODY: &str = r#"{
  "data": {
    "repo": "acme/web",
    "window": { "period_days": 30 },
    "summary": {
      "trace_count": 20000,
      "deployments_seen": 2,
      "functions_tracked": 2,
      "functions_hit": 1,
      "functions_unhit": 1,
      "functions_untracked": 0,
      "coverage_percent": 50,
      "last_received_at": "2026-04-30T10:00:00.000Z"
    },
    "functions": [
      {
        "file_path": "src/covered.ts",
        "function_name": "covered",
        "line_number": 1,
        "start_line": 1,
        "end_line": 3,
        "hit_count": 5000,
        "tracking_state": "called",
        "deployments_observed": 2
      },
      {
        "file_path": "src/covered.ts",
        "function_name": "indirectlyCovered",
        "line_number": 5,
        "start_line": 5,
        "end_line": 7,
        "hit_count": 0,
        "tracking_state": "never_called",
        "never_called_source": "runtime_observed",
        "deployments_observed": 2
      }
    ],
    "warnings": []
  }
}"#;

#[test]
fn tools_list_advertises_the_cloud_tool_and_its_inputs() {
    let mut server = McpServer::start(None);
    let tools = server.list_tools();
    let tool = tools
        .as_array()
        .expect("tools array")
        .iter()
        .find(|tool| tool["name"] == "get_cloud_runtime_context")
        .expect("get_cloud_runtime_context must be registered");

    let properties = tool["inputSchema"]["properties"]
        .as_object()
        .expect("input schema properties");
    for parameter in [
        "repo",
        "project_id",
        "period_days",
        "environment",
        "commit_sha",
        "production",
        "top",
        "min_invocations_hot",
    ] {
        assert!(
            properties.contains_key(parameter),
            "{parameter} must be on the published input schema: {properties:?}"
        );
    }
    assert!(
        !properties.contains_key("api_key"),
        "the key is server environment, never a call argument: {properties:?}"
    );
}

#[test]
fn a_call_with_a_key_returns_the_runtime_coverage_block() {
    let (endpoint, request, handle) = serve_once(RUNTIME_CONTEXT_BODY);
    let mut server = McpServer::start(Some(&endpoint));
    let result = server.call_cloud_runtime_context();
    handle.join().expect("stub server joins");

    assert_ne!(
        result["isError"], true,
        "call with a key must succeed: {result}"
    );
    let payload = tool_payload(&result);
    let runtime = &payload["runtime_coverage"];

    assert_eq!(runtime["summary"]["data_source"], "cloud");
    let findings = runtime["findings"].as_array().expect("findings array");
    assert!(
        findings
            .iter()
            .any(|finding| finding["function"] == "indirectlyCovered"),
        "the never-called function must surface as a finding: {runtime}"
    );
    let hot_paths = runtime["hot_paths"].as_array().expect("hot_paths array");
    assert!(
        hot_paths.iter().any(|path| path["function"] == "covered"),
        "the frequently called function must surface as a hot path: {runtime}"
    );

    let request = request.lock().expect("request lock").clone();
    assert!(
        request.starts_with("GET /v1/coverage/acme%2Fweb/runtime-context?"),
        "the tool must reach the runtime-context endpoint: {request}"
    );
    assert!(
        request.to_lowercase().contains("authorization: bearer "),
        "the request must carry the server's key: {request}"
    );
}

#[test]
fn a_call_without_a_key_is_refused_with_a_typed_body() {
    let mut server = McpServer::start(None);
    let result = server.call_cloud_runtime_context();

    assert_eq!(
        result["isError"], true,
        "a missing key must fail the call: {result}"
    );
    let payload = tool_payload(&result);
    assert_eq!(payload["error"], true);
    assert_eq!(payload["exit_code"], 2);
    assert_eq!(payload["code"], "cloud_api_key_missing");
    assert_eq!(payload["context"], "get_cloud_runtime_context.api_key");
    assert_eq!(
        payload["message"],
        fallow_types::cloud::CLOUD_API_KEY_MISSING_MESSAGE,
        "the refusal must read exactly as the CLI's does"
    );
}

/// The JSON body of a `tools/call` result, parsed out of its text content.
fn tool_payload(result: &serde_json::Value) -> serde_json::Value {
    let text = result["content"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("text content: {result}"));
    serde_json::from_str(text).unwrap_or_else(|err| panic!("tool body is JSON: {err}\n{text}"))
}

/// A single-request HTTP server standing in for fallow cloud. Returns its base
/// URL, the captured request, and the thread to join once the call is done.
fn serve_once(body: &'static str) -> (String, Arc<Mutex<String>>, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind stub cloud");
    let addr = listener.local_addr().expect("stub cloud address");
    let request = Arc::new(Mutex::new(String::new()));
    let captured = Arc::clone(&request);
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept request");
        let mut buffer = [0_u8; 4096];
        let read = stream.read(&mut buffer).expect("read request");
        *captured.lock().expect("capture lock") =
            String::from_utf8_lossy(&buffer[..read]).into_owned();
        let response = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{body}",
            body.len()
        );
        stream
            .write_all(response.as_bytes())
            .expect("write response");
    });
    (format!("http://{addr}"), request, handle)
}

/// The workspace root, two levels above this crate.
fn workspace_root() -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop();
    path.pop();
    path
}

/// The Cargo profile directory of the running test binary. Test binaries
/// live in `<target>/<profile>/deps`, next to the `fallow` binary one level
/// up, so this follows `CARGO_TARGET_DIR` and `build.target-dir`.
fn cargo_profile_dir() -> PathBuf {
    let exe = std::env::current_exe().expect("test binary path");
    let dir = exe.parent().expect("test binary directory");
    if dir.ends_with("deps") {
        dir.parent().expect("profile directory").to_path_buf()
    } else {
        dir.to_path_buf()
    }
}

/// The `fallow` binary the MCP server shells out to. Built by
/// `cargo test --workspace`; build it with `cargo build -p fallow-cli` when
/// running this crate's tests alone.
fn fallow_binary() -> PathBuf {
    let mut path = cargo_profile_dir().join("fallow");
    if cfg!(windows) {
        path.set_extension("exe");
    }
    assert!(
        path.is_file(),
        "fallow binary not found at {}. Build it first: cargo build -p fallow-cli",
        path.display()
    );
    path
}

/// The built `fallow-mcp` binary as a child process, spoken to over the stdio
/// transport's newline-delimited JSON-RPC.
struct McpServer {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<String>,
    next_id: u64,
}

impl McpServer {
    /// Start a server that reaches `api_endpoint` as fallow cloud and holds an
    /// API key, or, with `None`, one with neither, which is the shape the
    /// refusal path needs.
    fn start(api_endpoint: Option<&str>) -> Self {
        let mut command = Command::new(env!("CARGO_BIN_EXE_fallow-mcp"));
        command
            .env("FALLOW_BIN", fallow_binary())
            .env("FALLOW_TELEMETRY_DISABLED", "1")
            .env("NO_COLOR", "1")
            .env("RUST_LOG", "")
            .env_remove("FALLOW_REPO")
            .env_remove("FALLOW_RUNTIME_COVERAGE_SOURCE");
        match api_endpoint {
            Some(endpoint) => {
                command
                    .env("FALLOW_API_URL", endpoint)
                    .env("FALLOW_API_KEY", "fallow_live_test");
            }
            None => {
                command
                    .env_remove("FALLOW_API_URL")
                    .env_remove("FALLOW_API_KEY");
            }
        }

        let mut child = command
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
            next_id: 1,
        };
        server.initialize();
        server
    }

    fn initialize(&mut self) {
        let id = self.request(&serde_json::json!({
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": { "name": "cloud-runtime-context", "version": "0" }
            }
        }));
        let response = self.response(id);
        assert!(
            response["result"]["serverInfo"].is_object(),
            "initialize must return server info: {response}"
        );
        self.send(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#);
    }

    fn list_tools(&mut self) -> serde_json::Value {
        let id = self.request(&serde_json::json!({ "method": "tools/list", "params": {} }));
        self.response(id)["result"]["tools"].clone()
    }

    /// Call the tool against the `coverage-gaps` fixture, whose `covered.ts`
    /// holds both functions the stubbed cloud reports.
    fn call_cloud_runtime_context(&mut self) -> serde_json::Value {
        let root = workspace_root().join("tests/fixtures/coverage-gaps");
        let id = self.request(&serde_json::json!({
            "method": "tools/call",
            "params": {
                "name": "get_cloud_runtime_context",
                "arguments": {
                    "repo": "acme/web",
                    "root": root.display().to_string(),
                    "no_cache": true
                }
            }
        }));
        self.response(id)["result"].clone()
    }

    /// Send one request with the next free id and return that id.
    fn request(&mut self, body: &serde_json::Value) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        let mut message = body.clone();
        message["jsonrpc"] = serde_json::Value::from("2.0");
        message["id"] = serde_json::Value::from(id);
        self.send(&serde_json::to_string(&message).expect("serialize request"));
        id
    }

    fn send(&mut self, message: &str) {
        writeln!(self.stdin, "{message}").expect("write message");
        self.stdin.flush().expect("flush message");
    }

    /// The response carrying `id`, skipping any notification the server emits
    /// while the analysis runs.
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

impl Drop for McpServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
