#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]
#![allow(
    clippy::significant_drop_tightening,
    reason = "the external Criterion macro owns the benchmark lifecycle"
)]
#![expect(
    deprecated,
    reason = "Core-internal policy: benchmark exercises the workspace path-dep fallow_core::analyze surface"
)]

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use tempfile::TempDir;

mod helpers;

struct ConfigInput {
    _temp_dir: TempDir,
    config: fallow_config::ResolvedConfig,
}

fn create_config_input(name: &str, file_count: usize, no_cache: bool) -> ConfigInput {
    let (temp_dir, config) =
        helpers::create_synthetic_project_with_cache(name, file_count, no_cache);
    ConfigInput {
        _temp_dir: temp_dir,
        config,
    }
}

fn create_warm_config_input(name: &str, file_count: usize) -> ConfigInput {
    let input = create_config_input(name, file_count, false);
    let _ = fallow_core::analyze(&input.config);
    input
}

fn bench_large_analysis(c: &mut Criterion) {
    let mut group = c.benchmark_group("large_analysis");

    group.bench_function("full_pipeline_5000_files", |bencher| {
        bencher.iter_batched_ref(
            || create_config_input("5000", 5000, true),
            |input| fallow_core::analyze(&input.config),
            BatchSize::LargeInput,
        );
    });

    group.bench_function("full_pipeline_1000_files_warm_cache", |bencher| {
        bencher.iter_batched_ref(
            || create_warm_config_input("1000-warm", 1000),
            |input| fallow_core::analyze(&input.config),
            BatchSize::LargeInput,
        );
    });

    group.bench_function("full_pipeline_5000_files_warm_cache", |bencher| {
        bencher.iter_batched_ref(
            || create_warm_config_input("5000-warm", 5000),
            |input| fallow_core::analyze(&input.config),
            BatchSize::LargeInput,
        );
    });

    group.finish();
}

criterion_group!(benches, bench_large_analysis);
criterion_main!(benches);
