#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "benches use unwrap and expect to keep fixture setup concise"
)]
#![expect(
    deprecated,
    reason = "ADR-008: benchmark exercises the workspace path-dep fallow_core::analyze surface"
)]

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use tempfile::TempDir;

mod helpers;

struct ConfigInput {
    _temp_dir: TempDir,
    config: fallow_config::ResolvedConfig,
}

struct DupesInput {
    _temp_dir: TempDir,
    root: std::path::PathBuf,
    files: Vec<fallow_core::discover::DiscoveredFile>,
    config: fallow_config::DuplicatesConfig,
}

fn create_config_input(name: &str, file_count: usize) -> ConfigInput {
    let (temp_dir, config) = helpers::create_synthetic_project(name, file_count);
    ConfigInput {
        _temp_dir: temp_dir,
        config,
    }
}

fn create_dupes_input(name: &str, file_count: usize) -> DupesInput {
    let (temp_dir, resolved_config) = helpers::create_dupe_project(name, file_count);
    let files = fallow_core::discover::discover_files(&resolved_config);
    DupesInput {
        _temp_dir: temp_dir,
        root: resolved_config.root,
        files,
        config: fallow_config::DuplicatesConfig::default(),
    }
}

fn bench_scaling_analysis(c: &mut Criterion) {
    let mut group = c.benchmark_group("scaling_analysis");

    group.bench_function("full_pipeline_2000_files", |bencher| {
        bencher.iter_batched_ref(
            || create_config_input("2000", 2000),
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

    group.finish();
}

criterion_group!(benches, bench_scaling_analysis);
criterion_main!(benches);
