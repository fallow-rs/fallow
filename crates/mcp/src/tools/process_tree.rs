use std::io;
use std::time::{Duration, Instant};

const CLEANUP_GRACE: Duration = Duration::from_secs(1);
const REAP_RETRY_GRACE: Duration = Duration::from_millis(100);
const CLEANUP_POLL_INTERVAL: Duration = Duration::from_millis(10);

/// Configure a Tokio command so its descendants can be terminated as one tree.
pub(super) fn configure_tokio_command(command: &mut tokio::process::Command) {
    configure_std_command(command.as_std_mut());
}

/// Configure a standard-library command so its descendants can be terminated as one tree.
pub(super) fn configure_std_command(command: &mut std::process::Command) {
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

    fn raw(&self) -> io::Result<windows_sys::Win32::Foundation::HANDLE> {
        self.job
            .as_ref()
            .map(WindowsHandle::raw)
            .ok_or_else(|| io::Error::other("Windows Job Object guard is disarmed"))
    }

    fn disarm(mut self) -> io::Result<WindowsHandle> {
        self.job
            .take()
            .ok_or_else(|| io::Error::other("Windows Job Object guard is already disarmed"))
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
        // SAFETY: The guard owns the live Job Object handle. Its WindowsHandle
        // field closes the handle immediately after this Drop implementation.
        unsafe { TerminateJobObject(job.raw(), 1) };
    }
}

/// Platform-specific ownership needed to terminate a spawned process tree.
pub(super) struct ProcessTree {
    #[cfg(unix)]
    process_group_id: i32,
    #[cfg(windows)]
    job: WindowsHandle,
}

impl ProcessTree {
    #[cfg(unix)]
    pub(super) fn for_tokio_child(child: &tokio::process::Child) -> io::Result<Self> {
        let pid = child
            .id()
            .ok_or_else(|| io::Error::other("fallow subprocess exited before setup"))?;
        Self::for_pid(pid)
    }

    #[cfg(windows)]
    pub(super) fn for_tokio_child(child: &tokio::process::Child) -> io::Result<Self> {
        let pid = child
            .id()
            .ok_or_else(|| io::Error::other("fallow subprocess exited before setup"))?;
        let handle = child
            .raw_handle()
            .ok_or_else(|| io::Error::other("fallow subprocess exited before setup"))?;
        Self::for_windows_handle(handle, pid)
    }

    #[cfg(not(any(unix, windows)))]
    pub(super) fn for_tokio_child(_child: &tokio::process::Child) -> io::Result<Self> {
        Ok(Self {})
    }

    #[cfg(unix)]
    pub(super) fn for_std_child(child: &std::process::Child) -> io::Result<Self> {
        Self::for_pid(child.id())
    }

    #[cfg(windows)]
    pub(super) fn for_std_child(child: &std::process::Child) -> io::Result<Self> {
        use std::os::windows::io::AsRawHandle;

        Self::for_windows_handle(child.as_raw_handle(), child.id())
    }

    #[cfg(not(any(unix, windows)))]
    pub(super) fn for_std_child(_child: &std::process::Child) -> io::Result<Self> {
        Ok(Self {})
    }

    #[cfg(unix)]
    fn for_pid(pid: u32) -> io::Result<Self> {
        let process_group_id = i32::try_from(pid)
            .map_err(|_| io::Error::other(format!("invalid fallow subprocess PID {pid}")))?;
        Ok(Self { process_group_id })
    }

    #[cfg(windows)]
    #[expect(unsafe_code, reason = "Windows Job Objects require Win32 FFI calls")]
    fn for_windows_handle(process: std::os::windows::io::RawHandle, pid: u32) -> io::Result<Self> {
        use std::ptr;

        use windows_sys::Win32::System::JobObjects::{AssignProcessToJobObject, CreateJobObjectW};

        // SAFETY: Both pointers are null by contract, creating an unnamed job
        // with default security attributes.
        let job = unsafe { CreateJobObjectW(ptr::null(), ptr::null()) };
        if job.is_null() {
            return Err(io::Error::last_os_error());
        }
        let job = WindowsJobGuard::new(WindowsHandle(job as isize));

        // SAFETY: `job` is a live handle from CreateJobObjectW and `process` is
        // borrowed from the freshly spawned child for the duration of this call.
        if unsafe { AssignProcessToJobObject(job.raw()?, process.cast()) } == 0 {
            return Err(io::Error::last_os_error());
        }

        resume_suspended_process(pid)?;
        Ok(Self { job: job.disarm()? })
    }

    #[cfg(unix)]
    #[expect(
        unsafe_code,
        reason = "POSIX process-group termination requires libc::kill"
    )]
    pub(super) fn terminate(&self) -> io::Result<()> {
        // SAFETY: A negative PID targets the dedicated process group created by
        // `process_group(0)`. SIGKILL has no borrowed-memory requirements.
        if unsafe { libc::kill(-self.process_group_id, libc::SIGKILL) } == 0 {
            return Ok(());
        }

        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ESRCH) {
            return Ok(());
        }
        Err(error)
    }

    #[cfg(windows)]
    #[expect(
        unsafe_code,
        reason = "Windows Job Object termination requires a Win32 FFI call"
    )]
    pub(super) fn terminate(&self) -> io::Result<()> {
        use windows_sys::Win32::System::JobObjects::TerminateJobObject;

        // SAFETY: The handle remains owned by this ProcessTree until Drop.
        if unsafe { TerminateJobObject(self.job.raw(), 1) } != 0 {
            return Ok(());
        }
        Err(io::Error::last_os_error())
    }

    #[cfg(not(any(unix, windows)))]
    pub(super) fn terminate(&self) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "process-tree termination is unsupported on this platform",
        ))
    }
}

