//! Integration tests for the signal-handler -> child-process cleanup
//! contract introduced by issue #477.
//!
//! Uses a sub-process pattern (NOT self-signal): each test spawns a
//! child `fallow` binary with `FALLOW_TEST_SIGNAL_HELPER=1`, which goes
//! through the signal-helper code path at the top of `main()`. That
//! helper spawns `sleep 30` via the `ScopedChild` registry and prints
//! the inner PID to stdout. The test reads the PID, delivers a signal
//! (SIGINT or SIGTERM) to the child fallow via `kill`, then asserts:
//!  - The fallow exit code matches POSIX convention (130 / 143).
//!  - The inner `sleep` PID is no longer alive (the signal handler's
//!    `drain_and_kill` worked).
//!
//! Why the sub-process pattern: `cargo test` runs tests in parallel
//! threads of one binary. Sending `kill(getpid(), SIGINT)` from inside
//! a test thread would take down every parallel test in the same
//! binary because the signal disposition is process-global. Spawning a
//! separate fallow child means each test gets its own PID and its own
//! signal disposition.
//!
//! Cross-ref: `.plans/issue-477-signal-handlers.md` and project memory
//! on the sub-process test pattern.

#![cfg(unix)]

use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Spawn a child `fallow` binary in signal-helper mode and read the
/// PID it prints to stdout. Returns the (child, inner PID) pair.
/// Pass `graceful = true` to opt the helper into graceful-mode (no
/// process::exit after drain).
///
/// Killing + waiting the spawned fallow on PID-read failure is the
/// caller's job: the caller of this function always reaps the returned
/// child either via the test's `wait_timeout_compat` (happy path) or
/// the panic handler unwinds and Drop reaps via try_wait. clippy's
/// `zombie_processes` lint fires if the function has any return path
/// that drops the Child handle without a visible `wait()`; we satisfy
/// it by killing-and-waiting on the not-found path explicitly.
fn spawn_signal_helper_with(graceful: bool) -> (std::process::Child, u32) {
    let fallow_bin = env!("CARGO_BIN_EXE_fallow");
    let mut builder = Command::new(fallow_bin);
    builder
        .env("FALLOW_TEST_SIGNAL_HELPER", "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if graceful {
        builder.env("FALLOW_TEST_SIGNAL_HELPER_GRACEFUL", "1");
    }
    let mut child = builder.spawn().expect("spawn fallow signal helper");
    let stdout = child.stdout.take().expect("piped stdout");
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        line.clear();
        if reader.read_line(&mut line).is_ok()
            && let Ok(pid) = line.trim().parse::<u32>()
        {
            return (child, pid);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let _ = child.kill();
    let _ = child.wait();
    panic!("signal helper did not print PID within 5s");
}

/// Wait up to `timeout` for `pid` to be reaped. Returns whether the
/// PID is dead. Uses `kill -0 <pid>` rather than relying on the
/// crates/cli-internal `process_is_alive` to keep the test
/// dependency-free.
fn wait_for_pid_dead(pid: u32, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        let status = Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        match status {
            Ok(s) if !s.success() => return true,
            _ => {}
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

fn send_signal(pid: u32, signal: &str) {
    let status = Command::new("kill")
        .args([signal, &pid.to_string()])
        .status()
        .expect("kill command spawn");
    assert!(status.success(), "kill {signal} {pid} failed");
}

fn spawn_signal_helper() -> (std::process::Child, u32) {
    spawn_signal_helper_with(false)
}

#[test]
fn sigint_kills_registered_child_and_exits_130() {
    let (mut fallow, sleep_pid) = spawn_signal_helper();
    // Sanity: the inner sleep should be alive RIGHT NOW.
    assert!(
        !wait_for_pid_dead(sleep_pid, Duration::from_millis(100)),
        "inner sleep PID {sleep_pid} should be alive after helper start"
    );
    send_signal(fallow.id(), "-INT");
    let status = fallow
        .wait_timeout_compat(Duration::from_secs(10))
        .expect("fallow helper exit within 10s");
    assert_eq!(
        status.code(),
        Some(130),
        "SIGINT must yield exit code 128+2=130; got {:?}",
        status.code(),
    );
    assert!(
        wait_for_pid_dead(sleep_pid, Duration::from_secs(5)),
        "inner sleep PID {sleep_pid} must be dead after the signal handler drains",
    );
}

#[test]
fn sigterm_kills_registered_child_and_exits_143() {
    let (mut fallow, sleep_pid) = spawn_signal_helper();
    assert!(
        !wait_for_pid_dead(sleep_pid, Duration::from_millis(100)),
        "inner sleep PID {sleep_pid} should be alive after helper start"
    );
    send_signal(fallow.id(), "-TERM");
    let status = fallow
        .wait_timeout_compat(Duration::from_secs(10))
        .expect("fallow helper exit within 10s");
    assert_eq!(
        status.code(),
        Some(143),
        "SIGTERM must yield exit code 128+15=143; got {:?}",
        status.code(),
    );
    assert!(
        wait_for_pid_dead(sleep_pid, Duration::from_secs(5)),
        "inner sleep PID {sleep_pid} must be dead after the signal handler drains",
    );
}

/// Watch-mode cooperative-shutdown contract: in graceful mode the
/// signal handler MUST still drain registered children (so git / shell
/// subprocesses spawned mid-analysis are reaped), but MUST NOT call
/// `std::process::exit`; the helper returns cleanly with exit code 0.
/// Regression coverage for the BLOCK from Codex's review of #477:
/// without this, watch held SIGINT delivery until `analyze_and_report`
/// completed naturally, defeating the entire "Ctrl+C reaps in-flight
/// git work" contract.
#[test]
fn sigint_in_graceful_mode_drains_children_but_does_not_exit() {
    let (mut fallow, sleep_pid) = spawn_signal_helper_with(true);
    assert!(
        !wait_for_pid_dead(sleep_pid, Duration::from_millis(100)),
        "inner sleep PID {sleep_pid} should be alive after helper start"
    );
    send_signal(fallow.id(), "-INT");
    let status = fallow
        .wait_timeout_compat(Duration::from_secs(10))
        .expect("graceful helper exits within 10s");
    assert_eq!(
        status.code(),
        Some(0),
        "graceful mode must exit cleanly with code 0; got {:?}",
        status.code(),
    );
    assert!(
        wait_for_pid_dead(sleep_pid, Duration::from_secs(5)),
        "inner sleep PID {sleep_pid} must still be drained in graceful mode (BLOCK regression)",
    );
}

// Tiny polling-wait helper so we do not need to add `wait-timeout` as
// a dev-dep. std::process::Child::wait blocks indefinitely; we poll
// `try_wait` with a small sleep until the deadline.
trait WaitTimeoutCompat {
    fn wait_timeout_compat(&mut self, dur: Duration) -> Option<std::process::ExitStatus>;
}

impl WaitTimeoutCompat for std::process::Child {
    fn wait_timeout_compat(&mut self, dur: Duration) -> Option<std::process::ExitStatus> {
        let deadline = Instant::now() + dur;
        while Instant::now() < deadline {
            if let Ok(Some(status)) = self.try_wait() {
                return Some(status);
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        // Last-ditch: blocking wait so we do not leak a zombie.
        let _ = self.kill();
        self.wait().ok()
    }
}
