//! Run a Code Mode snippet in its own process, so that the server can always
//! stop it.
//!
//! The JS engine calls its interrupt handler between bytecode steps, not
//! inside a native builtin. One builtin call, for example `reverse` on an
//! array-like object with a `length` of `2 ** 53 - 1`, can therefore run for
//! years and never see the deadline or a client cancel. A thread cannot be
//! stopped from outside, so the server starts each snippet in a child process:
//! the server binary with [`CODE_MODE_WORKER_FLAG`]. The server kills that
//! process tree when the deadline plus [`WORKER_GRACE`] passes, or when the
//! client cancels the request.
//!
//! The host calls of the snippet still run in the server process. The snippet
//! process sends each host call and each batch to the server on its stdout and
//! reads the result on its stdin. The warm parse store, the abandoned-call
//! count and the kill of a subprocess host call therefore stay in the server,
//! and the snippet process starts no process of its own.
//!
//! Each message is one line of JSON. The server writes one [`WorkerStart`],
//! then one reply for each [`WorkerMessage::Call`] or [`WorkerMessage::Batch`].
//! The last message of the snippet process is [`WorkerMessage::Done`].

use std::ffi::OsString;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, ExitCode, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant};

use fallow_process::{
    ChildCleanup, ProcessTree, cleanup_std_child, configure_std_command,
    spawn_retrying_busy_executable,
};
use rustc_hash::FxHashMap;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use crate::params::CodeExecuteParams;

use super::code_mode_tools::CodeModeTool;
use super::{
    BatchOutcome, CANCEL_POLL_INTERVAL, CANCELLED_BY_CLIENT, HostDispatch, MAX_HOST_CALLS,
    PendingHostCall, SnippetLimits, dispatch_host_call, execute_code_mode_with, is_cancelled,
    refuse_before_run, run_batch_dispatches,
};

/// The argument that starts the server binary as a snippet process. It is
/// internal: only the server passes it.
pub const CODE_MODE_WORKER_FLAG: &str = "--code-mode-worker";

/// The token that routes the multicall `fallow` binary to the MCP server
/// entry. The standalone `fallow-mcp` binary ignores it.
const MCP_SERVER_SUBCOMMAND: &str = "mcp-server";

/// Time after `timeout_ms` before the server kills the snippet process. The
/// interrupt handler of the snippet process stops a snippet that runs
/// bytecode at `timeout_ms` and answers with its own envelope, which keeps the
/// `calls[]` trace. The kill is for a snippet inside a long builtin call.
const WORKER_GRACE: Duration = Duration::from_secs(2);

/// Longest message line that the server reads from a snippet process. The
/// final envelope is bounded by `max_output_bytes` and the trace limits; this
/// cap only keeps a defect in the snippet process from growing the server
/// memory without limit.
const MAX_WORKER_LINE_BYTES: u64 = 64 * 1024 * 1024;

const TIMED_OUT: &str = "code mode execution timed out";

/// Exit code of a snippet process that ended itself at its own time limit.
const WORKER_TIMED_OUT_EXIT: i32 = 124;

/// How to start a snippet process.
#[derive(Clone, Debug)]
pub struct CodeModeWorker {
    program: PathBuf,
    args: Vec<OsString>,
}

impl CodeModeWorker {
    /// The running server binary as the snippet process, or `None` when the
    /// path of the running binary is not available.
    pub fn current() -> Option<Self> {
        std::env::current_exe().ok().map(Self::server_binary)
    }

    /// `program` as a server binary: `fallow-mcp`, or the multicall `fallow`.
    pub fn server_binary(program: PathBuf) -> Self {
        Self {
            program,
            args: vec![
                OsString::from(MCP_SERVER_SUBCOMMAND),
                OsString::from(CODE_MODE_WORKER_FLAG),
            ],
        }
    }

    #[cfg(all(test, unix))]
    fn with_args(program: PathBuf, args: &[&str]) -> Self {
        Self {
            program,
            args: args.iter().map(OsString::from).collect(),
        }
    }
}

/// The first message to a snippet process: the snippet and its limits.
#[derive(Serialize, Deserialize)]
struct WorkerStart {
    code: String,
    root: Option<String>,
    timeout_ms: Option<u64>,
    max_output_bytes: Option<usize>,
}

