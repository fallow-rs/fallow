#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]
#![expect(
    deprecated,
    reason = "ADR-008: benchmark exercises the workspace path-dep fallow_core::analyze surface"
)]

use std::time::Duration;

use criterion::{Criterion, criterion_group, criterion_main};

mod helpers;

fn bench_full_pipeline_5000(c: &mut Criterion) {
    let (temp_dir, config) = helpers::create_synthetic_project("5000", 5000);

    c.bench_function("full_pipeline_5000_files", |b| {
        b.iter(|| {
            let _ = fallow_core::analyze(&config);
        });
    });

    let _ = std::fs::remove_dir_all(&temp_dir);
}

fn bench_full_pipeline_1000_warm(c: &mut Criterion) {
    let (temp_dir, config) = helpers::create_synthetic_project_with_cache("1000-warm", 1000, false);

    let _ = fallow_core::analyze(&config);

    c.bench_function("full_pipeline_1000_files_warm_cache", |b| {
        b.iter(|| {
            let _ = fallow_core::analyze(&config);
        });
    });

    let _ = std::fs::remove_dir_all(&temp_dir);
}

fn bench_full_pipeline_5000_warm(c: &mut Criterion) {
    let (temp_dir, config) = helpers::create_synthetic_project_with_cache("5000-warm", 5000, false);

    let _ = fallow_core::analyze(&config);

    c.bench_function("full_pipeline_5000_files_warm_cache", |b| {
        b.iter(|| {
            let _ = fallow_core::analyze(&config);
        });
    });

    let _ = std::fs::remove_dir_all(&temp_dir);
}

fn bench_dupes_full_1000(c: &mut Criterion) {
    let (temp_dir, config) = helpers::create_dupe_project("1000", 1000);
    let files = fallow_core::discover::discover_files(&config);
    let dupes_config = fallow_config::DuplicatesConfig::default();

    c.bench_function("dupes_full_pipeline_1000_files", |b| {
        b.iter(|| {
            fallow_core::duplicates::find_duplicates(&config.root, &files, &dupes_config);
        });
    });

    let _ = std::fs::remove_dir_all(&temp_dir);
}

fn bench_dupes_full_5000(c: &mut Criterion) {
    let (temp_dir, config) = helpers::create_dupe_project("5000", 5000);
    let files = fallow_core::discover::discover_files(&config);
    let dupes_config = fallow_config::DuplicatesConfig::default();

    c.bench_function("dupes_full_pipeline_5000_files", |b| {
        b.iter(|| {
            fallow_core::duplicates::find_duplicates(&config.root, &files, &dupes_config);
        });
    });

    let _ = std::fs::remove_dir_all(&temp_dir);
}

criterion_group! {
    name = large_scale_benches;
    config = Criterion::default()
        .sample_size(10)
        .measurement_time(Duration::from_mins(1))
        .warm_up_time(Duration::from_secs(5));
    targets =
        bench_full_pipeline_5000,
        bench_full_pipeline_1000_warm,
        bench_full_pipeline_5000_warm,
        bench_dupes_full_1000,
        bench_dupes_full_5000,
}

criterion_main!(large_scale_benches);
