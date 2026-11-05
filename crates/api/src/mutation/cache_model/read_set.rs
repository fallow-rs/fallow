//! The read-set oracle of the result cache key: a change to each project
//! file that a cold run of the runner reads must change the key.
//!
//! The oracle sees the key through the cache counts of a real run: after a
//! neutral edit of a traced file, a cached run must not reuse a result. A
//! control run with no edit must reuse each result, so a cache that never
//! hits cannot pass the oracle.

use super::project::{Project, available_runners, vitest_package};
use super::template::{Runner, append_neutral_edit};
use super::trace::trace;

/// The exact set that each trace must give. A smaller set means that the
/// trace stopped to see a read, and the oracle would check too little. A
/// larger set means that the runner reads a new file: add it here, so the
/// change gets a review.
fn expected_reads(runner: Runner) -> Vec<&'static str> {
    let mut reads = vec![
        "package.json",
        "src/math.ts",
        "test/helper.ts",
        "test/math.test.ts",
    ];
    match runner {
        Runner::Vitest => reads.extend(["test/setup.ts", "vitest.config.mjs"]),
        Runner::Bun => reads.extend([
            "bunfig.toml",
            "test/preload.ts",
            "tsconfig.base.json",
            "tsconfig.json",
        ]),
    }
    reads.sort_unstable();
    reads
}

fn check_read_set(runner: Runner) {
    let Some(vitest) = vitest_package() else {
        return;
    };
    if !available_runners().contains(&runner) {
        return;
    }
    let project = Project::new(runner, &vitest);
    let traced = trace(&project.root, runner);
    assert_eq!(
        traced.iter().map(String::as_str).collect::<Vec<_>>(),
        expected_reads(runner),
        "the {runner:?} trace"
    );

    let first = project.run(true);
    assert!(
        first.stored > 0,
        "the first run stores no result: {:?}",
        first.observation
    );
    let control = project.run(true);
    assert_eq!(
        control.hits, first.stored,
        "a run with no edit reuses each stored result"
    );

    let uncovered: Vec<&String> = traced
        .iter()
        .filter(|path| {
            append_neutral_edit(&project.root, path);
            project.run(true).hits > 0
        })
        .collect();
    assert!(
        uncovered.is_empty(),
        "the {runner:?} runner reads these files, but a change to them does not change the result cache key: {uncovered:?}\ntraced: {traced:?}"
    );
}

#[test]
fn each_file_that_vitest_reads_is_a_key_part() {
    check_read_set(Runner::Vitest);
}

#[test]
fn each_file_that_bun_test_reads_is_a_key_part() {
    check_read_set(Runner::Bun);
}
