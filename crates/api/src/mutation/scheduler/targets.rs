//! The two-phase runs of `--kill-matrix-for` target tests.

use super::*;

impl Worker<'_> {
    /// Runs the target tests of `job` without bail, so each target test that
    /// kills the mutant is known. After a terminal result, runs the other
    /// covering tests with bail: the first other killer shows that the kills
    /// of the targets are not unique, and a survivor of the targets gets the
    /// same status as in normal mode.
    pub(super) fn attempt_targets(
        &mut self,
        job: &Job,
        caps: Capabilities,
    ) -> Result<Attempt, ScheduleError> {
        let targets = Job {
            tests: job.targets.clone(),
            ..job.clone()
        };
        let first = self.attempt_tests(&targets, caps, false)?;
        let terminal = match first.status {
            MutantStatus::Killed => first.kill_matrix_complete,
            MutantStatus::Survived => true,
            _ => false,
        };
        let others: Vec<String> = job
            .tests
            .iter()
            .filter(|test| !job.targets.contains(test))
            .cloned()
            .collect();
        if !terminal || others.is_empty() {
            return Ok(first);
        }
        let others = Job {
            tests: others,
            ..job.clone()
        };
        let second = self.attempt_tests(&others, caps, true)?;
        let target_kill = first.status == MutantStatus::Killed;
        let mut merged = merge_target_phases(first, second);
        let other_killer = merged
            .killing_tests
            .iter()
            .any(|test| !job.targets.contains(test));
        if target_kill && other_killer && merged.kill_matrix_complete {
            merged.kill_matrix_complete = self.other_killer_confirmed(&others, caps)?;
        }
        Ok(merged)
    }

    /// True when the other tests of a target kill pass in a control run with
    /// no active mutant. The other run is a part of the test files, so a test
    /// that depends on a target test can fail there without the mutant. Such
    /// a failure must not remove the uniqueness of a target kill. The memo
    /// gives each test set at most one control run.
    fn other_killer_confirmed(
        &mut self,
        others: &Job,
        caps: Capabilities,
    ) -> Result<bool, ScheduleError> {
        let tests: Vec<String> = others
            .tests
            .iter()
            .filter(|test| !self.memo.is_failed(test))
            .cloned()
            .collect();
        if tests.len() != others.tests.len() {
            return Ok(false);
        }
        Ok(matches!(
            self.check_control(&tests, others, caps)?,
            ControlCheck::Passed
        ))
    }
}

/// The attempt of a target-mode mutant from its two phases: `targets`, the
/// run of the target tests without bail, and `others`, the run of the other
/// covering tests with bail. `killing_tests` holds each target killer and the
/// first other killer. The evidence is complete when the target run is exact
/// and, after a target kill, the other run gave a terminal result.
fn merge_target_phases(targets: Attempt, others: Attempt) -> Attempt {
    let duration_ms = match (targets.duration_ms, others.duration_ms) {
        (Some(first), Some(second)) => Some(first.saturating_add(second)),
        (first, second) => first.or(second),
    };
    let mut dropped_tests = targets.dropped_tests;
    dropped_tests.extend(others.dropped_tests.iter().cloned());
    dropped_tests.sort();
    dropped_tests.dedup();
    let other_killer = (others.status == MutantStatus::Killed)
        .then(|| others.killed_by.clone())
        .flatten();
    // Each other test failed with no active mutant: the target run decides.
    if others.status == MutantStatus::NoCoverage {
        return Attempt {
            kill_matrix_complete: targets.kill_matrix_complete && dropped_tests.is_empty(),
            dropped_tests,
            ..targets
        };
    }
    if targets.status != MutantStatus::Killed {
        return Attempt {
            killing_tests: other_killer.into_iter().collect(),
            kill_matrix_complete: targets.kill_matrix_complete && dropped_tests.is_empty(),
            duration_ms,
            dropped_tests,
            ..others
        };
    }
    let others_terminal = match others.status {
        MutantStatus::Killed => other_killer.is_some(),
        MutantStatus::Survived => true,
        _ => false,
    };
    let mut killing_tests = targets.killing_tests;
    killing_tests.extend(other_killer);
    killing_tests.sort();
    killing_tests.dedup();
    Attempt {
        killing_tests,
        kill_matrix_complete: targets.kill_matrix_complete
            && others_terminal
            && dropped_tests.is_empty(),
        duration_ms,
        dropped_tests,
        issue: targets.issue.or(others.issue),
        ..targets
    }
}

/// The target tests of `tests`, in the same order. Empty outside target mode.
pub(super) fn job_targets(context: &JobContext<'_>, tests: &[String]) -> Vec<String> {
    context.targets.map_or_else(Vec::new, |targets| {
        tests
            .iter()
            .filter(|test| targets.contains(*test))
            .cloned()
            .collect()
    })
}

/// The test ids that the targets of `config` select, and the resolved
/// targets in `outcome`. `None` outside target mode.
pub(super) fn config_targets(
    config: &ScheduleConfig,
    tests: &[DryTest],
    caps: Capabilities,
    outcome: &mut ScheduleOutcome,
) -> Result<Option<FxHashSet<String>>, ScheduleError> {
    if config.targets.is_empty() {
        return Ok(None);
    }
    let (targets, ids) = resolve_targets(&config.targets, tests, caps)?;
    outcome.targets = targets;
    Ok(Some(ids))
}

/// Matches each `--kill-matrix-for` target against the tests of the dry run.
/// A target selects each test whose id or file equals it. Gives the targets
/// with their tests, and the union of the selected test ids.
pub(super) fn resolve_targets(
    targets: &[String],
    tests: &[DryTest],
    caps: Capabilities,
) -> Result<(Vec<MutationKillMatrixTarget>, FxHashSet<String>), ScheduleError> {
    if !caps.test_filter {
        return Err(ScheduleError::TargetsNeedTestFilter);
    }
    let mut resolved = Vec::with_capacity(targets.len());
    let mut unmatched = Vec::new();
    let mut ids = FxHashSet::default();
    for target in targets {
        let mut test_ids: Vec<String> = tests
            .iter()
            .filter(|test| test.test_id == *target || test.file == *target)
            .map(|test| test.test_id.clone())
            .collect();
        test_ids.sort();
        test_ids.dedup();
        if test_ids.is_empty() {
            unmatched.push(target.clone());
        }
        ids.extend(test_ids.iter().cloned());
        resolved.push(MutationKillMatrixTarget {
            target: target.clone(),
            test_ids,
        });
    }
    if unmatched.is_empty() {
        Ok((resolved, ids))
    } else {
        Err(ScheduleError::UnmatchedTargets(unmatched))
    }
}
