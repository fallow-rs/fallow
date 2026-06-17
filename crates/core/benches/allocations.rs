#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

//! Allocation tracking benchmark using dhat.
//!
//! This benchmark measures heap allocation statistics for the fallow analysis
//! pipeline. It uses a dedicated harness because dhat requires being the global
//! allocator.
//!
//! Run with: `cargo bench --bench allocations`
//!
//! Output is printed in a machine-parseable `key: value` format so that
//! `scripts/alloc-check.sh` can compare against a saved baseline.

#![expect(
    deprecated,
    reason = "ADR-008: benchmark exercises the workspace path-dep fallow_core::analyze surface"
)]

#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

mod helpers;

fn main() {
    let (temp_dir, config) = helpers::create_synthetic_project("alloc-bench", 100);

    let profiler = dhat::Profiler::builder().testing().build();

    let _ = fallow_core::analyze(&config);

    let stats = dhat::HeapStats::get();
    drop(profiler);

    let _ = std::fs::remove_dir_all(&temp_dir);

    #[expect(
        clippy::print_stdout,
        reason = "intentional bench output for alloc-check.sh"
    )]
    {
        println!("alloc_total_bytes: {}", stats.total_bytes);
        println!("alloc_total_blocks: {}", stats.total_blocks);
        println!("alloc_max_bytes: {}", stats.max_bytes);
        println!("alloc_max_blocks: {}", stats.max_blocks);
    }
}
