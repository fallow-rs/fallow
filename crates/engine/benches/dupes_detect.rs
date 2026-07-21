#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

use std::path::PathBuf;

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use fallow_engine::duplicates::{
    CloneGroup, CloneInstance, DuplicationReport, refresh_clone_families,
};

fn make_hashed_tokens(hashes: &[u64]) -> Vec<fallow_engine::duplicates::normalize::HashedToken> {
    hashes
        .iter()
        .enumerate()
        .map(
            |(i, &hash)| fallow_engine::duplicates::normalize::HashedToken {
                hash,
                original_index: i,
            },
        )
        .collect()
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "bench span values are trivially small"
)]
fn make_file_tokens_for(count: usize) -> fallow_engine::duplicates::tokenize::FileTokens {
    use fallow_engine::duplicates::tokenize::{FileTokens, SourceToken, TokenKind};
    use oxc_span::Span;

    let tokens: Vec<SourceToken> = (0..count)
        .map(|i| SourceToken {
            kind: TokenKind::Identifier(format!("t{i}")),
            span: Span::new((i * 3) as u32, (i * 3 + 2) as u32),
        })
        .collect();

    let mut source = String::with_capacity(count * 4);
    for i in 0..count {
        source.push_str("xx");
        if i < count - 1 {
            source.push('\n');
        }
    }
    let line_count = source.lines().count().max(1);
    FileTokens {
        tokens,
        atomic_invocation_spans: Vec::new(),
        source,
        line_count,
    }
}

type DupeInput = Vec<(
    PathBuf,
    Vec<fallow_engine::duplicates::normalize::HashedToken>,
    fallow_engine::duplicates::tokenize::FileTokens,
)>;

/// Build N identical files with `tokens_per_file` tokens each.
fn make_identical_files(n: usize, tokens_per_file: usize) -> DupeInput {
    let hashes: Vec<u64> = (1..=tokens_per_file as u64).collect();
    (0..n)
        .map(|i| {
            (
                PathBuf::from(format!("dir{i}/file{i}.ts")),
                make_hashed_tokens(&hashes),
                make_file_tokens_for(tokens_per_file),
            )
        })
        .collect()
}

/// Build files with diverse content (low duplication).
fn make_diverse_files(n: usize, tokens_per_file: usize) -> DupeInput {
    (0..n)
        .map(|i| {
            let base = (i * tokens_per_file * 10) as u64;
            let hashes: Vec<u64> = (base..base + tokens_per_file as u64).collect();
            (
                PathBuf::from(format!("dir{i}/file{i}.ts")),
                make_hashed_tokens(&hashes),
                make_file_tokens_for(tokens_per_file),
            )
        })
        .collect()
}

/// Build files that repeat several shared blocks with per-file separators.
/// This creates many high-LCP intervals without requiring a huge fixture.
fn make_interval_pressure_files(n: usize, blocks: usize, block_tokens: usize) -> DupeInput {
    (0..n)
        .map(|i| {
            let mut hashes = Vec::with_capacity(blocks * (block_tokens + 1));
            for block in 0..blocks {
                hashes.extend((1..=block_tokens as u64).map(|token| token + block as u64 * 10_000));
                hashes.push(1_000_000 + (i * blocks + block) as u64);
            }
            (
                PathBuf::from(format!("dir{i}/pressure{i}.ts")),
                make_hashed_tokens(&hashes),
                make_file_tokens_for(hashes.len()),
            )
        })
        .collect()
}

fn make_family_grouping_report(family_count: usize, groups_per_family: usize) -> DuplicationReport {
    let mut report = DuplicationReport::default();
    report
        .clone_groups
        .reserve(family_count * groups_per_family);
    for family in 0..family_count {
        for group in 0..groups_per_family {
            let start_line = group * 10 + 1;
            report.clone_groups.push(CloneGroup {
                instances: vec![
                    CloneInstance {
                        file: PathBuf::from(format!("src/family-{family}/left.ts")),
                        start_line,
                        end_line: start_line + 5,
                        start_col: 0,
                        end_col: 0,
                        fragment: String::new(),
                    },
                    CloneInstance {
                        file: PathBuf::from(format!("src/family-{family}/right.ts")),
                        start_line,
                        end_line: start_line + 5,
                        start_col: 0,
                        end_col: 0,
                        fragment: String::new(),
                    },
                ],
                token_count: 30,
                line_count: 6,
            });
        }
    }
    report
}

fn dupe_detect_2x500_identical(c: &mut Criterion) {
    use fallow_engine::duplicates::detect::CloneDetector;
    let data = make_identical_files(2, 500);
    c.bench_function("dupe_detect_2x500_identical", |bencher| {
        bencher.iter_batched(
            || data.clone(),
            |d| CloneDetector::new(30, 5, false).detect(d),
            BatchSize::LargeInput,
        );
    });
}

