//! Process-wide registry of live spawned-child PIDs.
//!
//! Keyed by a monotonic `AtomicU64` counter rather than `Child::id()`
//! because POSIX recycles PIDs aggressively on long-running runners; a
//! recycled PID would collide with a previously-deregistered entry.
//!
//! Stores termination targets (not `Child` handles): the `ScopedChild` wrapper
//! owns the `Child` outright so it can call `wait_with_output` / `wait`
//! normally. Most children register one PID; subprocess wrappers can register
//! a dedicated process tree so signal cleanup also terminates descendants.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use rustc_hash::FxHashMap;

use crate::process_tree::ProcessTree;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);
static REGISTRY: OnceLock<Mutex<FxHashMap<u64, KillTarget>>> = OnceLock::new();

#[derive(Clone)]
enum KillTarget {
    Process(u32),
    ProcessTree(ProcessTreeHandle),
}

pub type ProcessTreeHandle = Arc<ProcessTree>;

/// One-shot guard: repeated signals during drain (signal storm) no-op
/// the second-and-onwards entries.
static DRAINING: AtomicU64 = AtomicU64::new(0);

fn registry() -> &'static Mutex<FxHashMap<u64, KillTarget>> {
    REGISTRY.get_or_init(|| Mutex::new(FxHashMap::default()))
}

/// Register `pid`. Returns a monotonic key the caller stores in their
/// `ScopedChild` for deregister at wait/drop time.
pub fn register(pid: u32) -> u64 {
    register_target(KillTarget::Process(pid))
}

/// Register an owned process tree for process-wide signal cleanup.
pub fn register_process_tree(process_tree: ProcessTreeHandle) -> u64 {
    register_target(KillTarget::ProcessTree(process_tree))
}

fn register_target(target: KillTarget) -> u64 {
    let id = NEXT_ID.fetch_add(1, Ordering::SeqCst);
    registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(id, target);
    id
}

/// Remove the registry entry for `id`. Idempotent.
pub fn deregister(id: u64) {
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
pub fn drain_and_kill() {
    if DRAINING
        .compare_exchange(0, 1, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return;
    }

    let targets: Vec<KillTarget> = {
        registry()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .drain()
            .map(|(_id, target)| target)
            .collect()
    };

    for target in &targets {
        kill_target(target);
    }

    let deadline = Instant::now() + drain_budget();
    while Instant::now() < deadline {
        if !targets.iter().any(target_is_alive) {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn kill_target(target: &KillTarget) {
    match target {
        KillTarget::Process(pid) => kill_pid(*pid),
        KillTarget::ProcessTree(process_tree) => {
            let _ = process_tree.terminate();
        }
    }
}

fn target_is_alive(target: &KillTarget) -> bool {
    match target {
        KillTarget::Process(pid) => pid_is_alive(*pid),
        KillTarget::ProcessTree(process_tree) => process_tree.is_alive(),
    }
}

#[cfg(unix)]
pub fn kill_pid(pid: u32) {
    let _ = std::process::Command::new("kill")
        .args(["-9", &pid.to_string()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

#[cfg(windows)]
#[expect(
    unsafe_code,
    reason = "FFI to Win32 OpenProcess/TerminateProcess/CloseHandle; preconditions documented inline"
)]
pub fn kill_pid(pid: u32) {
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
pub fn kill_pid(_pid: u32) {}

#[cfg(unix)]
pub fn pid_is_alive(pid: u32) -> bool {
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

#[cfg(windows)]
#[expect(
    unsafe_code,
    reason = "FFI to Win32 OpenProcess/WaitForSingleObject/CloseHandle; preconditions documented inline"
)]
pub fn pid_is_alive(pid: u32) -> bool {
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
pub fn pid_is_alive(_pid: u32) -> bool {
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
#[expect(
    clippy::expect_used,
    reason = "test setup failures should fail at the exact setup operation"
)]
mod tests {
    use super::*;

    #[test]
    fn register_deregister_roundtrip() {
        let id = register(42);
        assert!(id > 0);
        deregister(id);
        deregister(id);
    }

    #[cfg(unix)]
    #[test]
    fn register_process_tree_roundtrip() {
        let process_tree = Arc::new(ProcessTree::for_pid(42).expect("test process tree"));
        let id = register_process_tree(Arc::clone(&process_tree));
        assert!(id > 0);
        assert!(matches!(
            registry()
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .get(&id),
            Some(KillTarget::ProcessTree(registered))
                if Arc::ptr_eq(registered, &process_tree)
        ));
        deregister(id);
    }

    #[cfg(unix)]
    #[test]
    fn process_tree_target_terminates_descendants() {
        use std::fs;

        let root = tempfile::tempdir().expect("temporary process-tree root");
        let mut command = std::process::Command::new("sh");
        command
            .args([
                "-c",
                "sleep 30 & child=$!; printf '%s' \"$child\" > child.pid; wait \"$child\"",
            ])
            .current_dir(root.path());
        crate::process_tree::configure_std_command(&mut command);
        let mut leader = command.spawn().expect("spawn process tree");
        let process_tree = Arc::new(ProcessTree::for_std_child(&leader).expect("own process tree"));
        let pid_path = root.path().join("child.pid");
        let deadline = Instant::now() + Duration::from_secs(2);
        while !pid_path.is_file() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        let child_pid = fs::read_to_string(&pid_path)
            .expect("descendant pid")
            .trim()
            .parse::<u32>()
            .expect("numeric descendant pid");

        kill_target(&KillTarget::ProcessTree(process_tree));
        let _ = leader.wait();
        let deadline = Instant::now() + Duration::from_secs(2);
        while pid_is_alive(child_pid) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(!pid_is_alive(child_pid), "descendant survived tree cleanup");
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
