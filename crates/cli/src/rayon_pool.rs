use fallow_engine::thread_pool::worker_pool_builder;

#[allow(
    dead_code,
    reason = "used by the CLI binary; the library build uses per-call pools"
)]
pub fn configure_global_pool(threads: usize) {
    let _ = worker_pool_builder(threads).build_global();
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
