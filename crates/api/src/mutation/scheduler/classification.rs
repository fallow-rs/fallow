//! Classification and exact matrix evidence for companion results.

use super::*;

/// Maps a `run_result` to a status with the fail-closed rules 5 and 6.
/// `tests` are the test ids of the request: the covering tests of `job`
/// without the tests that a restarted companion dropped.
///
/// An `Err` is a runtime error with its reason.
pub(super) fn classify(
    job: &Job,
    tests: &[String],
    result: &RunResult,
) -> Result<(MutantStatus, Option<String>), String> {
    if result.mutant_id.as_deref() != Some(job.id.as_str()) {
        return Err(format!(
            "the run result names mutant `{}`, expected `{}`",
            result.mutant_id.as_deref().unwrap_or("null"),
            job.id
        ));
    }
    let requested: FxHashSet<&str> = tests.iter().map(String::as_str).collect();
    match result.status {
        RunStatus::Killed => match &result.killed_by {
            Some(test) if requested.contains(test.as_str()) => {
                Ok((MutantStatus::Killed, Some(test.clone())))
            }
            Some(test) => Err(format!(
                "the run result names killer `{test}`, which was not requested"
            )),
            None if !result.unhandled_errors.is_empty() => Ok((MutantStatus::Killed, None)),
            None => Err("the run result is `killed` with no failed test and no error".to_owned()),
        },
        RunStatus::Timeout => Ok((MutantStatus::Timeout, None)),
        RunStatus::RuntimeError => Err("the companion reported a runtime error".to_owned()),
        RunStatus::Survived => {
            if result.tests_executed == 0 {
                return Err("zero tests ran".to_owned());
            }
            if result.tests_executed != tests.len() {
                return Err(format!(
                    "{} of {} requested tests ran",
                    result.tests_executed,
                    tests.len()
                ));
            }
            if !result.unhandled_errors.is_empty() {
                return Err("the run result is `survived` with an unhandled error".to_owned());
            }
            let passed: FxHashSet<&str> = result
                .tests
                .iter()
                .filter(|test| test.status == TestStatus::Pass)
                .map(|test| test.test_id.as_str())
                .collect();
            let all_pass = result.tests.iter().all(|test| {
                test.status == TestStatus::Pass && requested.contains(test.test_id.as_str())
            });
            if !all_pass || requested.iter().any(|test| !passed.contains(test)) {
                return Err(
                    "the run result is `survived` but a requested test did not pass".to_owned(),
                );
            }
            Ok((MutantStatus::Survived, None))
        }
    }
}

/// How a `killed` run detected the mutant: the failure kind of the killing
/// test, or `error` for a kill by an error outside each test.
pub(super) fn kill_kind(
    result: &RunResult,
    status: MutantStatus,
    killed_by: Option<&str>,
) -> Option<MutantFailureKind> {
    if status != MutantStatus::Killed {
        return None;
    }
    let Some(killer) = killed_by else {
        return Some(MutantFailureKind::Error);
    };
    result
        .tests
        .iter()
        .find(|test| test.test_id == killer)
        .and_then(|test| test.failure_kind.as_deref())
        .and_then(MutantFailureKind::from_wire)
}

/// The evidence of one run, for the attempt history of a retried mutant.
pub(super) fn attempt_record(attempt: &Attempt) -> MutantAttempt {
    let detail = (attempt.status == MutantStatus::RuntimeError)
        .then_some(attempt.issue.as_ref())
        .flatten()
        .map(|issue| first_line(&issue.message).to_owned());
    MutantAttempt {
        status: attempt.status,
        duration_ms: attempt.duration_ms,
        killing_tests: attempt.killing_tests.clone(),
        kill_matrix_complete: attempt.kill_matrix_complete,
        killed_by: attempt.killed_by.clone(),
        failure_kind: attempt.failure_kind,
        detail,
    }
}

/// Keep observed failures, but require an exact terminal result set for candidate evidence.
fn terminal(test: &super::super::protocol::RunTest) -> bool {
    matches!(test.status, TestStatus::Pass | TestStatus::Fail)
        && test.failure_kind.as_deref() != Some("timeout")
}

pub(super) fn matrix_evidence(tests: &[String], result: &RunResult) -> (Vec<String>, bool) {
    let requested: FxHashSet<&str> = tests.iter().map(String::as_str).collect();
    let mut seen = FxHashSet::default();
    let membership = result.tests.iter().all(|test| {
        requested.contains(test.test_id.as_str())
            && seen.insert(test.test_id.as_str())
            && terminal(test)
    });
    let mut killers: Vec<String> = if matches!(result.status, RunStatus::Killed) {
        result
            .tests
            .iter()
            .filter(|test| {
                test.status == TestStatus::Fail
                    && terminal(test)
                    && requested.contains(test.test_id.as_str())
            })
            .map(|test| test.test_id.clone())
            .collect()
    } else {
        Vec::new()
    };
    killers.sort();
    killers.dedup();
    let consistent = match result.status {
        RunStatus::Killed => result
            .killed_by
            .as_ref()
            .is_some_and(|test| killers.contains(test)),
        RunStatus::Survived => result
            .tests
            .iter()
            .all(|test| test.status == TestStatus::Pass),
        _ => false,
    };
    let complete = membership
        && consistent
        && result.unhandled_errors.is_empty()
        && result.tests_executed == tests.len()
        && result.tests.len() == tests.len()
        && requested.len() == tests.len();
    (killers, complete)
}
