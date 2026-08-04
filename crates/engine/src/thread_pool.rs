//! Shared Rayon pool construction for analysis workloads.

/// Worker stack size for analysis thread pools.
///
/// Recursive AST extraction and analysis overflow the default Rayon worker
/// stack on deeply nested sources, so every pool that runs analysis work must
/// be built with this size. The CLI global pool and the programmatic per-call
/// pools share this builder so embedders cannot drift.
pub const WORKER_STACK_SIZE: usize = 16 * 1024 * 1024;

/// Rayon pool builder preconfigured with the analysis worker stack size.
#[must_use]
pub fn worker_pool_builder(threads: usize) -> rayon::ThreadPoolBuilder {
    rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .stack_size(WORKER_STACK_SIZE)
}
