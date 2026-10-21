//! Control runs: runs of covering tests with no active mutant. A kill with
//! no executed test, and a kill in a survivor retry by a test that the first
//! run did not execute, need a control run of their tests.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use rustc_hash::{FxHashMap, FxHashSet};

use super::super::protocol::{RunResult, RunStatus, TestStatus};

/// Why a control run did not pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ControlFault {
    /// The run executed no test: the run path is broken.
    pub(super) no_tests: bool,
    /// The requested tests that ran and failed. A failed test is a baseline
    /// failure of the run, not a fault of the run path.
    pub(super) failed_tests: Vec<String>,
    pub(super) detail: String,
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
    let failed_tests: Vec<String> = result
        .tests
        .iter()
        .filter(|test| test.status == TestStatus::Fail && requested.contains(test.test_id.as_str()))
        .map(|test| test.test_id.clone())
        .collect();
    if let Some(first) = failed_tests.first() {
        return Some(ControlFault {
            no_tests: false,
            detail: format!("the test `{first}` failed"),
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
/// that failed in a control run: each later mutant run leaves them out.
#[derive(Default)]
pub(super) struct ControlMemo {
    entries: Mutex<FxHashMap<Vec<String>, ControlCell>>,
    pub(super) failed_tests: Mutex<BTreeSet<String>>,
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

    pub(super) fn mark_failed(&self, tests: &[String]) {
        if let Ok(mut failed) = self.failed_tests.lock() {
            failed.extend(tests.iter().cloned());
        }
    }

    pub(super) fn is_failed(&self, test: &str) -> bool {
        self.failed_tests
            .lock()
            .is_ok_and(|failed| failed.contains(test))
    }

    pub(super) fn failed(&self) -> BTreeSet<String> {
        self.failed_tests
            .lock()
            .map(|failed| failed.clone())
            .unwrap_or_default()
    }
}
