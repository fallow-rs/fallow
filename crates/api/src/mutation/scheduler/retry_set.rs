//! The test sets of a mutant job: the covering tests in fastest-first
//! order, the whole-file widening, and the retry set of a survivor with its
//! weight for the time reserve.

use std::collections::BTreeSet;

use rustc_hash::FxHashSet;

use super::{DryRunFacts, timeout_ms};

/// Widens `tests` to each passing test of their files. Without
/// `test_filter` the runner runs whole test files (`protocol.md` section 5).
pub(super) fn whole_files(facts: &DryRunFacts, tests: &BTreeSet<String>) -> BTreeSet<String> {
    let files: FxHashSet<&String> = tests
        .iter()
        .filter_map(|test| facts.passing.get(test).map(|(file, _)| file))
        .collect();
    facts
        .passing
        .iter()
        .filter(|(_, (file, _))| files.contains(file))
        .map(|(test, _)| test.clone())
        .collect()
}

/// `tests` in measured fastest-first order, with their total dry-run
/// duration.
pub(super) fn fastest_first(facts: &DryRunFacts, tests: BTreeSet<String>) -> (Vec<String>, f64) {
    let mut timed: Vec<(f64, String)> = tests
        .into_iter()
        .map(|test| (facts.passing.get(&test).map_or(0.0, |entry| entry.1), test))
        .collect();
    timed.sort_by(|left, right| {
        left.0
            .total_cmp(&right.0)
            .then_with(|| left.1.cmp(&right.1))
    });
    let total = timed.iter().map(|(duration, _)| duration).sum();
    (timed.into_iter().map(|(_, test)| test).collect(), total)
}

/// The tests and the timeout of the retry of a survivor: each passing test
/// of the files of `tests`. A test can read a value that an earlier test of
/// its file cached, so the dry run attributes no hit to it (issue #3670).
/// `None` when the files have no other test. The last value is the dry-run
/// time of the retry tests.
pub(super) fn confirmation_tests(
    facts: &DryRunFacts,
    tests: &BTreeSet<String>,
) -> Option<((Vec<String>, u64), f64)> {
    let widened = whole_files(facts, tests);
    (widened.len() > tests.len()).then(|| {
        let (widened, total) = fastest_first(facts, widened);
        ((widened, timeout_ms(total)), total)
    })
}

/// The dry-run time that the retry tests add to the first-run tests, in
/// whole milliseconds, rounded up. The fixed cost of a run is in the
/// measured first run already, so only the difference counts.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a reserve estimate in milliseconds; the value is clamped first"
)]
pub(super) fn retry_extra_ms(retry_total: f64, first_total: f64) -> u64 {
    let extra = (retry_total - first_total).max(0.0).ceil();
    if extra.is_finite() { extra as u64 } else { 0 }
}
