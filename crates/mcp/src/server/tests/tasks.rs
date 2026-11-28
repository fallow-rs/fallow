//! A long tool call returns an MCP task only when the request declares the
//! tasks extension, and `tasks/cancel` stops the work of the task.

use std::time::Duration;

use rmcp::ServiceExt;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, ReadHalf, WriteHalf};

use super::super::FallowMcp;

const GIVE_UP: Duration = Duration::from_secs(60);
const TASKS_EXTENSION: &str = "io.modelcontextprotocol/tasks";

/// The fake CLI writes this JSON and exits.
const FAKE_OUTPUT: &str = r#"{"kind":"fake","ok":true}"#;

struct Client {
    read: BufReader<ReadHalf<tokio::io::DuplexStream>>,
    write: WriteHalf<tokio::io::DuplexStream>,
    running: tokio::task::JoinHandle<()>,
}

impl Client {
    fn start(binary: &std::path::Path) -> Self {
        let server = FallowMcp {
            binary: binary.to_string_lossy().into_owned(),
            cargo_fallow: "cargo-fallow".to_string(),
            tool_router: FallowMcp::tool_router(),
            confirm_timeout: crate::tools::CONFIRM_TIMEOUT,
            tasks: crate::tasks::TaskStore::new(),
            code_mode_worker: None,
        };
        let (client, server_io) = tokio::io::duplex(64 * 1024);
        let (server_read, server_write) = tokio::io::split(server_io);
        let running = tokio::spawn(async move {
            let service = server
                .serve((server_read, server_write))
                .await
                .expect("the server starts");
            let _ = service.waiting().await;
        });
        let (read, write) = tokio::io::split(client);
        Self {
            read: BufReader::new(read),
            write,
            running,
        }
    }

    async fn send(&mut self, message: &serde_json::Value) {
        let mut line = message.to_string();
        line.push('\n');
        self.write
            .write_all(line.as_bytes())
            .await
            .expect("write a message to the server");
    }

    async fn request(
        &mut self,
        id: u64,
        method: &str,
        mut params: serde_json::Value,
        tasks: bool,
    ) -> serde_json::Value {
        params["_meta"] = request_meta(tasks);
        self.send(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        }))
        .await;
        let mut line = String::new();
        loop {
            line.clear();
            let read = tokio::time::timeout(GIVE_UP, self.read.read_line(&mut line))
                .await
                .expect("the server answers in time")
                .expect("read a server message");
            assert!(read > 0, "the server closed the stream");
            let message: serde_json::Value =
                serde_json::from_str(&line).expect("server messages are JSON");
            if message["id"] == id {
                return message;
            }
        }
    }

    /// Poll `tasks/get` until the task reaches a terminal status.
    async fn settle(&mut self, task_id: &str, mut id: u64) -> serde_json::Value {
        let give_up_at = std::time::Instant::now() + GIVE_UP;
        loop {
            let response = self
                .request(
                    id,
                    "tasks/get",
                    serde_json::json!({ "taskId": task_id }),
                    true,
                )
                .await;
            let status = response["result"]["status"].as_str().unwrap_or_default();
            if matches!(status, "completed" | "failed" | "cancelled") {
                return response;
            }
            assert!(
                std::time::Instant::now() < give_up_at,
                "the task never settled: {response}"
            );
            id += 1;
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        self.running.abort();
    }
}

/// The request `_meta` of a 2026-07-28 client, with or without the tasks
/// extension.
fn request_meta(tasks: bool) -> serde_json::Value {
    let extensions = if tasks {
        serde_json::json!({ TASKS_EXTENSION: {} })
    } else {
        serde_json::json!({})
    };
    serde_json::json!({
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientInfo": { "name": "tasks-test", "version": "0" },
        "io.modelcontextprotocol/clientCapabilities": { "extensions": extensions },
    })
}

