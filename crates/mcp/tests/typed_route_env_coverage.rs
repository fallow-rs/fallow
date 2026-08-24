#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests use unwrap and expect to keep fixture setup concise"
)]

//! #2368: the `check_health` typed route reads `FALLOW_COVERAGE` and
//! `FALLOW_COVERAGE_ROOT` from the real process environment.
//!
//! The in-crate typed-route tests inject the environment layer as a value so a
//! shared process environment cannot make them flaky, which leaves the
//! production read itself unpinned. This test drives the built `fallow-mcp`
//! binary over stdio with the variables set for real, in its own process, and
//! compares it against a control server started without them: the same call on
//! the same project must score `branchy` from the Istanbul map in the first
//! case and from the reachability estimate in the second.

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Mutex, MutexGuard};
use std::thread;
use std::time::Duration;

/// Absolute prefix the Istanbul map records its files under, so
/// `FALLOW_COVERAGE_ROOT` has to rebase them onto the project.
const COVERAGE_ROOT: &str = "/ci/workspace";

/// Project-relative location of the map, outside every auto-detected path, so
/// only an explicit input can find it.
const COVERAGE_FILE: &str = "artifacts/coverage-final.json";

const RESPONSE_TIMEOUT: Duration = Duration::from_mins(3);
static MCP_SERVER_TEST_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn check_health_typed_route_reads_coverage_from_the_process_environment() {
    let _guard = lock_mcp_server_test();
    let project = tempfile::tempdir().expect("project dir");
    write_project(project.path());

    let with_env = branchy_coverage_source(project.path(), true);
    assert_eq!(
        with_env.as_deref(),
        Some("istanbul"),
        "FALLOW_COVERAGE and FALLOW_COVERAGE_ROOT must reach the typed route"
    );

    let without_env = branchy_coverage_source(project.path(), false);
    assert_eq!(
        without_env.as_deref(),
        Some("estimated"),
        "the same call without the variables scores from the reachability estimate"
    );
}

#[test]
fn analyze_typed_route_reads_max_file_size_from_the_process_environment() {
    let _guard = lock_mcp_server_test();
    let project = tempfile::tempdir().expect("project dir");
    write_large_file_project(project.path());

    let mut with_env = McpServer::start_with_options(false, Some("1"), false);
    let limited = with_env.analyze(project.path());
    assert!(
        limited["workspace_diagnostics"]
            .as_array()
            .is_some_and(|diagnostics| diagnostics.iter().any(|diagnostic| {
                diagnostic["kind"] == "skipped-large-file" && diagnostic["path"] == "src/huge.ts"
            })),
        "FALLOW_MAX_FILE_SIZE must reach the typed analyze route: {limited}"
    );

    let mut without_env = McpServer::start_with_options(false, None, false);
    let unlimited = without_env.analyze(project.path());
    assert!(
        unlimited["unused_files"]
            .as_array()
            .is_some_and(|files| files.iter().any(|file| file["path"] == "src/huge.ts")),
        "the same file stays analyzable under the default limit: {unlimited}"
    );
}

#[test]
fn trace_symbol_typed_route_does_not_credit_an_unreachable_consumer() {
    let _guard = lock_mcp_server_test();
    let project = tempfile::tempdir().expect("project dir");
    write_type_aware_trace_project(project.path());

    let mut server = McpServer::start_type_aware();
    let trace = server.trace_symbol(project.path(), "src/lonely.ts", "helper");

    assert_eq!(trace["is_used"], false);
    assert_eq!(
        trace["semantic"]["assertion"],
        "references-only-in-unreachable-files"
    );
    assert_eq!(trace["semantic"]["status"], "partial");
    assert_eq!(trace["semantic"]["references"][0]["path"], "src/orphan.ts");
}

/// Each fixture launches an analysis server with production thread defaults.
/// Serial execution prevents Windows test runners from multiplying those pools
/// while preserving the route behavior under test.
fn lock_mcp_server_test() -> MutexGuard<'static, ()> {
    MCP_SERVER_TEST_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// `coverage_source` of the `branchy` finding from a `check_health` call on a
