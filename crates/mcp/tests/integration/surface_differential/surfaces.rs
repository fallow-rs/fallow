//! Each surface on its real path:
//!
//! - the `fallow` CLI binary;
//! - the `fallow-mcp` binary over the stdio transport, which calls
//!   `fallow_api` on the typed route and the CLI on the subprocess route;
//! - the `fallow-lsp` binary over the Language Server Protocol on stdio;
//! - `fallow_api`, the runners and serializers that the Node bindings call;
//! - the built Node bindings, when `FALLOW_SURFACE_NODE_ADDON` names the
//!   `crates/napi` directory of a build.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::facts::Diagnostic;

const RESPONSE_TIMEOUT: Duration = Duration::from_mins(3);
/// After `fallow/analysisComplete`, the server can still publish the last
/// diagnostics of the run.
const LSP_SETTLE: Duration = Duration::from_millis(500);
const EXIT_WAIT: Duration = Duration::from_secs(5);

/// Environment controls that change the result of an in-process
/// `fallow_api` call. The test process must not set them, because only a
/// child process can start without them.
const RESULT_ENV: [&str; 16] = [
    "FALLOW_CACHE_DIR",
    "FALLOW_CACHE_MAX_SIZE",
    "FALLOW_CHANGED_SINCE",
    "FALLOW_DIFF_FILE",
    "FALLOW_COVERAGE",
    "FALLOW_COVERAGE_ROOT",
    "FALLOW_MAX_FILE_SIZE",
    "FALLOW_PRODUCTION",
    "FALLOW_PRODUCTION_DEAD_CODE",
    "FALLOW_PRODUCTION_HEALTH",
    "FALLOW_PRODUCTION_DUPES",
    "FALLOW_PACKAGE_BASELINES",
    "FALLOW_ROOT",
    "FALLOW_TYPE_AWARE",
    "FALLOW_TYPE_AWARE_BACKEND",
    "FALLOW_RUNTIME_COVERAGE_SOURCE",
];

/// The environment of one surface run, as `(name, value)`.
pub type Env<'a> = &'a [(&'a str, &'a str)];

/// A command for `program` without any `FALLOW_*` variable of the test
/// process, with `env` added.
pub fn command(program: &Path, env: Env<'_>) -> Command {
    let mut command = Command::new(program);
    for (name, _) in std::env::vars_os() {
        if name.to_string_lossy().starts_with("FALLOW_") {
            command.env_remove(name);
        }
    }
    command.env("NO_COLOR", "1");
    for (name, value) in env {
        command.env(name, value);
    }
    command
}

/// Fail when the test process sets a control that changes an in-process
/// `fallow_api` result, so a difference cannot come from the environment.
fn assert_clean_test_env() {
    let set: Vec<&str> = RESULT_ENV
        .into_iter()
        .filter(|name| std::env::var_os(name).is_some())
        .collect();
    assert!(
        set.is_empty(),
        "unset {set:?} before you run the surface differential: they change the in-process fallow_api results"
    );
}

/// The `fallow-lsp` binary next to the `fallow` binary.
pub fn lsp_binary() -> PathBuf {
    let path = crate::cargo_profile_dir().join("fallow-lsp");
    assert!(
        path.is_file(),
        "fallow-lsp binary not found at {}. Build it first: cargo build -p fallow-lsp",
        path.display()
    );
    path
}

/// Run the CLI with `--format json` and return its JSON. Exit code 1 means
/// that a gate failed on the findings, which these projects have on purpose.
pub fn cli(root: &Path, args: &[&str], env: Env<'_>) -> Value {
    let output = command(&crate::fallow_binary(), env)
        .args(args)
        .arg("--root")
        .arg(root)
        .args(["--format", "json", "--quiet"])
        .current_dir(root)
        .output()
        .expect("run fallow");
    assert!(
        matches!(output.status.code(), Some(0 | 1)),
        "fallow {args:?} failed with {:?}: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "fallow {args:?} prints JSON ({error}): {}",
            String::from_utf8_lossy(&output.stdout)
        )
    })
}

