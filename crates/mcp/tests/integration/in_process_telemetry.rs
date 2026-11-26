#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests use unwrap and expect to keep fixture setup concise"
)]

//! Each tool call gives one telemetry event: from the CLI run when the call
//! runs the CLI, or from the server when the call runs in its process. The
//! test drives the built `fallow-mcp` over stdio with `FALLOW_TELEMETRY=inspect`,
//! which prints each event on stderr instead of sending it.

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

const RESPONSE_TIMEOUT: Duration = Duration::from_mins(3);
/// The record process ends after the answer; this bounds the wait for it.
const EVENT_WAIT: Duration = Duration::from_secs(20);
/// How long a test waits after the last expected event for an extra one.
const SETTLE: Duration = Duration::from_secs(2);

#[test]
fn an_in_process_call_and_a_cli_call_each_give_one_event() {
    let project = tempfile::tempdir().expect("project");
    write_project(project.path());
    let home = tempfile::tempdir().expect("home");
    let mut server = McpServer::start(home.path(), Some("on"));
    let root = serde_json::json!({ "root": project.path() });

    server.call("project_info", &root);
    server.call("list_suppressions", &root);
    wait_for_spool(home.path(), &["project_info", "list_suppressions"]);

    thread::sleep(SETTLE);
    let spool = spool_text(home.path());
    assert_eq!(count(&spool, "\"mcp_tool\":\"project_info\""), 1, "{spool}");
    assert_eq!(
        count(&spool, "\"mcp_tool\":\"list_suppressions\""),
        1,
        "{spool}"
    );
    assert_eq!(
        count(&spool, "\"integration_surface\":\"mcp\""),
        2,
        "{spool}"
    );
}

#[test]
fn a_task_call_gives_one_event() {
    let project = tempfile::tempdir().expect("project");
    write_project(project.path());
    let home = tempfile::tempdir().expect("home");
    let mut server = McpServer::start(home.path(), Some("on"));
    server.call_as_task("audit", &serde_json::json!({ "root": project.path() }));
    wait_for_spool(home.path(), &["audit"]);
    thread::sleep(SETTLE);
    let spool = spool_text(home.path());
    assert_eq!(count(&spool, "\"mcp_tool\":\"audit\""), 1, "{spool}");
}

#[test]
fn in_inspect_mode_an_in_process_call_prints_its_event() {
    let project = tempfile::tempdir().expect("project");
    write_project(project.path());
    let home = tempfile::tempdir().expect("home");
    let mut server = McpServer::start(home.path(), Some("inspect"));
    server.call(
        "project_info",
        &serde_json::json!({ "root": project.path() }),
    );
    server.wait_for_events("\"mcp_tool\": \"project_info\"", 1);
    thread::sleep(SETTLE);
    let stderr = server.stderr();
    assert_eq!(
        count(&stderr, "\"mcp_tool\": \"project_info\""),
        1,
        "{stderr}"
    );
    assert!(
        stderr.contains("\"workflow\": \"project_inventory\""),
        "{stderr}"
    );
}

/// The spool lines of the temporary home. The CLI run of a fallback call
/// and the record process of an in-process call both append to it.
fn spool_text(home: &Path) -> String {
    find_spool(home)
        .and_then(|path| std::fs::read_to_string(path).ok())
        .unwrap_or_default()
}

/// The config folder differs per platform, so the test searches the home.
fn find_spool(dir: &Path) -> Option<std::path::PathBuf> {
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = find_spool(&path) {
                return Some(found);
            }
        } else if path.ends_with("telemetry-spool.jsonl") {
            return Some(path);
        }
    }
    None
}

fn wait_for_spool(home: &Path, tools: &[&str]) {
    let deadline = Instant::now() + EVENT_WAIT;
    loop {
        let spool = spool_text(home);
        let missing: Vec<&&str> = tools
            .iter()
            .filter(|tool| !spool.contains(&format!("\"mcp_tool\":\"{tool}\"")))
            .collect();
        if missing.is_empty() {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "no spooled event for {missing:?}; spool: {spool}"
        );
        thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn with_telemetry_off_an_in_process_call_gives_no_event() {
    let project = tempfile::tempdir().expect("project");
    write_project(project.path());
    let home = tempfile::tempdir().expect("home");
    let mut server = McpServer::start(home.path(), None);
    server.call(
        "project_info",
        &serde_json::json!({ "root": project.path() }),
    );
    thread::sleep(SETTLE);
    let stderr = server.stderr();
    assert_eq!(count(&stderr, "\"mcp_tool\""), 0, "{stderr}");
}

fn count(haystack: &str, needle: &str) -> usize {
    haystack.matches(needle).count()
}

fn write_project(root: &Path) {
    std::fs::create_dir_all(root.join("src")).expect("src");
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"telemetry-probe","main":"src/index.ts"}"#,
    )
    .expect("package.json");
    std::fs::write(root.join("src/index.ts"), "export const value = 1;\n").expect("index");
}

