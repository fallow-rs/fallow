#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests use unwrap and expect to keep fixture setup concise"
)]

//! The one-time telemetry consent form. The test drives the built
//! `fallow-mcp` over stdio as a client that declares form elicitation, and
//! answers the form like a person would. Each test waits for the idle time
//! before the form, about 20 seconds.

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

const RESPONSE_TIMEOUT: Duration = Duration::from_mins(3);
/// The idle time before the form, plus margin.
const FORM_WAIT: Duration = Duration::from_secs(45);
/// How long a test waits to prove that no form comes.
const NO_FORM_WAIT: Duration = Duration::from_secs(30);

/// The idle time that the server waits before the form, minus margin.
const MIN_IDLE: Duration = Duration::from_secs(19);

fn forms() -> serde_json::Value {
    serde_json::json!({ "elicitation": {} })
}

#[test]
fn a_checked_box_turns_telemetry_on_after_the_idle_wait() {
    let home = tempfile::tempdir().expect("home");
    let project = project();
    let mut server = Server::start(home.path(), &[], &forms());
    server.call("project_info", &root_of(&project));
    let answered = Instant::now();
    let form = server
        .next_request("elicitation/create", FORM_WAIT)
        .expect("the server sends the form");
    assert!(
        answered.elapsed() >= MIN_IDLE,
        "the form came {:?} after the answer, before the idle wait",
        answered.elapsed()
    );
    assert!(
        form["params"]["message"]
            .as_str()
            .is_some_and(|message| message.starts_with("Fallow asks once"))
    );
    server.answer(&form, &serde_json::json!({ "share_usage_data": true }));
    let status = wait_for_decision(home.path());
    assert_eq!(status["state"], "on", "{status}");
}

#[test]
fn a_clear_box_records_no_and_the_form_does_not_come_again() {
    let home = tempfile::tempdir().expect("home");
    let project = project();
    let mut server = Server::start(home.path(), &[], &forms());
    server.call("project_info", &root_of(&project));
    let form = server
        .next_request("elicitation/create", FORM_WAIT)
        .expect("the server sends the form");
    server.answer(&form, &serde_json::json!({ "share_usage_data": false }));
    let status = wait_for_decision(home.path());
    drop(server);
    assert_eq!(status["state"], "off", "{status}");
    // Without the ask count, only the recorded decision can stop the form.
    remove_prompt_state(home.path());
    let mut again = Server::start(home.path(), &[], &forms());
    again.call("project_info", &root_of(&project));
    assert!(
        again
            .next_request("elicitation/create", NO_FORM_WAIT)
            .is_none(),
        "a recorded decision must stop the form"
    );
}

#[test]
fn one_process_sends_one_form() {
    let home = tempfile::tempdir().expect("home");
    let project = project();
    let mut server = Server::start(home.path(), &[], &forms());
    server.call("project_info", &root_of(&project));
    let form = server
        .next_request("elicitation/create", FORM_WAIT)
        .expect("the server sends the form");
    server.reply(&form["id"], &serde_json::json!({ "action": "cancel" }));
    // Without the ask count, only the once-per-process rule can stop a form.
    remove_prompt_state(home.path());
    server.call("project_info", &root_of(&project));
    assert!(
        server
            .next_request("elicitation/create", NO_FORM_WAIT)
            .is_none(),
        "one process sends one form"
    );
}

#[test]
fn a_client_without_form_elicitation_gets_no_form() {
    let home = tempfile::tempdir().expect("home");
    let project = project();
    let mut server = Server::start(home.path(), &[], &serde_json::json!({}));
    server.call("project_info", &root_of(&project));
    assert!(
        server
            .next_request("elicitation/create", NO_FORM_WAIT)
            .is_none()
    );
}

#[test]
fn a_kill_switch_stops_the_form() {
    let home = tempfile::tempdir().expect("home");
    let project = project();
    let mut server = Server::start(home.path(), &[("DO_NOT_TRACK", "1")], &forms());
    server.call("project_info", &root_of(&project));
    assert!(
        server
            .next_request("elicitation/create", NO_FORM_WAIT)
            .is_none()
    );
}

fn project() -> tempfile::TempDir {
    let project = tempfile::tempdir().expect("project");
    std::fs::create_dir_all(project.path().join("src")).expect("src");
    std::fs::write(
        project.path().join("package.json"),
        r#"{"name":"consent-probe"}"#,
    )
    .expect("package.json");
    std::fs::write(project.path().join("src/index.ts"), "export const a = 1;\n").expect("index");
    project
}

fn root_of(project: &tempfile::TempDir) -> serde_json::Value {
    serde_json::json!({ "root": project.path() })
}

/// Remove the ask count of `home`. The config folder differs per platform,
/// so the test searches the home.
fn remove_prompt_state(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            remove_prompt_state(&path);
        } else if path.ends_with("telemetry-prompt.json") {
            std::fs::remove_file(&path).expect("remove the ask count");
        }
    }
}

/// The telemetry status of `home` once a decision is on record.
fn wait_for_decision(home: &Path) -> serde_json::Value {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let output = fallow(home)
            .args(["telemetry", "status", "--format", "json", "--quiet"])
            .output()
            .expect("run fallow telemetry status");
        let value: serde_json::Value = serde_json::from_slice(&output.stdout).expect("status JSON");
        if value["telemetry"]["explicit_decision"] == true {
            return value["telemetry"].clone();
        }
        assert!(Instant::now() < deadline, "no decision recorded: {value}");
        thread::sleep(Duration::from_millis(200));
    }
}

/// A `fallow` command with the clean telemetry environment of `home`.
fn fallow(home: &Path) -> Command {
    let mut command = Command::new(crate::fallow_binary());
    clean_env(&mut command, home);
    command
}

fn clean_env(command: &mut Command, home: &Path) {
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
    command
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("APPDATA", home.join("AppData"))
        .env("RUST_LOG", "off")
        .env("FALLOW_API_URL", "http://127.0.0.1:9");
}

struct Server {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<String>,
    next_id: u64,
}

impl Server {
    fn start(home: &Path, env: &[(&str, &str)], capabilities: &serde_json::Value) -> Self {
        let mut command = Command::new(env!("CARGO_BIN_EXE_fallow-mcp"));
        command.env("FALLOW_BIN", crate::fallow_binary());
        clean_env(&mut command, home);
        for (key, value) in env {
            command.env(key, value);
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
        let id = server.take_id();
        server.send(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": capabilities,
                "clientInfo": { "name": "consent-probe", "version": "0" }
            }
        }));
        server.response(id);
        server.send(&serde_json::json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        }));
        server
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
        assert_ne!(response["result"]["isError"], true, "{response}");
    }

    /// The next request of the server with `method`, within `wait`.
    fn next_request(&self, method: &str, wait: Duration) -> Option<serde_json::Value> {
        let deadline = Instant::now() + wait;
        loop {
            let left = deadline.checked_duration_since(Instant::now())?;
            let line = self.lines.recv_timeout(left).ok()?;
            let Ok(message) = serde_json::from_str::<serde_json::Value>(&line) else {
                continue;
            };
            if message["method"] == method && message.get("id").is_some() {
                return Some(message);
            }
        }
    }

    /// Accept `form` with `content`.
    fn answer(&mut self, form: &serde_json::Value, content: &serde_json::Value) {
        self.reply(
            &form["id"],
            &serde_json::json!({ "action": "accept", "content": content }),
        );
    }

    fn reply(&mut self, id: &serde_json::Value, result: &serde_json::Value) {
        self.send(&serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": result }));
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
            if message["id"].as_u64() == Some(id) && message.get("method").is_none() {
                return message;
            }
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
