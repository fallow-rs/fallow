#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

//! Quick profiling harness for clone detection steps.
//! Run with: cargo test -p fallow-core --test dupes_profile --release -- --nocapture
//!
//! For step-level timing: FALLOW_PROFILE=1 cargo test -p fallow-core --test dupes_profile --release -- --nocapture

use std::path::PathBuf;
use std::time::Instant;

use fallow_engine::duplicates::detect::CloneDetector;
use fallow_engine::duplicates::normalize::HashedToken;
use fallow_engine::duplicates::tokenize::{FileTokens, SourceToken, TokenKind};
use oxc_span::Span;

fn make_hashed_tokens(hashes: &[u64]) -> Vec<HashedToken> {
    hashes
        .iter()
        .enumerate()
        .map(|(i, &hash)| HashedToken {
            hash,
            original_index: i,
        })
        .collect()
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "test span values are trivially small"
)]
fn make_file_tokens_for(count: usize) -> FileTokens {
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

type DupeInput = Vec<(PathBuf, Vec<HashedToken>, FileTokens)>;

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

fn make_mixed_files(n_identical: usize, n_diverse: usize, tokens_per_file: usize) -> DupeInput {
    let shared_hashes: Vec<u64> = (1..=tokens_per_file as u64).collect();
    let mut data = Vec::new();
    for i in 0..n_identical {
        data.push((
            PathBuf::from(format!("dir{i}/file{i}.ts")),
            make_hashed_tokens(&shared_hashes),
            make_file_tokens_for(tokens_per_file),
        ));
    }
    for i in 0..n_diverse {
        let base = ((n_identical + i) * 10000) as u64;
        let hashes: Vec<u64> = (base..base + tokens_per_file as u64).collect();
        data.push((
            PathBuf::from(format!("dir{}/file{}.ts", n_identical + i, n_identical + i)),
            make_hashed_tokens(&hashes),
            make_file_tokens_for(tokens_per_file),
        ));
    }
    data
}

#[expect(clippy::print_stderr, reason = "intentional profiling output")]
fn profile_scenario(name: &str, data: &DupeInput, runs: usize) {
    let total_tokens: usize = data.iter().map(|(_, h, _)| h.len()).sum();
    let n_files = data.len();

    eprintln!("\n=== {name} ({n_files} files, {total_tokens} total tokens) ===");

    let _ = CloneDetector::new(30, 5, false).detect(data.clone());

    let mut times = Vec::with_capacity(runs);
    let mut groups = 0;
    for _ in 0..runs {
        let d = data.clone();
        let t0 = Instant::now();
        let report = CloneDetector::new(30, 5, false).detect(d);
        let elapsed = t0.elapsed();
        times.push(elapsed.as_micros() as f64);
        groups = report.clone_groups.len();
    }

    times.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median = times[times.len() / 2];
    let min = times[0];
    let max = times[times.len() - 1];
    let mean = times.iter().sum::<f64>() / times.len() as f64;

    eprintln!(
        "  min={min:.0}µs  median={median:.0}µs  mean={mean:.0}µs  max={max:.0}µs  groups={groups}"
    );
    eprintln!(
        "  throughput: {:.0} tokens/ms",
        total_tokens as f64 / (median / 1000.0)
    );
}

#[test]
#[ignore = "profiling harness, run manually with -- --ignored"]
fn profile_dupe_detection() {
    if std::env::var("FALLOW_PROFILE").is_ok() {
        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .with_target(false)
            .with_writer(std::io::stderr)
            .init();
    }

    let runs = 10;

    profile_scenario("2x500 identical", &make_identical_files(2, 500), runs);
    profile_scenario("2x1000 identical", &make_identical_files(2, 1000), runs);
    profile_scenario("2x2000 identical", &make_identical_files(2, 2000), runs);
    profile_scenario("2x5000 identical", &make_identical_files(2, 5000), runs);
    profile_scenario("10x500 identical", &make_identical_files(10, 500), runs);
    profile_scenario("20x200 identical", &make_identical_files(20, 200), runs);
    profile_scenario(
        "100x200 mixed (20 identical + 80 diverse)",
        &make_mixed_files(20, 80, 200),
        runs,
    );
    profile_scenario("50x200 diverse", &make_mixed_files(0, 50, 200), runs);
    profile_scenario("2x10000 identical", &make_identical_files(2, 10000), runs);
}
