//! Tokio runtime for the stdio server.

use fallow_api::WORKER_STACK_SIZE;

/// Tokio worker threads of the stdio server.
const SERVER_WORKER_THREADS: usize = 2;

/// Build the multi-threaded runtime that serves the stdio protocol.
///
/// Analysis runs in `spawn_blocking`. A small project parses on the blocking
/// thread itself and a larger one on the global Rayon pool. Both default to a
/// 2 MiB stack, which deeply nested sources overflow, so both get the stack of
/// an analysis worker.
pub fn build_server_runtime() -> std::io::Result<tokio::runtime::Runtime> {
    if !fallow_api::configure_global_analysis_pool(0) {
        // The server still runs, and a small project still parses on the
        // sized blocking thread. A large project can overflow the default
        // pool, so say why.
        #[expect(
            clippy::print_stderr,
            reason = "the tracing subscriber starts after the runtime"
        )]
        {
            eprintln!(
                "fallow-lsp: could not build the analysis thread pool; \
                 parallel analysis keeps the default 2 MiB stack"
            );
        }
    }
    tokio::runtime::Builder::new_multi_thread()
        // The workers only move protocol messages and timers. Analysis runs
        // on the blocking threads and the Rayon pool, so two workers are
        // enough and keep the reserved stack small.
        .worker_threads(SERVER_WORKER_THREADS)
        .thread_stack_size(WORKER_STACK_SIZE)
        .enable_all()
        .build()
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use rayon::prelude::*;

    const STACK_PROBE_ENV: &str = "FALLOW_LSP_STACK_PROBE_CHILD";
    const STACK_PROBE_TEST: &str =
        "runtime::tests::server_runtime_survives_deep_analysis_stack_probe";

    // A stack overflow aborts the process, so the probe re-runs this test
    // binary as a child. The child drops RUST_MIN_STACK (pinned to 16 MiB in
    // .cargo/config.toml), because the shipped server does not get it and
    // default-sized Tokio and Rayon threads inherit it.
    #[test]
    fn server_runtime_survives_deep_analysis_stack_probe() {
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

    /// Analysis runs in `spawn_blocking`: a small project parses on the
    /// blocking thread itself, a larger one on the global Rayon pool.
    fn run_stack_probe_child() {
        let runtime = super::build_server_runtime().expect("server runtime should build");
        let probe = runtime.spawn_blocking(|| {
            let sequential = consume_stack(5_000);
            let parallel: Vec<usize> = [5_000_usize, 5_000]
                .par_iter()
                .map(|depth| consume_stack(*depth))
                .collect();
            (sequential, parallel)
        });
        let depths = runtime
            .block_on(probe)
            .expect("blocking probe should finish");
        assert_eq!(depths, (5_000, vec![5_000, 5_000]));
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
