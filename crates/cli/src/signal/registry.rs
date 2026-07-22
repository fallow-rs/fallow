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

static NEXT_ID: AtomicU64 = AtomicU64::new(1);
static REGISTRY: OnceLock<Mutex<FxHashMap<u64, KillTarget>>> = OnceLock::new();

#[derive(Clone)]
enum KillTarget {
    Process(u32),
    ProcessTree(ProcessTreeHandle),
}

pub(super) type ProcessTreeHandle = Arc<ProcessTree>;

#[cfg(windows)]
struct WindowsHandle(isize);

#[cfg(windows)]
impl WindowsHandle {
    fn raw(&self) -> windows_sys::Win32::Foundation::HANDLE {
        self.0 as _
    }
}

#[cfg(windows)]
#[expect(unsafe_code, reason = "owned Windows handles require CloseHandle")]
impl Drop for WindowsHandle {
    fn drop(&mut self) {
        use windows_sys::Win32::Foundation::CloseHandle;

        // SAFETY: The handle is owned by this value and Drop runs once.
        unsafe { CloseHandle(self.raw()) };
    }
}

#[cfg(windows)]
struct WindowsJobGuard {
    job: Option<WindowsHandle>,
}

#[cfg(windows)]
impl WindowsJobGuard {
    fn new(job: WindowsHandle) -> Self {
        Self { job: Some(job) }
    }

    fn raw(&self) -> std::io::Result<windows_sys::Win32::Foundation::HANDLE> {
        self.job
            .as_ref()
            .map(WindowsHandle::raw)
            .ok_or_else(|| std::io::Error::other("Windows Job Object guard is disarmed"))
    }

    fn disarm(mut self) -> std::io::Result<WindowsHandle> {
        self.job
            .take()
            .ok_or_else(|| std::io::Error::other("Windows Job Object guard is already disarmed"))
    }
}

#[cfg(windows)]
#[expect(
    unsafe_code,
    reason = "armed Windows Job Object cleanup requires TerminateJobObject"
)]
impl Drop for WindowsJobGuard {
    fn drop(&mut self) {
        use windows_sys::Win32::System::JobObjects::TerminateJobObject;

        let Some(job) = self.job.as_ref() else {
            return;
        };
        // SAFETY: The guard owns the live Job Object handle. WindowsHandle
        // closes it immediately after this Drop implementation.
        unsafe { TerminateJobObject(job.raw(), 1) };
    }
}

/// Owned operating-system primitive for one subprocess tree.
pub(super) struct ProcessTree {
    #[cfg(unix)]
    process_group_id: i32,
    #[cfg(windows)]
    job: WindowsHandle,
}

impl ProcessTree {
    pub(super) fn configure_command(command: &mut std::process::Command) {
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;

            command.process_group(0);
        }

        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;

            use windows_sys::Win32::System::Threading::CREATE_SUSPENDED;

