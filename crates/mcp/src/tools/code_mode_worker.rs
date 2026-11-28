//! Run a Code Mode snippet in a snippet process, so that the server can always
//! stop it.
//!
//! The JS engine calls its interrupt handler between bytecode steps, not
//! inside a native builtin. One builtin call, for example `reverse` on an
//! array-like object with a `length` of `2 ** 53 - 1`, can therefore run for
//! years and never see the deadline or a client cancel. A thread cannot be
//! stopped from outside, so the server runs each snippet in a child process:
//! the server binary with [`CODE_MODE_WORKER_FLAG`]. The server kills that
//! process tree when the deadline plus [`WORKER_GRACE`] passes, or when the
//! client cancels the request. The process runs one snippet at a time, each
//! in a new JS runtime, and the server keeps it for the next snippet when the
//! snippet ends normally.
//!
//! The host calls of the snippet still run in the server process. The snippet
//! process sends each host call and each batch to the server on its stdout and
//! reads the result on its stdin. The warm parse store, the abandoned-call
//! count and the kill of a subprocess host call therefore stay in the server,
//! and the snippet process starts no process of its own.
//!
//! Each message is one line of JSON. The snippet process first writes
//! [`WorkerMessage::Ready`]. For each snippet, the server writes one
//! [`WorkerStart`], then one reply for each [`WorkerMessage::Call`] or
//! [`WorkerMessage::Batch`]. The snippet ends with [`WorkerMessage::Done`].
//! Both sides refuse a peer of another protocol version or build version.

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use rustc_hash::FxHashMap;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use crate::params::CodeExecuteParams;

use super::code_mode_tools::CodeModeTool;
use super::{
    BatchOutcome, CANCELLED_BY_CLIENT, HostDispatch, MAX_HOST_CALLS, PendingHostCall,
    SnippetLimits, dispatch_host_call, execute_code_mode_with, refuse_before_run,
    run_batch_dispatches,
};

#[path = "code_mode_heap.rs"]
mod heap;
#[path = "code_mode_worker_process.rs"]
mod process;

use heap::CountingAllocator;
pub(super) use heap::new_engine_runtime;

pub use process::CodeModeWorker;
use process::WorkerProcess;

/// The argument that starts the server binary as a snippet process. It is
/// internal: only the server passes it.
pub const CODE_MODE_WORKER_FLAG: &str = "--code-mode-worker";

/// The version of the line protocol between the server and a snippet process.
const PROTOCOL_VERSION: u32 = 1;

/// The build of this binary. A server and a snippet process of another build
/// refuse each other.
const BUILD_VERSION: &str = env!("CARGO_PKG_VERSION");

/// The end of each error that a restart of the server fixes.
const RESTART_HINT: &str = "restart the MCP server";

/// Time after `timeout_ms` before the server kills the snippet process. The
/// interrupt handler of the snippet process stops a snippet that runs
/// bytecode at `timeout_ms` and answers with its own envelope, which keeps the
/// `calls[]` trace. The kill is for a snippet inside a long builtin call.
const WORKER_GRACE: Duration = Duration::from_secs(2);

const TIMED_OUT: &str = "code mode execution timed out";

/// Engine heap peak of one snippet above which its process ends after the
/// snippet: half of the engine memory limit. A snippet that fills the engine
/// memory passes it, whatever its error text.
const REPLACE_ENGINE_PEAK_BYTES: usize = super::MEMORY_LIMIT_BYTES / 2;

/// How often the watchdog of a snippet process checks its parent and its time
/// limit.
const WATCH_INTERVAL: Duration = Duration::from_millis(100);

/// Exit code of a snippet process that ended itself at its own time limit.
const WORKER_TIMED_OUT_EXIT: i32 = 124;

/// Exit code of a snippet process whose server is gone.
#[cfg(unix)]
const WORKER_ORPHANED_EXIT: i32 = 125;

/// The start of one snippet: the protocol and build of the server, the server
/// folder, and the snippet with its limits.
#[derive(Serialize, Deserialize)]
struct WorkerStart {
    protocol: u32,
    version: String,
    cwd: PathBuf,
    code: String,
    root: Option<String>,
    timeout_ms: Option<u64>,
    max_output_bytes: Option<usize>,
}

/// A message from a snippet process to the server.
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum WorkerMessage {
    /// The first message of a snippet process: its protocol and build.
    Ready { protocol: u32, version: String },
    /// Run one host call. The reply is a `Result<String, String>`.
    Call {
        tool: String,
        params: serde_json::Value,
        max_output_bytes: usize,
    },
    /// Run the dispatches of one `fallow.all` batch. The reply is one
    /// [`WireOutcome`] for each call, in the same order.
    Batch {
        calls: Vec<WireCall>,
        element_output_bytes: usize,
    },
    /// The snippet ended. `output` is the response text. `replace` asks the
    /// server for a new snippet process, because this one holds too much
    /// memory.
    Done {
        ok: bool,
        output: String,
        #[serde(default)]
        replace: bool,
    },
}

