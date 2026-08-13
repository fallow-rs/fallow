#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "benches use unwrap and expect to keep fixture setup concise"
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

fn create_config_input(name: &str, file_count: usize) -> ConfigInput {
    let (temp_dir, config) = helpers::create_synthetic_project(name, file_count);
    ConfigInput {
        _temp_dir: temp_dir,
        config,
    }
}

fn bench_scaling_analysis(c: &mut Criterion) {
    let mut group = c.benchmark_group("scaling_analysis");

    group.bench_function("scaling_full_pipeline_2000_files", |bencher| {
        bencher.iter_batched_ref(
            || create_config_input("2000", 2000),
            |input| fallow_core::analyze(&input.config),
            BatchSize::LargeInput,
        );
    });

    group.finish();
}

criterion_group!(benches, bench_scaling_analysis);
criterion_main!(benches);