pub(super) async fn cleanup_tokio_child(
    process_tree: Option<&ProcessTree>,
    child: &mut tokio::process::Child,
) -> Vec<String> {
    let mut errors = Vec::new();
    if !request_tree_termination(process_tree, &mut errors)
        && let Err(error) = child.start_kill()
    {
        errors.push(format!("failed to kill direct subprocess: {error}"));
    }

    match tokio::time::timeout(CLEANUP_GRACE, child.wait()).await {
        Ok(Ok(_)) => return errors,
        Ok(Err(error)) => {
            errors.push(format!("failed to reap direct subprocess: {error}"));
            return errors;
        }
        Err(_) => errors.push(format!(
            "direct subprocess did not exit within {}ms cleanup grace",
            CLEANUP_GRACE.as_millis()
        )),
    }

    if let Err(error) = child.start_kill() {
        errors.push(format!("failed to retry direct subprocess kill: {error}"));
    }
    match tokio::time::timeout(REAP_RETRY_GRACE, child.wait()).await {
        Ok(Ok(_)) => {}
        Ok(Err(error)) => {
            errors.push(format!(
                "failed to reap direct subprocess after retry: {error}"
            ));
        }
        Err(_) => errors.push(format!(
            "direct subprocess still did not exit after {}ms kill retry",
            REAP_RETRY_GRACE.as_millis()
        )),
    }
    errors
}

pub(super) fn cleanup_std_child(
    process_tree: Option<&ProcessTree>,
    child: &mut std::process::Child,
) -> Vec<String> {
    let mut errors = Vec::new();
    if !request_tree_termination(process_tree, &mut errors)
        && let Err(error) = child.kill()
    {
        errors.push(format!("failed to kill direct subprocess: {error}"));
    }

    match poll_std_child(child, CLEANUP_GRACE) {
        Ok(true) => return errors,
        Ok(false) => errors.push(format!(
            "direct subprocess did not exit within {}ms cleanup grace",
            CLEANUP_GRACE.as_millis()
        )),
        Err(error) => {
            errors.push(format!("failed to reap direct subprocess: {error}"));
            return errors;
        }
    }

    if let Err(error) = child.kill() {
        errors.push(format!("failed to retry direct subprocess kill: {error}"));
    }
    match poll_std_child(child, REAP_RETRY_GRACE) {
        Ok(true) => {}
        Ok(false) => errors.push(format!(
            "direct subprocess still did not exit after {}ms kill retry",
            REAP_RETRY_GRACE.as_millis()
        )),
        Err(error) => errors.push(format!(
            "failed to reap direct subprocess after retry: {error}"
        )),
    }
    errors
}

fn request_tree_termination(process_tree: Option<&ProcessTree>, errors: &mut Vec<String>) -> bool {
    let Some(process_tree) = process_tree else {
        return false;
    };
    match process_tree.terminate() {
        Ok(()) => true,
        Err(error) => {
            errors.push(format!("failed to terminate subprocess tree: {error}"));
            false
        }
    }
}

fn poll_std_child(child: &mut std::process::Child, grace: Duration) -> io::Result<bool> {
    let deadline = Instant::now() + grace;
    loop {
        if child.try_wait()?.is_some() {
            return Ok(true);
        }

        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Ok(false);
        }
        std::thread::sleep(CLEANUP_POLL_INTERVAL.min(remaining));
    }
}

#[cfg(windows)]
fn resume_suspended_process(pid: u32) -> io::Result<()> {
    const THREAD_DISCOVERY_ATTEMPTS: usize = 20;
    const THREAD_DISCOVERY_DELAY: std::time::Duration = std::time::Duration::from_millis(5);

    for _ in 0..THREAD_DISCOVERY_ATTEMPTS {
        match find_process_thread(pid) {
            Ok(thread) => return resume_thread(&thread),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                std::thread::sleep(THREAD_DISCOVERY_DELAY);
            }
            Err(error) => return Err(error),
        }
    }

    Err(io::Error::new(
        io::ErrorKind::NotFound,
        format!("could not find suspended primary thread for fallow subprocess {pid}"),
    ))
}

#[cfg(windows)]
#[expect(
    unsafe_code,
    reason = "thread discovery requires Windows ToolHelp FFI calls"
)]
fn find_process_thread(pid: u32) -> io::Result<WindowsHandle> {
    use std::mem;

    use windows_sys::Win32::Foundation::{ERROR_NO_MORE_FILES, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
    };
    use windows_sys::Win32::System::Threading::{OpenThread, THREAD_SUSPEND_RESUME};

    // SAFETY: The flags and process ID follow the ToolHelp API contract.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
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
                return Err(io::Error::last_os_error());
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
fn thread_enumeration_error(pid: u32, no_more_files: u32) -> io::Error {
    let error = io::Error::last_os_error();
    if error.raw_os_error() == i32::try_from(no_more_files).ok() {
        return io::Error::new(
            io::ErrorKind::NotFound,
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
fn resume_thread(thread: &WindowsHandle) -> io::Result<()> {
    use windows_sys::Win32::System::Threading::ResumeThread;

    // SAFETY: The handle was opened with THREAD_SUSPEND_RESUME access.
    let previous_count = unsafe { ResumeThread(thread.raw()) };
    if previous_count == u32::MAX {
        return Err(io::Error::last_os_error());
    }
    if previous_count == 0 {
        return Err(io::Error::other(
            "fallow subprocess primary thread was not suspended",
        ));
    }

    Ok(())
}
