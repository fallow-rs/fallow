//! Unix signal listener.
//!
//! Spawns a dedicated `std::thread` that does a blocking `sigwait`
//! (via `signal_hook::iterator::Signals`) over [`DRAINED_SIGNALS`]. The
//! listener thread body is regular Rust: locks, allocations, and stderr
//! writes are all allowed because the thread is NOT running inside a real
//! signal handler. signal-hook installs a small handler that pipes the
//! signal number to the listener, then the listener wakes via `recv`.
//!
//! This sidesteps async-signal-safety entirely; see signal-hook's own docs
//! ("Anatomy of the crate") for the rationale.

use std::io;
use std::thread;

use signal_hook::consts::signal::{SIGHUP, SIGINT, SIGQUIT, SIGTERM};
use signal_hook::iterator::Signals;

use super::handle_signal;

/// The signals that kill the registered children before the exit. A closed
/// terminal sends SIGHUP and Ctrl+\ sends SIGQUIT. Without a handler, each
/// one ends fallow and leaves children in their own process group, such as
/// the mutation companions, running.
const DRAINED_SIGNALS: [i32; 4] = [SIGINT, SIGTERM, SIGHUP, SIGQUIT];

/// The conventional `128 + signum` exit code of `signal`.
const fn exit_code(signal: i32) -> i32 {
    match signal {
        SIGHUP => 129,
        SIGINT => 130,
        SIGQUIT => 131,
        SIGTERM => 143,
        _ => 128,
    }
}

/// True when the process started with `signal` ignored.
#[expect(
    unsafe_code,
    reason = "reading a signal disposition requires sigaction FFI"
)]
fn is_ignored(signal: i32) -> bool {
    let mut current = std::mem::MaybeUninit::<libc::sigaction>::zeroed();
    // SAFETY: a null new action only reads the current disposition into
    // `current`, which is a valid, writable `sigaction`.
    let result = unsafe { libc::sigaction(signal, std::ptr::null(), current.as_mut_ptr()) };
    if result != 0 {
        return false;
    }
    // SAFETY: `sigaction` returned 0, so it wrote `current`.
    let current = unsafe { current.assume_init() };
    current.sa_sigaction == libc::SIG_IGN
}

/// True when fallow installs a handler for `signal`. `nohup` and daemon
/// wrappers start fallow with SIGHUP ignored, so that a closed terminal does
/// not end the run. A handler would replace that inherited disposition, so
/// fallow keeps an ignored SIGHUP ignored, as a POSIX shell does.
fn is_drained(signal: i32) -> bool {
    signal != SIGHUP || !is_ignored(SIGHUP)
}

/// Install handlers for [`DRAINED_SIGNALS`], except an inherited ignored
/// SIGHUP. Spawns a daemon thread that outlives every fallow subcommand; the
/// OS reaps it on process exit.
pub fn install() -> io::Result<()> {
    let drained = DRAINED_SIGNALS
        .into_iter()
        .filter(|signal| is_drained(*signal));
    let mut signals = Signals::new(drained)?;
    thread::Builder::new()
        .name("fallow-signal-listener".into())
        .spawn(move || {
            for signal in signals.forever() {
                handle_signal(exit_code(signal));
            }
        })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_drained_signal_exits_with_128_plus_its_number() {
        for signal in DRAINED_SIGNALS {
            assert_eq!(exit_code(signal), 128 + signal, "signal {signal}");
        }
    }
}
