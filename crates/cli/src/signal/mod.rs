//! Process-wide signal handling and scoped child-process registry.
//!
//! On SIGINT or SIGTERM (Unix) and the equivalent console-control events on
//! Windows, fallow's default unwind drops `std::process::Child` handles
//! without killing the underlying children. The `fallow-cov` sidecar,
//! `npm install -g`, and self-invoked `fallow health` can run for minutes
//! and accumulate as orphan processes when the user hits Ctrl+C.
//!
//! This module installs a single handler (see `install_handlers`) that on
//! signal delivery: kills every `ScopedChild` currently registered, drains
//! them with a bounded budget (500ms Unix, 1500ms Windows), and exits with
//! the conventional 128+signum exit code (130 for SIGINT, 143 for SIGTERM).
//!
//! Watch mode opts into cooperative shutdown via `set_graceful_mode`: the
//! handler then only flips the shutdown flag and returns, letting the watch
//! loop exit cleanly with code 0 because Ctrl+C is its documented
//! termination path. Other commands keep the forceful 128+signum behavior.
//!
//! See `.plans/issue-477-signal-handlers.md` for the design rationale and
//! `crates/lsp/src/main.rs` for the LSP-side cooperative cancellation.

pub mod registry;
pub mod scoped_child;

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

pub use scoped_child::ScopedChild;

/// True once a termination signal has been observed.
static SHUTDOWN: AtomicBool = AtomicBool::new(false);

/// True when a cooperative consumer (`fallow watch`) is active. The handler
/// then flips `SHUTDOWN` and returns instead of killing children and
/// exiting; the consumer is responsible for clean teardown.
static GRACEFUL: AtomicBool = AtomicBool::new(false);

/// Idempotency guard for `install_handlers`. Repeated calls (e.g. when
/// `run_watch` reinstalls the handler) are silently no-ops.
static INSTALLED: OnceLock<()> = OnceLock::new();

/// Install the signal handler. Idempotent; safe to call multiple times.
/// Returns the original error from the underlying primitive on first call
/// failure.
pub fn install_handlers() -> std::io::Result<()> {
    if INSTALLED.get().is_some() {
        return Ok(());
    }
    let result = platform_install();
    if result.is_ok() {
        let _ = INSTALLED.set(());
    }
    result
}

/// True after a signal has been observed. Read by long-running loops
/// (currently `fallow watch`) to break out cooperatively.
pub fn is_shutting_down() -> bool {
    SHUTDOWN.load(Ordering::SeqCst)
}

/// Enter cooperative shutdown mode. Subsequent signals set `SHUTDOWN`
/// without killing children or calling `exit()`. The caller is responsible
/// for polling `is_shutting_down()` and exiting cleanly.
pub fn set_graceful_mode() {
    GRACEFUL.store(true, Ordering::SeqCst);
}

/// Leave cooperative shutdown mode. Subsequent signals revert to the
/// forceful behavior (kill registered children, `exit(128 + signum)`).
pub fn clear_graceful_mode() {
    GRACEFUL.store(false, Ordering::SeqCst);
}

/// RAII guard that calls `set_graceful_mode` on construction and
/// `clear_graceful_mode` on drop. Used by `run_watch` so any return path
/// (success, panic-with-unwind in debug, early return on config error)
/// restores forceful-exit behavior for the next command.
pub struct GracefulModeGuard;

impl GracefulModeGuard {
    pub fn new() -> Self {
        set_graceful_mode();
        Self
    }
}

impl Default for GracefulModeGuard {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for GracefulModeGuard {
    fn drop(&mut self) {
        clear_graceful_mode();
    }
}

#[cfg(unix)]
fn platform_install() -> std::io::Result<()> {
    unix::install()
}

#[cfg(windows)]
fn platform_install() -> std::io::Result<()> {
    windows::install()
}

#[cfg(not(any(unix, windows)))]
fn platform_install() -> std::io::Result<()> {
    // No-op on unknown platforms; ScopedChild's Drop still cleans up
    // normal early-return paths but signal-driven cleanup is unavailable.
    Ok(())
}

/// Mark shutdown, drain the registry (kills every registered child
/// regardless of mode so in-flight subprocesses do not survive the
/// signal), then either exit (default) or return for cooperative
/// consumers in graceful mode (`fallow watch`).
///
/// Graceful mode MUST still drain children: watch's `analyze_and_
/// report` spawns git subprocesses (via `fallow_core::changed_files`
/// and `fallow_core::churn`) that need reaping mid-analysis. Without
/// drain, a Ctrl+C during analysis would let the parent return from
/// the inner pass only after every git child completed naturally,
/// defeating the entire "Ctrl+C reaps in-flight git work" contract.
/// Invoked by the platform-specific handler thread.
fn handle_signal(exit_code: i32) {
    SHUTDOWN.store(true, Ordering::SeqCst);
    registry::drain_and_kill();
    if GRACEFUL.load(Ordering::SeqCst) {
        return;
    }
    #[expect(
        clippy::exit,
        reason = "signal handler MUST terminate the process; that is the entire point of the path"
    )]
    std::process::exit(exit_code);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graceful_mode_guard_sets_and_clears() {
        assert!(!GRACEFUL.load(Ordering::SeqCst));
        {
            let _g = GracefulModeGuard::new();
            assert!(GRACEFUL.load(Ordering::SeqCst));
        }
        assert!(!GRACEFUL.load(Ordering::SeqCst));
    }

    #[test]
    fn install_handlers_is_idempotent() {
        // The first call may succeed or fail depending on test ordering
        // (signal disposition is process-global), but the second call MUST
        // be a no-op and return Ok.
        let _ = install_handlers();
        assert!(install_handlers().is_ok());
    }
}
