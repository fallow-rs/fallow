//! Control runs: runs of covering tests with no active mutant. A kill with
//! no executed test, and a kill in a survivor retry by a test that the first
//! run did not execute, need a control run of their tests. The module also
//! records the baseline failures of control runs and of the dry runs of
//! restarted companions.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use rustc_hash::{FxHashMap, FxHashSet};

use super::super::protocol::{RunResult, RunStatus, TestStatus};
use super::{BaselineFailure, DryRunFacts, ScheduleOutcome};

/// Why a control run did not pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ControlFault {
    /// The run executed no test: the run path is broken.
    pub(super) no_tests: bool,
    /// The requested tests that ran and failed. A failed test is a baseline
    /// failure of the run, not a fault of the run path.
    pub(super) failed_tests: Vec<FailedTest>,
    pub(super) detail: String,
}

/// A test that failed in a control run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct FailedTest {
    pub(super) test_id: String,
    /// The error of the test from the companion. An older companion sends
    /// none.
    pub(super) error: Option<String>,
}

impl FailedTest {
    /// The message of the baseline failure of this test: `reason`, then the
    /// error of the test. The first line then has the reason and the first
    /// line of the error, and the stack follows. With no error, the message
    /// is `reason`.
    pub(super) fn message(&self, reason: &str) -> String {
        self.error
            .as_deref()
            .and_then(bounded_test_error)
            .map_or_else(|| reason.to_owned(), |error| format!("{reason}: {error}"))
    }
}

/// The error of a test from the companion, trimmed and cut at
/// [`MAX_TEST_ERROR_BYTES`]. Gives `None` for an error with no text.
pub(super) fn bounded_test_error(error: &str) -> Option<String> {
    let error = error.trim();
    if error.is_empty() {
        return None;
    }
    let kept = &error[..error.floor_char_boundary(MAX_TEST_ERROR_BYTES)];
    let ellipsis = if kept.len() < error.len() { "..." } else { "" };
    Some(format!("{kept}{ellipsis}"))
}

impl ControlFault {
    pub(super) fn untrusted(detail: String) -> Self {
        Self {
            no_tests: false,
            failed_tests: Vec::new(),
            detail,
        }
    }
}

/// Checks the result of a control run of `tests` with no active mutant. Each
/// requested test must run and pass, with no error.
pub(super) fn control_fault(tests: &[String], result: &RunResult) -> Option<ControlFault> {
    let first_error = result
        .unhandled_errors
        .first()
        .map(|error| format!("; first error: {}", first_line(&error.message)));
    if result.tests_executed == 0 {
        return Some(ControlFault {
            no_tests: true,
            failed_tests: Vec::new(),
            detail: format!(
                "0 of {} requested tests ran{}",
                tests.len(),
                first_error.unwrap_or_default()
            ),
        });
    }
    if let Some(id) = &result.mutant_id {
        return Some(ControlFault::untrusted(format!(
            "the run result names mutant `{id}`, expected none"
        )));
    }
    let requested: FxHashSet<&str> = tests.iter().map(String::as_str).collect();
    let failed_tests: Vec<FailedTest> = result
        .tests
        .iter()
        .filter(|test| test.status == TestStatus::Fail && requested.contains(test.test_id.as_str()))
        .map(|test| FailedTest {
            test_id: test.test_id.clone(),
            error: test.message.clone(),
        })
        .collect();
    if let Some(first) = failed_tests.first() {
        return Some(ControlFault {
            no_tests: false,
            detail: format!("the test `{}` failed", first.test_id),
            failed_tests,
        });
    }
    if let Some(first_error) = first_error {
        return Some(ControlFault::untrusted(format!(
            "the run reported an error{first_error}"
        )));
    }
    let passed: FxHashSet<&str> = result
        .tests
        .iter()
        .filter(|test| test.status == TestStatus::Pass)
        .map(|test| test.test_id.as_str())
        .collect();
    if result.status != RunStatus::Survived
        || result.tests_executed != tests.len()
        || tests.iter().any(|test| !passed.contains(test.as_str()))
    {
        return Some(ControlFault::untrusted(format!(
            "{} of {} requested tests ran and passed",
            passed.len(),
            tests.len()
        )));
    }
    None
}

/// The message of a test that fails in a control run. The dry run runs the
/// whole test file, and a mutant run runs only the covering tests.
pub(super) const CONTROL_RUN_FAILURE: &str = "the test fails in a mutant run with no active mutant, but passed in the dry run of its whole file; it can depend on another test of its file";

