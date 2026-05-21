//! Unix signal listener.
//!
//! Spawns a dedicated `std::thread` that does a blocking `sigwait`
//! (via `signal_hook::iterator::Signals`) over `[SIGINT, SIGTERM]`. The
//! listener thread body is regular Rust: locks, allocations, and stderr
//! writes are all allowed because the thread is NOT running inside a real
//! signal handler. signal-hook installs a small handler that pipes the
//! signal number to the listener, then the listener wakes via `recv`.
//!
//! This sidesteps async-signal-safety entirely; see signal-hook's own docs
//! ("Anatomy of the crate") for the rationale.

use std::io;
use std::thread;

use signal_hook::consts::signal::{SIGINT, SIGTERM};
use signal_hook::iterator::Signals;

use super::handle_signal;

/// Install handlers for SIGINT + SIGTERM. Spawns a daemon thread that
/// outlives every fallow subcommand; the OS reaps it on process exit.
pub fn install() -> io::Result<()> {
    let mut signals = Signals::new([SIGINT, SIGTERM])?;
    thread::Builder::new()
        .name("fallow-signal-listener".into())
        .spawn(move || {
            for signal in signals.forever() {
                let exit_code = match signal {
                    SIGINT => 130,
                    SIGTERM => 143,
                    // signal-hook only delivers the signals we registered.
                    _ => 128,
                };
                handle_signal(exit_code);
                // If we returned from handle_signal we are in graceful
                // mode; keep listening for the next signal. A second
                // SIGINT in graceful mode is still graceful (the watch
                // loop polls the shutdown flag and exits cleanly).
            }
        })?;
    Ok(())
}
