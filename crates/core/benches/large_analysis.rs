#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]
#![expect(
    deprecated,
    reason = "ADR-008: benchmark exercises the workspace path-dep fallow_core::analyze surface"
)]

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};

mod helpers;

struct ConfigInput {
    temp_dir: std::path::PathBuf,
    config: fallow_config::ResolvedConfig,
}

impl Drop for ConfigInput {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.temp_dir);
    }
}

struct DupesInput {
    temp_dir: std::path::PathBuf,
    root: std::path::PathBuf,
    files: Vec<fallow_core::discover::DiscoveredFile>,
    config: fallow_config::DuplicatesConfig,
}

impl Drop for DupesInput {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.temp_dir);
    }
}

fn create_config_input(name: &str, file_count: usize, no_cache: bool) -> ConfigInput {
    let (temp_dir, config) =
        helpers::create_synthetic_project_with_cache(name, file_count, no_cache);
    ConfigInput { temp_dir, config }
}

fn create_warm_config_input(name: &str, file_count: usize) -> ConfigInput {
    let input = create_config_input(name, file_count, false);
    let _ = fallow_core::analyze(&input.config);
    input
}

fn create_dupes_input(name: &str, file_count: usize) -> DupesInput {
    let (temp_dir, resolved_config) = helpers::create_dupe_project(name, file_count);
    let files = fallow_core::discover::discover_files(&resolved_config);
    DupesInput {
        temp_dir,
        root: resolved_config.root,
        files,
        config: fallow_config::DuplicatesConfig::default(),
    }
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

    group.bench_function("dupes_full_pipeline_1000_files", |bencher| {
        bencher.iter_batched_ref(
            || create_dupes_input("1000", 1000),
            |input| {
                fallow_core::duplicates::find_duplicates(&input.root, &input.files, &input.config);
            },
            BatchSize::LargeInput,
        );
    });

    group.bench_function("dupes_full_pipeline_5000_files", |bencher| {
        bencher.iter_batched_ref(
            || create_dupes_input("5000", 5000),
            |input| {
                fallow_core::duplicates::find_duplicates(&input.root, &input.files, &input.config);
            },
            BatchSize::LargeInput,
        );
    });

    group.finish();
}

criterion_group!(benches, bench_large_analysis);
criterion_main!(benches);