/// Why the server stopped a snippet process.
#[derive(Debug, PartialEq, Eq)]
enum Stop {
    TimedOut,
    Cancelled,
    Failed(String),
}

impl Stop {
    fn message(&self) -> &str {
        match self {
            Self::TimedOut => TIMED_OUT,
            Self::Cancelled => CANCELLED_BY_CLIENT,
            Self::Failed(error) => error,
        }
    }
}

/// The error for a peer of another protocol or build.
fn mismatch_message(protocol: u32, version: &str) -> String {
    format!(
        "code mode snippet process and server do not match (protocol {protocol}, version \
         {version}; expected protocol {PROTOCOL_VERSION}, version {BUILD_VERSION}); {RESTART_HINT}"
    )
}

#[derive(Serialize, Deserialize)]
struct WireCall {
    tool: String,
    params: serde_json::Value,
}

#[derive(Serialize, Deserialize)]
struct WireOutcome {
    duration_ms: u64,
    result: Result<String, String>,
    refused: bool,
}

impl WireOutcome {
    fn failed(error: String) -> Self {
        Self {
            duration_ms: 0,
            result: Err(error),
            refused: false,
        }
    }
}

impl From<BatchOutcome> for WireOutcome {
    fn from(outcome: BatchOutcome) -> Self {
        Self {
            duration_ms: u64::try_from(outcome.duration_ms).unwrap_or(u64::MAX),
            result: outcome.result,
            refused: outcome.refused,
        }
    }
}

impl From<WireOutcome> for BatchOutcome {
    fn from(outcome: WireOutcome) -> Self {
        Self {
            duration_ms: u128::from(outcome.duration_ms),
            result: outcome.result,
            refused: outcome.refused,
        }
    }
}

/// Run one Code Mode snippet in a snippet process and answer by the deadline.
///
/// The server kills the snippet process tree when `timeout_ms` plus a short
/// grace time passes, or when the client cancels `request`, also while the
/// snippet is inside a native builtin call. Host calls run in this process,
/// under the deadline of the snippet and the cancel of `request`. Without a
/// `worker`, the snippet does not run.
pub fn execute_code_mode_isolated(
    worker: Option<&CodeModeWorker>,
    binary: String,
    params: CodeExecuteParams,
    request: Option<&CancellationToken>,
) -> Result<String, String> {
    execute_isolated_in(worker, binary, params, request, Path::new("."))
}

fn execute_isolated_in(
    worker: Option<&CodeModeWorker>,
    binary: String,
    params: CodeExecuteParams,
    request: Option<&CancellationToken>,
    cwd: &Path,
) -> Result<String, String> {
    let started = Instant::now();
    let limits = SnippetLimits::new(&params);
    refuse_before_run(&params, &limits, cwd)?;
    let Some(worker) = worker else {
        return Err(limits.error(&format!(
            "code mode could not start the snippet process: the path of the server binary is \
             unknown; {RESTART_HINT}"
        )));
    };
    let deadlines = Deadlines::new(started, Duration::from_millis(limits.timeout_ms));

    let mut process = worker.take().map_err(|error| limits.error(&error))?;
    let start = WorkerStart {
        protocol: PROTOCOL_VERSION,
        version: BUILD_VERSION.to_string(),
        cwd: cwd.to_path_buf(),
        code: params.code,
        root: params.root,
        timeout_ms: params.timeout_ms,
        max_output_bytes: params.max_output_bytes,
    };
    let mut host = ServerHost {
        binary,
        deadline: deadlines.host,
        request,
        max_output_bytes: limits.max_output_bytes,
        dispatched: 0,
    };
    match run_snippet(&mut process, &start, &mut host, deadlines) {
        Ok(done) => {
            if done.replace {
                drop(process);
                worker.refill_in_background();
            } else {
                worker.give_back(process);
            }
            if done.ok {
                Ok(done.output)
            } else {
                Err(done.output)
            }
        }
        Err(stop) => {
            drop(process);
            worker.refill_in_background();
            Err(limits.error(stop.message()))
        }
    }
}

/// The deadlines of one snippet on the server side.
#[derive(Clone, Copy)]
struct Deadlines {
    timeout: Duration,
    /// The deadline of the host calls.
    host: Instant,
    /// When the server kills the snippet process.
    stop_at: Instant,
    /// The latest host deadline after a slow process start.
    latest_host: Instant,
}

impl Deadlines {
    fn new(started: Instant, timeout: Duration) -> Self {
        let host = started + timeout;
        Self {
            timeout,
            host,
            stop_at: host + WORKER_GRACE,
            latest_host: host + WORKER_GRACE,
        }
    }