/// server started with or without the coverage variables.
fn branchy_coverage_source(root: &Path, with_env: bool) -> Option<String> {
    let mut server = McpServer::start(with_env);
    let health = server.check_health(root);
    let finding = health["findings"]
        .as_array()
        .expect("findings array")
        .iter()
        .find(|finding| finding["name"].as_str() == Some("branchy"))
        .unwrap_or_else(|| panic!("branchy stays above max_crap in both modes: {health}"));
    finding["coverage_source"].as_str().map(str::to_string)
}

/// A project whose `branchy` function is complex enough to exceed `max_crap`
/// under either scoring model, plus an Istanbul map that records it as never
/// called under [`COVERAGE_ROOT`].
fn write_project(root: &Path) {
    std::fs::create_dir_all(root.join("src")).expect("create src");
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"mcp-env-coverage","type":"module","main":"src/index.ts"}"#,
    )
    .expect("write package");
    std::fs::write(
        root.join("src/index.ts"),
        "import { used } from './utils';\n\
         used();\n\
         function branchy(n: number): number {\n\
           if (n < 0) return -1;\n\
           if (n === 0) return 0;\n\
           if (n < 10) return 1;\n\
           if (n < 100) return 2;\n\
           if (n < 1000) return 3;\n\
           if (n < 10000) return 4;\n\
           return 5;\n\
         }\n\
         branchy(used());\n",
    )
    .expect("write source");
    std::fs::write(root.join("src/utils.ts"), "export const used = () => 42;\n")
        .expect("write utils");

    let source = format!("{COVERAGE_ROOT}/src/index.ts");
    let map = serde_json::json!({
        &source: {
            "path": source,
            "statementMap": {},
            "fnMap": {
                "0": {
                    "name": "branchy",
                    "line": 3,
                    "decl": {
                        "start": { "line": 3, "column": 9 },
                        "end": { "line": 3, "column": 16 }
                    },
                    "loc": {
                        "start": { "line": 3, "column": 35 },
                        "end": { "line": 11, "column": 10 }
                    }
                }
            },
            "branchMap": {},
            "s": {},
            "f": { "0": 0 },
            "b": {}
        }
    });
    let coverage = root.join(COVERAGE_FILE);
    std::fs::create_dir_all(coverage.parent().expect("coverage parent")).expect("create artifacts");
    std::fs::write(
        coverage,
        serde_json::to_string(&map).expect("serialize map"),
    )
    .expect("write coverage");
}

fn write_large_file_project(root: &Path) {
    std::fs::create_dir_all(root.join("src")).expect("create src");
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"mcp-env-max-file-size","type":"module","main":"src/index.ts"}"#,
    )
    .expect("write package");
    std::fs::write(root.join("src/index.ts"), "export const entry = true;\n").expect("write entry");
    let mut source = String::from("export const huge = '");
    source.push_str(&"x".repeat(1_100_000));
    source.push_str("';\n");
    std::fs::write(root.join("src/huge.ts"), source).expect("write large source");
}

fn write_type_aware_trace_project(root: &Path) {
    std::fs::create_dir_all(root.join("src")).expect("create src");
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"mcp-type-aware-trace","type":"module","main":"src/index.ts"}"#,
    )
    .expect("write package");
    std::fs::write(
        root.join("tsconfig.json"),
        r#"{"compilerOptions":{"strict":true,"module":"ESNext","moduleResolution":"Bundler","target":"ES2022","noEmit":true},"include":["src/**/*.ts"]}"#,
    )
    .expect("write tsconfig");
    std::fs::write(root.join("src/index.ts"), "export const entry = true;\n").expect("write entry");
    std::fs::write(
        root.join("src/lonely.ts"),
        "export const helper = (): number => 1;\n",
    )
    .expect("write declaration");
    std::fs::write(
        root.join("src/orphan.ts"),
        "import { helper } from './lonely';\nexport const orphan = helper();\n",
    )
    .expect("write unreachable consumer");
}

/// The built `fallow-mcp` binary as a child process, spoken to over the stdio
/// transport's newline-delimited JSON-RPC.
struct McpServer {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<String>,
}

impl McpServer {
    fn start(with_coverage_env: bool) -> Self {
        Self::start_with_options(with_coverage_env, None, false)
    }

