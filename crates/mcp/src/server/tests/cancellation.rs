//! A `notifications/cancelled` from the client reaches the work of a tool
//! call through the server's `call_tool`, not only rmcp's request token.

use std::time::Duration;

use rmcp::ServiceExt;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use super::super::FallowMcp;

const GIVE_UP: Duration = Duration::from_secs(60);
/// Below the fake CLI sleep, the 120 s tool deadline and the 30 s Code Mode
/// deadline, so only the cancel can end the subprocess in time.
const EXIT_WAIT: Duration = Duration::from_secs(20);

async fn send(writer: &mut (impl AsyncWriteExt + Unpin), message: serde_json::Value) {
    let mut line = message.to_string();
    line.push('\n');
    writer
        .write_all(line.as_bytes())
        .await
        .expect("write a message to the server");
}

/// Read server messages until the response with `id` arrives.
async fn response(reader: &mut (impl AsyncBufReadExt + Unpin), id: u64) -> serde_json::Value {
    let mut line = String::new();
    loop {
        line.clear();
        let read = tokio::time::timeout(GIVE_UP, reader.read_line(&mut line))
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

fn process_exists(pid: u32) -> bool {
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .status()
        .is_ok_and(|status| status.success())
}

async fn wait_for_exit(pid: u32) -> bool {
    let give_up_at = std::time::Instant::now() + EXIT_WAIT;
    while process_exists(pid) {
        if std::time::Instant::now() >= give_up_at {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    true
}

/// Write a fake CLI that records its PID and then sleeps far past every
/// deadline of the test.
fn fake_cli(dir: &std::path::Path, pid_path: &std::path::Path) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let binary = dir.join("fake-fallow");
    std::fs::write(
        &binary,
        format!(
            "#!/bin/sh\necho $$ > '{}'\nexec sleep 600\n",
            pid_path.display()
        ),
    )
    .expect("write the fake CLI");
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755))
        .expect("make the fake CLI executable");
    binary
}

/// Start a server on the fake CLI, send `tools/call` request 2 with `name`
/// and `arguments`, cancel it once the CLI runs, and report whether the CLI
/// process exited in time.
async fn cancel_tool_call(name: &str, arguments: serde_json::Value) -> (u32, bool) {
    let temp = tempfile::tempdir().expect("temp directory");
    let pid_path = temp.path().join("cli.pid");
    let binary = fake_cli(temp.path(), &pid_path);

    let server = FallowMcp {
        binary: binary.to_string_lossy().into_owned(),
        tool_router: FallowMcp::tool_router(),
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
    let (client_read, mut client_write) = tokio::io::split(client);
    let mut client_read = BufReader::new(client_read);

    send(
        &mut client_write,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": { "name": "cancel-test", "version": "0" }
            }
        }),
    )
    .await;
    response(&mut client_read, 1).await;
    send(
        &mut client_write,
        serde_json::json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
    )
    .await;
    send(
        &mut client_write,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": { "name": name, "arguments": arguments }
        }),
    )
    .await;
    let pid = wait_for_pid(&pid_path).await;

    send(
        &mut client_write,
        serde_json::json!({
            "jsonrpc": "2.0",
            "method": "notifications/cancelled",
            "params": { "requestId": 2, "reason": "test" }
        }),
    )
    .await;
    let exited = wait_for_exit(pid).await;
    if !exited {
        let _ = std::process::Command::new("kill")
            .args(["-9", &pid.to_string()])
            .status();
    }
    running.abort();
    (pid, exited)
}

#[tokio::test]
async fn a_client_cancel_kills_the_subprocess_of_a_direct_tool_call() {
    let (pid, exited) = cancel_tool_call("list_suppressions", serde_json::json!({})).await;
    assert!(
        exited,
        "the cancelled call left its subprocess {pid} running"
    );
}

/// Code Mode runs its snippet on a blocking thread, outside the request
/// scope. The cancel must still reach the subprocess of a host call.
#[tokio::test]
async fn a_client_cancel_kills_the_subprocess_of_a_code_mode_host_call() {
    let (pid, exited) = cancel_tool_call(
        "code_execute",
        serde_json::json!({
            "code": "return fallow.analyze({});",
            "timeout_ms": 30_000
        }),
    )
    .await;
    assert!(
        exited,
        "the cancelled Code Mode call left its subprocess {pid} running"
    );
}