    /// Start the deadlines again when a new snippet process is ready. The
    /// snippet process starts its own deadline when it reads the start, so a
    /// slow process start does not take the time of the snippet. The shift is
    /// at most one [`WORKER_GRACE`].
    fn restart_at_ready(&mut self, host: &mut ServerHost<'_>) {
        self.host = (Instant::now() + self.timeout).min(self.latest_host);
        self.stop_at = self.host + WORKER_GRACE;
        host.deadline = self.host;
    }
}

/// The `Done` message of a snippet.
struct Done {
    ok: bool,
    output: String,
    /// The snippet process asked for a replacement.
    replace: bool,
}

/// Drive one snippet on `process` until its `Done` message.
fn run_snippet(
    process: &mut WorkerProcess,
    start: &WorkerStart,
    host: &mut ServerHost<'_>,
    mut deadlines: Deadlines,
) -> Result<Done, Stop> {
    let request = host.request;
    process
        .send(start)
        .map_err(|stop| before_ready(process, stop))?;
    loop {
        let message = process
            .receive(deadlines.stop_at, request)
            .map_err(|stop| before_ready(process, stop))?;
        if !process.is_ready() && !matches!(message, WorkerMessage::Ready { .. }) {
            return Err(Stop::Failed(
                "code mode snippet process did not announce its protocol".to_string(),
            ));
        }
        match message {
            WorkerMessage::Ready { protocol, version } => {
                process.accept_ready(protocol, &version)?;
                deadlines.restart_at_ready(host);
            }
            WorkerMessage::Done {
                ok,
                output,
                replace,
            } => {
                return Ok(Done {
                    ok,
                    output,
                    replace,
                });
            }
            WorkerMessage::Call {
                tool,
                params,
                max_output_bytes,
            } => {
                let reply = host.call(&tool, params, max_output_bytes);
                process.send(&reply)?;
            }
            WorkerMessage::Batch {
                calls,
                element_output_bytes,
            } => {
                let reply = host.batch(calls, element_output_bytes);
                process.send(&reply)?;
            }
        }
    }
}

/// A new snippet process that ends before its `ready` line did not start.
/// On macOS, for example, the kernel kills each new process of a signed
/// binary that was written in place while it ran. A restart fixes that.
fn before_ready(process: &WorkerProcess, stop: Stop) -> Stop {
    match stop {
        Stop::Failed(error) if !process.is_ready() && error == process::ENDED_WITHOUT_RESULT => {
            Stop::Failed(format!(
                "code mode snippet process ended before it was ready; {RESTART_HINT}"
            ))
        }
        stop => stop,
    }
}

/// The server side of the host calls of one snippet process. It holds its own
/// host-call budget, so a defect in the snippet process cannot run more host
/// calls than one snippet may.
struct ServerHost<'a> {
    binary: String,
    deadline: Instant,
    request: Option<&'a CancellationToken>,
    max_output_bytes: usize,
    dispatched: usize,
}

impl ServerHost<'_> {
    fn call(
        &mut self,
        tool: &str,
        params: serde_json::Value,
        max_output_bytes: usize,
    ) -> Result<String, String> {
        let tool = CodeModeTool::from_name(tool)?;
        self.spend(1)?;
        dispatch_host_call(
            &self.binary,
            tool,
            params,
            self.deadline,
            self.request,
            max_output_bytes.min(self.max_output_bytes),
        )
    }

    fn batch(&mut self, calls: Vec<WireCall>, element_output_bytes: usize) -> Vec<WireOutcome> {
        if let Err(error) = self.spend(calls.len()) {
            return calls
                .iter()
                .map(|_| WireOutcome::failed(error.clone()))
                .collect();
        }
        let mut outcomes: Vec<Option<WireOutcome>> = Vec::with_capacity(calls.len());
        let mut pending = Vec::with_capacity(calls.len());
        for (index, call) in calls.into_iter().enumerate() {
            match CodeModeTool::from_name(&call.tool) {
                Ok(tool) => {
                    pending.push(PendingHostCall {
                        key: index.to_string(),
                        tool,
                        params: call.params,
                    });
                    outcomes.push(None);
                }
                Err(error) => outcomes.push(Some(WireOutcome::failed(error))),
            }
        }
        let mut finished = run_batch_dispatches(
            &self.binary,
            self.deadline,
            self.request,
            element_output_bytes.min(self.max_output_bytes),
            pending,
        );
        outcomes
            .into_iter()
            .enumerate()
            .map(|(index, outcome)| {
                outcome
                    .or_else(|| finished.remove(&index.to_string()).map(WireOutcome::from))
                    .unwrap_or_else(|| {
                        WireOutcome::failed("code mode batch host call did not complete".into())
                    })
            })
            .collect()
    }

    fn spend(&mut self, calls: usize) -> Result<(), String> {
        if self.dispatched + calls > MAX_HOST_CALLS {
            return Err(format!(
                "code mode host call limit exceeded ({MAX_HOST_CALLS})"
            ));
        }
        self.dispatched += calls;
        Ok(())
    }
}

