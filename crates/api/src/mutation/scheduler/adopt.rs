//! The start of a companion, and companions that take the test index of the
//! first dry run.
//!
//! A runner with the `adopt` capability gives an index entry for each test of
//! its dry run. A new companion then takes the entries of the test files of
//! its jobs from the scheduler and runs those tests with no dry run of its
//! own. The passing tests come from the first dry run.
//!
//! A companion with no dry run has not shown that a test passes in it. A
//! test can fail there for a reason outside the mutant, for example a file
//! or a port that another companion uses. So a kill or a timeout by a taken
//! test counts only after a control run, with no active mutant, of the taken
//! tests of the test files of the killing tests. A test that fails in that
//! control run leaves the companion, as a test that fails in the dry run of
//! a restarted companion does, and the job runs again without it.

use super::super::protocol::{AdoptRequest, AdoptedTest};
use super::control::ADOPTED_TEST_FAILURE;
use super::*;

/// The index entries of the passing tests of the first dry run, by test
/// file.
pub(super) struct TestIndex {
    by_file: FxHashMap<String, Vec<AdoptedTest>>,
}

impl TestIndex {
    /// The index of `dry`. `None` when the runner has no `adopt` capability:
    /// then each new companion does a dry run.
    pub(super) fn new(caps: Capabilities, dry: &DryRunResult) -> Option<Self> {
        if !caps.adopt {
            return None;
        }
        let mut by_file: FxHashMap<String, Vec<AdoptedTest>> = FxHashMap::default();
        for test in dry
            .tests
            .iter()
            .filter(|test| test.status == TestStatus::Pass)
        {
            // A test with no entry is not known to a new companion, so its
            // jobs run without it, as after a dry run where it failed.
            if let Some(index) = &test.index {
                by_file
                    .entry(test.file.clone())
                    .or_default()
                    .push(AdoptedTest {
                        test_id: test.test_id.clone(),
                        index: index.clone(),
                    });
            }
        }
        Some(Self { by_file })
    }
}

/// The outcome of the control run that a kill by a taken test needs.
pub(super) enum Proven {
    /// The killing tests pass with no active mutant: the attempt counts.
    Kept(Attempt),
    /// A taken test failed with no active mutant and left the companion:
    /// the job must run again.
    Rerun,
    /// The control run gave no result.
    Fault(Attempt),
}