fn line_reader(stdout: impl Read + Send + 'static) -> Receiver<String> {
    let (sender, lines) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if sender.send(line).is_err() {
                return;
            }
        }
    });
    lines
}

fn stop(child: &mut Child) {
    let deadline = Instant::now() + EXIT_WAIT;
    while Instant::now() < deadline {
        if child.try_wait().ok().flatten().is_some() {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// The `fallow-mcp` binary as a child process, spoken to over the stdio
/// transport's newline-delimited JSON-RPC.
pub struct Mcp {
    child: Child,
    stdin: Option<ChildStdin>,
    lines: Receiver<String>,
    next_id: u64,
}

impl Mcp {
    /// Start the server in `cwd`, which is outside each project.
    pub fn start(cwd: &Path, env: Env<'_>) -> Self {
        let mut child = command(Path::new(env!("CARGO_BIN_EXE_fallow-mcp")), env)
            .env("FALLOW_BIN", crate::fallow_binary())
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn fallow-mcp");
        let stdin = child.stdin.take();
        let lines = line_reader(child.stdout.take().expect("child stdout"));
        let mut server = Self {
            child,
            stdin,
            lines,
            next_id: 1,
        };
        let id = server.request(
            "initialize",
            &json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": { "name": "surface-differential", "version": "0" }
            }),
        );
        let answer = server.response(id);
        assert!(
            answer["result"]["serverInfo"].is_object(),
            "initialize must return server info: {answer}"
        );
        server.send(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }));
        server
    }

    /// The JSON answer of `tool` on `root`. The call must succeed.
    pub fn call(&mut self, tool: &str, root: &Path, arguments: &Value) -> Value {
        let mut arguments = arguments.clone();
        arguments["root"] = json!(root);
        let id = self.request(
            "tools/call",
            &json!({ "name": tool, "arguments": arguments }),
        );
        let answer = self.response(id);
        let result = &answer["result"];
        assert_ne!(result["isError"], true, "`{tool}` must succeed: {answer}");
        let text = result["content"][0]["text"]
            .as_str()
            .unwrap_or_else(|| panic!("`{tool}` answers with text: {answer}"));
        serde_json::from_str(text)
            .unwrap_or_else(|error| panic!("`{tool}` answers with JSON ({error}): {text}"))
    }

    fn request(&mut self, method: &str, params: &Value) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        id
    }

    fn send(&mut self, message: &Value) {
        let stdin = self.stdin.as_mut().expect("server stdin");
        writeln!(stdin, "{message}").expect("write message");
        stdin.flush().expect("flush message");
    }

    fn response(&self, id: u64) -> Value {
        let deadline = Instant::now() + RESPONSE_TIMEOUT;
        loop {
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .unwrap_or_else(|| panic!("no answer to request {id}"));
            let line = self
                .lines
                .recv_timeout(remaining)
                .unwrap_or_else(|_| panic!("no answer to request {id}"));
            let Ok(message) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            if message["id"] == id {
                return message;
            }
        }
    }
}

impl Drop for Mcp {
    fn drop(&mut self) {
        drop(self.stdin.take());
        stop(&mut self.child);
    }
}

fn lsp_reader(stdout: impl Read + Send + 'static) -> Receiver<Value> {
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
            let Ok(message) = serde_json::from_slice::<Value>(&body) else {
                return;
            };
            if sender.send(message).is_err() {
                return;
            }
        }
    });
    receiver
}

fn file_uri(path: &Path) -> String {
    format!("file://{}", path.display())
}

/// A `fallow-lsp` child process. Drop closes its input and stops it.
struct LspServer {
    child: Child,
    stdin: Option<ChildStdin>,
    messages: Receiver<Value>,
}

impl LspServer {
    fn start(cwd: &Path, env: Env<'_>) -> Self {
        let mut child = command(&lsp_binary(), env)
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("start fallow-lsp");
        let messages = lsp_reader(child.stdout.take().expect("server stdout"));
        let stdin = child.stdin.take();
        Self {
            child,
            stdin,
            messages,
        }
    }