fn dupe_detect_2x2000_identical(c: &mut Criterion) {
    use fallow_engine::duplicates::detect::CloneDetector;
    let data = make_identical_files(2, 2000);
    c.bench_function("dupe_detect_2x2000_identical", |bencher| {
        bencher.iter_batched(
            || data.clone(),
            |d| CloneDetector::new(30, 5, false).detect(d),
            BatchSize::LargeInput,
        );
    });
}

fn dupe_detect_10x500_identical(c: &mut Criterion) {
    use fallow_engine::duplicates::detect::CloneDetector;
    let data = make_identical_files(10, 500);
    c.bench_function("dupe_detect_10x500_identical", |bencher| {
        bencher.iter_batched(
            || data.clone(),
            |d| CloneDetector::new(30, 5, false).detect(d),
            BatchSize::LargeInput,
        );
    });
}

fn dupe_detect_50x200_diverse(c: &mut Criterion) {
    use fallow_engine::duplicates::detect::CloneDetector;
    let data = make_diverse_files(50, 200);
    c.bench_function("dupe_detect_50x200_diverse", |bencher| {
        bencher.iter_batched(
            || data.clone(),
            |d| CloneDetector::new(30, 5, false).detect(d),
            BatchSize::LargeInput,
        );
    });
}

fn dupe_detect_100x200_mixed(c: &mut Criterion) {
    use fallow_engine::duplicates::detect::CloneDetector;
    let hashes: Vec<u64> = (1..=200).collect();
    let data: DupeInput = (0..100)
        .map(|i| {
            let h = if i < 20 {
                make_hashed_tokens(&hashes)
            } else {
                let base = (i * 10000) as u64;
                let unique_hashes: Vec<u64> = (base..base + 200).collect();
                make_hashed_tokens(&unique_hashes)
            };
            (
                PathBuf::from(format!("dir{i}/file{i}.ts")),
                h,
                make_file_tokens_for(200),
            )
        })
        .collect();

    c.bench_function("dupe_detect_100x200_mixed", |bencher| {
        bencher.iter_batched(
            || data.clone(),
            |d| CloneDetector::new(30, 5, false).detect(d),
            BatchSize::LargeInput,
        );
    });
}

fn dupe_detect_100x200_mixed_focused(c: &mut Criterion) {
    use fallow_engine::duplicates::detect::CloneDetector;
    use rustc_hash::FxHashSet;

    let hashes: Vec<u64> = (1..=200).collect();
    let data: DupeInput = (0..100)
        .map(|i| {
            let h = if i < 20 {
                make_hashed_tokens(&hashes)
            } else {
                let base = (i * 10000) as u64;
                let unique_hashes: Vec<u64> = (base..base + 200).collect();
                make_hashed_tokens(&unique_hashes)
            };
            (
                PathBuf::from(format!("dir{i}/file{i}.ts")),
                h,
                make_file_tokens_for(200),
            )
        })
        .collect();
    let focus: FxHashSet<PathBuf> = std::iter::once(PathBuf::from("dir0/file0.ts")).collect();

    c.bench_function("dupe_detect_100x200_mixed_focused", |bencher| {
        bencher.iter_batched(
            || data.clone(),
            |d| CloneDetector::new(30, 5, false).detect_touching_files(d, &focus),
            BatchSize::LargeInput,
        );
    });
}

fn dupe_detect_80x20x80_interval_pressure(c: &mut Criterion) {
    use fallow_engine::duplicates::detect::CloneDetector;
    let data = make_interval_pressure_files(80, 20, 80);
    c.bench_function("dupe_detect_80x20x80_interval_pressure", |bencher| {
        bencher.iter_batched(
            || data.clone(),
            |d| CloneDetector::new(30, 5, false).detect(d),
            BatchSize::LargeInput,
        );
    });
}

fn dupe_detect_2x5000_identical(c: &mut Criterion) {
    use fallow_engine::duplicates::detect::CloneDetector;
    let data = make_identical_files(2, 5000);
    c.bench_function("dupe_detect_2x5000_identical", |bencher| {
        bencher.iter_batched(
            || data.clone(),
            |d| CloneDetector::new(30, 5, false).detect(d),
            BatchSize::LargeInput,
        );
    });
}

fn clone_family_grouping_1000x3(c: &mut Criterion) {
    let report = make_family_grouping_report(1_000, 3);
    c.bench_function("clone_family_grouping_1000x3", |bencher| {
        bencher.iter_batched(
            || report.clone(),
            |mut report| {
                refresh_clone_families(&mut report, PathBuf::new().as_path());
                std::hint::black_box(report.clone_families.len())
            },
            BatchSize::LargeInput,
        );
    });
}

criterion_group!(
    benches,
    dupe_detect_2x500_identical,
    dupe_detect_2x2000_identical,
    dupe_detect_10x500_identical,
    dupe_detect_50x200_diverse,
    dupe_detect_100x200_mixed,
    dupe_detect_100x200_mixed_focused,
    dupe_detect_80x20x80_interval_pressure,
    dupe_detect_2x5000_identical,
    clone_family_grouping_1000x3
);
criterion_main!(benches);