/// A message from a snippet process to the server.
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum WorkerMessage {
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
    /// The snippet ended. `output` is the response text.
    Done { ok: bool, output: String },
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
        return Err(limits.error(
            "code mode could not start the snippet process: the path of the server binary is unknown",
        ));
    };
    let host_deadline = started + Duration::from_millis(limits.timeout_ms);
    let stop_at = host_deadline + WORKER_GRACE;

    let mut run = WorkerRun::spawn(worker, cwd).map_err(|error| limits.error(&error))?;
    let start = WorkerStart {
        code: params.code,
        root: params.root,
        timeout_ms: params.timeout_ms,
        max_output_bytes: params.max_output_bytes,
    };
    if let Err(error) = run.send(&start) {
        return Err(run.stop(&limits, &error));
    }
    let mut host = ServerHost {
        binary,
        deadline: host_deadline,
        request,
        max_output_bytes: limits.max_output_bytes,
        dispatched: 0,
    };

    loop {
        if is_cancelled(request) {
            return Err(run.stop(&limits, CANCELLED_BY_CLIENT));
        }
        let left = stop_at.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(run.stop(&limits, TIMED_OUT));
        }
        let message = match run.messages.recv_timeout(left.min(CANCEL_POLL_INTERVAL)) {
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => {
                return Err(run.stop(&limits, "code mode snippet process ended without a result"));
            }
            Ok(Err(error)) => return Err(run.stop(&limits, &error)),
            Ok(Ok(message)) => message,
        };
        let sent = match message {
            WorkerMessage::Done { ok, output } => {
                run.finish();
                return if ok { Ok(output) } else { Err(output) };
            }
            WorkerMessage::Call {
                tool,
                params,
                max_output_bytes,
            } => run.send(&host.call(&tool, params, max_output_bytes)),
            WorkerMessage::Batch {
                calls,
                element_output_bytes,
            } => run.send(&host.batch(calls, element_output_bytes)),
        };
        if let Err(error) = sent {
            return Err(run.stop(&limits, &error));
        }
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

/// One running snippet process. Drop kills its process tree.
struct WorkerRun {
    child: Child,
    tree: ProcessTree,
    stdin: Option<ChildStdin>,
    messages: Receiver<Result<WorkerMessage, String>>,
    cleaned: bool,
}

impl WorkerRun {
    fn spawn(worker: &CodeModeWorker, cwd: &Path) -> Result<Self, String> {
        let mut command = Command::new(&worker.program);
        // The snippet process speaks only the line protocol on stdin and
        // stdout. Its stderr would reach the MCP client log, so it is closed.
        command
            .args(&worker.args)
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        configure_std_command(&mut command);
        let mut child = spawn_retrying_busy_executable(&mut command).map_err(|err| {
            format!(
                "code mode could not start the snippet process '{}': {err}",
                worker.program.display()
            )
        })?;
        let tree = match ProcessTree::for_std_child(&child) {
            Ok(tree) => tree,
            Err(error) => {
                let cleanup = cleanup_std_child(None, &mut child);
                log_cleanup_errors(&cleanup);
                return Err(format!(
                    "code mode could not track the snippet process: {error}"
                ));
            }
        };
        let stdin = child.stdin.take();
        let Some(stdout) = child.stdout.take() else {
            let cleanup = cleanup_std_child(Some(&tree), &mut child);
            log_cleanup_errors(&cleanup);
            return Err("code mode snippet process has no stdout".to_string());
        };
        let (sender, messages) = mpsc::channel();
        thread::spawn(move || read_messages(stdout, &sender));
        Ok(Self {
            child,
            tree,
            stdin,
            messages,
            cleaned: false,
        })
    }

    fn send<T: Serialize>(&mut self, message: &T) -> Result<(), String> {
        let stdin = self
            .stdin
            .as_mut()
            .ok_or("code mode snippet process input is closed")?;
        let mut line = serde_json::to_vec(message)
            .map_err(|err| format!("failed to encode a code mode message: {err}"))?;
        line.push(b'\n');
        stdin
            .write_all(&line)
            .and_then(|()| stdin.flush())
            .map_err(|err| format!("failed to write to the code mode snippet process: {err}"))
    }

    /// Kill the process tree and build the error envelope. Cleanup errors go
    /// to the log, so the error keeps its exact wording.
    fn stop(mut self, limits: &SnippetLimits, error: &str) -> String {
        self.finish();
        limits.error(error)
    }

    fn finish(&mut self) {
        if self.cleaned {
            return;
        }
        self.cleaned = true;
        self.stdin = None;
        let cleanup = cleanup_std_child(Some(&self.tree), &mut self.child);
        log_cleanup_errors(&cleanup);
    }
}

impl Drop for WorkerRun {
    fn drop(&mut self) {
        self.finish();
    }
}

fn log_cleanup_errors(cleanup: &ChildCleanup) {
    if cleanup.errors.is_empty() {
        return;
    }
    tracing::warn!(
        errors = cleanup.errors.join("; "),
        "code mode snippet process cleanup reported errors"
    );
}