    fn send(&mut self, message: &Value) {
        let stdin = self.stdin.as_mut().expect("server stdin");
        let body = message.to_string();
        write!(stdin, "Content-Length: {}\r\n\r\n{body}", body.len()).expect("write message");
        stdin.flush().expect("flush message");
    }

    /// Initialize the session on `root` and open `opened`.
    fn open(&mut self, root: &Path, opened: &Path) {
        self.send(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": { "processId": null, "rootUri": file_uri(root), "capabilities": {} }
        }));
        // The server drops notifications that arrive before it answers
        // `initialize`.
        let answered = std::iter::from_fn(|| self.messages.recv_timeout(RESPONSE_TIMEOUT).ok())
            .any(|message| message["id"] == 1);
        assert!(answered, "the server must answer initialize");
        self.send(&json!({ "jsonrpc": "2.0", "method": "initialized", "params": {} }));
        self.send(&json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": { "textDocument": {
                "uri": file_uri(opened),
                "languageId": "typescript",
                "version": 1,
                "text": std::fs::read_to_string(opened).expect("opened source"),
            } }
        }));
    }

    /// The last published diagnostics of each URI, until the startup
    /// analysis completes and the server has had time to publish its last
    /// diagnostics.
    fn published(&mut self) -> BTreeMap<String, Vec<Value>> {
        let mut published = BTreeMap::new();
        let mut deadline = Instant::now() + RESPONSE_TIMEOUT;
        let mut completed = false;
        while let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
            let Ok(message) = self.messages.recv_timeout(remaining) else {
                break;
            };
            match message["method"].as_str() {
                Some("textDocument/publishDiagnostics") => {
                    let uri = message["params"]["uri"].as_str().unwrap_or_default();
                    let diagnostics = message["params"]["diagnostics"]
                        .as_array()
                        .cloned()
                        .unwrap_or_default();
                    published.insert(uri.to_owned(), diagnostics);
                }
                Some("fallow/analysisComplete") if !completed => {
                    completed = true;
                    deadline = Instant::now() + LSP_SETTLE;
                }
                // The server waits for answers to its own requests, such as
                // `client/registerCapability`.
                Some(_) if !message["id"].is_null() => {
                    self.send(&json!({ "jsonrpc": "2.0", "id": message["id"], "result": null }));
                }
                _ => {}
            }
        }
        assert!(completed, "the server must finish the startup analysis");
        published
    }
}

impl Drop for LspServer {
    fn drop(&mut self) {
        if self.stdin.is_some() {
            self.send(&json!({ "jsonrpc": "2.0", "id": 2, "method": "shutdown", "params": null }));
            self.send(&json!({ "jsonrpc": "2.0", "method": "exit", "params": null }));
        }
        drop(self.stdin.take());
        stop(&mut self.child);
    }
}

/// The diagnostics with a finding id, with paths relative to `root`.
fn finding_diagnostics(
    root: &Path,
    published: BTreeMap<String, Vec<Value>>,
) -> BTreeSet<Diagnostic> {
    let prefix = format!("{}/", file_uri(root));
    let mut diagnostics = BTreeSet::new();
    for (uri, items) in published {
        let path = uri
            .strip_prefix(&prefix)
            .unwrap_or_else(|| panic!("diagnostic URI {uri} is under {prefix}"))
            .to_owned();
        for item in items {
            let Some(finding_id) = item["data"]["findingId"].as_str() else {
                continue;
            };
            let start = &item["range"]["start"];
            diagnostics.insert(Diagnostic {
                finding_id: finding_id.to_owned(),
                path: path.clone(),
                line: start["line"].as_u64().unwrap_or_default(),
                character: start["character"].as_u64().unwrap_or_default(),
            });
        }
    }
    diagnostics
}

/// Run one editor session on `root` with `opened` open, until the startup
/// analysis completes, and stop the server. Returns the diagnostics that
/// carry a finding id. The server process runs in `cwd`, so a relative path
/// in `env` must resolve from the project root and not from the process
/// directory.
pub fn lsp(root: &Path, opened: &str, cwd: &Path, env: Env<'_>) -> BTreeSet<Diagnostic> {
    let mut server = LspServer::start(cwd, env);
    server.open(root, &root.join(opened));
    let published = server.published();
    drop(server);
    finding_diagnostics(root, published)
}

