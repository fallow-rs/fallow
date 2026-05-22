//! RAII wrapper around `std::process::Child` that registers the child's
//! PID with the process-wide signal registry on spawn and deregisters
//! on drop or explicit consume (`wait_with_output`, `wait`).
//!
//! Storage model: the wrapper OWNS the `Child` outright. The registry
//! stores only the PID. On signal, `registry::drain_and_kill` kills by
//! PID (`kill -9 <pid>` subprocess on Unix, `TerminateProcess` on
//! Windows), which does not require ownership of the `Child`. The
//! wrapper's wait then returns with a non-zero status; callers handle
//! that the same way they would handle any subprocess failure.
//!
//! Why PID-based and not Child-based: the wrapper needs to call
//! `Child::wait_with_output(self)` which consumes the Child by value.
//! If the registry also held the Child, there would be no clean way to
//! transfer ownership for the wait while still letting the signal
//! handler kill it. Storing the PID sidesteps the problem entirely:
//! kill-by-PID is a side channel that does not interfere with wait.
//!
//! Known race (small window, low consequence): a child that completes
//! naturally is reaped inside `wait_with_output` BEFORE we deregister
//! its PID from the registry. If a signal arrives in the microseconds-
//! wide window between `wait_with_output` returning and `deregister`
//! running, the drain snapshots a now-recycled PID and sends `kill -9`
//! to whatever process the kernel assigned that PID to. The window is
//! small (one async-write to a Mutex), the consequence is one stray
//! SIGKILL during shutdown, and recovery requires a more invasive
//! design (an `Arc<Mutex<Option<Child>>>` shared with the registry).
//! Documented here so future maintainers don't re-derive the trade-off.

use std::io;
use std::process::{Child, ChildStdin, Command, ExitStatus, Output, Stdio};

use super::registry;

/// RAII handle wrapping a spawned `Child` with registry tracking.
pub struct ScopedChild {
    /// `None` after the wrapper has consumed the child (`wait_with_output`,
    /// `wait`). Drop checks this and reaps non-blockingly if the child
    /// is still here.
    inner: Option<Child>,
    /// Registry key. `None` after deregister so Drop does not redo it.
    id: Option<u64>,
}

impl ScopedChild {
    /// Spawn the command and register the resulting child's PID.
    pub fn spawn(command: &mut Command) -> io::Result<Self> {
        let child = command.spawn()?;
        let id = registry::register(child.id());
        Ok(Self {
            inner: Some(child),
            id: Some(id),
        })
    }

    /// OS-level process id of the underlying child. Returns `0` if the
    /// child has been consumed; used by the test-helper subcommand to
    /// surface the PID so integration tests can probe its liveness.
    ///
    /// The only call site is the cfg(unix) `signal_test_helper` in
    /// `main.rs`; on Windows the helper is excluded so this method has
    /// no caller. Tag with `dead_code` exempt rather than gating with
    /// `#[cfg(unix)]` to keep the public surface symmetric for embedders.
    #[cfg_attr(
        not(unix),
        allow(
            dead_code,
            reason = "only consumed by the cfg(unix) signal_test_helper in main.rs; the lint does not consistently fire on Windows under -D warnings so allow is safer than expect"
        )
    )]
    pub fn id(&self) -> u32 {
        self.inner.as_ref().map_or(0, Child::id)
    }

    /// Take the child's stdin handle, if it was piped. Same semantics
    /// as `Child::stdin.take()`. Returns `None` if stdin was not piped
    /// or the child has been consumed.
    pub fn take_stdin(&mut self) -> Option<ChildStdin> {
        self.inner.as_mut().and_then(|c| c.stdin.take())
    }

    /// Consume self and wait for the child to exit, collecting stdout
    /// and stderr. The signal handler may have already killed the
    /// child via the PID side channel; in that case wait returns
    /// normally with a non-zero status.
    pub fn wait_with_output(mut self) -> io::Result<Output> {
        let child = self.inner.take().expect("inner already taken");
        let id = self.id.take();
        let result = child.wait_with_output();
        if let Some(id) = id {
            registry::deregister(id);
        }
        result
    }

    /// Wait for the child to exit, returning the status. Same signal-
    /// kill-by-PID semantics as `wait_with_output`.
    pub fn wait(mut self) -> io::Result<ExitStatus> {
        let mut child = self.inner.take().expect("inner already taken");
        let id = self.id.take();
        let result = child.wait();
        if let Some(id) = id {
            registry::deregister(id);
        }
        result
    }
}

impl Drop for ScopedChild {
    fn drop(&mut self) {
        if let Some(id) = self.id.take() {
            registry::deregister(id);
        }
        // Non-blocking reap so the PID is released if the child has
        // already exited. Callers wanting a real wait should call
        // `wait` / `wait_with_output` explicitly; Drop never blocks.
        if let Some(mut child) = self.inner.take() {
            let _ = child.try_wait();
        }
    }
}

/// Convenience: spawn and wait for exit, returning the status.
pub fn status(command: &mut Command) -> io::Result<ExitStatus> {
    let scoped = ScopedChild::spawn(command)?;
    scoped.wait()
}

/// Convenience: spawn and collect full output (stdout + stderr).
///
/// Mirrors `Command::output` semantics by unconditionally setting
/// stdout / stderr to piped and stdin to null. Callers that need
/// different stdio (e.g. inherited stdin for interactive prompts)
/// must use `ScopedChild::spawn` directly and drive the wait
/// themselves.
pub fn output(command: &mut Command) -> io::Result<Output> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    ScopedChild::spawn(command)?.wait_with_output()
}

// Every test in this module exec's `/bin/sh` / `true` / `echo`, so the
// entire suite is cfg(unix)-only. Gating at the module level keeps the
// Windows `unused_imports` lint quiet on `use super::*;` (since the
// module would otherwise be reduced to just the import line on Windows).
#[cfg(all(test, unix))]
mod tests {
    use super::*;

    // The only callers (`scoped_child_drop_deregisters` and
    // `scoped_child_wait_deregisters_and_succeeds`) are cfg(unix), so on
    // Windows this helper has no consumer. Gate with cfg(unix) rather than
    // a per-arm `#[expect(dead_code)]` to keep the intent obvious.
    #[cfg(unix)]
    fn assert_deregistered(id: u64) {
        // The registry is private; deregister is idempotent so calling
        // it again is the cheapest way to assert "no longer present".
        registry::deregister(id);
    }

    #[test]
    #[cfg(unix)]
    fn scoped_child_drop_deregisters() {
        let mut cmd = Command::new("true");
        let child = ScopedChild::spawn(&mut cmd).expect("spawn true");
        let id = child.id.expect("freshly spawned wrapper has an id");
        assert!(id > 0);
        drop(child);
        assert_deregistered(id);
    }

    #[test]
    #[cfg(unix)]
    fn scoped_child_wait_deregisters_and_succeeds() {
        let mut cmd = Command::new("true");
        let child = ScopedChild::spawn(&mut cmd).expect("spawn true");
        let id = child.id.expect("freshly spawned wrapper has an id");
        let status = child.wait().expect("wait true");
        assert!(status.success());
        assert_deregistered(id);
    }

    #[test]
    #[cfg(unix)]
    fn output_helper_collects_stdout() {
        let mut cmd = Command::new("echo");
        cmd.arg("hello")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let output = output(&mut cmd).expect("echo");
        assert!(output.status.success());
        assert_eq!(output.stdout, b"hello\n");
    }
}
