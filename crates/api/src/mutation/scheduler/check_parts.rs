//! The self-check of a first job whose tests are in the shares of more than
//! one companion (issue #5444).
//!
//! The first dry run divides the test files over the companions, and each
//! companion keeps its share. One control run of all tests of a wide first
//! job takes as long as an unsplit mutant run of that job, and the other
//! companions stay idle meanwhile. So each companion that holds some of the
//! tests checks its own tests, at the same time as the others. The parts give
//! one outcome with the rules of one control run.

use super::control::ControlFault;
use super::dry_phase::{run_path_error, run_path_stopped};
use super::*;

/// The tests of `job` for each companion of `links` whose share holds some
/// of them, with the index of the companion, in the order of `links`.
/// `None` when one companion holds each test, or when no companion holds a
/// test: the single check covers both cases.
pub(super) fn shares(job: &Job, links: &[WarmLink]) -> Option<Vec<(usize, Vec<String>)>> {
    let mut shares: Vec<(usize, Vec<String>)> = Vec::new();
    for test in &job.tests {
        let owner = links.iter().position(|warm| warm.known.contains(test))?;
        match shares.iter_mut().find(|(index, _)| *index == owner) {
            Some((_, tests)) => tests.push(test.clone()),
            None => shares.push((owner, vec![test.clone()])),
        }
    }
    shares.sort_by_key(|(index, _)| *index);
    (shares.len() > 1).then_some(shares)
}

/// The facts of one self-check in parts.
pub(super) struct Check<'a> {
    pub(super) runner: PhaseRunner<'a>,
    /// The test files of the phase.
    pub(super) files: &'a [String],
    pub(super) facts: &'a DryRunFacts,
    pub(super) ready: &'a Ready,
    /// The first job.
    pub(super) job: &'a Job,
}

/// The outcome of one part and its companion when the companion still runs.
type PartResult = (Result<ControlOutcome, ScheduleError>, Option<WarmLink>);

impl Check<'_> {
    /// Runs each share of `shares` as a control run on the companion that
    /// holds it, at the same time. Gives the companions with the companions
    /// of the check first, and the tests that failed in a part. A fault, a
    /// timeout or a run with no executed test in any part stops the run, as
    /// in the single check.
    pub(super) fn run(
        &self,
        links: Vec<WarmLink>,
        shares: Vec<(usize, Vec<String>)>,
    ) -> Result<(Vec<WarmLink>, Vec<FailedTest>), ScheduleError> {
        let mut slots: Vec<Option<WarmLink>> = links.into_iter().map(Some).collect();
        let parts: Vec<(WarmLink, Vec<String>)> = shares
            .into_iter()
            .filter_map(|(index, tests)| Some((slots.get_mut(index)?.take()?, tests)))
            .collect();
        debug_assert!(
            parts.iter().map(|(_, tests)| tests.len()).sum::<usize>() == self.job.tests.len(),
            "each test of the job has a part"
        );
        let results: Vec<PartResult> = std::thread::scope(|scope| {
            #[expect(
                clippy::needless_collect,
                reason = "each part starts before the first join, so the parts run at the same time"
            )]
            let handles: Vec<_> = parts
                .into_iter()
                .map(|(warm, tests)| scope.spawn(move || self.part(warm, &tests)))
                .collect();
            handles
                .into_iter()
                .map(|handle| {
                    handle
                        .join()
                        .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
                })
                .collect()
        });
        let (outcomes, kept): (Vec<_>, Vec<_>) = results.into_iter().unzip();
        let lost = kept.iter().any(Option::is_none);
        let mut links: Vec<WarmLink> = kept.into_iter().flatten().collect();
        links.extend(slots.into_iter().flatten());
        let outcome = match merge(outcomes) {
            Ok(outcome) => outcome,
            Err(error) => {
                shutdown_all(links);
                return Err(error);
            }
        };
        let failed = match outcome {
            ControlOutcome::Passed => Vec::new(),
            ControlOutcome::Failed(fault) if !fault.failed_tests.is_empty() => fault.failed_tests,
            other => {
                shutdown_all(links);
                return Err(run_path_error(self.runner, self.ready, self.job, other));
            }
        };
        if lost {
            shutdown_all(links);
            return Err(run_path_stopped(self.runner, self.ready));
        }
        let verdict = if failed.is_empty() {
            ControlVerdict::Passed
        } else {
            ControlVerdict::Failed
        };
        self.runner.memo.store(&self.job.tests, verdict);
        Ok((links, failed))
    }

    /// Runs `tests` with no active mutant on the companion of `warm`. The
    /// part keeps the timeouts of the whole job.
    fn part(&self, warm: WarmLink, tests: &[String]) -> PartResult {
        let runner = self.runner;
        let mut worker = Worker::new(
            runner.spawner,
            runner.config,
            self.files,
            runner.config.runner_timeout,
            runner.memo,
        );
        worker.index = runner.index;
        worker.link = Some(warm.link);
        worker.known_tests = warm.known;
        worker.loaded_files = warm.loaded;
        worker.unproven = warm.unproven;
        let job = Job {
            tests: tests.to_vec(),
            test_files: split::files_of(self.facts, tests),
            ..self.job.clone()
        };
        let outcome = worker
            .control_run(tests, &job, runner.caps)
            .map(|(outcome, _)| outcome);
        let kept = worker.link.take().map(|link| WarmLink {
            link,
            known: std::mem::take(&mut worker.known_tests),
            loaded: worker.loaded_files.take(),
            unproven: std::mem::take(&mut worker.unproven),
        });
        (outcome, kept)
    }
}

