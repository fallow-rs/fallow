/// Build the global analysis pool and register the stack of the main thread.
/// A project with few files parses on the main thread, and a bounded parse on
/// a thread with an unknown stack runs on a new thread each time.
#[allow(
    dead_code,
    reason = "used by the CLI binary; the library build uses per-call pools"
)]
pub fn configure_global_pool(threads: usize) {
    let _ = fallow_api::configure_global_analysis_pool(threads);
    // The size is the stack of the process main thread. Another thread that
    // calls this, for example a test thread, keeps an unknown stack.
    if std::thread::current().name() == Some("main")
        && let Some(size) = main_thread_stack_size()
    {
        fallow_engine::thread_pool::register_thread_stack(size);
    }
}

/// Stack to assume for a main thread without a stack limit.
#[cfg(unix)]
const UNLIMITED_MAIN_STACK: usize = 8 * 1024 * 1024;

#[cfg(unix)]
#[expect(
    unsafe_code,
    reason = "reading the stack resource limit requires getrlimit FFI"
)]
fn main_thread_stack_size() -> Option<usize> {
    let mut limit = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: `limit` is a valid, writable `rlimit` that outlives the call.
    if unsafe { libc::getrlimit(libc::RLIMIT_STACK, &raw mut limit) } != 0 {
        return None;
    }
    if limit.rlim_cur == libc::RLIM_INFINITY {
        return Some(UNLIMITED_MAIN_STACK);
    }
    usize::try_from(limit.rlim_cur).ok()
}

#[cfg(windows)]
#[expect(
    unsafe_code,
    reason = "reading the stack bounds of the current thread requires Win32 FFI"
)]
fn main_thread_stack_size() -> Option<usize> {
    let mut low = 0_usize;
    let mut high = 0_usize;
    // SAFETY: both pointers refer to writable `usize` values that outlive
    // the call. The function has no other preconditions.
    unsafe {
        windows_sys::Win32::System::Threading::GetCurrentThreadStackLimits(
            &raw mut low,
            &raw mut high,
        );
    }
    high.checked_sub(low).filter(|size| *size > 0)
}

#[cfg(not(any(unix, windows)))]
fn main_thread_stack_size() -> Option<usize> {
    None
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    #[test]
    fn main_thread_stack_size_is_known_on_supported_platforms() {
        let size = super::main_thread_stack_size();
        if cfg!(any(unix, windows)) {
            assert!(size.is_some_and(|size| size >= 512 * 1024));
        }
    }

    const STACK_PROBE_ENV: &str = "FALLOW_RAYON_STACK_PROBE_CHILD";
    const STACK_PROBE_TEST: &str =
        "rayon_pool::tests::configured_pool_survives_deep_worker_stack_probe";

    #[test]
    fn configured_pool_survives_deep_worker_stack_probe() {
        if std::env::var_os(STACK_PROBE_ENV).is_some() {
            run_stack_probe_child();
            return;
        }

        let current_exe = std::env::current_exe().expect("current test binary should be known");
        // Drop RUST_MIN_STACK (pinned to 16 MiB in .cargo/config.toml, and
        // inherited by default-sized rayon workers) so the probe still fails
        // if the pool loses its explicit stack_size.
        let output = Command::new(current_exe)
            .arg("--exact")
            .arg(STACK_PROBE_TEST)
            .arg("--nocapture")
            .env(STACK_PROBE_ENV, "1")
            .env_remove("RUST_MIN_STACK")
            .output()
            .expect("stack probe child should start");

        assert!(
            output.status.success(),
            "stack probe child failed with status {:?}\nstdout:\n{}\nstderr:\n{}",
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn run_stack_probe_child() {
        fallow_engine::thread_pool::worker_pool_builder(1)
            .build_global()
            .expect("stack probe must own the global Rayon pool");

        let (tx, rx) = std::sync::mpsc::channel();
        rayon::spawn(move || {
            tx.send(consume_stack(5_000))
                .expect("stack probe parent should still be alive");
        });
        assert_eq!(
            rx.recv().expect("stack probe worker should send a result"),
            5_000
        );
    }

    #[inline(never)]
    fn consume_stack(depth: usize) -> usize {
        let frame = [0_u8; 2048];
        std::hint::black_box(&frame);
        if depth == 0 {
            usize::from(frame[0])
        } else {
            1 + consume_stack(depth - 1)
        }
    }
}