    fn start_type_aware() -> Self {
        Self::start_with_options(false, None, true)
    }

    fn start_with_options(
        with_coverage_env: bool,
        max_file_size: Option<&str>,
        with_type_aware_sidecar: bool,
    ) -> Self {
        let mut command = Command::new(env!("CARGO_BIN_EXE_fallow-mcp"));
        if with_type_aware_sidecar {
            configure_type_aware_sidecar(&mut command);
        }
        if with_coverage_env {
            command
                .env("FALLOW_COVERAGE", COVERAGE_FILE)
                .env("FALLOW_COVERAGE_ROOT", COVERAGE_ROOT);
        } else {
            command
                .env_remove("FALLOW_COVERAGE")
                .env_remove("FALLOW_COVERAGE_ROOT");
        }
        if let Some(max_file_size) = max_file_size {
            command.env("FALLOW_MAX_FILE_SIZE", max_file_size);
        } else {
            command.env_remove("FALLOW_MAX_FILE_SIZE");
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
        };
        server.initialize();
        server
    }

    fn initialize(&mut self) {
        self.send(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"typed-route-env-coverage","version":"0"}}}"#,
        );
        let response = self.response(1);
        assert!(
            response["result"]["serverInfo"].is_object(),
            "initialize must return server info: {response}"
        );
        self.send(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#);
    }

    /// Call `check_health` with parameters that stay on the typed route and
    /// pass no coverage input of their own.
    fn check_health(&mut self, root: &Path) -> serde_json::Value {
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {
                "name": "check_health",
                "arguments": {
                    "root": root.display().to_string(),
                    "complexity": true,
                    "max_crap": 10.0,
                    "no_cache": true
                }
            }
        });
        self.send(&serde_json::to_string(&request).expect("serialize request"));
        let response = self.response(2);
        let result = &response["result"];
        assert_ne!(
            result["isError"], true,
            "check_health must succeed: {response}"
        );
        let text = result["content"][0]["text"]
            .as_str()
            .unwrap_or_else(|| panic!("text content: {response}"));
        serde_json::from_str(text).expect("health payload")
    }

    fn analyze(&mut self, root: &Path) -> serde_json::Value {
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {
                "name": "analyze",
                "arguments": {
                    "root": root.display().to_string(),
                    "no_cache": true
                }
            }
        });
        self.send(&serde_json::to_string(&request).expect("serialize request"));
        let response = self.response(2);
        let result = &response["result"];
        assert_ne!(result["isError"], true, "analyze must succeed: {response}");
        let text = result["content"][0]["text"]
            .as_str()
            .unwrap_or_else(|| panic!("text content: {response}"));
        serde_json::from_str(text).expect("analyze payload")
    }

    fn trace_symbol(&mut self, root: &Path, file: &str, export_name: &str) -> serde_json::Value {
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {
                "name": "trace_symbol",
                "arguments": {
                    "root": root.display().to_string(),
                    "file": file,
                    "export_name": export_name,
                    "no_cache": true
                }
            }
        });
        self.send(&serde_json::to_string(&request).expect("serialize request"));
        let response = self.response(2);
        let result = &response["result"];
        assert_ne!(
            result["isError"], true,
            "trace_symbol must succeed: {response}"
        );
        let text = result["content"][0]["text"]
            .as_str()
            .unwrap_or_else(|| panic!("text content: {response}"));
        serde_json::from_str(text).expect("trace payload")
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

fn configure_type_aware_sidecar(command: &mut Command) {
    let repository_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repository root");
    let sidecar = repository_root.join("tools/type-aware-sidecar/fallow-type-aware.mjs");

    #[cfg(windows)]
    {
        let path = std::env::var_os("PATH").expect("PATH must contain Node.js");
        let node = std::env::split_paths(&path)
            .map(|entry| entry.join("node.exe"))
            .find(|candidate| candidate.is_file())
            .expect("Node.js executable");
        command
            .env("FALLOW_TYPE_AWARE_BIN", node)
            .env("FALLOW_TYPE_AWARE_SCRIPT", sidecar);
    }
    #[cfg(not(windows))]
    command.env("FALLOW_TYPE_AWARE_BIN", sidecar);
}

impl Drop for McpServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
