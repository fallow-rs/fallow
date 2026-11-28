#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests use unwrap and expect to keep fixture setup concise"
)]

//! The snippet process of Code Mode: the `fallow-mcp` binary started with the
//! internal `--code-mode-worker` flag. The server writes one start line per
//! snippet on its stdin and reads its answers on its stdout.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const WORKER_FLAG: &str = "--code-mode-worker";
const PROTOCOL: u32 = 1;

fn start_line(protocol: u32, code: &str, timeout_ms: u64) -> String {
    let start = serde_json::json!({
        "protocol": protocol,
        "version": env!("CARGO_PKG_VERSION"),
        "cwd": ".",
        "code": code,
        "timeout_ms": timeout_ms,
    });
    format!("{start}\n")
}

#[test]
fn a_snippet_process_refuses_a_start_of_another_protocol() {
    let cwd = tempfile::tempdir().expect("tempdir");
    let mut child = Command::new(env!("CARGO_BIN_EXE_fallow-mcp"))
        .arg(WORKER_FLAG)
        .current_dir(cwd.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn the snippet process");
    let mut stdin = child.stdin.take().expect("stdin");
    stdin
        .write_all(start_line(PROTOCOL + 998, "return 1;", 1_000).as_bytes())
        .expect("write the start");
    stdin.flush().expect("flush the start");

    let stdout = BufReader::new(child.stdout.take().expect("stdout"));
    let done = stdout
        .lines()
        .map_while(Result::ok)
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(&line).ok())
        .find(|message| message["kind"] == "done")
        .expect("a done message");
    assert_eq!(done["ok"], false, "{done}");
    assert!(
        done["output"]
            .as_str()
            .is_some_and(|output| output.contains("restart the MCP server")),
        "{done}"
    );
    drop(stdin);
    let _ = child.kill();
    let _ = child.wait();
}

/// A snippet process inside a long builtin call must end soon after its server
/// is killed, also when its own time limit is far away.
#[cfg(unix)]
#[test]
fn a_snippet_process_ends_when_its_server_is_killed() {
    let dir = tempfile::tempdir().expect("tempdir");
    let start = dir.path().join("start.json");
    let pid_file = dir.path().join("worker.pid");
    std::fs::write(
        &start,
        start_line(
            PROTOCOL,
            "return Array.prototype.reverse.call({ length: 2 ** 53 - 1 }).length;",
            30_000,
        ),
    )
    .expect("write the start");

    // The shell stands for the server: it starts the snippet process and waits.
    let mut server = Command::new("/bin/sh")
        .arg("-c")
        .arg(r#""$0" --code-mode-worker < "$1" > /dev/null & echo $! > "$2"; wait"#)
        .arg(env!("CARGO_BIN_EXE_fallow-mcp"))
        .arg(&start)
        .arg(&pid_file)
        .current_dir(dir.path())
        .spawn()
        .expect("spawn the stand-in server");
    let pid = wait_for_pid(&pid_file);
    // Give the snippet process time to read its start and enter the builtin.
    std::thread::sleep(Duration::from_millis(500));
    assert!(process_exists(pid), "the snippet process must still run");

    server.kill().expect("kill the stand-in server");
    let _ = server.wait();

    let killed = Instant::now();
    while process_exists(pid) {
        if killed.elapsed() > Duration::from_secs(3) {
            let _ = Command::new("kill").args(["-9", &pid.to_string()]).status();
            panic!("the snippet process {pid} still runs 3 s after its server was killed");
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

#[cfg(unix)]
fn wait_for_pid(path: &std::path::Path) -> u32 {
    let give_up = Instant::now() + Duration::from_secs(30);
    loop {
        if let Ok(text) = std::fs::read_to_string(path)
            && let Ok(pid) = text.trim().parse()
        {
            return pid;
        }
        assert!(Instant::now() < give_up, "no PID file");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(unix)]
fn process_exists(pid: u32) -> bool {
    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}
