//! The snippet processes of Code Mode: start, pool, bounded writes and kill.
//!
//! The server keeps one idle snippet process. A snippet takes it, and gives it
//! back when the snippet ends normally, so a normal snippet pays no process
//! start and no process cleanup. A snippet that the server stops (deadline,
//! cancel, protocol error) drops its process: the drop kills the process tree
//! on a background thread, and a background thread starts the next idle
//! process.

use std::ffi::OsString;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::Instant;

use fallow_process::{
    ChildCleanup, ProcessTree, cleanup_std_child, configure_std_command,
    spawn_retrying_busy_executable,
};
use serde::Serialize;
use tokio_util::sync::CancellationToken;

use super::super::{CANCEL_POLL_INTERVAL, is_cancelled};
use super::{
    BUILD_VERSION, CODE_MODE_WORKER_FLAG, PROTOCOL_VERSION, RESTART_HINT, Stop, WorkerMessage,
};

/// The token that routes the multicall `fallow` binary to the MCP server
/// entry. The standalone `fallow-mcp` binary ignores it.
const MCP_SERVER_SUBCOMMAND: &str = "mcp-server";

/// Longest message line that the server reads from a snippet process. The
/// final envelope is bounded by `max_output_bytes` and the trace limits; this
/// cap only keeps a defect in the snippet process from growing the server
/// memory without limit.
const MAX_WORKER_LINE_BYTES: u64 = 64 * 1024 * 1024;

/// The error of a snippet process that closed its pipes.
pub(super) const ENDED_WITHOUT_RESULT: &str = "code mode snippet process ended without a result";

/// How to start a snippet process, and the idle process of the server.
#[derive(Clone)]
pub struct CodeModeWorker {
    pool: Arc<Pool>,
}

struct Pool {
    program: PathBuf,
    args: Vec<OsString>,
    idle: Mutex<Option<WorkerProcess>>,
}

impl CodeModeWorker {
    /// The running server binary as the snippet process, or `None` when the
    /// path of the running binary is not available. On Linux the path is
    /// `/proc/self/exe`, which stays the running binary when the file on disk
    /// is removed or replaced.
    pub fn current() -> Option<Self> {
        #[cfg(target_os = "linux")]
        let program = Some(PathBuf::from("/proc/self/exe"));
        #[cfg(not(target_os = "linux"))]
        let program = std::env::current_exe().ok();
        program.map(Self::server_binary)
    }

    /// `program` as a server binary: `fallow-mcp`, or the multicall `fallow`.
    pub fn server_binary(program: PathBuf) -> Self {
        Self::new(
            program,
            vec![
                OsString::from(MCP_SERVER_SUBCOMMAND),
                OsString::from(CODE_MODE_WORKER_FLAG),
            ],
        )
    }

    #[cfg(test)]
    pub(super) fn with_args(program: PathBuf, args: &[&str]) -> Self {
        Self::new(program, args.iter().map(OsString::from).collect())
    }

    fn new(program: PathBuf, args: Vec<OsString>) -> Self {
        Self {
            pool: Arc::new(Pool {
                program,
                args,
                idle: Mutex::new(None),
            }),
        }
    }

    /// The idle snippet process, or a new one when there is none.
    pub(super) fn take(&self) -> Result<WorkerProcess, String> {
        let idle = self.pool.idle.lock().ok().and_then(|mut slot| slot.take());
        if let Some(mut process) = idle
            && process.is_running()
        {
            return Ok(process);
        }
        self.spawn()
    }

    /// Keep `process` as the idle snippet process. When the pool already has
    /// one, `process` is dropped.
    pub(super) fn give_back(&self, process: WorkerProcess) {
        if let Ok(mut slot) = self.pool.idle.lock()
            && slot.is_none()
        {
            *slot = Some(process);
        }
    }

    /// Start a new idle snippet process on a background thread.
    pub(super) fn refill_in_background(&self) {
        let worker = self.clone();
        let _ = thread::Builder::new()
            .name("code-mode-refill".to_string())
            .spawn(move || worker.refill());
    }

