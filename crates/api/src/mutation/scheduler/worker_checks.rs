//! The worker mode of each set of covering test files.
//!
//! A Vitest companion runs the test files of a mutant run in a new worker
//! for each file, unless a check proved that the tests give the same result
//! in one shared worker. The scheduler asks for that check the first time a
//! worker runs a set of test files that enough jobs share. The check runs
//! each test that a job of the set runs, so each later run of the set selects
//! only checked tests. The scheduler keeps the mode for the run, and each
//! worker sends it with each run of the set.
//!
//! The check of a set finds the interactions between the test files of the
//! set. A run of other files in the same shared worker cannot occur, because
//! a shared worker holds the files of one run. A reused worker process holds
//! later runs of other sets too, and the companion guards that state: it
//! loads the source modules again for each run, gives a new process after a
//! test changed the global state, and runs a kill in a later file, or in the
//! first run of a set after another set, again in a new worker.

use super::super::protocol::{CheckWorkersRequest, RunWorkerMode};
use super::*;

/// The fewest jobs with the same test files that get a check. A check costs
/// about two runs of the tests, and each later run in a shared or reused
/// worker saves most of the start of one worker for each test file.
const MIN_JOBS_FOR_CHECK: usize = 3;

/// The most rounds of a check: one in a new process, then one that loads
/// each module a second time or, after a first round that failed, the plain
/// shared check.
const CHECK_ROUNDS: u64 = 2;

/// The jobs of one set of test files.
#[derive(Debug, Default)]
struct FileSet {
    jobs: usize,
    /// The tests of the jobs of the set.
    tests: BTreeSet<String>,
    state: Option<CheckState>,
}

#[derive(Debug, PartialEq, Eq)]
enum CheckState {
    /// A worker runs the check now.
    Running,
    /// The mode that the check gave. `tests` are the tests that ran in the
    /// check, `candidates` the tests of the set when the check started. A
    /// companion leaves out a test that it cannot run, so `tests` can be
    /// fewer.
    Decided {
        mode: RunWorkerMode,
        tests: BTreeSet<String>,
        candidates: BTreeSet<String>,
    },
}

/// One set of test files in a check, with the tests that the check runs
/// for it.
pub(super) type CheckedSet = (Vec<String>, Vec<String>);

/// What a worker does before a mutant run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Claim {
    /// Send this mode with the run.
    Decided(RunWorkerMode),
    /// Check the tests of these sets together first. This worker owns the
    /// check.
    Check(Vec<CheckedSet>),
    /// Run with a new worker for each test file: too few jobs share the
    /// files, another worker checks them now, or the run has a test that no
    /// check ran.
    Isolated,
}

/// The jobs and the checked mode of each set of test files, for all workers
/// of a run. A key is the sorted test files of a job.
///
/// The first check of a phase runs the tests of each set that needs a check
/// together, which costs about one shared run of the covering tests of the
/// phase. When that check passes, it decides each of these sets. When it
/// fails, each set gets a check of its own, so one set with an interaction
/// keeps only its own runs isolated.
#[derive(Debug, Default)]
pub(super) struct WorkerChecks {
    sets: Mutex<FxHashMap<Vec<String>, FileSet>>,
    /// A check of more than one set failed: each later check is for one set.
    batch_failed: AtomicBool,
}

impl FileSet {
    /// True when the set needs a check: enough jobs and no check yet, or
    /// tests that a later phase added after the check.
    fn needs_check(&self) -> bool {
        match &self.state {
            Some(CheckState::Running) => false,
            Some(CheckState::Decided { candidates, .. }) => !self.tests.is_subset(candidates),
            None => self.jobs >= MIN_JOBS_FOR_CHECK,
        }
    }

    fn claim_check(&mut self, files: &[String]) -> CheckedSet {
        self.state = Some(CheckState::Running);
        (files.to_vec(), self.tests.iter().cloned().collect())
    }
}

