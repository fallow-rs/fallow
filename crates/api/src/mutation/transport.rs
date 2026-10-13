//! One companion process per worker: JSON lines over stdio, one request in
//! flight, a watchdog per request, process-tree kill through
//! `fallow-process`, and a bounded stderr tail (`protocol.md` sections 2, 4
//! and 8). Each protocol line starts with the session token of the
//! companion. Other stdout text is stray output and goes to the stderr tail.

use std::collections::VecDeque;
use std::collections::hash_map::RandomState;
use std::hash::BuildHasher;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use super::companion::InstalledCompanion;
use super::protocol::{LinkFault, Request, Response, decode_response};

/// The size of the stderr tail that an error report keeps.
pub const STDERR_TAIL_BYTES: usize = 64 * 1024;
/// The largest response line. A dry run of a large suite has many hits.
const MAX_LINE_BYTES: u64 = 256 * 1024 * 1024;
/// How long the companion has to exit after `shutdown` (section 3.5).
const SHUTDOWN_GRACE: Duration = Duration::from_secs(5);
/// How long the link waits for the stderr reader after the stdout end.
const STDERR_DRAIN: Duration = Duration::from_millis(500);

/// The session token of one companion.
///
/// The API puts the token in the environment of the companion. The companion
/// removes the variable before it loads the runner, and starts each protocol
/// line with the token and one space. A raw write of runner or user code to
/// stdout has no token, so it can never read as a protocol line. The token
/// separates stray output from protocol lines. It is not a secret against
/// hostile code.
#[derive(Debug, Clone)]
pub struct SessionToken {
    env: String,
    value: String,
}

impl SessionToken {
    /// Makes a new token for the variable `env`. Each call gives a different
    /// value.
    pub fn generate(env: &str) -> Self {
        static COUNT: AtomicU64 = AtomicU64::new(0);
        let count = COUNT.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_nanos());
        let seed = (nanos, count, std::process::id());
        let high = RandomState::new().hash_one((seed, 0_u8));
        let low = RandomState::new().hash_one((seed, 1_u8));
        Self {
            env: env.to_owned(),
            value: format!("{high:016x}{low:016x}"),
        }
    }

    /// The text that starts each protocol line.
    fn line_prefix(&self) -> String {
        format!("{} ", self.value)
    }
}

/// A live companion. The scheduler never sees a process.
pub trait CompanionLink: Send {
    /// Sends `request` and waits at most `timeout` for its response.
    ///
    /// A timeout, an exit, or a protocol violation kills the companion tree.
    /// An `error` response leaves the companion alive.
    fn request(&mut self, request: &Request, timeout: Duration) -> Result<Response, LinkFault>;

    /// Sends `shutdown` and kills the tree after the grace period.
    fn shutdown(&mut self);
}

/// Why a companion did not start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpawnFailure {
    /// The companion files changed after install. The run stops.
    Integrity(String),
    /// The process did not start.
    Fault(LinkFault),
}

/// Starts companions.
pub trait CompanionSpawner: Sync {
    fn spawn(&self) -> Result<Box<dyn CompanionLink>, SpawnFailure>;
}

/// Starts the embedded companion under a JavaScript runtime.
pub struct ProcessSpawner {
    pub runtime: PathBuf,
    pub companion: InstalledCompanion,
    pub root: PathBuf,
    pub wire_version: u32,
    /// The environment variable that carries the session token.
    pub session_token_env: String,
}

impl CompanionSpawner for ProcessSpawner {
    fn spawn(&self) -> Result<Box<dyn CompanionLink>, SpawnFailure> {
        self.companion.verify().map_err(SpawnFailure::Integrity)?;
        let mut command = Command::new(&self.runtime);
        command
            .arg(self.companion.entry())
            .current_dir(&self.root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let token = SessionToken::generate(&self.session_token_env);
        ProcessLink::spawn(&mut command, self.wire_version, &token)
            .map(|link| Box::new(link) as Box<dyn CompanionLink>)
            .map_err(SpawnFailure::Fault)
    }
}

/// Why a request line did not reach the companion.
enum WriteFault {
    /// The pipe is closed or the write failed.
    Closed,
    /// The companion did not read the line before the deadline.
    Timeout,
}

/// Writes request lines on a helper thread, so that a companion that never
/// reads stdin cannot block the watchdog. Dropping the writer closes stdin
/// after the line in progress.
struct StdinWriter {
    lines: mpsc::Sender<String>,
    done: Receiver<std::io::Result<()>>,
}

impl StdinWriter {
    fn new(mut stdin: ChildStdin) -> Self {
        let (lines, queue) = mpsc::channel::<String>();
        let (report, done) = mpsc::channel();
        std::thread::spawn(move || {
            for line in queue {
                let written = stdin
                    .write_all(line.as_bytes())
                    .and_then(|()| stdin.write_all(b"\n"))
                    .and_then(|()| stdin.flush());
                let failed = written.is_err();
                if report.send(written).is_err() || failed {
                    return;
                }
            }
        });
        Self { lines, done }
    }

