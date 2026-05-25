// 16 MiB stack: graph traversal and the AST visitor recurse deeply enough on
// large real-world projects to exceed Rust's default 8 MiB worker stack. The
// `configured_pool_survives_deep_worker_stack_probe` test asserts this floor.
const WORKER_STACK_SIZE: usize = 16 * 1024 * 1024;

#[allow(
    dead_code,
    reason = "used by the CLI binary; the library build uses per-call pools"
)]
pub fn configure_global_pool(threads: usize) {
    // `build_global` is process-wide and one-shot: subsequent calls (e.g. from
    // a NAPI host that constructs `AnalysisOptions` per request) return Err and
    // leave the first-set thread count and stack size in place. Errors are
    // intentionally discarded so re-entry is a no-op rather than a hard failure.
    let _ = build_pool(threads).build_global();
}

#[allow(
    dead_code,
    reason = "used by the library programmatic API; the CLI binary uses the global pool"
)]
pub fn build_thread_pool(threads: usize) -> Result<rayon::ThreadPool, rayon::ThreadPoolBuildError> {
    build_pool(threads).build()
}

fn build_pool(threads: usize) -> rayon::ThreadPoolBuilder {
    rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .stack_size(WORKER_STACK_SIZE)
}

#[cfg(test)]
mod tests {
    use std::process::Command;

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
        let output = Command::new(current_exe)
            .arg("--exact")
            .arg(STACK_PROBE_TEST)
            .arg("--nocapture")
            .env(STACK_PROBE_ENV, "1")
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
        super::build_pool(1)
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
