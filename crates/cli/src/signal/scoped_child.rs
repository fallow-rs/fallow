//! RAII wrapper around `std::process::Child` that registers the child's
//! PID with the process-wide signal registry on spawn and deregisters
//! on drop or explicit consume (`wait_with_output`, `wait`).
//!
//! Storage model: the wrapper owns the `Child` outright. Regular subprocesses
//! register their PID. Process-tree subprocesses share an owned process-group
//! or Windows Job Object handle with the signal registry and timeout watchers.
//! Termination therefore targets the same reserved tree identity without
//! executable lookup or PID reconstruction.
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
use std::process::{
    Child, ChildStderr, ChildStdin, ChildStdout, Command, ExitStatus, Output, Stdio,
};
use std::sync::Arc;

use super::registry;

/// RAII handle wrapping a spawned `Child` with registry tracking.
pub struct ScopedChild {
    /// `None` after the wrapper has consumed the child (`wait_with_output`,
    /// `wait`). Drop checks this and reaps non-blockingly if the child
    /// is still here.
    inner: Option<Child>,
    /// Registry key. `None` after deregister so Drop does not redo it.
    id: Option<u64>,
    /// Shared ownership of the reserved process-tree identity, when requested.
    process_tree: Option<registry::ProcessTreeHandle>,
}

/// Cloneable process-tree termination capability for timeout and I/O guards.
#[derive(Clone)]
pub struct ProcessTreeTerminator {
    process_tree: registry::ProcessTreeHandle,
}

impl ProcessTreeTerminator {
    pub fn terminate(&self) -> io::Result<()> {
        self.process_tree.terminate()
    }

    #[cfg(all(test, windows))]
    fn is_alive(&self) -> bool {
        self.process_tree.is_alive()
    }
}

impl ScopedChild {
    /// Spawn the command and register the resulting child's PID.
    pub fn spawn(command: &mut Command) -> io::Result<Self> {
        let child = command.spawn()?;
        let id = registry::register(child.id());
        Ok(Self {
            inner: Some(child),
            id: Some(id),
            process_tree: None,
        })
    }

    /// Spawn a subprocess wrapper in a dedicated process tree and register the
    /// tree for process-wide signal cleanup.
    pub fn spawn_process_tree(command: &mut Command) -> io::Result<Self> {
        registry::ProcessTree::configure_command(command);
        let mut child = command.spawn()?;
        let process_tree = match registry::ProcessTree::for_child(&child) {
            Ok(process_tree) => Arc::new(process_tree),
            Err(error) => {
                terminate_failed_setup(&mut child);
                return Err(error);
            }
        };
        let id = registry::register_process_tree(Arc::clone(&process_tree));
        #[cfg(windows)]
        {
            if let Err(error) = registry::ProcessTree::start(child.id()) {
                registry::deregister(id);
                let _ = process_tree.terminate();
                terminate_failed_setup(&mut child);
                return Err(error);
            }
        }
        Ok(Self {
            inner: Some(child),
            id: Some(id),
            process_tree: Some(process_tree),
        })
    }

    /// Return a cloneable handle that terminates the exact registered tree.
    pub(crate) fn process_tree_terminator(&self) -> Option<ProcessTreeTerminator> {
        self.process_tree
            .as_ref()
            .map(|process_tree| ProcessTreeTerminator {
                process_tree: Arc::clone(process_tree),
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

    /// Take the child's stdout handle, if it was piped. Same semantics as
    /// `Child::stdout.take()`. Returns `None` if stdout was not piped or the
    /// child has been consumed. Used by long-lived readers (e.g. the audit
    /// base-file `cat-file --batch` reader) that drive both pipes while leaving
    /// the wrapper owning the child for registry tracking and the terminal wait.
    pub fn take_stdout(&mut self) -> Option<ChildStdout> {
        self.inner.as_mut().and_then(|c| c.stdout.take())
    }

    /// Take the child's stderr handle, if it was piped. Same semantics as
    /// `Child::stderr.take()`.
    pub fn take_stderr(&mut self) -> Option<ChildStderr> {
        self.inner.as_mut().and_then(|child| child.stderr.take())
    }

    /// Consume self and wait for the child to exit, collecting stdout
    /// and stderr. The signal handler may have already killed the
    /// child via the registered termination target; in that case wait returns
    /// normally with a non-zero status.
    #[expect(
        clippy::expect_used,
        reason = "ScopedChild owns inner until one terminal wait method consumes it"
    )]
    pub fn wait_with_output(mut self) -> io::Result<Output> {
        let child = self.inner.take().expect("inner already taken");
        let id = self.id.take();
        let result = child.wait_with_output();
        if let Some(id) = id {
            registry::deregister(id);
        }
        result
    }

    /// Wait for the child to exit, returning the status. Same signal-cleanup
    /// semantics as `wait_with_output`.
    #[expect(
        clippy::expect_used,
        reason = "ScopedChild owns inner until one terminal wait method consumes it"
    )]
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