    fn write(&self, line: String, deadline: Instant) -> Result<(), WriteFault> {
        self.lines.send(line).map_err(|_| WriteFault::Closed)?;
        let left = deadline.saturating_duration_since(Instant::now());
        match self.done.recv_timeout(left) {
            Ok(Ok(())) => Ok(()),
            Ok(Err(_)) | Err(RecvTimeoutError::Disconnected) => Err(WriteFault::Closed),
            Err(RecvTimeoutError::Timeout) => Err(WriteFault::Timeout),
        }
    }
}

enum LineEvent {
    Line(String),
    End,
    Failed(String),
}

#[derive(Default)]
struct StderrTail {
    bytes: VecDeque<u8>,
    done: bool,
}

type SharedTail = Arc<(Mutex<StderrTail>, Condvar)>;

/// Appends `bytes` to the tail and keeps only the last `STDERR_TAIL_BYTES`.
fn push_tail(tail: &(Mutex<StderrTail>, Condvar), bytes: &[u8]) {
    let Ok(mut guard) = tail.0.lock() else {
        return;
    };
    guard.bytes.extend(bytes);
    let excess = guard.bytes.len().saturating_sub(STDERR_TAIL_BYTES);
    guard.bytes.drain(..excess);
}

/// A companion process.
pub struct ProcessLink {
    child: Option<fallow_process::ScopedChild>,
    terminator: fallow_process::ProcessTreeTerminator,
    stdin: Option<StdinWriter>,
    lines: Receiver<LineEvent>,
    stderr: SharedTail,
    next_id: u64,
    version: u32,
    alive: bool,
}

impl ProcessLink {
    /// Spawns `command` in its own process tree, with `token` in its
    /// environment.
    pub fn spawn(
        command: &mut Command,
        version: u32,
        token: &SessionToken,
    ) -> Result<Self, LinkFault> {
        command.env(&token.env, &token.value);
        let mut child = fallow_process::ScopedChild::spawn_process_tree(command)
            .map_err(|error| LinkFault::Io(format!("cannot start the companion: {error}")))?;
        let terminator = child
            .process_tree_terminator()
            .ok_or_else(|| LinkFault::Io("the companion process tree is missing".to_owned()))?;
        let (Some(stdin), Some(stdout), Some(stderr)) =
            (child.take_stdin(), child.take_stdout(), child.take_stderr())
        else {
            let _ = terminator.terminate();
            return Err(LinkFault::Io("the companion pipes are missing".to_owned()));
        };
        let tail: SharedTail = Arc::new((Mutex::new(StderrTail::default()), Condvar::new()));
        let (sender, lines) = mpsc::channel();
        let stray = Arc::clone(&tail);
        let prefix = token.line_prefix();
        std::thread::spawn(move || read_lines(stdout, &sender, &prefix, &stray));
        let writer = Arc::clone(&tail);
        std::thread::spawn(move || read_stderr(stderr, &writer));
        Ok(Self {
            child: Some(child),
            terminator,
            stdin: Some(StdinWriter::new(stdin)),
            lines,
            stderr: tail,
            next_id: 1,
            version,
            alive: true,
        })
    }

    fn kill(&mut self) {
        self.alive = false;
        self.stdin = None;
        let _ = self.terminator.terminate();
    }

