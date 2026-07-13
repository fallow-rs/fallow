#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests use unwrap and expect to keep fixture setup concise"
)]

//! Server-subcommand dispatch for the multicall binary.
//!
//! `fallow-multicall lsp-server` / `mcp-server` must reach the bundled LSP and
//! MCP server entries. The `--version` routing tests prove each subcommand
//! lands in the correct server (each prints its own binary name), and the LSP
//! handshake test proves the server actually serves protocol when started as a
//! multicall subcommand, not just that the entry was called.

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

const MULTICALL: &str = env!("CARGO_BIN_EXE_fallow-multicall");

#[test]
fn lsp_server_subcommand_reports_lsp_version() {
    let out = Command::new(MULTICALL)
        .args(["lsp-server", "--version"])
        .output()
        .expect("run lsp-server --version");
    assert!(out.status.success(), "lsp-server --version exited non-zero");
    let stdout = String::from_utf8(out.stdout).expect("utf8 stdout");
    assert!(
        stdout.trim().starts_with("fallow-lsp "),
        "lsp-server --version must route to the LSP entry: {stdout:?}"
    );
}

#[test]
fn mcp_server_subcommand_reports_mcp_version() {
    let out = Command::new(MULTICALL)
        .args(["mcp-server", "--version"])
        .output()
        .expect("run mcp-server --version");
    assert!(out.status.success(), "mcp-server --version exited non-zero");
    let stdout = String::from_utf8(out.stdout).expect("utf8 stdout");
    assert!(
        stdout.trim().starts_with("fallow-mcp "),
        "mcp-server --version must route to the MCP entry: {stdout:?}"
    );
}

#[test]
fn lsp_server_subcommand_completes_initialize_handshake() {
    let mut child = Command::new(MULTICALL)
        .arg("lsp-server")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn lsp-server");

    // Keep `stdin` in scope through the read so the server does not see EOF and
    // shut down before it answers the request.
    let mut stdin = child.stdin.take().expect("child stdin");
    let body = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{},"processId":null,"rootUri":null}}"#;
    write!(stdin, "Content-Length: {}\r\n\r\n{}", body.len(), body).expect("write initialize");
    stdin.flush().expect("flush initialize");

    let mut stdout = child.stdout.take().expect("child stdout");
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let _ = tx.send(read_lsp_message(&mut stdout));
    });

    let Ok(response) = rx.recv_timeout(Duration::from_secs(20)) else {
        let _ = child.kill();
        panic!("timed out waiting for the LSP initialize response");
    };
    let _ = child.kill();
    let _ = child.wait();

    assert!(
        response.contains("\"result\""),
        "initialize reply must carry a result: {response}"
    );
    assert!(
        response.contains("\"capabilities\""),
        "initialize reply must advertise capabilities: {response}"
    );
}

/// Read one `Content-Length` framed LSP message body from `reader`.
fn read_lsp_message(reader: &mut impl Read) -> String {
    let mut buffered = BufReader::new(reader);
    let mut content_length = 0usize;
    loop {
        let mut line = String::new();
        if buffered.read_line(&mut line).expect("read header line") == 0 {
            return String::new();
        }
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            break;
        }
        if let Some(value) = trimmed.strip_prefix("Content-Length:") {
            content_length = value.trim().parse().expect("parse Content-Length");
        }
    }
    let mut body = vec![0u8; content_length];
    buffered.read_exact(&mut body).expect("read message body");
    String::from_utf8(body).expect("utf8 message body")
}