            command.creation_flags(CREATE_SUSPENDED);
        }

        #[cfg(not(any(unix, windows)))]
        let _ = command;
    }

    #[cfg(unix)]
    pub(super) fn for_child(child: &std::process::Child) -> std::io::Result<Self> {
        let process_group_id = i32::try_from(child.id()).map_err(|_| {
            std::io::Error::other(format!("invalid fallow subprocess PID {}", child.id()))
        })?;
        Ok(Self { process_group_id })
    }

    #[cfg(windows)]
    #[expect(unsafe_code, reason = "Windows Job Objects require Win32 FFI calls")]
    pub(super) fn for_child(child: &std::process::Child) -> std::io::Result<Self> {
        use std::mem;
        use std::os::windows::io::AsRawHandle;
        use std::ptr;

        use windows_sys::Win32::System::JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
            SetInformationJobObject,
        };

        // SAFETY: Both pointers are null by contract, creating an unnamed job
        // with default security attributes.
        let job = unsafe { CreateJobObjectW(ptr::null(), ptr::null()) };
        if job.is_null() {
            return Err(std::io::Error::last_os_error());
        }
        let job = WindowsJobGuard::new(WindowsHandle(job as isize));
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;

        // SAFETY: The buffer has the exact information-class layout and remains
        // alive for the duration of this call.
        if unsafe {
            SetInformationJobObject(
                job.raw()?,
                JobObjectExtendedLimitInformation,
                (&raw const limits).cast(),
                mem::size_of_val(&limits) as u32,
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error());
        }

        // SAFETY: `job` is live and the process handle is borrowed from the
        // freshly spawned, still-suspended child.
        if unsafe { AssignProcessToJobObject(job.raw()?, child.as_raw_handle().cast()) } == 0 {
            return Err(std::io::Error::last_os_error());
        }

        Ok(Self { job: job.disarm()? })
    }

    #[cfg(not(any(unix, windows)))]
    pub(super) fn for_child(_child: &std::process::Child) -> std::io::Result<Self> {
        Ok(Self {})
    }

    #[cfg(windows)]
    pub(super) fn start(&self, pid: u32) -> std::io::Result<()> {
        resume_suspended_process(pid)
    }

    #[cfg(unix)]
    #[expect(
        unsafe_code,
        reason = "POSIX process-group termination requires libc::kill"
    )]
    pub(super) fn terminate(&self) -> std::io::Result<()> {
        // SAFETY: A negative PID targets the dedicated process group created by
        // process_group(0). SIGKILL has no borrowed-memory requirements.
        if unsafe { libc::kill(-self.process_group_id, libc::SIGKILL) } == 0 {
            return Ok(());
        }
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ESRCH) {
            return Ok(());
        }
        Err(error)
    }

    #[cfg(windows)]
    #[expect(
        unsafe_code,
        reason = "Windows Job Object termination requires TerminateJobObject"
    )]
    pub(super) fn terminate(&self) -> std::io::Result<()> {
        use windows_sys::Win32::System::JobObjects::TerminateJobObject;

        // SAFETY: The handle remains owned by this ProcessTree until its last
        // shared handle is dropped.
        if unsafe { TerminateJobObject(self.job.raw(), 1) } != 0 {
            return Ok(());
        }
        Err(std::io::Error::last_os_error())
    }

    #[cfg(not(any(unix, windows)))]
    pub(super) fn terminate(&self) -> std::io::Result<()> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "process-tree termination is unsupported on this platform",
        ))
    }

    #[cfg(unix)]
    #[expect(
        unsafe_code,
        reason = "POSIX process-group liveness checks require libc::kill"
    )]
    pub(super) fn is_alive(&self) -> bool {
        // SAFETY: Signal 0 checks existence without delivering a signal.
        unsafe { libc::kill(-self.process_group_id, 0) == 0 }
    }

    #[cfg(windows)]
    #[expect(
        unsafe_code,
        reason = "Windows Job Object liveness requires QueryInformationJobObject"
    )]
    pub(super) fn is_alive(&self) -> bool {
        use std::mem;
        use std::ptr;

        use windows_sys::Win32::System::JobObjects::{
            JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, JobObjectBasicAccountingInformation,
            QueryInformationJobObject,
        };

        let mut accounting = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
        // SAFETY: The output buffer has the requested information-class layout
        // and remains writable for the call.
        unsafe {
            QueryInformationJobObject(
                self.job.raw(),
                JobObjectBasicAccountingInformation,
                (&raw mut accounting).cast(),
                mem::size_of_val(&accounting) as u32,
                ptr::null_mut(),
            ) != 0
                && accounting.ActiveProcesses > 0
        }
    }

    #[cfg(not(any(unix, windows)))]
    pub(super) fn is_alive(&self) -> bool {
        false
    }
}

/// One-shot guard: repeated signals during drain (signal storm) no-op
/// the second-and-onwards entries.
static DRAINING: AtomicU64 = AtomicU64::new(0);

fn registry() -> &'static Mutex<FxHashMap<u64, KillTarget>> {
    REGISTRY.get_or_init(|| Mutex::new(FxHashMap::default()))
}

/// Register `pid`. Returns a monotonic key the caller stores in their
/// `ScopedChild` for deregister at wait/drop time.
pub(super) fn register(pid: u32) -> u64 {
    register_target(KillTarget::Process(pid))
}