impl WorkerChecks {
    /// Adds the jobs of a phase to the sets of their test files.
    pub(super) fn count_jobs(&self, jobs: &VecDeque<Job>) {
        let Ok(mut sets) = self.sets.lock() else {
            return;
        };
        for job in jobs {
            let set = sets.entry(job.test_files.clone()).or_default();
            set.jobs += 1;
            set.tests.extend(job.tests.iter().cloned());
        }
    }

    /// The step before a mutant run of `tests` of `files`. Without
    /// `may_check`, for example after the time budget, no check starts.
    pub(super) fn claim(&self, files: &[String], tests: &[String], may_check: bool) -> Claim {
        let Ok(mut sets) = self.sets.lock() else {
            return Claim::Isolated;
        };
        let Some(set) = sets.get_mut(files) else {
            return Claim::Isolated;
        };
        if let Some(CheckState::Decided {
            mode,
            tests: checked,
            ..
        }) = &set.state
            && tests.iter().all(|test| checked.contains(test))
        {
            return Claim::Decided(*mode);
        }
        if !may_check || !set.needs_check() {
            return Claim::Isolated;
        }
        if self.batch_failed.load(Ordering::Relaxed) {
            return Claim::Check(vec![set.claim_check(files)]);
        }
        let mut batch: Vec<CheckedSet> = sets
            .iter_mut()
            .filter(|(_, each)| each.needs_check())
            .map(|(key, each)| each.claim_check(key))
            .collect();
        batch.sort_unstable();
        Claim::Check(batch)
    }

    /// Keeps the mode that a check gave for each set of `batch`. `runnable`
    /// tells which tests the check could run.
    pub(super) fn decide(
        &self,
        batch: &[CheckedSet],
        runnable: impl Fn(&String) -> bool,
        mode: RunWorkerMode,
    ) {
        let Ok(mut sets) = self.sets.lock() else {
            return;
        };
        for (files, candidates) in batch {
            sets.entry(files.clone()).or_default().state = Some(CheckState::Decided {
                mode,
                tests: candidates
                    .iter()
                    .filter(|test| runnable(test))
                    .cloned()
                    .collect(),
                candidates: candidates.iter().cloned().collect(),
            });
        }
    }

    /// Opens the sets of a failed check of more than one set again, so that
    /// each gets a check of its own.
    pub(super) fn reopen(&self, batch: &[CheckedSet]) {
        self.batch_failed.store(true, Ordering::Relaxed);
        let Ok(mut sets) = self.sets.lock() else {
            return;
        };
        for (files, _) in batch {
            if let Some(set) = sets.get_mut(files) {
                set.state = None;
            }
        }
    }
}

/// The mode for a mutant run, or a fault of the check that ended the
/// companion.
pub(super) enum ModeStep {
    Mode(Option<RunWorkerMode>),
    Restart,
}