fn write_script(dir: &std::path::Path, body: &str) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let binary = dir.join("fake-fallow");
    std::fs::write(&binary, format!("#!/bin/sh\n{body}\n")).expect("write the fake CLI");
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755))
        .expect("make the fake CLI executable");
    binary
}

fn call_params() -> serde_json::Value {
    serde_json::json!({ "name": "find_similar_code", "arguments": {} })
}

#[tokio::test]
async fn a_task_tool_returns_a_task_and_tasks_get_returns_the_result() {
    let temp = tempfile::tempdir().expect("temp directory");
    let binary = write_script(temp.path(), &format!("echo '{FAKE_OUTPUT}'"));
    let mut client = Client::start(&binary);

    let created = client.request(1, "tools/call", call_params(), true).await;
    let task = &created["result"];
    assert_eq!(task["resultType"], "task", "{created}");
    assert_eq!(task["status"], "working", "{created}");
    let task_id = task["taskId"].as_str().expect("a task id");
    assert!(
        task_id.len() <= 128 && task_id.bytes().all(|byte| byte.is_ascii_graphic()),
        "the task id must be at most 128 printable ASCII characters: {task_id}"
    );
    for field in ["createdAt", "lastUpdatedAt", "statusMessage"] {
        assert!(task[field].is_string(), "{field} is missing: {created}");
    }
    assert!(
        task["ttlMs"]
            .as_u64()
            .is_some_and(|ttl| ttl >= 60 * 60 * 1000),
        "the result must stay readable for an hour: {created}"
    );
    assert_eq!(task["pollIntervalMs"], 10_000, "{created}");

    let settled = client.settle(task_id, 2).await;
    let result = &settled["result"];
    assert_eq!(result["status"], "completed", "{settled}");
    assert_eq!(result["resultType"], "complete", "{settled}");
    let text = result["result"]["content"][0]["text"]
        .as_str()
        .expect("the task result has the tool text");
    assert!(text.contains(r#""kind":"fake""#), "{settled}");
}

#[tokio::test]
async fn a_request_without_the_extension_keeps_the_synchronous_result() {
    let temp = tempfile::tempdir().expect("temp directory");
    let binary = write_script(temp.path(), &format!("echo '{FAKE_OUTPUT}'"));
    let mut client = Client::start(&binary);

    let response = client.request(1, "tools/call", call_params(), false).await;
    let result = &response["result"];
    assert_ne!(result["resultType"], "task", "{response}");
    assert!(result.get("taskId").is_none(), "{response}");
    let text = result["content"][0]["text"]
        .as_str()
        .expect("a synchronous tool result");
    assert!(text.contains(r#""kind":"fake""#), "{response}");
}

#[tokio::test]
async fn the_capabilities_of_initialize_alone_keep_the_synchronous_result() {
    let temp = tempfile::tempdir().expect("temp directory");
    let binary = write_script(temp.path(), &format!("echo '{FAKE_OUTPUT}'"));
    let mut client = Client::start(&binary);

    client
        .send(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-11-25",
                "capabilities": { "extensions": { TASKS_EXTENSION: {} } },
                "clientInfo": { "name": "tasks-test", "version": "0" }
            }
        }))
        .await;
    client
        .send(&serde_json::json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }))
        .await;
    let mut line = String::new();
    tokio::time::timeout(GIVE_UP, client.read.read_line(&mut line))
        .await
        .expect("the server answers initialize in time")
        .expect("read the initialize response");

    client
        .send(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": call_params()
        }))
        .await;
    let response = loop {
        line.clear();
        tokio::time::timeout(GIVE_UP, client.read.read_line(&mut line))
            .await
            .expect("the server answers in time")
            .expect("read a server message");
        let message: serde_json::Value = serde_json::from_str(&line).expect("JSON");
        if message["id"] == 2 {
            break message;
        }
    };
    assert_ne!(response["result"]["resultType"], "task", "{response}");
    assert!(response["result"]["content"].is_array(), "{response}");
}

