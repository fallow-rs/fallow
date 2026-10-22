//! Runs a fuzz input on a thread with the stack of a production worker.

use fallow_engine::thread_pool::WORKER_STACK_SIZE;

/// AddressSanitizer pads every stack frame with redzones. A measurement on
/// nested calls gave the recursive parser six to eight times the stack of a
/// release build. The harness scales the worker stack by a smaller factor, so
/// it stays stricter than production.
const SANITIZER_FRAME_FACTOR: usize = 4;

/// Runs `run` on a thread whose stack matches an analysis worker. The libFuzzer
/// main thread has the 8 MiB process default, which production extraction
/// does not rely on.
pub fn on_worker_stack<T: Send>(run: impl FnOnce() -> T + Send) -> T {
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .stack_size(WORKER_STACK_SIZE * SANITIZER_FRAME_FACTOR)
            .spawn_scoped(scope, run)
            .expect("fuzz worker thread starts")
            .join()
            .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
    })
}