    fn stderr_tail(&self, wait: Duration) -> String {
        let (lock, ready) = &*self.stderr;
        let bytes: Vec<u8> = {
            let Ok(guard) = lock.lock() else {
                return String::new();
            };
            let Ok((guard, _)) = ready.wait_timeout_while(guard, wait, |tail| !tail.done) else {
                return String::new();
            };
            guard.bytes.iter().copied().collect()
        };
        String::from_utf8_lossy(&bytes).into_owned()
    }

    fn exited(&mut self) -> LinkFault {
        self.alive = false;
        self.stdin = None;
        let stderr_tail = self.stderr_tail(STDERR_DRAIN);
        let _ = self.terminator.terminate();
        LinkFault::Exited { stderr_tail }
    }

    fn write_line(&self, line: String, deadline: Instant) -> Result<(), WriteFault> {
        self.stdin
            .as_ref()
            .ok_or(WriteFault::Closed)?
            .write(line, deadline)
    }
}

impl CompanionLink for ProcessLink {
    fn request(&mut self, request: &Request, timeout: Duration) -> Result<Response, LinkFault> {
        if !self.alive {
            return Err(LinkFault::Io("the companion is not running".to_owned()));
        }
        let id = self.next_id;
        self.next_id += 1;
        // The watchdog covers the write too: a companion that never reads
        // stdin must not block a request larger than the pipe buffer.
        let deadline = Instant::now() + timeout;
        match self.write_line(request.to_line(self.version, id), deadline) {
            Ok(()) => {}
            Err(WriteFault::Closed) => return Err(self.exited()),
            Err(WriteFault::Timeout) => {
                self.kill();
                return Err(LinkFault::Timeout);
            }
        }
        let Some(expected) = request.response_type() else {
            return Err(LinkFault::Protocol(format!(
                "`{}` has no response",
                request.type_name()
            )));
        };
        match self
            .lines
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        {
            Ok(LineEvent::Line(line)) => {
                let decoded = decode_response(&line, self.version, id, expected);
                if matches!(decoded, Err(LinkFault::Protocol(_))) {
                    self.kill();
                }
                decoded
            }
            Ok(LineEvent::Failed(message)) => {
                self.kill();
                Err(LinkFault::Protocol(message))
            }
            Ok(LineEvent::End) | Err(RecvTimeoutError::Disconnected) => Err(self.exited()),
            Err(RecvTimeoutError::Timeout) => {
                self.kill();
                Err(LinkFault::Timeout)
            }
        }
    }

    fn shutdown(&mut self) {
        let deadline = Instant::now() + SHUTDOWN_GRACE;
        if self.alive
            && self
                .write_line(
                    Request::Shutdown.to_line(self.version, self.next_id),
                    deadline,
                )
                .is_ok()
        {
            self.stdin = None;
            loop {
                let left = deadline.saturating_duration_since(Instant::now());
                match self.lines.recv_timeout(left) {
                    Ok(LineEvent::Line(_)) => {}
                    Ok(_) | Err(_) => break,
                }
            }
        }
        // The companion exits on its own; the kill removes runner workers
        // that outlive it.
        self.kill();
        if let Some(child) = self.child.take() {
            let _ = child.wait();
        }
    }
}

impl Drop for ProcessLink {
    fn drop(&mut self) {
        if self.child.is_some() {
            self.kill();
        }
    }
}

/// Splits one stdout line at the session token. The text after the token is
/// the protocol line. The text before it, or the whole line when it has no
/// token, is stray output. The search covers the whole line, so an
/// unterminated stray write before a protocol line does not hide it.
fn split_protocol_line<'a>(line: &'a str, prefix: &str) -> (&'a str, Option<&'a str>) {
    match line.find(prefix) {
        Some(at) => (&line[..at], Some(&line[at + prefix.len()..])),
        None => (line, None),
    }
}

