//! Process-wide registry of live spawned-child PIDs.
//!
//! Keyed by a monotonic `AtomicU64` counter rather than `Child::id()`
//! because POSIX recycles PIDs aggressively on long-running runners; a
//! recycled PID would collide with a previously-deregistered entry.
//!
//! Stores PIDs (not `Child` handles): the `ScopedChild` wrapper owns
//! the `Child` outright so it can call `wait_with_output` / `wait`
//! normally, and the signal handler kills by PID via a `kill -9
//! <pid>` shell subprocess on Unix (avoids adding `libc` as a
//! workspace dep) or `OpenProcess + TerminateProcess` on Windows. No
//! ownership transfer, no race between wait and kill.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use rustc_hash::FxHashMap;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);
static REGISTRY: OnceLock<Mutex<FxHashMap<u64, u32>>> = OnceLock::new();

/// One-shot guard: repeated signals during drain (signal storm) no-op
/// the second-and-onwards entries.
static DRAINING: AtomicU64 = AtomicU64::new(0);

fn registry() -> &'static Mutex<FxHashMap<u64, u32>> {
    REGISTRY.get_or_init(|| Mutex::new(FxHashMap::default()))
}

/// Register `pid`. Returns a monotonic key the caller stores in their
/// `ScopedChild` for deregister at wait/drop time.
pub(super) fn register(pid: u32) -> u64 {
    let id = NEXT_ID.fetch_add(1, Ordering::SeqCst);
    registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(id, pid);
    id
}

/// Remove the registry entry for `id`. Idempotent.
pub(super) fn deregister(id: u64) {
    registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&id);
}

/// Snapshot every registered PID and kill each. Polls for liveness
/// with a bounded budget. Caller is the platform signal handler thread.
///
/// First-call-wins via the `DRAINING` guard: subsequent invocations
/// during the same shutdown skip the body to avoid re-entering the
/// lock under signal storm.
pub(super) fn drain_and_kill() {
    if DRAINING
        .compare_exchange(0, 1, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return;
    }

    let pids: Vec<u32> = {
        registry()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .drain()
            .map(|(_id, pid)| pid)
            .collect()
    };

    for pid in &pids {
        kill_pid(*pid);
    }

    let deadline = Instant::now() + drain_budget();
    while Instant::now() < deadline {
        if !pids.iter().copied().any(pid_is_alive) {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[cfg(unix)]
fn kill_pid(pid: u32) {
    // SIGKILL has the value 9 on every POSIX system fallow targets.
    // No libc dep in the workspace, so fork `/bin/kill -9 <pid>`
    // instead. Costs one extra process per signal delivery, which
    // happens at most once per fallow invocation, so the overhead is
    // negligible. PIDs from Child::id() are always positive; pid 0 / -1
    // (broadcast semantics) cannot occur on this path.
    let _ = std::process::Command::new("kill")
        .args(["-9", &pid.to_string()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

#[cfg(windows)]
fn kill_pid(pid: u32) {
    use windows_sys::Win32::Foundation::{CloseHandle, FALSE, HANDLE};
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_TERMINATE, TerminateProcess};
    // SAFETY: OpenProcess returns null on failure (which we check),
    // TerminateProcess with exit code 1 is a no-op if the handle is
    // null. CloseHandle on a valid handle is well-defined.
    unsafe {
        let handle: HANDLE = OpenProcess(PROCESS_TERMINATE, FALSE, pid);
        if handle.is_null() {
            return;
        }
        let _ = TerminateProcess(handle, 1);
        let _ = CloseHandle(handle);
    }
}

#[cfg(not(any(unix, windows)))]
fn kill_pid(_pid: u32) {
    // Unknown platform; no kill primitive available.
}

#[cfg(unix)]
fn pid_is_alive(pid: u32) -> bool {
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

#[cfg(windows)]
fn pid_is_alive(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, FALSE, HANDLE, WAIT_OBJECT_0};
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, WaitForSingleObject,
    };
    // SAFETY: identical safety contract as kill_pid.
    unsafe {
        let handle: HANDLE = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, pid);
        if handle.is_null() {
            return false;
        }
        let result = WaitForSingleObject(handle, 0);
        let _ = CloseHandle(handle);
        result != WAIT_OBJECT_0
    }
}

#[cfg(not(any(unix, windows)))]
fn pid_is_alive(_pid: u32) -> bool {
    false
}

#[cfg(unix)]
const fn drain_budget() -> Duration {
    Duration::from_millis(500)
}

#[cfg(windows)]
const fn drain_budget() -> Duration {
    Duration::from_millis(1500)
}

#[cfg(not(any(unix, windows)))]
const fn drain_budget() -> Duration {
    Duration::from_millis(500)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_deregister_roundtrip() {
        let id = register(42);
        assert!(id > 0);
        deregister(id);
        // Idempotent: second deregister is a no-op.
        deregister(id);
    }

    #[test]
    fn ids_are_monotonic() {
        let a = register(100);
        let b = register(200);
        assert!(b > a);
        deregister(a);
        deregister(b);
    }
}