/// Register an owned process tree for process-wide signal cleanup.
pub(super) fn register_process_tree(process_tree: ProcessTreeHandle) -> u64 {
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
fn kill_pid(pid: u32) {
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
fn kill_pid(_pid: u32) {}

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
#[expect(
    unsafe_code,
    reason = "FFI to Win32 OpenProcess/WaitForSingleObject/CloseHandle; preconditions documented inline"
)]
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

#[cfg(windows)]
fn resume_suspended_process(pid: u32) -> std::io::Result<()> {
    const THREAD_DISCOVERY_ATTEMPTS: usize = 20;
    const THREAD_DISCOVERY_DELAY: Duration = Duration::from_millis(5);

    for _ in 0..THREAD_DISCOVERY_ATTEMPTS {
        match find_process_thread(pid) {
            Ok(thread) => return resume_thread(&thread),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                std::thread::sleep(THREAD_DISCOVERY_DELAY);
            }
            Err(error) => return Err(error),
        }
    }

    Err(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        format!("could not find suspended primary thread for fallow subprocess {pid}"),
    ))
}

#[cfg(windows)]
#[expect(
    unsafe_code,
    reason = "thread discovery requires Windows ToolHelp FFI calls"
)]
fn find_process_thread(pid: u32) -> std::io::Result<WindowsHandle> {
    use std::mem;

    use windows_sys::Win32::Foundation::{ERROR_NO_MORE_FILES, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
    };
    use windows_sys::Win32::System::Threading::{OpenThread, THREAD_SUSPEND_RESUME};

    // SAFETY: The flags and process ID follow the ToolHelp API contract.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err(std::io::Error::last_os_error());
    }
    let snapshot = WindowsHandle(snapshot as isize);
    let mut entry = THREADENTRY32 {
        dwSize: mem::size_of::<THREADENTRY32>() as u32,
        ..THREADENTRY32::default()
    };

    // SAFETY: `entry` has the required size and remains valid for the call.
    if unsafe { Thread32First(snapshot.raw(), &raw mut entry) } == 0 {
        return Err(thread_enumeration_error(pid, ERROR_NO_MORE_FILES));
    }

    loop {
        if entry.th32OwnerProcessID == pid {
            // SAFETY: The thread ID came from a live ToolHelp snapshot.
            let thread = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID) };
            if thread.is_null() {
                return Err(std::io::Error::last_os_error());
            }
            return Ok(WindowsHandle(thread as isize));
        }

        // SAFETY: `entry` remains initialized with the required size.
        if unsafe { Thread32Next(snapshot.raw(), &raw mut entry) } == 0 {
            return Err(thread_enumeration_error(pid, ERROR_NO_MORE_FILES));
        }
    }
}

#[cfg(windows)]
fn thread_enumeration_error(pid: u32, no_more_files: u32) -> std::io::Error {
    let error = std::io::Error::last_os_error();
    if error.raw_os_error() == i32::try_from(no_more_files).ok() {
        return std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("no thread found for fallow subprocess {pid}"),
        );
    }
    error
}

#[cfg(windows)]
#[expect(
    unsafe_code,
    reason = "resuming a Windows thread requires ResumeThread"
)]
fn resume_thread(thread: &WindowsHandle) -> std::io::Result<()> {
    use windows_sys::Win32::System::Threading::ResumeThread;

    // SAFETY: The handle was opened with THREAD_SUSPEND_RESUME access.
    let previous_count = unsafe { ResumeThread(thread.raw()) };
    if previous_count == u32::MAX {
        return Err(std::io::Error::last_os_error());
    }
    if previous_count == 0 {
        return Err(std::io::Error::other(
            "fallow subprocess primary thread was not suspended",
        ));
    }

    Ok(())
}

#[cfg(test)]
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
        let process_tree = Arc::new(ProcessTree {
            process_group_id: 42,
        });
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
        ProcessTree::configure_command(&mut command);
        let mut leader = command.spawn().expect("spawn process tree");
        let process_tree = Arc::new(ProcessTree::for_child(&leader).expect("own process tree"));
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