fn read_lines(
    stdout: impl Read,
    sender: &mpsc::Sender<LineEvent>,
    prefix: &str,
    stray: &(Mutex<StderrTail>, Condvar),
) {
    let mut reader = BufReader::new(stdout);
    loop {
        let mut line = Vec::new();
        let read = (&mut reader)
            .take(MAX_LINE_BYTES + 1)
            .read_until(b'\n', &mut line);
        let event = match read {
            Ok(0) => LineEvent::End,
            Ok(_) if line.last() != Some(&b'\n') && line.len() as u64 > MAX_LINE_BYTES => {
                LineEvent::Failed(format!(
                    "a response line is longer than {MAX_LINE_BYTES} bytes"
                ))
            }
            Ok(_) => {
                while matches!(line.last(), Some(b'\n' | b'\r')) {
                    line.pop();
                }
                let text = String::from_utf8_lossy(&line);
                let (before, protocol) = split_protocol_line(&text, prefix);
                if !before.is_empty() {
                    push_tail(stray, format!("{before}\n").as_bytes());
                }
                match protocol {
                    Some(protocol) => LineEvent::Line(protocol.to_owned()),
                    None => continue,
                }
            }
            Err(error) => LineEvent::Failed(format!("cannot read the companion stdout: {error}")),
        };
        let stop = !matches!(event, LineEvent::Line(_));
        if sender.send(event).is_err() || stop {
            return;
        }
    }
}

fn read_stderr(mut stderr: impl Read, tail: &(Mutex<StderrTail>, Condvar)) {
    let mut buffer = [0_u8; 8192];
    loop {
        let read = stderr.read(&mut buffer).unwrap_or(0);
        if read > 0 {
            push_tail(tail, &buffer[..read]);
            continue;
        }
        if let Ok(mut guard) = tail.0.lock() {
            guard.done = true;
            tail.1.notify_all();
        }
        return;
    }
}

