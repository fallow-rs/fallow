//! The two-phase runs of `--kill-matrix-for` target tests.

use super::*;

/// One `--kill-matrix-for` target and the dry-run tests that it selects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedTarget {
    /// The target as the caller gave it, after path normalization.
    pub target: String,
    /// The dry-run test ids that the target selects, sorted.
    pub test_ids: Vec<String>,
    /// Why the target selects no test. `None` when `test_ids` is nonempty,
    /// and also before the scheduler checks the target against the files of
    /// the dry run.
    pub unmatched: Option<Unmatched>,
}

/// Why a `--kill-matrix-for` target selects no test of the dry run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unmatched {
    /// The runner does not list the file of the target as a test file.
    NotATestFile,
    /// The runner lists the file, but the file reaches no planned code, so
    /// the dry run does not run it. A test that runs the code only in a
    /// child process is an example.
    NoPlannedCode,
    /// The dry run runs the file, but no test of the file has the target
    /// id, or each test of the file has a duplicate id.
    NoTest,
}

impl Unmatched {
    /// The reason code of the report. `NoTest` has no code of its own:
    /// `unmatched-target` describes it fully.
    pub const fn reason(self) -> Option<&'static str> {
        match self {
            Self::NotATestFile => Some("not-a-test-file"),
            Self::NoPlannedCode => Some("no-planned-code"),
            Self::NoTest => None,
        }
    }

    const fn describe(self) -> &'static str {
        match self {
            Self::NotATestFile => "the runner does not list the file as a test file",
            Self::NoPlannedCode => {
                "the file reaches no planned code, so the dry run does not run it"
            }
            Self::NoTest => "no test of the dry run has this id",
        }
    }
}

/// The test files that decide if a target can match: `runner` lists each
/// test file of the runner, and `dry` the files that the dry run runs.
#[derive(Clone, Copy)]
pub(super) struct TargetFiles<'a> {
    runner: &'a [String],
    dry: &'a [String],
}

impl<'a> TargetFiles<'a> {
    pub(super) const fn new(runner: &'a [String], dry: &'a [String]) -> Self {
        Self { runner, dry }
    }

    /// Why `target` cannot select a test before the dry run runs. `None`
    /// when the dry run runs the file of the target.
    fn unmatched(self, target: &str) -> Option<Unmatched> {
        let names = |files: &[String]| files.iter().any(|file| names_file(target, file));
        if names(self.dry) {
            None
        } else if names(self.runner) {
            Some(Unmatched::NoPlannedCode)
        } else {
            Some(Unmatched::NotATestFile)
        }
    }
}

/// True when `target` is `file` or a test id (`file::name`) of `file`.
fn names_file(target: &str, file: &str) -> bool {
    target
        .strip_prefix(file)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with("::"))
}

/// Checks the targets of `config` before the dry run. The dry run can take
/// a long time, so when the dry run runs the file of no target, the run
/// stops before it. A run with at least one possible target goes on.
pub(super) fn check_targets(
    config: &ScheduleConfig,
    caps: Capabilities,
    files: TargetFiles<'_>,
) -> Result<(), ScheduleError> {
    if config.targets.is_empty() {
        return Ok(());
    }
    if !caps.test_filter {
        return Err(ScheduleError::TargetsNeedTestFilter);
    }
    let unmatched: Option<Vec<ResolvedTarget>> = config
        .targets
        .iter()
        .map(|target| {
            files.unmatched(target).map(|reason| ResolvedTarget {
                target: target.clone(),
                test_ids: Vec::new(),
                unmatched: Some(reason),
            })
        })
        .collect();
    match unmatched {
        Some(unmatched) => Err(ScheduleError::UnmatchedTargets(unmatched)),
        None => Ok(()),
    }
}