fn terminate_failed_setup(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

impl Drop for ScopedChild {
    fn drop(&mut self) {
        if let Some(id) = self.id.take() {
            registry::deregister(id);
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    fn assert_deregistered(id: u64) {
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

    #[cfg(windows)]
    #[test]
    fn windows_job_object_terminates_descendants_without_taskkill_lookup() {
        const HELPER_ENV: &str = "FALLOW_WINDOWS_JOB_OBJECT_TEST_ROOT";
        const TASKKILL_MARKER_ENV: &str = "FALLOW_FAKE_TASKKILL_MARKER";
        const TEST_NAME: &str = "signal::scoped_child::tests::windows_job_object_terminates_descendants_without_taskkill_lookup";

        if let Some(root) = std::env::var_os(HELPER_ENV) {
            run_windows_job_object_helper(std::path::Path::new(&root));
            return;
        }

        let root = tempfile::tempdir().expect("temporary Windows Job Object root");
        compile_fake_taskkill(root.path(), TASKKILL_MARKER_ENV);
        std::fs::write(
            root.path().join("descendant.cmd"),
            "@echo off\r\necho ready>descendant-ready\r\nping.exe -n 30 127.0.0.1 >NUL\r\n",
        )
        .expect("write descendant script");
        std::fs::write(
            root.path().join("leader.cmd"),
            "@echo off\r\nstart \"\" /B cmd.exe /D /S /C call descendant.cmd\r\nping.exe -n 30 127.0.0.1 >NUL\r\n",
        )
        .expect("write leader script");
        let mut search_paths = vec![root.path().to_path_buf()];
        if let Some(path) = std::env::var_os("PATH") {
            search_paths.extend(std::env::split_paths(&path));
        }
        let search_path =
            std::env::join_paths(search_paths).expect("prepend fake taskkill to PATH");

        let output = Command::new(std::env::current_exe().expect("current test executable"))
            .args(["--exact", TEST_NAME, "--nocapture"])
            .current_dir(root.path())
            .env(HELPER_ENV, root.path())
            .env(TASKKILL_MARKER_ENV, root.path().join("taskkill-invoked"))
            .env("PATH", search_path)
            .env_remove("NoDefaultCurrentDirectoryInExePath")
            .output()
            .expect("run Windows Job Object helper");

        assert!(
            output.status.success(),
            "helper failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            !root.path().join("taskkill-invoked").exists(),
            "cleanup executed project-local taskkill"
        );
    }

    #[cfg(windows)]
    fn compile_fake_taskkill(root: &std::path::Path, marker_env: &str) {
        let source = root.join("fake-taskkill.rs");
        let executable = root.join("taskkill.exe");
        std::fs::write(
            &source,
            format!(
                "fn main() {{ let marker = std::env::var_os({marker_env:?}).expect(\"marker path\"); std::fs::write(marker, b\"invoked\").expect(\"write marker\"); }}"
            ),
        )
        .expect("write fake taskkill source");
        let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
        let output = Command::new(rustc)
            .args(["--edition=2024", "-o"])
            .arg(&executable)
            .arg(&source)
            .output()
            .expect("compile fake taskkill executable");
        assert!(
            output.status.success(),
            "fake taskkill compilation failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[cfg(windows)]
    fn run_windows_job_object_helper(root: &std::path::Path) {
        use std::time::{Duration, Instant};

        let mut command = Command::new("cmd.exe");
        command
            .args(["/D", "/S", "/C", "call leader.cmd"])
            .current_dir(root)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let child = ScopedChild::spawn_process_tree(&mut command).expect("spawn Windows job tree");
        let terminator = child
            .process_tree_terminator()
            .expect("Windows process-tree terminator");
        let ready = root.join("descendant-ready");
        let ready_deadline = Instant::now() + Duration::from_secs(5);
        while !ready.exists() && Instant::now() < ready_deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(ready.exists(), "descendant did not start inside the job");

        let started = Instant::now();
        terminator
            .terminate()
            .expect("terminate Windows Job Object");
        let status = child.wait().expect("reap Windows job leader");
        assert!(
            !status.success(),
            "terminated job leader exited successfully"
        );
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "wait remained blocked after Job Object termination"
        );

        let exit_deadline = Instant::now() + Duration::from_secs(2);
        while terminator.is_alive() && Instant::now() < exit_deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            !terminator.is_alive(),
            "job descendants survived termination"
        );
    }
}
