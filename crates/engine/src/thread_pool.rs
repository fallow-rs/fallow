//! Shared Rayon pool construction for analysis workloads.

/// Worker stack size for analysis thread pools.
///
/// Recursive AST extraction and analysis overflow the default Rayon worker
/// stack on deeply nested sources, so every pool that runs analysis work must
/// be built with this size. The CLI and LSP global pools and the programmatic
/// per-call pools share this builder so embedders cannot drift. A thread that
/// runs analysis outside these pools, such as a Tokio blocking thread in a
/// server, needs this size too.
pub const WORKER_STACK_SIZE: usize = 16 * 1024 * 1024;

/// Rayon pool builder preconfigured with the analysis worker stack size.
///
/// Each worker registers its stack size, so a bounded parse of an ordinary
/// file runs on the worker and only a file that needs more stack gets its own
/// thread (`fallow_extract::parse_bounded`).
#[must_use]
pub fn worker_pool_builder(threads: usize) -> rayon::ThreadPoolBuilder {
    rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .stack_size(WORKER_STACK_SIZE)
        .start_handler(|_| fallow_extract::register_thread_stack(WORKER_STACK_SIZE))
}

/// Record that the current thread has `size` bytes of stack, so a bounded
/// parse on it stays on it when the stack is large enough. A process calls
/// this for its main thread. Pool workers from [`worker_pool_builder`] do it
/// themselves.
pub fn register_thread_stack(size: usize) {
    fallow_extract::register_thread_stack(size);
}