/// The outcome of the whole check from the outcomes of its parts, in share
/// order. The first error that stops the run decides. Then the first
/// outcome that is neither a pass nor a failure with failed tests decides.
/// Otherwise the failed tests of each part add up.
fn merge(
    outcomes: Vec<Result<ControlOutcome, ScheduleError>>,
) -> Result<ControlOutcome, ScheduleError> {
    let outcomes = outcomes.into_iter().collect::<Result<Vec<_>, _>>()?;
    let mut failed: Option<ControlFault> = None;
    for outcome in outcomes {
        match outcome {
            ControlOutcome::Passed => {}
            ControlOutcome::Failed(fault) if !fault.failed_tests.is_empty() => match &mut failed {
                Some(known) => known.failed_tests.extend(fault.failed_tests),
                None => failed = Some(fault),
            },
            other => return Ok(other),
        }
    }
    Ok(failed.map_or(ControlOutcome::Passed, ControlOutcome::Failed))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn failed(ids: &[&str]) -> ControlOutcome {
        ControlOutcome::Failed(ControlFault {
            no_tests: false,
            failed_tests: ids
                .iter()
                .map(|id| FailedTest {
                    test_id: (*id).to_owned(),
                    error: None,
                })
                .collect(),
            detail: "failed".to_owned(),
        })
    }

    fn failed_ids(outcome: &ControlOutcome) -> Vec<&str> {
        match outcome {
            ControlOutcome::Failed(fault) => fault
                .failed_tests
                .iter()
                .map(|test| test.test_id.as_str())
                .collect(),
            _ => Vec::new(),
        }
    }

    #[test]
    fn the_failed_tests_of_each_part_add_up() {
        let merged = merge(vec![
            Ok(failed(&["a"])),
            Ok(ControlOutcome::Passed),
            Ok(failed(&["c"])),
        ])
        .expect("merged");
        assert_eq!(failed_ids(&merged), ["a", "c"]);
    }

    #[test]
    fn an_error_that_stops_the_run_wins_over_a_part_with_no_result() {
        let merged = merge(vec![
            Ok(ControlOutcome::Fault("gone".to_owned())),
            Err(ScheduleError::Integrity("changed".to_owned())),
        ]);
        assert!(matches!(merged, Err(ScheduleError::Integrity(_))));
    }

    #[test]
    fn each_passed_part_gives_a_pass() {
        let merged = merge(vec![Ok(ControlOutcome::Passed), Ok(ControlOutcome::Passed)]);
        assert!(matches!(merged, Ok(ControlOutcome::Passed)));
    }

    #[test]
    fn a_part_with_no_result_wins_over_failed_tests() {
        let merged = merge(vec![
            Ok(failed(&["a"])),
            Ok(ControlOutcome::TimedOut("slow".to_owned())),
            Ok(ControlOutcome::Fault("gone".to_owned())),
        ]);
        assert!(matches!(merged, Ok(ControlOutcome::TimedOut(_))));
    }
}