    fn refill(&self) {
        if self.pool.idle.lock().map_or(true, |slot| slot.is_some()) {
            return;
        }
        if let Ok(process) = self.spawn() {
            self.give_back(process);
        }
    }

    #[cfg(all(test, unix))]
    pub(super) fn discard_idle(&self) {
        let idle = self.pool.idle.lock().ok().and_then(|mut slot| slot.take());
        drop(idle);
    }

    #[cfg(test)]
    pub(super) fn idle_pid(&self) -> Option<u32> {
        self.pool
            .idle
            .lock()
            .ok()
            .and_then(|slot| slot.as_ref().and_then(WorkerProcess::pid))
    }

    fn spawn(&self) -> Result<WorkerProcess, String> {
        let program = &self.pool.program;
        let mut command = Command::new(program);
        // The snippet process speaks only the line protocol on stdin and
        // stdout. Its stderr would reach the MCP client log, so it is closed.
        command
            .args(&self.pool.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        configure_std_command(&mut command);
        let mut child = spawn_child(command).map_err(|err| {
            format!(
                "code mode could not start the snippet process '{}': {err}; {RESTART_HINT}",
                program.display()
            )
        })?;
        let tree = match ProcessTree::for_std_child(&child) {
            Ok(tree) => tree,
            Err(error) => {
                log_cleanup_errors(&cleanup_std_child(None, &mut child));
                return Err(format!(
                    "code mode could not track the snippet process: {error}"
                ));
            }
        };
        let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
            log_cleanup_errors(&cleanup_std_child(Some(&tree), &mut child));
            return Err("code mode snippet process has no stdin or stdout".to_string());
        };
        let (message_sender, messages) = mpsc::channel();
        thread::spawn(move || read_messages(stdout, &message_sender));
        let (lines, line_receiver) = mpsc::channel::<Vec<u8>>();
        let (error_sender, write_errors) = mpsc::channel();
        thread::spawn(move || write_lines(stdin, &line_receiver, &error_sender));
        Ok(WorkerProcess {
            child: Some(child),
            tree: Some(tree),
            lines: Some(lines),
            write_errors,
            messages,
            ready: false,
        })
    }
}

type SpawnRequest = (Command, Sender<io::Result<Child>>);

/// Start `command` on one thread that lives as long as the server.
///
/// On Linux the snippet process asks for `PR_SET_PDEATHSIG`. That signal
/// fires when the thread that started the process ends, not the process. A
/// blocking thread of the async runtime ends after an idle time, so the
/// snippet processes start on this long-lived thread instead.
fn spawn_child(mut command: Command) -> io::Result<Child> {
    static SPAWNER: OnceLock<Option<Sender<SpawnRequest>>> = OnceLock::new();
    let spawner = SPAWNER.get_or_init(|| {
        let (sender, requests) = mpsc::channel::<SpawnRequest>();
        thread::Builder::new()
            .name("code-mode-spawner".to_string())
            .spawn(move || {
                for (mut command, reply) in requests {
                    let _ = reply.send(spawn_retrying_busy_executable(&mut command));
                }
            })
            .ok()
            .map(|_| sender)
    });
    let Some(spawner) = spawner else {
        return spawn_retrying_busy_executable(&mut command);
    };
    let (reply, result) = mpsc::channel();
    if let Err(mpsc::SendError((mut command, _))) = spawner.send((command, reply)) {
        return spawn_retrying_busy_executable(&mut command);
    }
    result
        .recv()
        .unwrap_or_else(|_| Err(io::Error::other("the code mode spawner thread ended")))
}

/// One running snippet process. Drop kills its process tree on a background
/// thread.
pub(super) struct WorkerProcess {
    child: Option<Child>,
    tree: Option<ProcessTree>,
    lines: Option<Sender<Vec<u8>>>,
    write_errors: Receiver<String>,
    messages: Receiver<Result<WorkerMessage, String>>,
    /// Whether the process sent a [`WorkerMessage::Ready`] of this build.
    ready: bool,
}

impl WorkerProcess {
    fn is_running(&mut self) -> bool {
        self.child
            .as_mut()
            .is_some_and(|child| matches!(child.try_wait(), Ok(None)))
    }

