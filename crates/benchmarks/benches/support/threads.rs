//! Thread count for the scoped pools that benchmarks pass to fallow.

/// Threads for a local benchmark run.
const DEFAULT_BENCH_THREADS: usize = 4;

/// The thread count that a benchmark passes to fallow's scoped pools.
///
/// CodSpeed simulation sets `RAYON_NUM_THREADS=1`. Rayon splits work
/// adaptively on steals, so with more than one thread the instruction count
/// changes from run to run on identical code. A local run without the
/// variable keeps several threads.
pub fn bench_threads() -> usize {
    std::env::var("RAYON_NUM_THREADS")
        .ok()
        .and_then(|value| value.parse().ok())
        .filter(|&threads| threads > 0)
        .unwrap_or(DEFAULT_BENCH_THREADS)
}
