#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests use unwrap and expect to keep fixture setup concise"
)]

//! A `code_execute` snippet must end at its `timeout_ms`, also when it spends
//! that time inside one native builtin of the JS engine. The engine checks its
//! interrupt handler only between bytecode steps, so a long builtin call does
//! not see the deadline. The server must still answer on time, keep serving
//! other calls, and exit when the client closes stdin.

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

const SNIPPET_TIMEOUT_MS: u64 = 1_000;

/// Time on top of `timeout_ms` that an answer may take. It covers the start of
/// the snippet process and a slow CI runner, and it is far below the run time
/// of the long builtin calls below.
const ANSWER_MARGIN: Duration = Duration::from_secs(6);

const INITIALIZE_TIMEOUT: Duration = Duration::from_secs(60);
const EXIT_TIMEOUT: Duration = Duration::from_secs(10);

/// Snippets that each spend their time inside one builtin call.
const LONG_BUILTIN_SNIPPETS: &[&str] = &[
    "return Array.prototype.reverse.call({ length: 2 ** 53 - 1 }).length;",
    "return Array.prototype.copyWithin.call({ length: 2 ** 53 - 1 }, 0, 1).length;",
    "return new Array(2 ** 32 - 1).reverse().length;",
    "return Array.prototype.join.call({ length: 2 ** 31 }, '').length;",
    "return String.raw({ raw: { length: 2 ** 53 - 1 } }).length;",
    "return 'a'.repeat(400000).indexOf('a'.repeat(200000) + 'b');",
    "return 'a'.repeat(400000).split('a'.repeat(200000) + 'b').length;",
];

/// Snippets that ask a builtin for a large allocation.
const LARGE_ALLOCATION_SNIPPETS: &[&str] = &[
    "return Array(2 ** 26).fill(0).length;",
    "return 'x'.repeat(2 ** 29).length;",
    "return JSON.stringify(Array(2 ** 20).fill('x'.repeat(64))).length;",
    "return Array.from({ length: 2 ** 22 }, (_, i) => -i).sort().length;",
];

#[test]
fn a_snippet_in_a_long_builtin_call_ends_at_its_timeout() {
    let mut server = McpServer::start();
    for snippet in LONG_BUILTIN_SNIPPETS {
        let (envelope, elapsed) = server.code_execute(snippet, SNIPPET_TIMEOUT_MS);
        assert_eq!(envelope["ok"], false, "{snippet}: {envelope}");
        assert_eq!(
            envelope["error"], "code mode execution timed out",
            "{snippet}: {envelope}"
        );
        assert!(
            elapsed < Duration::from_millis(SNIPPET_TIMEOUT_MS) + ANSWER_MARGIN,
            "{snippet}: answered after {elapsed:?}"
        );
    }

    let (envelope, _) = server.code_execute("return 1 + 1;", SNIPPET_TIMEOUT_MS);
    assert_eq!(envelope["ok"], true, "{envelope}");
    assert_eq!(envelope["result"], 2, "{envelope}");

    server.close_and_wait_for_exit();
}

#[test]
fn a_snippet_with_a_large_allocation_ends_with_an_error() {
    let mut server = McpServer::start();
    for snippet in LARGE_ALLOCATION_SNIPPETS {
        let (envelope, elapsed) = server.code_execute(snippet, SNIPPET_TIMEOUT_MS);
        assert_eq!(envelope["ok"], false, "{snippet}: {envelope}");
        assert!(
            envelope["error"]
                .as_str()
                .is_some_and(|error| !error.is_empty()),
            "{snippet}: {envelope}"
        );
        assert!(
            elapsed < Duration::from_millis(SNIPPET_TIMEOUT_MS) + ANSWER_MARGIN,
            "{snippet}: answered after {elapsed:?}"
        );
    }
    server.close_and_wait_for_exit();
}

#[test]
fn host_calls_still_reach_the_server() {
    let project = tempfile::tempdir().expect("tempdir");
    write_project(project.path());
    let root = project.path().display().to_string();
    let mut server = McpServer::start();

    let snippet = "const info = fallow.projectInfo({ files: true });\n\
         const batch = fallow.all([{ tool: 'project_info', params: { files: true } }]);\n\
         return { files: info.files.length, batch: batch[0].ok };";
    let (envelope, _) = server.code_execute_in(snippet, 30_000, Some(&root));
    assert_eq!(envelope["ok"], true, "{envelope}");
    assert_eq!(envelope["result"]["files"], 1, "{envelope}");
    assert_eq!(envelope["result"]["batch"], true, "{envelope}");
    assert_eq!(envelope["calls"][0]["tool"], "project_info", "{envelope}");
    assert_eq!(envelope["calls"][1]["cache_hit"], true, "{envelope}");

    server.close_and_wait_for_exit();
}