/// The snippet-process side of the host calls: requests go out on stdout, and
/// replies come in on stdin.
pub(super) struct ServerLink {
    input: std::io::Stdin,
}

impl ServerLink {
    pub(super) fn call(
        &self,
        tool: CodeModeTool,
        params: serde_json::Value,
        max_output_bytes: usize,
    ) -> Result<String, String> {
        send_to_server(&WorkerMessage::Call {
            tool: tool.name().to_string(),
            params,
            max_output_bytes,
        })?;
        self.receive::<Result<String, String>>()?
    }

    pub(super) fn batch(
        &self,
        element_output_bytes: usize,
        pending: Vec<PendingHostCall>,
    ) -> FxHashMap<String, BatchOutcome> {
        let mut keys = Vec::with_capacity(pending.len());
        let mut calls = Vec::with_capacity(pending.len());
        for call in pending {
            keys.push(call.key);
            calls.push(WireCall {
                tool: call.tool.name().to_string(),
                params: call.params,
            });
        }
        let reply = send_to_server(&WorkerMessage::Batch {
            calls,
            element_output_bytes,
        })
        .and_then(|()| self.receive::<Vec<WireOutcome>>());
        let error = match reply {
            Ok(outcomes) if outcomes.len() == keys.len() => {
                return keys
                    .into_iter()
                    .zip(outcomes.into_iter().map(BatchOutcome::from))
                    .collect();
            }
            Ok(outcomes) => format!(
                "code mode server sent {} batch results for {} calls",
                outcomes.len(),
                keys.len()
            ),
            Err(error) => error,
        };
        keys.into_iter()
            .map(|key| (key, WireOutcome::failed(error.clone()).into()))
            .collect()
    }

    fn receive<T: DeserializeOwned>(&self) -> Result<T, String> {
        let mut line = String::new();
        let read = read_line(&self.input, &mut line);
        match read {
            Ok(0) => Err("code mode server closed the snippet process input".to_string()),
            Ok(_) => serde_json::from_str(&line)
                .map_err(|err| format!("code mode server sent an invalid reply: {err}")),
            Err(err) => Err(format!("failed to read a code mode server reply: {err}")),
        }
    }
}

fn send_to_server(message: &WorkerMessage) -> Result<(), String> {
    let mut stdout = std::io::stdout().lock();
    serde_json::to_writer(&mut stdout, message)
        .map_err(|err| err.to_string())
        .and_then(|()| {
            stdout
                .write_all(b"\n")
                .and_then(|()| stdout.flush())
                .map_err(|err| err.to_string())
        })
        .map_err(|err| format!("failed to write to the code mode server: {err}"))
}

/// The entry of a snippet process: announce the protocol and build, then run
/// one snippet for each [`WorkerStart`] on stdin, each in a new JS runtime,
/// until stdin closes.
pub fn run_code_mode_worker() -> ExitCode {
    let watchdog = Watchdog::start();
    let input = std::io::stdin();
    let ready = WorkerMessage::Ready {
        protocol: PROTOCOL_VERSION,
        version: BUILD_VERSION.to_string(),
    };
    if send_to_server(&ready).is_err() {
        return ExitCode::FAILURE;
    }
    loop {
        let mut line = String::new();
        let read = read_line(&input, &mut line);
        match read {
            Ok(0) => return ExitCode::SUCCESS,
            Ok(_) => {}
            Err(_) => return ExitCode::FAILURE,
        }
        let start = match serde_json::from_str::<WorkerStart>(&line) {
            Ok(start) if start.protocol == PROTOCOL_VERSION && start.version == BUILD_VERSION => {
                start
            }
            Ok(start) => return refuse_start(mismatch_message(start.protocol, &start.version)),
            Err(err) => {
                return refuse_start(format!(
                    "code mode snippet process got an invalid start ({err}); {RESTART_HINT}"
                ));
            }
        };
        let params = CodeExecuteParams {
            code: start.code,
            root: start.root,
            timeout_ms: start.timeout_ms,
            max_output_bytes: start.max_output_bytes,
        };
        watchdog.busy_for(SnippetLimits::new(&params).timeout_ms);
        CountingAllocator::reset_peak();
        let result = execute_code_mode_with(
            HostDispatch::Server(ServerLink {
                input: std::io::stdin(),
            }),
            params,
            None,
            &start.cwd,
        );
        watchdog.idle();
        let replace = needs_replacement(CountingAllocator::peak());
        let (ok, output) = match result {
            Ok(output) => (true, output),
            Err(output) => (false, output),
        };
        if send_to_server(&WorkerMessage::Done {
            ok,
            output,
            replace,
        })
        .is_err()
        {
            return ExitCode::FAILURE;
        }
    }
}

