#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests use unwrap and expect to keep fixture setup concise"
)]

//! Typed tool calls share parsed modules across calls in one server process.
//! The store must not change any answer. This test drives two built
//! `fallow-mcp` servers over stdio: one with the store on (the default) and
//! one with `FALLOW_MCP_WARM_SESSION=0`. Both servers get the same calls on
//! the same project, with edits between the rounds. The text of each answer
//! must be the same bytes. The only exception is the `elapsed_ms` wall clock,
//! which differs between any two runs and is set to 0 on both sides.

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::Duration;

const RESPONSE_TIMEOUT: Duration = Duration::from_mins(3);

#[test]
fn typed_tool_answers_are_the_same_bytes_with_and_without_the_warm_store() {
    let project = tempfile::tempdir().expect("project");
    let root = project.path();
    write_project(root);

    let mut warm = McpServer::start(None);
    let mut cold = McpServer::start(Some("0"));
    let mut compare = |step: &str, tool: &str, extra: &serde_json::Value| {
        let warm_text = without_wall_clock(&warm.call_text(tool, &arguments(root, extra)));
        let cold_text = without_wall_clock(&cold.call_text(tool, &arguments(root, extra)));
        assert_eq!(
            warm_text, cold_text,
            "`{tool}` ({step}) must answer the same bytes with the warm store"
        );
    };

    let none = serde_json::json!({});
    let trace_file = serde_json::json!({ "file": "src/index.ts" });
    let trace_export = serde_json::json!({ "file": "src/utils.ts", "export_name": "used" });
    let sequence = |compare: &mut dyn FnMut(&str, &str, &serde_json::Value), step: &str| {
        compare(step, "analyze", &none);
        compare(step, "find_dupes", &none);
        compare(step, "check_health", &none);
        compare(step, "trace_file", &trace_file);
        compare(step, "trace_export", &trace_export);
        compare(step, "feature_flags", &none);
        compare(step, "analyze", &serde_json::json!({ "production": true }));
    };

    sequence(&mut compare, "first calls");
    sequence(&mut compare, "repeated calls");

    std::fs::write(
        root.join("src/utils.ts"),
        "export const used = () => 42;\nexport const renamed = 2;\n",
    )
    .expect("edit utils");
    sequence(&mut compare, "after an edit");

    std::fs::write(root.join("src/added.ts"), "export const added = 3;\n").expect("add a file");
    std::fs::remove_file(root.join("src/orphan.ts")).expect("remove a file");
    sequence(&mut compare, "after an added and a removed file");
}

/// `text` with the digits of each `"elapsed_ms":` value replaced by `0`.
fn without_wall_clock(text: &str) -> String {
    const FIELD: &str = "\"elapsed_ms\":";
    let mut normalized = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find(FIELD) {
        let value_start = start + FIELD.len();
        normalized.push_str(&rest[..value_start]);
        normalized.push('0');
        rest = rest[value_start..].trim_start_matches(|c: char| c.is_ascii_digit() || c == '.');
    }
    normalized.push_str(rest);
    normalized
}

fn arguments(root: &Path, extra: &serde_json::Value) -> serde_json::Value {
    let mut arguments = serde_json::json!({ "root": root.display().to_string() });
    if let (Some(target), Some(source)) = (arguments.as_object_mut(), extra.as_object()) {
        for (key, value) in source {
            target.insert(key.clone(), value.clone());
        }
    }
    arguments
}

/// A small project with a finding for each tool in the sequence: an unused
/// file and export, a duplicate block, a complex function, a feature flag, a
/// test file that production mode leaves out, and a file that is not valid
/// UTF-8, which gives a read-failure diagnostic.
fn write_project(root: &Path) {
    std::fs::create_dir_all(root.join("src")).expect("create src");
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"mcp-warm-session","type":"module","main":"src/index.ts","dependencies":{"left-pad":"1.0.0"}}"#,
    )
    .expect("write package");
    std::fs::write(
        root.join("src/index.ts"),
        "import { used } from './utils';\n\
         import { first } from './first';\n\
         import { second } from './second';\n\
         if (process.env.FEATURE_NEW_CHECKOUT === 'on') { used(); }\n\
         export function branchy(n: number): number {\n\
           if (n < 0) return -1;\n\
           if (n === 0) return 0;\n\
           if (n < 10) return 1;\n\
           if (n < 100) return 2;\n\
           if (n < 1000) return first(n) + second(n);\n\
           return 5;\n\
         }\n\
         branchy(used());\n",
    )
    .expect("write index");
    std::fs::write(
        root.join("src/utils.ts"),
        "export const used = () => 42;\nexport const unused = 1;\n",
    )
    .expect("write utils");
    let block = "  const values = [n, n + 1, n + 2, n + 3, n + 4];\n\
                 \x20 let total = 0;\n\
                 \x20 for (const value of values) {\n\
                 \x20   if (value % 2 === 0) { total += value * 2; } else { total -= value; }\n\
                 \x20 }\n\
                 \x20 return total + values.length;\n";
    std::fs::write(
        root.join("src/first.ts"),
        format!("export function first(n: number): number {{\n{block}}}\n"),
    )
    .expect("write first");
    std::fs::write(
        root.join("src/second.ts"),
        format!("export function second(n: number): number {{\n{block}}}\n"),
    )
    .expect("write second");
    std::fs::write(root.join("src/orphan.ts"), "export const orphan = true;\n")
        .expect("write orphan");
    std::fs::write(
        root.join("src/utils.test.ts"),
        "import { used } from './utils';\nused();\n",
    )
    .expect("write test file");
    std::fs::write(root.join("src/broken.ts"), [0xff, 0xfe, 0x00]).expect("write invalid UTF-8");
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
    fn start(warm_session: Option<&str>) -> Self {
        let mut command = Command::new(env!("CARGO_BIN_EXE_fallow-mcp"));
        for name in [
            "FALLOW_COVERAGE",
            "FALLOW_COVERAGE_ROOT",
            "FALLOW_DIFF_FILE",
            "FALLOW_CHANGED_SINCE",
            "FALLOW_MAX_FILE_SIZE",
        ] {
            command.env_remove(name);
        }
        match warm_session {
            Some(value) => command.env("FALLOW_MCP_WARM_SESSION", value),
            None => command.env_remove("FALLOW_MCP_WARM_SESSION"),
        };
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
        let id = self.take_id();
        self.send(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": { "name": "warm-session", "version": "0" }
            }
        }));
        let response = self.response(id);
        assert!(
            response["result"]["serverInfo"].is_object(),
            "initialize must return server info: {response}"
        );
        self.send(&serde_json::json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        }));
    }

    /// The text of the answer to `tool`. The call must succeed.
    fn call_text(&mut self, tool: &str, arguments: &serde_json::Value) -> String {
        let id = self.take_id();
        self.send(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/call",
            "params": { "name": tool, "arguments": arguments }
        }));
        let response = self.response(id);
        let result = &response["result"];
        assert_ne!(result["isError"], true, "`{tool}` must succeed: {response}");
        result["content"][0]["text"]
            .as_str()
            .unwrap_or_else(|| panic!("text content: {response}"))
            .to_string()
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
