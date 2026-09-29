#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests use unwrap and expect to keep fixture setup concise"
)]

//! `FALLOW_CACHE_DIR` must move the persistent cache of an editor session.
//!
//! The test drives the real `fallow-lsp` binary over stdio, so the server
//! reads the variable from its own process environment, as it does under an
//! editor.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

const ANALYSIS_COMPLETE: &str = "fallow/analysisComplete";
const ANALYSIS_TIMEOUT: Duration = Duration::from_mins(1);

fn send(stdin: &mut ChildStdin, message: &serde_json::Value) {
    let body = message.to_string();
    write!(stdin, "Content-Length: {}\r\n\r\n{body}", body.len()).expect("write message");
    stdin.flush().expect("flush message");
}

/// Read framed server messages on a thread and forward each one.
fn spawn_reader(child: &mut Child) -> mpsc::Receiver<serde_json::Value> {
    let stdout = child.stdout.take().expect("server stdout");
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        loop {
            let mut length = None;
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 {
                    return;
                }
                let line = line.trim_end();
                if line.is_empty() {
                    break;
                }
                if let Some(value) = line.strip_prefix("Content-Length: ") {
                    length = value.parse::<usize>().ok();
                }
            }
            let Some(length) = length else { return };
            let mut body = vec![0; length];
            if reader.read_exact(&mut body).is_err() {
                return;
            }
            let message: serde_json::Value =
                serde_json::from_slice(&body).expect("server message is JSON");
            if sender.send(message).is_err() {
                return;
            }
        }
    });
    receiver
}

fn write_project(root: &Path) -> std::path::PathBuf {
    std::fs::create_dir_all(root.join("src")).expect("src dir");
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"cache-dir-env","main":"src/index.ts"}"#,
    )
    .expect("package.json");
    std::fs::write(
        root.join("src/index.ts"),
        "import { used } from './used';\nexport const main = used;\n",
    )
    .expect("index.ts");
    let used = root.join("src/used.ts");
    std::fs::write(&used, "export const used = 1;\nexport const unused = 2;\n").expect("used.ts");
    used
}

fn file_uri(path: &Path) -> String {
    format!("file://{}", path.display())
}

#[test]
#[cfg(unix)]
fn fallow_cache_dir_moves_the_editor_session_cache_out_of_the_project() {
    let project = tempfile::tempdir().expect("project dir");
    let root = dunce::canonicalize(project.path()).expect("canonical root");
    let cache = tempfile::tempdir().expect("cache dir");
    let cache_dir = cache.path().join("fallow-cache");
    let opened = write_project(&root);

    let mut child = Command::new(env!("CARGO_BIN_EXE_fallow-lsp"))
        .current_dir(&root)
        .env("FALLOW_CACHE_DIR", &cache_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("start fallow-lsp");
    let messages = spawn_reader(&mut child);
    let mut stdin = child.stdin.take().expect("server stdin");

    send(
        &mut stdin,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "processId": null,
                "rootUri": file_uri(&root),
                "capabilities": {},
            },
        }),
    );
    // The server drops notifications that arrive before it answers
    // `initialize`, so wait for that answer first.
    let initialized = messages
        .iter()
        .any(|message| message.get("id") == Some(&serde_json::json!(1)));
    assert!(initialized, "the server must answer initialize");
    send(
        &mut stdin,
        &serde_json::json!({"jsonrpc": "2.0", "method": "initialized", "params": {}}),
    );
    send(
        &mut stdin,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": file_uri(&opened),
                    "languageId": "typescript",
                    "version": 1,
                    "text": std::fs::read_to_string(&opened).expect("opened source"),
                },
            },
        }),
    );

    let deadline = std::time::Instant::now() + ANALYSIS_TIMEOUT;
    let mut completed = false;
    while let Some(remaining) = deadline.checked_duration_since(std::time::Instant::now()) {
        let Ok(message) = messages.recv_timeout(remaining) else {
            break;
        };
        let method = message.get("method").and_then(|value| value.as_str());
        if method == Some(ANALYSIS_COMPLETE) {
            completed = true;
            break;
        }
        // The server waits for answers to its own requests, such as
        // `client/registerCapability`, so answer each one.
        if let (Some(_), Some(id)) = (method, message.get("id")) {
            send(
                &mut stdin,
                &serde_json::json!({"jsonrpc": "2.0", "id": id, "result": null}),
            );
        }
    }

    send(
        &mut stdin,
        &serde_json::json!({"jsonrpc": "2.0", "id": 2, "method": "shutdown", "params": null}),
    );
    send(
        &mut stdin,
        &serde_json::json!({"jsonrpc": "2.0", "method": "exit", "params": null}),
    );
    drop(stdin);
    let exited = (0..100).any(|_| {
        std::thread::sleep(Duration::from_millis(50));
        child.try_wait().ok().flatten().is_some()
    });
    if !exited {
        let _ = child.kill();
        let _ = child.wait();
    }

    assert!(completed, "the server must finish the startup analysis");
    assert!(
        !root.join(".fallow").exists(),
        "the session wrote a cache into the project although FALLOW_CACHE_DIR points elsewhere",
    );
    assert!(
        cache_dir.join("cache.bin").is_file(),
        "the parse cache must land in the FALLOW_CACHE_DIR directory",
    );
    assert!(
        cache_dir.join("graph-cache.bin").is_file(),
        "the graph cache must land in the FALLOW_CACHE_DIR directory",
    );
}