/// Read one line of `input`. The lock of the shared stdin buffer ends with
/// the call.
fn read_line(input: &std::io::Stdin, line: &mut String) -> std::io::Result<usize> {
    input.lock().read_line(line)
}

/// Whether this snippet process should end after a snippet whose engine
/// heap peaked at `peak` bytes. The allocator keeps the pages of a large
/// heap, so an idle process would hold them until the next restart.
fn needs_replacement(peak: usize) -> bool {
    peak > REPLACE_ENGINE_PEAK_BYTES
}

fn refuse_start(error: String) -> ExitCode {
    let _ = send_to_server(&WorkerMessage::Done {
        ok: false,
        output: error,
        replace: true,
    });
    ExitCode::from(2)
}

/// Ends the snippet process when its server is gone, or when one snippet runs
/// past its time limit plus twice [`WORKER_GRACE`] and no server kills it.
struct Watchdog {
    started: Instant,
    /// The end of the time limit of the running snippet, in milliseconds
    /// after `started`. Zero while no snippet runs.
    busy_until_ms: Arc<AtomicU64>,
}

impl Watchdog {
    fn start() -> Self {
        exit_with_parent();
        let started = Instant::now();
        let busy_until_ms = Arc::new(AtomicU64::new(0));
        let watched = Arc::clone(&busy_until_ms);
        #[cfg(unix)]
        let parent = std::os::unix::process::parent_id();
        let watch = move || {
            loop {
                thread::sleep(WATCH_INTERVAL);
                // A process whose parent ended gets a new parent. A job object
                // stops the process on Windows instead.
                #[cfg(unix)]
                if std::os::unix::process::parent_id() != parent {
                    exit_now(WORKER_ORPHANED_EXIT);
                }
                let until = watched.load(Ordering::Relaxed);
                if until != 0 && elapsed_ms(started) > until {
                    exit_now(WORKER_TIMED_OUT_EXIT);
                }
            }
        };
        if let Err(error) = thread::Builder::new()
            .name("code-mode-watchdog".to_string())
            .spawn(watch)
        {
            tracing::warn!(%error, "code mode snippet process has no watchdog");
        }
        Self {
            started,
            busy_until_ms,
        }
    }

    fn busy_for(&self, timeout_ms: u64) {
        let limit = Duration::from_millis(timeout_ms) + 2 * WORKER_GRACE;
        let limit_ms = u64::try_from(limit.as_millis()).unwrap_or(u64::MAX);
        let until = elapsed_ms(self.started).saturating_add(limit_ms).max(1);
        self.busy_until_ms.store(until, Ordering::Relaxed);
    }

    fn idle(&self) {
        self.busy_until_ms.store(0, Ordering::Relaxed);
    }
}

fn elapsed_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

#[expect(
    clippy::exit,
    reason = "the snippet process must end even while its main thread is inside a builtin call"
)]
fn exit_now(code: i32) -> ! {
    std::process::exit(code)
}

/// Ask Linux to kill this snippet process when the thread that started it
/// ends. The server starts each snippet process on one thread that lives as
/// long as the server.
#[cfg(target_os = "linux")]
fn exit_with_parent() {
    let signal = libc::c_ulong::from(libc::SIGKILL.unsigned_abs());
    #[expect(unsafe_code, reason = "prctl has no safe wrapper in std")]
    // SAFETY: PR_SET_PDEATHSIG reads one signal number and no memory.
    let _ = unsafe { libc::prctl(libc::PR_SET_PDEATHSIG, signal) };
}