/// The message of the error for targets that select no test.
pub(super) fn unmatched_message(targets: &[ResolvedTarget]) -> String {
    let named: Vec<String> = targets
        .iter()
        .map(|target| {
            let reason = target.unmatched.unwrap_or(Unmatched::NoTest).describe();
            format!("{} ({reason})", target.target)
        })
        .collect();
    format!(
        "no --kill-matrix-for target selects a test of the dry run: {}; the dry run runs the test files that reach the planned code",
        named.join(", ")
    )
}

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
        if target_kill && merged.kill_matrix_complete {
            merged.kill_matrix_complete = self.passes_without_mutant(&targets, caps)?
                && (!other_killer || self.passes_without_mutant(&others, caps)?);
        }
        Ok(merged)
    }

    /// True when the tests of `job` pass in a control run with no active
    /// mutant. Each phase runs a part of the covering tests, so a test that
    /// depends on the order of the tests can fail there without the mutant.
    /// A target that fails without the other tests must not count as a
    /// killer, and an other test that fails without the targets must not
    /// remove the uniqueness of a target kill. The memo gives each test set
    /// at most one control run.
    fn passes_without_mutant(
        &mut self,
        job: &Job,
        caps: Capabilities,
    ) -> Result<bool, ScheduleError> {
        let tests: Vec<String> = job
            .tests
            .iter()
            .filter(|test| !self.memo.is_failed(test))
            .cloned()
            .collect();
        if tests.len() != job.tests.len() {
            return Ok(false);
        }
        Ok(matches!(
            self.check_control(&tests, job, caps)?,
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
/// targets in `outcome`. `None` outside target mode. A target that selects
/// no test stays in `outcome` with the reason. The run stops only when no
/// target selects a test.
pub(super) fn config_targets(
    config: &ScheduleConfig,
    dry: &DryRunResult,
    caps: Capabilities,
    files: TargetFiles<'_>,
    outcome: &mut ScheduleOutcome,
) -> Result<Option<FxHashSet<String>>, ScheduleError> {
    if config.targets.is_empty() {
        return Ok(None);
    }
    let (targets, _) = resolve_targets(&config.targets, &dry.tests, caps)?;
    let (mut targets, ids) = without_duplicate_ids(targets, &reported_duplicates(dry))?;
    for target in targets
        .iter_mut()
        .filter(|target| target.test_ids.is_empty())
    {
        target.unmatched = Some(files.unmatched(&target.target).unwrap_or(Unmatched::NoTest));
    }
    if ids.is_empty() {
        return Err(ScheduleError::UnmatchedTargets(targets));
    }
    outcome.targets = targets;
    Ok(Some(ids))
}

/// The message of the error for targets that select a test with a duplicate id.
pub(super) fn duplicate_message(ids: &[String]) -> String {
    format!(
        "these --kill-matrix-for targets name a duplicate test id: {}; the runner numbers tests that share a title in report order, so the kill evidence of these tests is not reliable; give each test a unique title",
        ids.join(", ")
    )
}

/// Applies the duplicate test ids to the resolved targets. The companion
/// numbers tests that share a title in report order, so the kills of such a
/// test can belong to its twin. A target that names a duplicate id is an
/// error. A file target drops the duplicate ids and keeps its other tests;
/// a file target with no test left selects no test.
fn without_duplicate_ids(
    targets: Vec<ResolvedTarget>,
    duplicate_ids: &[String],
) -> Result<(Vec<ResolvedTarget>, FxHashSet<String>), ScheduleError> {
    let duplicates: FxHashSet<&String> = duplicate_ids.iter().collect();
    let named: Vec<String> = targets
        .iter()
        .filter(|target| duplicates.contains(&target.target))
        .map(|target| target.target.clone())
        .collect();
    if !named.is_empty() {
        return Err(ScheduleError::DuplicateTargetTestIds(named));
    }
    let mut ids = FxHashSet::default();
    let mut kept = Vec::with_capacity(targets.len());
    for mut target in targets {
        target.test_ids.retain(|id| !duplicates.contains(id));
        ids.extend(target.test_ids.iter().cloned());
        kept.push(target);
    }
    Ok((kept, ids))
}

/// Matches each `--kill-matrix-for` target against the tests of the dry run.
/// A target selects each test whose id or file equals it. Gives the targets
/// with their tests, and the union of the selected test ids. A target that
/// selects no test has an empty `test_ids`.
pub(super) fn resolve_targets(
    targets: &[String],
    tests: &[DryTest],
    caps: Capabilities,
) -> Result<(Vec<ResolvedTarget>, FxHashSet<String>), ScheduleError> {
    if !caps.test_filter {
        return Err(ScheduleError::TargetsNeedTestFilter);
    }
    let mut resolved = Vec::with_capacity(targets.len());
    let mut ids = FxHashSet::default();
    for target in targets {
        let mut test_ids: Vec<String> = tests
            .iter()
            .filter(|test| test.test_id == *target || test.file == *target)
            .map(|test| test.test_id.clone())
            .collect();
        test_ids.sort();
        test_ids.dedup();
        ids.extend(test_ids.iter().cloned());
        resolved.push(ResolvedTarget {
            target: target.clone(),
            test_ids,
            unmatched: None,
        });
    }
    Ok((resolved, ids))
}