#[tokio::test]
async fn audit_tasks_poll_every_two_seconds() {
    let temp = tempfile::tempdir().expect("temp directory");
    let binary = write_script(temp.path(), &format!("echo '{FAKE_OUTPUT}'"));
    let mut client = Client::start(&binary);
    let project = tempfile::tempdir().expect("temp directory");
    std::fs::write(project.path().join("package.json"), "{}").expect("write package.json");

    let created = client
        .request(
            1,
            "tools/call",
            serde_json::json!({
                "name": "audit",
                "arguments": { "root": project.path().to_string_lossy() }
            }),
            true,
        )
        .await;
    assert_eq!(created["result"]["resultType"], "task", "{created}");
    assert_eq!(created["result"]["pollIntervalMs"], 2_000, "{created}");
    let task_id = created["result"]["taskId"]
        .as_str()
        .expect("a task id")
        .to_string();
    client.settle(&task_id, 2).await;
}

#[tokio::test]
async fn a_tool_that_is_not_a_task_tool_answers_synchronously_for_a_task_client() {
    let temp = tempfile::tempdir().expect("temp directory");
    let binary = write_script(temp.path(), &format!("echo '{FAKE_OUTPUT}'"));
    let mut client = Client::start(&binary);

    let response = client
        .request(
            1,
            "tools/call",
            serde_json::json!({ "name": "list_suppressions", "arguments": {} }),
            true,
        )
        .await;
    assert_ne!(response["result"]["resultType"], "task", "{response}");
    assert!(response["result"]["content"].is_array(), "{response}");
}

#[tokio::test]
async fn tasks_cancel_kills_the_subprocess_and_the_task_settles_as_cancelled() {
    let temp = tempfile::tempdir().expect("temp directory");
    let pid_path = temp.path().join("cli.pid");
    let binary = write_script(
        temp.path(),
        &format!("echo $$ > '{}'\nexec sleep 600", pid_path.display()),
    );
    let mut client = Client::start(&binary);

    let created = client.request(1, "tools/call", call_params(), true).await;
    let task_id = created["result"]["taskId"]
        .as_str()
        .expect("a task id")
        .to_string();
    let pid = wait_for_pid(&pid_path).await;

    let ack = client
        .request(
            2,
            "tasks/cancel",
            serde_json::json!({ "taskId": task_id }),
            true,
        )
        .await;
    assert!(ack.get("error").is_none(), "{ack}");

    let settled = client.settle(&task_id, 3).await;
    assert_eq!(settled["result"]["status"], "cancelled", "{settled}");
    let exited = wait_for_exit(pid).await;
    if !exited {
        let _ = std::process::Command::new("kill")
            .args(["-9", &pid.to_string()])
            .status();
    }
    assert!(
        exited,
        "the cancelled task left its subprocess {pid} running"
    );
}

#[tokio::test]
async fn tasks_get_of_an_unknown_task_is_invalid_params() {
    let temp = tempfile::tempdir().expect("temp directory");
    let binary = write_script(temp.path(), "exit 0");
    let mut client = Client::start(&binary);

    let response = client
        .request(
            1,
            "tasks/get",
            serde_json::json!({ "taskId": "no-such-task" }),
            true,
        )
        .await;
    assert_eq!(response["error"]["code"], -32602, "{response}");
}

async fn wait_for_pid(path: &std::path::Path) -> u32 {
    let give_up_at = std::time::Instant::now() + GIVE_UP;
    loop {
        if let Ok(pid) = std::fs::read_to_string(path)
            && let Ok(pid) = pid.trim().parse()
        {
            return pid;
        }
        assert!(std::time::Instant::now() < give_up_at, "no PID file");
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

async fn wait_for_exit(pid: u32) -> bool {
    let give_up_at = std::time::Instant::now() + Duration::from_secs(20);
    while std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .status()
        .is_ok_and(|status| status.success())
    {
        if std::time::Instant::now() >= give_up_at {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    true
}