/// Read message lines from a snippet process until its stdout closes.
fn read_messages(stdout: ChildStdout, sender: &Sender<Result<WorkerMessage, String>>) {
    let mut reader = BufReader::new(stdout);
    loop {
        let mut line = Vec::new();
        let read = (&mut reader)
            .take(MAX_WORKER_LINE_BYTES + 1)
            .read_until(b'\n', &mut line);
        let message = match read {
            Ok(0) => return,
            Ok(_) if line.len() as u64 > MAX_WORKER_LINE_BYTES => Err(format!(
                "code mode snippet process sent a message over {MAX_WORKER_LINE_BYTES} bytes"
            )),
            Ok(_) => serde_json::from_slice(&line)
                .map_err(|err| format!("code mode snippet process sent an invalid message: {err}")),
            Err(err) => Err(format!(
                "failed to read from the code mode snippet process: {err}"
            )),
        };
        let stop = message.is_err();
        if sender.send(message).is_err() || stop {
            return;
        }
    }
}

/// The snippet-process side of the host calls: requests go out on stdout, and
/// replies come in on stdin.
pub(super) struct ServerLink {
    input: Box<dyn BufRead>,
}

impl ServerLink {
    pub(super) fn call(
        &mut self,
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
        &mut self,
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

    fn receive<T: DeserializeOwned>(&mut self) -> Result<T, String> {
        let mut line = String::new();
        match self.input.read_line(&mut line) {
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

/// The entry of a snippet process: read one [`WorkerStart`] on stdin, run the
/// snippet, and write [`WorkerMessage::Done`] on stdout.
pub fn run_code_mode_worker() -> ExitCode {
    let mut input: Box<dyn BufRead> = Box::new(std::io::stdin().lock());
    let mut line = String::new();
    let start = match input.read_line(&mut line) {
        Ok(_) => serde_json::from_str::<WorkerStart>(&line)
            .map_err(|err| format!("code mode snippet process got an invalid start: {err}")),
        Err(err) => Err(format!(
            "code mode snippet process could not read its start: {err}"
        )),
    };
    let start = match start {
        Ok(start) => start,
        Err(error) => {
            let _ = send_to_server(&WorkerMessage::Done {
                ok: false,
                output: error,
            });
            return ExitCode::from(2);
        }
    };
    let params = CodeExecuteParams {
        code: start.code,
        root: start.root,
        timeout_ms: start.timeout_ms,
        max_output_bytes: start.max_output_bytes,
    };
    start_exit_timer(SnippetLimits::new(&params).timeout_ms);
    let (ok, output) = match execute_code_mode_with(
        HostDispatch::Server(ServerLink { input }),
        params,
        None,
        Path::new("."),
    ) {
        Ok(output) => (true, output),
        Err(output) => (false, output),
    };
    match send_to_server(&WorkerMessage::Done { ok, output }) {
        Ok(()) => ExitCode::SUCCESS,
        Err(_) => ExitCode::FAILURE,
    }
}

/// End the snippet process after its time limit when no server kills it, for
/// example after the server itself was killed.
fn start_exit_timer(timeout_ms: u64) {
    let limit = Duration::from_millis(timeout_ms) + 2 * WORKER_GRACE;
    thread::spawn(move || {
        thread::sleep(limit);
        #[expect(
            clippy::exit,
            reason = "the snippet process must end even while its main thread is inside a builtin call"
        )]
        std::process::exit(WORKER_TIMED_OUT_EXIT);
    });
}

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
    }

    /// A snippet process that never answers stands for a snippet inside a
    /// long builtin call. The server must kill it and answer.
    #[cfg(unix)]
    #[test]
    fn a_silent_snippet_process_is_killed_at_the_deadline() {
        let cwd = tempfile::tempdir().expect("tempdir");
        let worker = CodeModeWorker::with_args(PathBuf::from("/bin/sh"), &["-c", "sleep 60"]);
        let started = Instant::now();
        let output = execute_isolated_in(
            Some(&worker),
            "fallow".to_string(),
            params("return 1;", 200),
            None,
            cwd.path(),
        )
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
        let worker = CodeModeWorker::with_args(PathBuf::from("/bin/sh"), &["-c", "sleep 60"]);
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
        let output = execute_isolated_in(
            Some(&worker),
            "fallow".to_string(),
            params("return 1;", 5_000),
            None,
            cwd.path(),
        )
        .expect_err("no result is an error");
        assert_eq!(
            error_of(&output),
            "code mode snippet process ended without a result"
        );
    }
}