impl Worker<'_> {
    /// Gives a companion that can run each test of the sorted `need` files.
    /// A companion that takes the test index gets the entries of the
    /// missing files. Otherwise a new companion dry-runs `scope`, or each
    /// file of the phase for `None`, and a companion with a narrower dry run
    /// is replaced: the protocol has no second dry run.
    pub(super) fn ensure(
        &mut self,
        scope: Option<&[String]>,
        need: &[String],
    ) -> Result<&mut Box<dyn CompanionLink>, EnsureError> {
        if self.link.is_some() && !self.holds(need) {
            if self.index.is_some() {
                self.adopt(need)?;
            } else {
                self.reset();
            }
        }
        if self.link.is_none() {
            let link = self.start()?;
            if self.index.is_some() {
                self.link = Some(link);
                self.loaded_files = Some(Vec::new());
                self.adopt(need)?;
            } else {
                self.dry_run(link, scope)?;
            }
        }
        self.link
            .as_mut()
            .ok_or_else(|| EnsureError::Fault("the companion is gone".to_owned(), None))
    }

    /// The fault of the companion `stage`.
    fn stage_fault(&self, stage: &str, fault: &LinkFault) -> EnsureError {
        EnsureError::Fault(
            format!(
                "the companion {stage} failed: {fault}{}{}",
                setup_timeout_hint(fault),
                parallel_state_hint(self.workers)
            ),
            fault.stderr_tail().map(str::to_owned),
        )
    }

    /// Starts a companion, sends `init`, and checks the runner version.
    fn start(&self) -> Result<Box<dyn CompanionLink>, EnsureError> {
        let mut link = self.spawner.spawn().map_err(|failure| match failure {
            SpawnFailure::Integrity(message) => {
                EnsureError::Fatal(ScheduleError::Integrity(message))
            }
            SpawnFailure::Fault(fault) => EnsureError::Fault(
                format!(
                    "the companion did not start: {fault}{}",
                    setup_timeout_hint(&fault)
                ),
                fault.stderr_tail().map(str::to_owned),
            ),
        })?;
        match link.request(
            &Request::Init(self.config.init.clone()),
            self.config.runner_timeout,
        ) {
            Ok(Response::Ready(ready)) => {
                if !self.config.runner.contains(&ready.runner_version) {
                    link.shutdown();
                    return Err(EnsureError::Fatal(version_error(
                        &self.config.runner,
                        &ready.runner_version,
                    )));
                }
                Ok(link)
            }
            Ok(_) => Err(EnsureError::Fault("init gave no ready".to_owned(), None)),
            Err(error) => Err(self.stage_fault("init", &error)),
        }
    }

    /// Dry-runs `scope`, or each file of the phase for `None`, on `link`,
    /// and keeps the tests that passed.
    fn dry_run(
        &mut self,
        mut link: Box<dyn CompanionLink>,
        scope: Option<&[String]>,
    ) -> Result<(), EnsureError> {
        let request = Request::DryRun {
            test_files: scope.unwrap_or(self.dry_files).to_vec(),
        };
        match link.request(&request, self.dry_run_timeout) {
            Ok(Response::DryRun(result)) => {
                for test in &result.tests {
                    if test.status == TestStatus::Fail
                        && let Some(error) = &test.message
                    {
                        self.memo.note_restarted_failure(
                            &test.test_id,
                            RESTARTED_DRY_RUN_FAILURE,
                            Some(error),
                        );
                    }
                }
                self.known_tests = result
                    .tests
                    .into_iter()
                    .filter(|test| test.status == TestStatus::Pass)
                    .map(|test| test.test_id)
                    .collect();
            }
            Ok(_) => {
                return Err(EnsureError::Fault(
                    "dry_run gave no dry_run_result".to_owned(),
                    None,
                ));
            }
            Err(error) => return Err(self.stage_fault("dry run", &error)),
        }
        self.link = Some(link);
        self.loaded_files = scope.map(<[String]>::to_vec);
        Ok(())
    }

    /// Gives the companion the index entries of each file of the sorted
    /// `need` files that it cannot run yet. A test that failed in a control run stays out, and so does a
    /// taken test that failed in a control run of a companion of this
    /// worker: each job without it then ends after at most one more run.
    fn adopt(&mut self, need: &[String]) -> Result<(), EnsureError> {
        let Some(index) = self.index else {
            return Ok(());
        };
        let loaded = self.loaded_files.clone().unwrap_or_default();
        let missing: Vec<&String> = need
            .iter()
            .filter(|file| loaded.binary_search(file).is_err())
            .collect();
        if missing.is_empty() {
            return Ok(());
        }
        let taken: Vec<(&String, &AdoptedTest)> = missing
            .iter()
            .flat_map(|file| {
                index
                    .by_file
                    .get(*file)
                    .into_iter()
                    .flatten()
                    .map(move |test| (*file, test))
            })
            .filter(|(_, test)| {
                !self.memo.is_failed(&test.test_id) && !self.dropped.contains(&test.test_id)
            })
            .collect();
        if !taken.is_empty() {
            let request = Request::Adopt(AdoptRequest {
                tests: taken.iter().map(|(_, test)| (*test).clone()).collect(),
            });
            let Some(link) = self.link.as_mut() else {
                return Err(EnsureError::Fault("the companion is gone".to_owned(), None));
            };
            match link.request(&request, self.config.runner_timeout) {
                Ok(Response::Adopted(_)) => {}
                Ok(_) => {
                    return Err(EnsureError::Fault("adopt gave no adopted".to_owned(), None));
                }
                Err(error) => return Err(self.stage_fault("adopt", &error)),
            }
        }
        for (file, test) in taken {
            self.known_tests.insert(test.test_id.clone());
            self.unproven.insert(test.test_id.clone(), file.clone());
        }
        let mut files = loaded;
        files.extend(missing.into_iter().cloned());
        files.sort_unstable();
        self.loaded_files = Some(files);
        Ok(())
    }

    /// The tests of `tests` that this companion took and that did not yet
    /// pass in it, with the test file of each. The worker reads them before
    /// a run, because a watchdog timeout or a restart after the run replaces
    /// the companion and its record.
    pub(super) fn taken_of(&self, tests: &[String]) -> FxHashMap<String, String> {
        if self.unproven.is_empty() {
            return FxHashMap::default();
        }
        tests
            .iter()
            .filter_map(|test| Some((test.clone(), self.unproven.get(test)?.clone())))
            .collect()
    }

    /// The test files of the killing tests of `attempt` that `taken` holds.
    /// A run with no named killing test needs the files of each of its
    /// `tests`. Empty when the attempt is no kill or timeout, or when each
    /// killing test was dry-run or proved in the companion of the run.
    fn taken_files<'t>(
        attempt: &Attempt,
        tests: &[String],
        taken: &'t FxHashMap<String, String>,
    ) -> FxHashSet<&'t str> {
        if taken.is_empty()
            || !matches!(attempt.status, MutantStatus::Killed | MutantStatus::Timeout)
        {
            return FxHashSet::default();
        }
        let killers: Vec<&String> = if !attempt.killing_tests.is_empty() {
            attempt.killing_tests.iter().collect()
        } else if let Some(killer) = &attempt.killed_by {
            vec![killer]
        } else {
            tests.iter().collect()
        };
        killers
            .iter()
            .filter_map(|test| taken.get(test.as_str()))
            .map(String::as_str)
            .collect()
    }

    /// Keeps the kill or the timeout of `attempt` only when the taken tests
    /// of the files of its killing tests pass with no active mutant. `taken`
    /// holds the taken tests of the run before it started. When the run
    /// ended the companion, a new companion takes the files and runs the
    /// control, so a test that hangs or fails with no active mutant never
    /// decides a status.
    pub(super) fn prove_taken(
        &mut self,
        job: &Job,
        tests: &[String],
        caps: Capabilities,
        attempt: Attempt,
        taken: &FxHashMap<String, String>,
    ) -> Result<Proven, ScheduleError> {
        let files = Self::taken_files(&attempt, tests, taken);
        if files.is_empty() {
            return Ok(Proven::Kept(attempt));
        }
        let fault = |worker: &mut Self, detail: String| {
            worker.reset();
            let mut fault = Attempt::runtime_error(
                job,
                format!(
                    "the control run of the tests that a restarted companion took from the first dry run gave no result: {detail}"
                ),
                None,
            );
            fault.dropped_tests.clone_from(&attempt.dropped_tests);
            Proven::Fault(fault)
        };
        if self.link.is_some() && !caps.hot_switch {
            self.reload();
        }
        match self.ensure_for(job, caps) {
            Ok(_) => {}
            Err(EnsureError::Fatal(error)) => return Err(error),
            Err(EnsureError::Fault(message, _)) => return Ok(fault(self, message)),
        }
        let mut proof: Vec<String> = self
            .unproven
            .iter()
            .filter(|(test, file)| {
                files.contains(file.as_str())
                    && self.known_tests.contains(*test)
                    && !self.memo.is_failed(test)
            })
            .map(|(test, _)| test.clone())
            .collect();
        if proof.is_empty() {
            return Ok(Proven::Kept(attempt));
        }
        proof.sort_unstable();
        let setup_ms = u64::try_from(self.config.runner_timeout.as_millis()).unwrap_or(u64::MAX);
        let timeout_ms = job.timeout_ms.max(setup_ms);
        let request = Request::Run(RunRequest {
            mutant_id: None,
            test_ids: proof.clone(),
            timeout_ms,
            loop_budget_ms: None,
            bail: false,
            worker_mode: None,
        });
        let watchdog = Duration::from_millis(timeout_ms + job.setup_ms + WATCHDOG_GRACE_MS);
        let Some(link) = self.link.as_mut() else {
            return Ok(fault(self, "the companion is gone".to_owned()));
        };
        let result = match link.request(&request, watchdog) {
            Ok(Response::Run(result)) if result.status != RunStatus::Timeout => result,
            Ok(Response::Run(_)) => {
                return Ok(fault(
                    self,
                    format!("a test ran longer than {timeout_ms} ms"),
                ));
            }
            Ok(_) => return Ok(fault(self, "run gave no run_result".to_owned())),
            Err(error) => return Ok(fault(self, error.to_string())),
        };
        Ok(match control_fault(&proof, &result) {
            None => {
                for test in &proof {
                    self.unproven.remove(test);
                }
                Proven::Kept(attempt)
            }
            Some(failed) if !failed.failed_tests.is_empty() => {
                for test in &failed.failed_tests {
                    self.known_tests.remove(&test.test_id);
                    self.unproven.remove(&test.test_id);
                    self.dropped.insert(test.test_id.clone());
                    self.memo.note_restarted_failure(
                        &test.test_id,
                        ADOPTED_TEST_FAILURE,
                        test.error.as_deref(),
                    );
                }
                Proven::Rerun
            }
            Some(failed) => fault(self, failed.detail),
        })
    }
}