#[expect(clippy::unwrap_used, reason = "tests unwrap to keep assertions short")]
#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn shell(script: &str) -> Command {
        let mut command = Command::new("/bin/sh");
        command
            .arg("-c")
            .arg(script)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command
    }

    const TOKEN_ENV: &str = "FALLOW_TEST_SESSION_TOKEN";

    /// Spawns `script` with a new session token in its environment.
    fn spawn(script: &str) -> ProcessLink {
        ProcessLink::spawn(&mut shell(script), 1, &SessionToken::generate(TOKEN_ENV))
            .expect("spawn")
    }

    /// A shell command that writes `line` as a protocol line.
    fn framed(line: &str) -> String {
        format!("printf '%s %s\\n' \"${TOKEN_ENV}\" '{line}'")
    }

    fn init() -> Request {
        Request::Reload
    }

    #[test]
    fn a_response_with_the_open_id_decodes() {
        let line = r#"{"v":1,"id":1,"type":"reloaded","method":"noop"}"#;
        let mut link = spawn(&format!("read line; {}; read rest", framed(line)));
        let response = link
            .request(&init(), Duration::from_secs(5))
            .expect("response");
        assert_eq!(
            response,
            Response::Reloaded(super::super::protocol::Reloaded {
                method: super::super::protocol::ReloadMethod::Noop
            })
        );
        link.shutdown();
    }

    #[test]
    fn the_spawner_checks_each_companion_file_before_each_spawn() {
        use super::super::companion::{EmbeddedFile, install};
        const FILES: &[EmbeddedFile] = &[EmbeddedFile {
            path: "companion.mjs",
            text: "exit 0\n",
        }];
        let cache = tempfile::tempdir().expect("dir");
        let companion = install(cache.path(), FILES).expect("install");
        let entry = companion.entry();
        let spawner = ProcessSpawner {
            runtime: PathBuf::from("/bin/sh"),
            companion,
            root: cache.path().to_path_buf(),
            wire_version: 1,
            session_token_env: TOKEN_ENV.to_owned(),
        };
        let mut link = spawner.spawn().expect("an intact companion starts");
        link.shutdown();
        std::fs::write(&entry, "echo tampered\n").expect("tamper");
        match spawner.spawn() {
            Err(SpawnFailure::Integrity(message)) => {
                assert!(message.contains("companion.mjs"), "{message}");
            }
            Err(other) => panic!("expected an integrity failure, got {other:?}"),
            Ok(_) => panic!("a changed companion must not start"),
        }
    }

    #[test]
    fn the_watchdog_kills_a_silent_companion_and_reports_a_timeout() {
        let mut link = spawn("read line; sleep 30");
        let started = std::time::Instant::now();
        let fault = link
            .request(&init(), Duration::from_millis(300))
            .unwrap_err();
        assert_eq!(fault, LinkFault::Timeout);
        assert!(started.elapsed() < Duration::from_secs(10));
        assert!(matches!(
            link.request(&init(), Duration::from_secs(1)),
            Err(LinkFault::Io(_))
        ));
    }

    #[test]
    fn the_watchdog_covers_a_request_larger_than_the_pipe_buffer() {
        use super::super::protocol::{InitOptions, InitRequest};
        let path_map = (0..20_000)
            .map(|index| {
                (
                    format!("src/module-{index}.ts"),
                    format!("/cache/mutation/runs/abc/src/module-{index}.ts"),
                )
            })
            .collect();
        let request = Request::Init(InitRequest {
            root: "/project".to_owned(),
            runner: "vitest".to_owned(),
            config_path: None,
            run_dir: "/cache/run".to_owned(),
            path_map,
            options: InitOptions::default(),
        });
        let mut link = spawn("sleep 30");
        let started = std::time::Instant::now();
        let fault = link
            .request(&request, Duration::from_millis(300))
            .unwrap_err();
        assert_eq!(fault, LinkFault::Timeout);
        assert!(started.elapsed() < Duration::from_secs(10));
        let started = std::time::Instant::now();
        link.shutdown();
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn an_exit_before_the_response_keeps_the_stderr_tail() {
        let mut link = spawn("read line; echo 'boom in runner' >&2; exit 3");
        let fault = link.request(&init(), Duration::from_secs(5)).unwrap_err();
        let LinkFault::Exited { stderr_tail } = fault else {
            panic!("expected an exit, got {fault:?}");
        };
        assert!(stderr_tail.contains("boom in runner"), "{stderr_tail}");
    }

    #[test]
    fn the_stderr_tail_keeps_only_the_last_64_kib() {
        let script = "read line; head -c 200000 /dev/zero | tr '\\0' 'a' >&2; echo END >&2; exit 1";
        let mut link = spawn(script);
        let LinkFault::Exited { stderr_tail } =
            link.request(&init(), Duration::from_secs(10)).unwrap_err()
        else {
            panic!("expected an exit");
        };
        assert_eq!(stderr_tail.len(), STDERR_TAIL_BYTES);
        assert!(stderr_tail.ends_with("END\n"));
    }

    #[test]
    fn a_response_with_another_id_kills_the_companion() {
        let line = r#"{"v":1,"id":7,"type":"reloaded","method":"noop"}"#;
        let mut link = spawn(&format!("read line; {}; sleep 30", framed(line)));
        let fault = link.request(&init(), Duration::from_secs(5)).unwrap_err();
        assert!(matches!(fault, LinkFault::Protocol(_)), "{fault:?}");
        assert!(matches!(
            link.request(&init(), Duration::from_secs(1)),
            Err(LinkFault::Io(_))
        ));
    }

    #[test]
    fn stdout_text_without_the_session_token_never_reads_as_a_response() {
        let stray = r#"{"v":1,"id":1,"type":"reloaded","method":"invalidate"}"#;
        let line = r#"{"v":1,"id":1,"type":"reloaded","method":"noop"}"#;
        let mut link = spawn(&format!(
            "read line; echo '{stray}'; printf 'unterminated noise'; {}; read rest",
            framed(line)
        ));
        let response = link
            .request(&init(), Duration::from_secs(5))
            .expect("the framed response");
        assert_eq!(
            response,
            Response::Reloaded(super::super::protocol::Reloaded {
                method: super::super::protocol::ReloadMethod::Noop
            })
        );
        link.shutdown();
    }

    #[test]
    fn stray_stdout_text_goes_to_the_stderr_tail() {
        let mut link = spawn("read line; echo 'stray from a worker'; exit 3");
        let LinkFault::Exited { stderr_tail } =
            link.request(&init(), Duration::from_secs(5)).unwrap_err()
        else {
            panic!("expected an exit");
        };
        assert_eq!(stderr_tail, "stray from a worker\n");
    }

    #[test]
    fn each_session_token_is_new_and_has_no_whitespace() {
        let first = SessionToken::generate(TOKEN_ENV);
        let second = SessionToken::generate(TOKEN_ENV);
        assert_ne!(first.value, second.value);
        assert_eq!(first.value.len(), 32);
        assert!(first.value.chars().all(|char| char.is_ascii_hexdigit()));
    }
}