    #[cfg(test)]
    fn pid(&self) -> Option<u32> {
        self.child.as_ref().map(Child::id)
    }

    /// Queue one message line for the writer thread. The call does not wait
    /// for the write: a process that stops reading holds only the writer
    /// thread, and [`Self::receive`] still ends at `stop_at` or a cancel.
    pub(super) fn send<T: Serialize>(&self, message: &T) -> Result<(), Stop> {
        self.write_failure()?;
        let mut line = serde_json::to_vec(message)
            .map_err(|err| Stop::Failed(format!("failed to encode a code mode message: {err}")))?;
        line.push(b'\n');
        let lines = self
            .lines
            .as_ref()
            .ok_or_else(|| Stop::Failed(ENDED_WITHOUT_RESULT.to_string()))?;
        lines
            .send(line)
            .map_err(|_| Stop::Failed(ENDED_WITHOUT_RESULT.to_string()))
    }

    /// The error of a failed write, when the writer thread reported one.
    fn write_failure(&self) -> Result<(), Stop> {
        match self.write_errors.try_recv() {
            Ok(error) => Err(Stop::Failed(error)),
            Err(TryRecvError::Empty) => Ok(()),
            Err(TryRecvError::Disconnected) => Err(Stop::Failed(ENDED_WITHOUT_RESULT.to_string())),
        }
    }

    /// The next message, by `stop_at` and before a cancel.
    pub(super) fn receive(
        &self,
        stop_at: Instant,
        request: Option<&CancellationToken>,
    ) -> Result<WorkerMessage, Stop> {
        loop {
            self.write_failure()?;
            match self.messages.recv_timeout(next_wait(stop_at, request)?) {
                Ok(message) => return message.map_err(Stop::Failed),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(Stop::Failed(ENDED_WITHOUT_RESULT.to_string()));
                }
            }
        }
    }

    /// Accept the [`WorkerMessage::Ready`] of the process, or refuse a
    /// process of another protocol or build.
    pub(super) fn accept_ready(&mut self, protocol: u32, version: &str) -> Result<(), Stop> {
        if self.ready {
            return Err(Stop::Failed(
                "code mode snippet process announced itself twice".to_string(),
            ));
        }
        if protocol != PROTOCOL_VERSION || version != BUILD_VERSION {
            return Err(Stop::Failed(super::mismatch_message(protocol, version)));
        }
        self.ready = true;
        Ok(())
    }

    pub(super) fn is_ready(&self) -> bool {
        self.ready
    }
}

impl Drop for WorkerProcess {
    fn drop(&mut self) {
        self.lines = None;
        let Some(mut child) = self.child.take() else {
            return;
        };
        let tree = self.tree.take();
        let cleanup = move || log_cleanup_errors(&cleanup_std_child(tree.as_ref(), &mut child));
        if let Err(error) = thread::Builder::new()
            .name("code-mode-cleanup".to_string())
            .spawn(cleanup)
        {
            tracing::warn!(%error, "code mode could not start a cleanup thread");
        }
    }
}

/// How long the next wait may take: up to [`CANCEL_POLL_INTERVAL`], and not
/// past `stop_at`.
fn next_wait(
    stop_at: Instant,
    request: Option<&CancellationToken>,
) -> Result<std::time::Duration, Stop> {
    if is_cancelled(request) {
        return Err(Stop::Cancelled);
    }
    let left = stop_at.saturating_duration_since(Instant::now());
    if left.is_zero() {
        return Err(Stop::TimedOut);
    }
    Ok(left.min(CANCEL_POLL_INTERVAL))
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

/// Write each line to the snippet process. Report the first failure and stop.
fn write_lines(mut stdin: ChildStdin, lines: &Receiver<Vec<u8>>, errors: &Sender<String>) {
    for line in lines {
        if let Err(err) = stdin.write_all(&line).and_then(|()| stdin.flush()) {
            let error = if err.kind() == io::ErrorKind::BrokenPipe {
                ENDED_WITHOUT_RESULT.to_string()
            } else {
                format!("failed to write to the code mode snippet process: {err}")
            };
            let _ = errors.send(error);
            return;
        }
    }
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