/// The message of a test that fails in the control run of a survivor retry.
/// That run already has each passing test of the file, so a missing covering
/// test is not the cause.
pub(super) const CONFIRMATION_RUN_FAILURE: &str = "the test fails with no active mutant in a run of each test of its file that passed in the dry run, but passed in the dry run of its whole file; it can depend on a test of its file that failed or was skipped in the dry run, or it can be flaky";

/// The message of a covering test that did not pass in the dry run of a
/// restarted companion.
pub(super) const RESTARTED_DRY_RUN_FAILURE: &str =
    "the test did not pass in the dry run of a restarted companion";

/// The longest test error that a baseline failure keeps, in bytes. The
/// companion bounds the error too. This bound guards the pipe from the
/// companion.
const MAX_TEST_ERROR_BYTES: usize = 8 * 1024;

/// Splits the message of a baseline failure from a control run or from the
/// dry run of a restarted companion into its fixed reason and the test error
/// after it. Gives `None` for another message or for a reason with no test
/// error.
#[must_use]
pub fn split_control_run_failure(message: &str) -> Option<(&str, &str)> {
    [
        CONTROL_RUN_FAILURE,
        CONFIRMATION_RUN_FAILURE,
        RESTARTED_DRY_RUN_FAILURE,
    ]
    .into_iter()
    .find_map(|reason| {
        let error = message.strip_prefix(reason)?.strip_prefix(": ")?;
        Some((reason, error))
    })
}

/// The first line of `message`.
pub(super) fn first_line(message: &str) -> &str {
    message.lines().next().unwrap_or_default()
}

/// The result of a control run.
pub(super) enum ControlOutcome {
    Passed,
    Failed(ControlFault),
    /// The companion did not give a run result.
    Fault(String),
    /// A test or the companion passed the timeout of the control run.
    TimedOut(String),
}

/// `tests`, sorted.
pub(super) fn sorted_tests(tests: &[String]) -> Vec<&str> {
    let mut sorted: Vec<&str> = tests.iter().map(String::as_str).collect();
    sorted.sort_unstable();
    sorted
}

/// The stored control result of one test set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ControlVerdict {
    /// The tests pass with no active mutant.
    Passed,
    /// A test failed with no active mutant.
    Failed,
    /// The control run timed out, with the reason. A later control run of
    /// the same tests would wait as long again.
    TimedOut(String),
}

/// The control evidence about one test set, from the memo or from a new
/// control run.
pub(super) enum ControlCheck {
    /// The tests pass with no active mutant.
    Passed,
    /// The tests do not pass with no active mutant, with the reason. A
    /// `settled` check is a fact about the tests. A fault of the companion is
    /// not settled.
    Failed { detail: String, settled: bool },
    /// The time budget ended before a control run.
    NoTime,
}

/// The control result of one test set: `None` before a control run gave a
/// result.
pub(super) type ControlCell = Arc<Mutex<Option<ControlVerdict>>>;

/// The control result of each test set within one run. A mutant kill with
/// no executed test needs a control run of its tests, and each test set gets
/// at most one. A timeout is stored, so a hanging test set costs one control
/// run. A fault of the companion is not stored, so a later mutant with the
/// same tests runs the control again. The memo also keeps the tests
/// that failed in a control run, each with the message of its baseline
/// failure: each later mutant run leaves them out. It also keeps the first
/// error of each test that failed in the dry run of a restarted companion,
/// for the baseline failure of that test.
#[derive(Default)]
pub(super) struct ControlMemo {
    entries: Mutex<FxHashMap<Vec<String>, ControlCell>>,
    pub(super) failed_tests: Mutex<BTreeMap<String, String>>,
    restarted_dry_run_errors: Mutex<BTreeMap<String, String>>,
}

impl ControlMemo {
    /// The cell of `tests`. A thread that holds the lock of the cell runs
    /// the control, and each other thread with the same tests waits for it.
    pub(super) fn cell(&self, tests: &[String]) -> ControlCell {
        let mut key = tests.to_vec();
        key.sort_unstable();
        let Ok(mut entries) = self.entries.lock() else {
            return Arc::new(Mutex::new(None));
        };
        Arc::clone(entries.entry(key).or_default())
    }