#[cfg(not(target_os = "linux"))]
fn exit_with_parent() {}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(code: &str, timeout_ms: u64) -> CodeExecuteParams {
        CodeExecuteParams {
            code: code.to_string(),
            root: None,
            timeout_ms: Some(timeout_ms),
            max_output_bytes: None,
        }
    }

    fn error_of(output: &str) -> String {
        let envelope: serde_json::Value = serde_json::from_str(output).expect("envelope json");
        envelope["error"].as_str().expect("error text").to_string()
    }

    fn run(
        worker: &CodeModeWorker,
        code: &str,
        timeout_ms: u64,
        cwd: &Path,
    ) -> Result<String, String> {
        execute_isolated_in(
            Some(worker),
            "fallow".to_string(),
            params(code, timeout_ms),
            None,
            cwd,
        )
    }

    /// The built `fallow-mcp` binary. `cargo test -p fallow-mcp` builds it for
    /// the integration tests.
    fn server_binary() -> PathBuf {
        let exe = std::env::current_exe().expect("test binary path");
        let dir = exe.parent().expect("test binary directory");
        let mut path = dir
            .ancestors()
            .find(|ancestor| ancestor.ends_with("deps") || ancestor.ends_with("build"))
            .and_then(Path::parent)
            .unwrap_or(dir)
            .join("fallow-mcp");
        if cfg!(windows) {
            path.set_extension("exe");
        }
        assert!(
            path.is_file(),
            "fallow-mcp binary not found at {path:?}. Build it first: cargo build -p fallow-mcp"
        );
        path
    }

    fn host_with_spent_budget() -> ServerHost<'static> {
        ServerHost {
            binary: "/definitely/not/fallow".to_string(),
            deadline: Instant::now() + Duration::from_secs(30),
            request: None,
            max_output_bytes: 1_000_000,
            dispatched: MAX_HOST_CALLS,
        }
    }

    #[test]
    fn the_server_keeps_its_own_host_call_budget() {
        let mut host = host_with_spent_budget();
        assert_eq!(
            host.call("project_info", serde_json::json!({}), 1_000),
            Err(format!(
                "code mode host call limit exceeded ({MAX_HOST_CALLS})"
            ))
        );
        let outcomes = host.batch(
            vec![WireCall {
                tool: "project_info".to_string(),
                params: serde_json::json!({}),
            }],
            1_000,
        );
        assert_eq!(outcomes.len(), 1);
        assert!(outcomes[0].result.is_err());
    }

    #[test]
    fn the_server_refuses_an_unknown_tool_from_the_snippet_process() {
        let mut host = host_with_spent_budget();
        host.dispatched = 0;
        let error = host
            .call("fix_apply", serde_json::json!({}), 1_000)
            .expect_err("a tool outside the allowlist must not run");
        assert!(!error.is_empty());
        assert_eq!(host.dispatched, 0, "a refused tool must spend no slot");
    }

    #[test]
    fn a_snippet_does_not_run_without_a_snippet_process() {
        let cwd = tempfile::tempdir().expect("tempdir");
        let output = execute_isolated_in(
            None,
            "fallow".to_string(),
            params("return 1;", 1_000),
            None,
            cwd.path(),
        )
        .expect_err("no snippet process means no run");
        assert!(error_of(&output).contains("could not start the snippet process"));
        assert!(error_of(&output).contains(RESTART_HINT));
    }

    /// A normal snippet must not pay a process start or a process cleanup.
    #[test]
    fn a_finished_snippet_keeps_its_process_for_the_next_snippet() {
        let cwd = tempfile::tempdir().expect("tempdir");
        let worker = CodeModeWorker::server_binary(server_binary());
        let first = run(&worker, "return 1 + 1;", 5_000, cwd.path()).expect("first snippet");
        assert!(first.contains(r#""result":2"#), "{first}");
        let pid = worker
            .idle_pid()
            .expect("the process stays idle in the pool");

        let error = run(&worker, "throw new Error('boom');", 5_000, cwd.path())
            .expect_err("a thrown error is a failed snippet");
        assert!(error_of(&error).contains("boom"), "{error}");
        assert_eq!(
            worker.idle_pid(),
            Some(pid),
            "a failed snippet keeps the process"
        );

        let second = run(
            &worker,
            "return typeof globalThis.leftover;",
            5_000,
            cwd.path(),
        )
        .expect("second snippet");
        assert!(second.contains(r#""result":"undefined""#), "{second}");
        assert_eq!(
            worker.idle_pid(),
            Some(pid),
            "the next snippet reuses the process"
        );
    }

    /// A new JS runtime per snippet: a global of one snippet is gone in the
    /// next one.
    #[test]
    fn a_snippet_does_not_see_the_globals_of_the_previous_snippet() {
        let cwd = tempfile::tempdir().expect("tempdir");
        let worker = CodeModeWorker::server_binary(server_binary());
        run(
            &worker,
            "globalThis.leftover = 1; return 0;",
            5_000,
            cwd.path(),
        )
        .expect("first snippet");
        let second = run(
            &worker,
            "return typeof globalThis.leftover;",
            5_000,
            cwd.path(),
        )
        .expect("second snippet");
        assert!(second.contains(r#""result":"undefined""#), "{second}");
    }

    #[test]
    fn a_removed_server_binary_is_refused_with_a_restart_hint() {
        let dir = tempfile::tempdir().expect("tempdir");
        let program = dir.path().join("server");
        std::fs::write(&program, "a").expect("write the binary");
        let worker = CodeModeWorker::with_args(program.clone(), &[]);
        std::fs::remove_file(&program).expect("remove the binary");

        let error = run(&worker, "return 1;", 5_000, dir.path()).expect_err("no binary");
        assert!(error_of(&error).contains(RESTART_HINT), "{error}");
    }

    /// A reinstall of the same build touches or replaces the binary file. A
    /// snippet process of that file still matches the server.
    #[cfg(unix)]
    #[test]
    fn a_touched_or_replaced_identical_server_binary_still_runs_snippets() {
        let dir = tempfile::tempdir().expect("tempdir");
        let program = dir.path().join("fallow-mcp");
        std::fs::copy(server_binary(), &program).expect("copy the binary");
        let worker = CodeModeWorker::server_binary(program.clone());
        run(&worker, "return 1;", 5_000, dir.path()).expect("first snippet");

        worker.discard_idle();
        // `touch` changes the times without a write to the file.
        std::fs::File::open(&program)
            .and_then(|file| file.set_modified(std::time::SystemTime::now()))
            .expect("touch the binary");
        run(&worker, "return 2;", 5_000, dir.path()).expect("snippet after a touch");

        worker.discard_idle();
        let copy = dir.path().join("fallow-mcp.new");
        std::fs::copy(server_binary(), &copy).expect("copy the binary again");
        std::fs::rename(&copy, &program).expect("replace the binary");
        run(&worker, "return 3;", 5_000, dir.path()).expect("snippet after a replace");
    }

    /// The allocator keeps the pages of a snippet that filled the engine
    /// memory, so the server replaces its process. The error text of such a
    /// snippet can be `out of memory` or `null`.
    #[test]
    fn a_snippet_that_fills_the_engine_memory_replaces_its_process() {
        let cwd = tempfile::tempdir().expect("tempdir");
        let worker = CodeModeWorker::server_binary(server_binary());
        run(&worker, "return 1;", 5_000, cwd.path()).expect("first snippet");
        let pid = worker.idle_pid().expect("an idle process");

        let code = "const a = []; for (;;) a.push('x'.repeat(1024) + a.length);";
        run(&worker, code, 5_000, cwd.path()).expect_err("the snippet runs out of memory");
        assert_ne!(
            worker.idle_pid(),
            Some(pid),
            "the process must not stay idle"
        );
        run(&worker, "return 1;", 5_000, cwd.path()).expect("a new process runs the next snippet");
    }

    #[test]
    fn a_snippet_with_a_large_heap_replaces_its_process() {
        let cwd = tempfile::tempdir().expect("tempdir");
        let worker = CodeModeWorker::server_binary(server_binary());
        run(&worker, "return 1;", 5_000, cwd.path()).expect("first snippet");
        let pid = worker.idle_pid().expect("an idle process");

        let code = "return 'x'.repeat(20 * 1024 * 1024).length;";
        run(&worker, code, 5_000, cwd.path()).expect("the large snippet fits the limit");
        assert_ne!(
            worker.idle_pid(),
            Some(pid),
            "the process must not stay idle"
        );
    }

    /// The decision uses the engine heap of the snippet, not the memory of the
    /// process. On Linux a new process starts with the peak memory of its
    /// parent in `ru_maxrss`, so a large server must not make each snippet
    /// replace its process.
    #[test]
    fn a_server_with_much_memory_keeps_its_snippet_process() {
        const SERVER_MEMORY: usize = 64 * 1024 * 1024;
        let mut held = vec![0_u8; SERVER_MEMORY];
        for page in held.chunks_mut(4096) {
            page[0] = 1;
        }
        let cwd = tempfile::tempdir().expect("tempdir");
        let worker = CodeModeWorker::server_binary(server_binary());
        run(&worker, "return 1;", 5_000, cwd.path()).expect("first snippet");
        let pid = worker.idle_pid().expect("an idle process");
        run(&worker, "return 2;", 5_000, cwd.path()).expect("second snippet");
        assert_eq!(
            worker.idle_pid(),
            Some(pid),
            "a small snippet keeps its process"
        );
        assert_eq!(std::hint::black_box(&held).len(), SERVER_MEMORY);
    }

    #[test]
    fn the_first_snippet_starts_the_process() {
        let cwd = tempfile::tempdir().expect("tempdir");
        let worker = CodeModeWorker::server_binary(server_binary());
        assert_eq!(
            worker.idle_pid(),
            None,
            "no process before the first snippet"
        );
        run(&worker, "return 1;", 5_000, cwd.path()).expect("first snippet");
        assert!(
            worker.idle_pid().is_some(),
            "the process stays idle after the snippet"
        );
    }

    /// The deadline of the server starts when a new snippet process is ready,
    /// so a slow process start does not take the time of the snippet.
    #[cfg(unix)]
    #[test]
    fn a_slow_process_start_does_not_take_the_time_of_the_snippet() {
        let cwd = tempfile::tempdir().expect("tempdir");
        let ready = format!(
            r#"{{"kind":"ready","protocol":{PROTOCOL_VERSION},"version":"{BUILD_VERSION}"}}"#
        );
        let done = r#"{"kind":"done","ok":false,"output":"own envelope"}"#;
        let script = format!(
            "sleep 2.2; printf '%s\\n' '{ready}'; read start; sleep 0.4; \
             printf '%s\\n' '{done}'; sleep 10"
        );
        let worker = CodeModeWorker::with_args(PathBuf::from("/bin/sh"), &["-c", &script]);
        let error = run(&worker, "return 1;", 500, cwd.path()).expect_err("the snippet fails");
        assert_eq!(error, "own envelope");
    }

    #[cfg(unix)]
    #[test]
    fn a_snippet_process_of_another_build_is_refused_with_a_restart_hint() {
        let cwd = tempfile::tempdir().expect("tempdir");
        let worker = CodeModeWorker::with_args(
            PathBuf::from("/bin/sh"),
            &[
                "-c",
                r#"printf '%s\n' '{"kind":"ready","protocol":1,"version":"0.0.0-other"}'; sleep 10"#,
            ],
        );
        let error = run(&worker, "return 1;", 5_000, cwd.path()).expect_err("other build");
        assert!(error_of(&error).contains("do not match"), "{error}");
        assert!(error_of(&error).contains(RESTART_HINT), "{error}");

        let ready = format!(
            r#"{{"kind":"ready","protocol":{},"version":"{BUILD_VERSION}"}}"#,
            PROTOCOL_VERSION + 1
        );
        let script = format!("printf '%s\\n' '{ready}'; sleep 10");
        let worker = CodeModeWorker::with_args(PathBuf::from("/bin/sh"), &["-c", &script]);
        let error = run(&worker, "return 1;", 5_000, cwd.path()).expect_err("other protocol");
        assert!(error_of(&error).contains("do not match"), "{error}");
        assert!(error_of(&error).contains(RESTART_HINT), "{error}");
    }

    /// A process that does not read its stdin must not hold the server past
    /// the deadline on a large reply.
    #[cfg(unix)]
    #[test]
    fn a_write_to_a_process_that_does_not_read_ends_at_the_deadline() {
        let worker = CodeModeWorker::with_args(PathBuf::from("/bin/sh"), &["-c", "sleep 10"]);
        let process = worker.take().expect("spawn");
        let reply: Result<String, String> = Ok("x".repeat(4_000_000));
        let started = Instant::now();
        process.send(&reply).expect("the reply is queued");
        process.send(&reply).expect("the second reply is queued");
        let answer = process.receive(started + Duration::from_millis(300), None);
        assert!(matches!(answer, Err(Stop::TimedOut)));
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "the wait ended after {:?}",
            started.elapsed()
        );
    }

    /// A snippet process that never answers stands for a snippet inside a
    /// long builtin call. The server must kill it and answer.
    #[cfg(unix)]
    #[test]
    fn a_silent_snippet_process_is_killed_at_the_deadline() {
        let cwd = tempfile::tempdir().expect("tempdir");
        let worker = CodeModeWorker::with_args(PathBuf::from("/bin/sh"), &["-c", "sleep 10"]);
        let started = Instant::now();
        let output = run(&worker, "return 1;", 200, cwd.path())
            .expect_err("the silent snippet must time out");
        assert_eq!(error_of(&output), TIMED_OUT);
        assert!(
            started.elapsed() < Duration::from_millis(200) + WORKER_GRACE + Duration::from_secs(3),
            "answered after {:?}",
            started.elapsed()
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_client_cancel_kills_the_snippet_process() {
        let cwd = tempfile::tempdir().expect("tempdir");
        let worker = CodeModeWorker::with_args(PathBuf::from("/bin/sh"), &["-c", "sleep 10"]);
        let request = CancellationToken::new();
        let cancel = request.clone();
        let canceller = thread::spawn(move || {
            thread::sleep(Duration::from_millis(100));
            cancel.cancel();
        });
        let started = Instant::now();
        let output = execute_isolated_in(
            Some(&worker),
            "fallow".to_string(),
            params("return 1;", 30_000),
            Some(&request),
            cwd.path(),
        )
        .expect_err("the cancel must stop the snippet");
        canceller.join().expect("canceller");
        assert_eq!(error_of(&output), CANCELLED_BY_CLIENT);
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "the cancel, not the deadline, must end the wait: {:?}",
            started.elapsed()
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_snippet_process_that_exits_without_a_result_is_an_error() {
        let cwd = tempfile::tempdir().expect("tempdir");
        let worker = CodeModeWorker::with_args(PathBuf::from("/bin/sh"), &["-c", "exit 3"]);
        let output = run(&worker, "return 1;", 5_000, cwd.path()).expect_err("no result");
        assert_eq!(
            error_of(&output),
            format!("code mode snippet process ended before it was ready; {RESTART_HINT}")
        );
    }
}