/// The built `fallow-mcp` with its stderr collected.
struct McpServer {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<String>,
    stderr: Arc<Mutex<String>>,
    next_id: u64,
}

impl McpServer {
    fn start(home: &Path, telemetry: Option<&str>) -> Self {
        let mut command = Command::new(env!("CARGO_BIN_EXE_fallow-mcp"));
        command.env("FALLOW_BIN", crate::fallow_binary());
        for name in [
            "CI",
            "GITHUB_ACTIONS",
            "GITLAB_CI",
            "DO_NOT_TRACK",
            "FALLOW_TELEMETRY",
            "FALLOW_TELEMETRY_DEBUG",
            "FALLOW_TELEMETRY_DISABLED",
            "FALLOW_INTEGRATION_SURFACE",
            "FALLOW_MCP_TOOL",
        ] {
            command.env_remove(name);
        }
        if let Some(mode) = telemetry {
            command.env("FALLOW_TELEMETRY", mode);
        }
        command
            .env("HOME", home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("APPDATA", home.join("AppData"))
            .env("RUST_LOG", "off")
            // An unreachable host keeps every drain from a real upload, so
            // the spooled events stay on disk for the assertions.
            .env("FALLOW_API_URL", "http://127.0.0.1:9");
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn fallow-mcp");

        let stdin = child.stdin.take().expect("child stdin");
        let stdout = child.stdout.take().expect("child stdout");
        let stderr_pipe = child.stderr.take().expect("child stderr");
        let (sender, lines) = mpsc::channel();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if sender.send(line).is_err() {
                    return;
                }
            }
        });
        let stderr = Arc::new(Mutex::new(String::new()));
        let sink = Arc::clone(&stderr);
        thread::spawn(move || {
            for line in BufReader::new(stderr_pipe).lines().map_while(Result::ok) {
                let mut text = sink.lock().expect("stderr lock");
                text.push_str(&line);
                text.push('\n');
            }
        });

        let mut server = Self {
            child,
            stdin,
            lines,
            stderr,
            next_id: 1,
        };
        server.initialize();
        server
    }

    fn initialize(&mut self) {
        let id = self.take_id();
        self.send(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": { "name": "telemetry-probe", "version": "0" }
            }
        }));
        let response = self.response(id);
        assert!(response["result"]["serverInfo"].is_object(), "{response}");
        self.send(&serde_json::json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        }));
    }

    fn call(&mut self, tool: &str, arguments: &serde_json::Value) {
        let id = self.take_id();
        self.send(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/call",
            "params": { "name": tool, "arguments": arguments }
        }));
        let response = self.response(id);
        assert_ne!(
            response["result"]["isError"], true,
            "`{tool}` must succeed: {response}"
        );
    }

    /// Call `tool` as an MCP task and poll `tasks/get` until it settles.
    /// The call may fail: the test counts its event, not its answer.
    fn call_as_task(&mut self, tool: &str, arguments: &serde_json::Value) {
        let meta = serde_json::json!({
            "io.modelcontextprotocol/protocolVersion": "2026-07-28",
            "io.modelcontextprotocol/clientInfo": { "name": "telemetry-probe", "version": "0" },
            "io.modelcontextprotocol/clientCapabilities": {
                "extensions": { "io.modelcontextprotocol/tasks": {} }
            },
        });
        let id = self.take_id();
        self.send(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/call",
            "params": { "name": tool, "arguments": arguments, "_meta": meta }
        }));
        let created = self.response(id);
        assert_eq!(created["result"]["resultType"], "task", "{created}");
        let task_id = created["result"]["taskId"]
            .as_str()
            .expect("a task id")
            .to_owned();
        let deadline = Instant::now() + RESPONSE_TIMEOUT;
        loop {
            let id = self.take_id();
            self.send(&serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": "tasks/get",
                "params": { "taskId": task_id, "_meta": meta }
            }));
            let status = self.response(id)["result"]["status"]
                .as_str()
                .unwrap_or_default()
                .to_owned();
            if matches!(status.as_str(), "completed" | "failed" | "cancelled") {
                return;
            }
            assert!(Instant::now() < deadline, "the task never settled");
            thread::sleep(Duration::from_millis(100));
        }
    }

    fn stderr(&self) -> String {
        self.stderr.lock().expect("stderr lock").clone()
    }

    fn wait_for_events(&self, needle: &str, expected: usize) {
        let deadline = Instant::now() + EVENT_WAIT;
        while count(&self.stderr(), needle) < expected {
            assert!(
                Instant::now() < deadline,
                "no event with {needle}; stderr: {}",
                self.stderr()
            );
            thread::sleep(Duration::from_millis(50));
        }
    }

    fn take_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    fn send(&mut self, message: &serde_json::Value) {
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

impl Drop for McpServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