    pub(super) fn store(&self, tests: &[String], verdict: ControlVerdict) {
        if let Ok(mut cell) = self.cell(tests).lock() {
            *cell = Some(verdict);
        }
    }

    /// Marks `tests` as failed for `reason`. The first message of a test
    /// stays.
    pub(super) fn mark_failed(&self, tests: &[FailedTest], reason: &str) {
        if let Ok(mut failed) = self.failed_tests.lock() {
            for test in tests {
                failed
                    .entry(test.test_id.clone())
                    .or_insert_with(|| test.message(reason));
            }
        }
    }

    pub(super) fn is_failed(&self, test: &str) -> bool {
        self.failed_tests
            .lock()
            .is_ok_and(|failed| failed.contains_key(test))
    }

    /// Keeps the error of `test` from the dry run of a restarted companion.
    /// The first error of a test stays.
    pub(super) fn note_restarted_dry_run_error(&self, test: &str, error: &str) {
        if let Ok(mut errors) = self.restarted_dry_run_errors.lock()
            && !errors.contains_key(test)
        {
            errors.insert(test.to_owned(), error.to_owned());
        }
    }

    /// The first error of `test` from the dry run of a restarted companion.
    pub(super) fn restarted_dry_run_error(&self, test: &str) -> Option<String> {
        self.restarted_dry_run_errors
            .lock()
            .ok()
            .and_then(|errors| errors.get(test).cloned())
    }

    /// Each failed test with the message of its baseline failure.
    pub(super) fn failed(&self) -> BTreeMap<String, String> {
        self.failed_tests
            .lock()
            .map(|failed| failed.clone())
            .unwrap_or_default()
    }
}

/// Records `test` as a baseline failure of a restarted companion that
/// affected `mutant_id`. The mutant ran with its other covering tests. The
/// message keeps the first error of the test from the dry run of a
/// restarted companion, when the companion sent one.
pub(super) fn note_restarted_baseline_failure(
    outcome: &mut ScheduleOutcome,
    facts: &DryRunFacts,
    memo: &ControlMemo,
    test: String,
    mutant_id: &str,
) {
    if let Some(failure) = outcome
        .baseline_failures
        .iter_mut()
        .find(|failure| failure.test_id.as_deref() == Some(test.as_str()))
    {
        failure.mutant_ids.push(mutant_id.to_owned());
        return;
    }
    outcome.baseline_failures.push(BaselineFailure {
        file: facts.passing.get(&test).map(|(file, _)| file.clone()),
        message: Some(
            FailedTest {
                error: memo.restarted_dry_run_error(&test),
                test_id: test.clone(),
            }
            .message(RESTARTED_DRY_RUN_FAILURE),
        ),
        test_id: Some(test),
        mutant_ids: vec![mutant_id.to_owned()],
    });
}

/// Adds each test that failed in a control run during the mutant runs as a
/// baseline failure. [`super::record`] then adds the mutants that ran without it.
pub(super) fn note_control_failures(
    outcome: &mut ScheduleOutcome,
    facts: &DryRunFacts,
    memo: &ControlMemo,
) {
    for (test, message) in memo.failed() {
        if outcome
            .baseline_failures
            .iter()
            .any(|failure| failure.test_id.as_deref() == Some(test.as_str()))
        {
            continue;
        }
        outcome.baseline_failures.push(BaselineFailure {
            file: facts.passing.get(&test).map(|(file, _)| file.clone()),
            test_id: Some(test),
            message: Some(message),
            mutant_ids: Vec::new(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_control_run_failure_message_splits_into_its_reason_and_the_test_error() {
        let error = "AssertionError: expected 1 to be 2\n    at test/a.test.ts:3:5";
        let test = FailedTest {
            test_id: "test/a.test.ts::adds".to_owned(),
            error: Some(error.to_owned()),
        };
        for reason in [
            CONTROL_RUN_FAILURE,
            CONFIRMATION_RUN_FAILURE,
            RESTARTED_DRY_RUN_FAILURE,
        ] {
            let message = test.message(reason);
            assert_eq!(split_control_run_failure(&message), Some((reason, error)));
        }
        let no_error = FailedTest {
            error: None,
            ..test
        };
        assert_eq!(
            split_control_run_failure(&no_error.message(CONTROL_RUN_FAILURE)),
            None
        );
        assert_eq!(
            split_control_run_failure("Cannot find module './setup'"),
            None
        );
    }
}