impl Worker<'_> {
    /// The worker mode to send with the next mutant run of `tests` of `job`.
    /// `None` sends no mode: a new worker for each test file.
    pub(super) fn worker_mode_for(
        &mut self,
        job: &Job,
        tests: &[String],
        caps: Capabilities,
    ) -> ModeStep {
        if !caps.worker_check {
            return ModeStep::Mode(None);
        }
        let may_check = !reserve::expired(self.config);
        let batch = match self.memo.checks.claim(&job.test_files, tests, may_check) {
            Claim::Decided(RunWorkerMode::Isolated) | Claim::Isolated => {
                return ModeStep::Mode(None);
            }
            Claim::Decided(mode) => return ModeStep::Mode(Some(mode)),
            Claim::Check(batch) => batch,
        };
        // The check runs only tests that this companion can run. A test
        // that it leaves out stays unchecked, so a run with it is isolated.
        let checked: Vec<String> = batch
            .iter()
            .flat_map(|(_, candidates)| candidates.iter())
            .filter(|test| self.known_tests.contains(*test) && !self.memo.is_failed(test))
            .cloned()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let answer = if checked.is_empty() {
            Some(RunWorkerMode::Isolated)
        } else {
            self.check_workers(job, &checked)
        };
        let passed = matches!(answer, Some(RunWorkerMode::Shared | RunWorkerMode::Reused));
        if batch.len() > 1 && !passed {
            self.memo.checks.reopen(&batch);
        } else {
            let checked_set: FxHashSet<&String> = checked.iter().collect();
            self.memo.checks.decide(
                &batch,
                |test| checked_set.contains(test),
                answer.unwrap_or(RunWorkerMode::Isolated),
            );
        }
        match answer {
            Some(mode @ (RunWorkerMode::Shared | RunWorkerMode::Reused))
                if tests.iter().all(|test| checked.contains(test)) =>
            {
                ModeStep::Mode(Some(mode))
            }
            Some(_) => ModeStep::Mode(None),
            None => ModeStep::Restart,
        }
    }

    /// Asks the companion for the worker mode of `tests`. `None` when the
    /// check gave no answer: the companion is then reset. One round runs
    /// fewer tests than a dry run of their files, so the setup timeout,
    /// which bounds a dry run, also bounds a round.
    fn check_workers(&mut self, job: &Job, tests: &[String]) -> Option<RunWorkerMode> {
        let setup_ms = u64::try_from(self.config.runner_timeout.as_millis()).unwrap_or(u64::MAX);
        let timeout_ms = job.timeout_ms.max(setup_ms);
        let request = Request::CheckWorkers(CheckWorkersRequest {
            test_ids: tests.to_vec(),
            timeout_ms,
        });
        let round_ms = timeout_ms.saturating_add(job.setup_ms);
        let watchdog = Duration::from_millis(
            round_ms
                .saturating_mul(CHECK_ROUNDS)
                .saturating_add(WATCHDOG_GRACE_MS),
        );
        let answer = self.link.as_mut()?.request(&request, watchdog);
        if let Ok(Response::WorkersChecked(checked)) = answer {
            return Some(checked.worker_mode);
        }
        self.reset();
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(tests: &[&str]) -> Vec<String> {
        tests.iter().map(|test| (*test).to_owned()).collect()
    }

    fn set_of(jobs: usize, tests: &[&str]) -> FileSet {
        FileSet {
            jobs,
            tests: ids(tests).into_iter().collect(),
            state: None,
        }
    }

    fn checks_with(entries: &[(&[&str], FileSet)]) -> WorkerChecks {
        let checks = WorkerChecks::default();
        let mut sets = checks.sets.lock().expect("lock");
        for (files, set) in entries {
            sets.insert(
                ids(files),
                FileSet {
                    jobs: set.jobs,
                    tests: set.tests.clone(),
                    state: None,
                },
            );
        }
        drop(sets);
        checks
    }

    const ALL: fn(&String) -> bool = |_| true;

    #[test]
    fn the_first_check_holds_each_set_that_needs_one() {
        let checks = checks_with(&[
            (
                &["a", "b"],
                set_of(MIN_JOBS_FOR_CHECK, &["a::1", "a::2", "b::1"]),
            ),
            (&["c"], set_of(MIN_JOBS_FOR_CHECK, &["c::1"])),
            (&["d"], set_of(MIN_JOBS_FOR_CHECK - 1, &["d::1"])),
        ]);
        let Claim::Check(batch) = checks.claim(&ids(&["c"]), &ids(&["c::1"]), true) else {
            panic!("the first claim starts a check");
        };
        assert_eq!(
            batch,
            vec![
                (ids(&["a", "b"]), ids(&["a::1", "a::2", "b::1"])),
                (ids(&["c"]), ids(&["c::1"])),
            ],
            "a rare set is not in the check"
        );
        assert_eq!(
            checks.claim(&ids(&["a", "b"]), &ids(&["a::2"]), true),
            Claim::Isolated,
            "a set in a running check runs isolated"
        );
        checks.decide(&batch, ALL, RunWorkerMode::Reused);
        assert_eq!(
            checks.claim(&ids(&["a", "b"]), &ids(&["a::2", "b::1"]), true),
            Claim::Decided(RunWorkerMode::Reused)
        );
        assert_eq!(
            checks.claim(&ids(&["d"]), &ids(&["d::1"]), true),
            Claim::Isolated
        );
    }

    #[test]
    fn a_failed_check_of_more_sets_gives_each_set_its_own_check() {
        let checks = checks_with(&[
            (&["a"], set_of(MIN_JOBS_FOR_CHECK, &["a::1"])),
            (&["c"], set_of(MIN_JOBS_FOR_CHECK, &["c::1"])),
        ]);
        let Claim::Check(batch) = checks.claim(&ids(&["a"]), &ids(&["a::1"]), true) else {
            panic!("the first claim starts a check");
        };
        assert_eq!(batch.len(), 2);
        checks.reopen(&batch);
        assert_eq!(
            checks.claim(&ids(&["c"]), &ids(&["c::1"]), true),
            Claim::Check(vec![(ids(&["c"]), ids(&["c::1"]))])
        );
        assert_eq!(
            checks.claim(&ids(&["a"]), &ids(&["a::1"]), true),
            Claim::Check(vec![(ids(&["a"]), ids(&["a::1"]))])
        );
    }

    #[test]
    fn tests_that_a_later_phase_adds_get_a_new_check() {
        let checks = checks_with(&[(&["a"], set_of(MIN_JOBS_FOR_CHECK, &["a::1"]))]);
        let files = ids(&["a"]);
        let Claim::Check(batch) = checks.claim(&files, &ids(&["a::1"]), true) else {
            panic!("the first claim starts a check");
        };
        checks.decide(&batch, ALL, RunWorkerMode::Shared);
        assert_eq!(
            checks.claim(&files, &ids(&["a::2"]), true),
            Claim::Isolated,
            "a test that no phase registered gets no check"
        );

        let mut retry = VecDeque::new();
        retry.push_back(Job {
            id: "m9".to_owned(),
            tests: ids(&["a::1", "a::2"]),
            targets: Vec::new(),
            timeout_ms: 1,
            loop_budget_ms: 1,
            setup_ms: 0,
            static_mutant: false,
            test_files: files.clone(),
            cache_key: None,
            confirmation: None,
            first_tests: Vec::new(),
            retry_extra_ms: 0,
            estimate_ms: 0,
            part: None,
        });
        checks.count_jobs(&retry);
        assert_eq!(
            checks.claim(&files, &ids(&["a::1", "a::2"]), true),
            Claim::Check(vec![(files.clone(), ids(&["a::1", "a::2"]))])
        );
    }

    #[test]
    fn a_companion_that_checked_fewer_tests_starts_no_second_check() {
        let checks = checks_with(&[(&["a"], set_of(MIN_JOBS_FOR_CHECK, &["a::1", "a::2"]))]);
        let files = ids(&["a"]);
        let all = ids(&["a::1", "a::2"]);
        let Claim::Check(batch) = checks.claim(&files, &all, true) else {
            panic!("the first claim starts a check");
        };
        checks.decide(&batch, |test| test == "a::1", RunWorkerMode::Reused);
        assert_eq!(checks.claim(&files, &all, true), Claim::Isolated);
        assert_eq!(
            checks.claim(&files, &ids(&["a::1"]), true),
            Claim::Decided(RunWorkerMode::Reused)
        );
    }

    #[test]
    fn no_check_starts_after_the_time_budget() {
        let checks = checks_with(&[(&["a"], set_of(MIN_JOBS_FOR_CHECK, &["a::1"]))]);
        let files = ids(&["a"]);
        let one = ids(&["a::1"]);
        assert_eq!(checks.claim(&files, &one, false), Claim::Isolated);
        assert_eq!(
            checks.claim(&files, &one, true),
            Claim::Check(vec![(files.clone(), one.clone())]),
            "a refused claim leaves the set open"
        );
    }
}