/// The `fallow_api` calls of the Node bindings, in this process.
pub mod api {
    use std::path::Path;

    use fallow_api::{
        AnalysisOptions, DeadCodeOptions, DuplicationOptions, TraceDependencyOptions,
        TraceExportOptions,
    };
    use serde_json::Value;

    fn analysis(root: &Path, changed_since: Option<&str>) -> AnalysisOptions {
        super::assert_clean_test_env();
        AnalysisOptions {
            root: Some(root.to_path_buf()),
            no_cache: true,
            changed_since: changed_since.map(str::to_owned),
            ..AnalysisOptions::default()
        }
    }

    fn ok(result: Result<Value, fallow_api::ProgrammaticError>, call: &str) -> Value {
        result.unwrap_or_else(|error| panic!("{call} failed: {}", error.message))
    }

    pub fn dead_code(root: &Path) -> Value {
        let options = DeadCodeOptions {
            analysis: analysis(root, None),
            ..DeadCodeOptions::default()
        };
        ok(
            fallow_api::run_dead_code(&options)
                .and_then(fallow_api::serialize_dead_code_programmatic_json),
            "run_dead_code",
        )
    }

    pub fn duplication(root: &Path, changed_since: Option<&str>) -> Value {
        let options = DuplicationOptions {
            analysis: analysis(root, changed_since),
            ..DuplicationOptions::default()
        };
        ok(
            fallow_api::run_duplication(&options)
                .and_then(fallow_api::serialize_duplication_programmatic_json),
            "run_duplication",
        )
    }

    pub fn trace_export(root: &Path, file: &str, export_name: &str) -> Value {
        let options = TraceExportOptions {
            analysis: analysis(root, None),
            file: file.to_owned(),
            export_name: export_name.to_owned(),
        };
        ok(
            fallow_api::run_trace_export(&options)
                .and_then(fallow_api::serialize_trace_export_programmatic_json),
            "run_trace_export",
        )
    }

    pub fn trace_dependency(root: &Path, package_name: &str) -> Value {
        let options = TraceDependencyOptions {
            analysis: analysis(root, None),
            package_name: package_name.to_owned(),
            ..TraceDependencyOptions::default()
        };
        ok(
            fallow_api::run_trace_dependency(&options)
                .and_then(fallow_api::serialize_trace_dependency_programmatic_json),
            "run_trace_dependency",
        )
    }
}

/// The environment variable that names the `crates/napi` directory of a
/// build of the Node bindings.
pub const NODE_ADDON_ENV: &str = "FALLOW_SURFACE_NODE_ADDON";

const NODE_CALL: &str = "const binding = require(process.argv[1]);\
const [name, options] = JSON.parse(process.argv[2]);\
binding[name](options).then(\
(result) => process.stdout.write(JSON.stringify(result)),\
(error) => { console.error(error); process.exit(2); });";

/// The built Node bindings, when [`NODE_ADDON_ENV`] is set.
pub fn node_addon() -> Option<PathBuf> {
    let dir = PathBuf::from(std::env::var_os(NODE_ADDON_ENV)?);
    assert!(
        dir.join("index.js").is_file(),
        "{NODE_ADDON_ENV} must name the crates/napi directory of a build"
    );
    Some(dir)
}

/// Call the Node binding `name` with `options`.
pub fn node(addon: &Path, name: &str, options: &Value, cwd: &Path, env: Env<'_>) -> Value {
    let output = command(Path::new("node"), env)
        .current_dir(cwd)
        .arg("-e")
        .arg(NODE_CALL)
        .arg(addon.join("index.js"))
        .arg(json!([name, options]).to_string())
        .output()
        .expect("run node");
    assert!(
        output.status.success(),
        "the Node binding {name} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("the Node binding answers with JSON")
}