/// A server that never runs a snippet starts no snippet process.
#[cfg(unix)]
#[test]
fn the_first_snippet_starts_the_snippet_process() {
    let mut server = McpServer::start();
    thread::sleep(Duration::from_secs(1));
    assert!(
        server.snippet_processes().is_empty(),
        "the server started a snippet process before any snippet"
    );
    let (envelope, _) = server.code_execute("return 1 + 1;", SNIPPET_TIMEOUT_MS);
    assert_eq!(envelope["ok"], true, "{envelope}");
    assert_eq!(
        server.snippet_processes().len(),
        1,
        "one idle snippet process"
    );
    server.close_and_wait_for_exit();
}

fn write_project(root: &Path) {
    std::fs::create_dir_all(root.join("src")).expect("create src");
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"code-mode-stop","type":"module","main":"src/index.ts"}"#,
    )
    .expect("write package.json");
    std::fs::write(root.join("src/index.ts"), "export const used = 1;\n").expect("write index");
}

/// The built `fallow-mcp` binary as a child process, spoken to over the stdio
/// transport's newline-delimited JSON-RPC.
struct McpServer {
    /// The server folder. It holds no `Cargo.toml`, so Code Mode runs.
    _cwd: tempfile::TempDir,
    child: Child,
    stdin: Option<ChildStdin>,
    lines: Receiver<String>,
    next_id: u64,
}

impl McpServer {
    fn start() -> Self {
        let cwd = tempfile::tempdir().expect("server folder");
        let mut child = Command::new(env!("CARGO_BIN_EXE_fallow-mcp"))
            .current_dir(cwd.path())
            .env("FALLOW_BIN", crate::fallow_binary())
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
            _cwd: cwd,
            child,
            stdin: Some(stdin),
            lines,
            next_id: 1,
        };
        let id = server.take_id();
        server.send(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": { "name": "code-mode-stop", "version": "0" }
            }
        }));
        let response = server.response(id, INITIALIZE_TIMEOUT);
        assert!(
            response["result"]["serverInfo"].is_object(),
            "initialize must return server info: {response}"
        );
        server.send(&serde_json::json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        }));
        server
    }

    fn code_execute(&mut self, code: &str, timeout_ms: u64) -> (serde_json::Value, Duration) {
        self.code_execute_in(code, timeout_ms, None)
    }

    /// The parsed `code_execute` envelope and the time the answer took.
    fn code_execute_in(
        &mut self,
        code: &str,
        timeout_ms: u64,
        root: Option<&str>,
    ) -> (serde_json::Value, Duration) {
        let id = self.take_id();
        let mut arguments = serde_json::json!({ "code": code, "timeout_ms": timeout_ms });
        if let Some(root) = root {
            arguments["root"] = serde_json::Value::from(root);
        }
        let started = Instant::now();
        self.send(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/call",
            "params": { "name": "code_execute", "arguments": arguments }
        }));
        let response = self.response(id, Duration::from_millis(timeout_ms) + ANSWER_MARGIN);
        let elapsed = started.elapsed();
        let text = response["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or_else(|| panic!("text content: {response}"));
        let envelope = serde_json::from_str(text).unwrap_or_else(|err| panic!("{err}: {text}"));
        (envelope, elapsed)
    }

    /// The PIDs of the snippet processes of the server.
    #[cfg(unix)]
    fn snippet_processes(&self) -> Vec<u32> {
        let output = Command::new("pgrep")
            .args(["-P", &self.child.id().to_string(), "-f", "code-mode-worker"])
            .output()
            .expect("run pgrep");
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|line| line.trim().parse().ok())
            .collect()
    }

    /// Close stdin, as a client that disconnects does. The server must exit.
    fn close_and_wait_for_exit(&mut self) {
        drop(self.stdin.take());
        let started = Instant::now();
        while started.elapsed() < EXIT_TIMEOUT {
            if self.child.try_wait().expect("poll fallow-mcp").is_some() {
                return;
            }
            thread::sleep(Duration::from_millis(50));
        }
        panic!("fallow-mcp did not exit {EXIT_TIMEOUT:?} after stdin closed");
    }

    fn take_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    fn send(&mut self, message: &serde_json::Value) {
        let stdin = self.stdin.as_mut().expect("stdin is open");
        writeln!(stdin, "{message}").expect("write message");
        stdin.flush().expect("flush message");
    }

    /// The response carrying `id`, skipping notifications.
    fn response(&self, id: u64, timeout: Duration) -> serde_json::Value {
        let deadline = Instant::now() + timeout;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let line = self
                .lines
                .recv_timeout(left)
                .unwrap_or_else(|err| panic!("no response for id {id} within {timeout:?}: {err}"));
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
