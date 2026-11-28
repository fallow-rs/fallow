//! The mutant scheduler (`protocol.md` sections 4, 8, 9 and 10).
//!
//! The scheduler owns every decision: the dry run, the mutant-to-test map,
//! the test order, bail, timeouts, retries and the final status. It talks to
//! companions only through [`CompanionLink`], so tests drive it with a fake
//! transport.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use fallow_output::{MutantAttempt, MutantFailureKind, MutantStatus};
use rustc_hash::{FxHashMap, FxHashSet};

use super::manifest::RunnerRange;
use super::protocol::{
    Capabilities, DryRunResult, DryTest, InitRequest, LinkFault, Ready, ReloadMethod, Request,
    Response, RunRequest, RunResult, RunStatus, TestStatus,
};
use super::setup_timeout::{self_check_help, setup_timeout_hint};
use super::transport::{CompanionLink, CompanionSpawner, SpawnFailure};
use super::{MutationProgress, MutationProgressSink};
use reserve::{CompanionStart, PhaseDeadline, Retry};
use retry_set::{
    WideTime, confirmation_tests, fastest_first, retried_status, retry_extra_ms, retry_order,
    whole_files,
};

mod adopt;
mod check_parts;
mod classification;
pub mod clock;
pub(super) mod control;
mod dry_facts;
mod dry_phase;
mod dry_store;
mod issues;
mod reach_select;
pub use reach_select::{ReachConfig, ReachLookup, ReachOutcome, ReachSelection, ShareProbes};
pub mod modes;
mod retry_set;
mod split;
pub mod targets;
mod window;
mod worker_checks;
use adopt::{Proven, TestIndex};
use classification::{attempt_record, is_load_kill, kill_kind, matrix_evidence};
use dry_facts::{DryRunFacts, covering_tests, read_dry_run, record_dry_run, reported_duplicates};
use dry_phase::{WarmLink, checked_jobs, holds, no_coverage_for_all, read_dry_phase, shutdown_all};
#[cfg(test)]
pub use dry_store::StoredTest;
use dry_store::joined_dry_phase;
pub use dry_store::{DryRunEntry, DryRunLookup, StoredDryRun};
use issues::{no_covering_test_message, static_map_miss_issue, undiscovered_issue};
use targets::{
    TargetFiles, check_targets, config_targets, confirmed_kill, job_targets, retries_unique_kill,
};
use window::{loop_budget_ms, round_ms, scaled_ms, settles_loop_timeout, timeout_ms};
mod stored;

pub use stored::{CachedResult, OutcomeJournal, ResultLookup};
use stored::{FirstRun, FirstRuns, job_key, journal_first_run, journal_retry, record_stored};

use control::{
    CONFIRMATION_RUN_FAILURE, CONTROL_RUN_FAILURE, ControlCheck, ControlMemo, ControlOutcome,
    ControlVerdict, FailedTest, RESTARTED_DRY_RUN_FAILURE, control_fault, first_line,
    note_control_failures, note_restarted_baseline_failure, parallel_state_hint, sorted_tests,
};

/// The extra wait of the watchdog after `timeout_ms` (section 8).
const WATCHDOG_GRACE_MS: u64 = 2000;
/// The factor on the first dry-run time, for each worker, in the dry-run
/// timeout of a respawned companion.
const RESPAWN_DRY_RUN_FACTOR: u32 = 2;

/// One planned mutant, in rank order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduledMutant {
    pub id: String,
    /// The root-relative source file.
    pub file: String,
}

/// What the scheduler runs.
#[derive(Debug, Clone)]
pub struct SchedulePlan {
    /// The planned mutants in rank order.
    pub mutants: Vec<ScheduledMutant>,
    /// The mutants that the static map marks `no_coverage`, in plan order.
    /// A dry-run hit on one is a static map miss: the scheduler runs it like
    /// a planned mutant and reports the miss (architecture section 11).
    pub unreached: Vec<ScheduledMutant>,
    /// The mutants after the `--max-mutants` limit, in rank order. A pending
    /// mutant that no passing dry-run test hits gets `no_coverage`, so the
    /// coverage does not depend on the limit. A planned mutant with no
    /// covering test does not use the limit: the first covered pending
    /// mutants in rank order run in its place.
    pub pending: Vec<ScheduledMutant>,
    /// The mutants that the `--mutants-per-line` limit left out. The
    /// scheduler never runs them. A mutant that no passing dry-run test
    /// covers gets `no_coverage`, so the coverage does not depend on the
    /// limit.
    pub sampled_out: Vec<ScheduledMutant>,
    /// Each mutant id of the plan. A hit outside this set stops the run.
    pub known_ids: FxHashSet<String>,
    /// The static test files of the plan.
    pub test_files: Vec<String>,
    /// The static test files of each source file.
    pub file_test_files: BTreeMap<String, Vec<String>>,
    /// The test files that reach each source file of the plan through static
    /// imports, also through a module that `vi.mock` replaces. A superset of
    /// `file_test_files`, used for the dry-run facts that the store keeps.
    pub reaching_test_files: BTreeMap<String, Vec<String>>,
}

/// How the scheduler runs.
#[derive(Debug, Clone)]
pub struct ScheduleConfig {
    /// The `init` request of each companion.
    pub init: InitRequest,
    /// The supported runner versions.
    pub runner: RunnerRange,
    /// The number of companions that run mutants.
    pub concurrency: usize,
    /// Collect exact test-to-mutant kill evidence without bail.
    pub kill_matrix: bool,
    /// The `--kill-matrix-for` targets: test files or runner test ids. Empty
    /// outside target mode. In target mode, a mutant runs its covering target
    /// tests without bail, then its other covering tests with bail.
    pub targets: Vec<String>,
    /// In target mode, run the other covering tests also after a target
    /// survivor, and retry each survivor. Without it, a mutant that no
    /// target kills is `not_killed_by_target`, and only a unique target kill
    /// gets a retry.
    pub target_full: bool,
    /// The timeout of `init` and the first `dry_run`. It is also the minimum
    /// dry-run timeout of a respawned companion.
    pub runner_timeout: Duration,
    /// The end of the time budget.
    pub deadline: Option<Instant>,
    /// The clock that the time budget reads.
    pub clock: Arc<dyn clock::Clock>,
    /// The result cache. `None` runs each covered mutant.
    pub cache: Option<Arc<dyn ResultLookup>>,
    /// The stored dry-run facts of each test file. `None` dry-runs each file.
    pub dry_runs: Option<Arc<dyn DryRunLookup>>,
    /// The reach selection of the dry-run files, when the run selects by reach.
    pub reach: Option<ReachConfig>,
    /// The outcome journal. It receives each final result when it is final,
    /// and gives the results of an interrupted earlier run on `--resume`.
    pub journal: Option<Arc<dyn OutcomeJournal>>,
}

/// The result of one mutant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MutantOutcome {
    pub status: MutantStatus,
    pub killed_by: Option<String>,
    /// All observed failing tests, sorted. Timeouts do not count.
    pub killing_tests: Vec<String>,
    /// True only for exact requested membership with terminal pass or fail results.
    pub kill_matrix_complete: bool,
    pub covering_tests: Vec<String>,
    pub static_mutant: bool,
    pub duration_ms: Option<u64>,
    /// True when the status comes from a first run and a retry.
    pub retried: bool,
    /// The first run and the retry of a `retried` mutant. Empty otherwise.
    pub attempts: Vec<MutantAttempt>,
    /// True when the result comes from the result cache. The mutant did not
    /// run.
    pub cached: bool,
    /// True when the result comes from the journal of an interrupted run.
    /// The mutant did not run.
    pub resumed: bool,
}

impl MutantOutcome {
    fn no_coverage() -> Self {
        Self {
            status: MutantStatus::NoCoverage,
            killed_by: None,
            killing_tests: Vec::new(),
            kill_matrix_complete: false,
            covering_tests: Vec::new(),
            static_mutant: false,
            duration_ms: None,
            retried: false,
            attempts: Vec::new(),
            cached: false,
            resumed: false,
        }
    }
}

/// A test or test file that failed with no active mutant.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct BaselineFailure {
    pub file: Option<String>,
    pub test_id: Option<String>,
    pub message: Option<String>,
    /// The mutants that ran without this test, because it did not pass in
    /// the dry run of a restarted companion or in a control run during the
    /// mutant runs. Sorted. Empty for a failure of the first dry run and of
    /// the self-check.
    pub mutant_ids: Vec<String>,
}

/// An error that did not stop the run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduleIssue {
    pub code: &'static str,
    pub message: String,
    pub mutant_id: Option<String>,
    pub stderr_tail: Option<String>,
}

/// The wall time of each scheduler phase.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ScheduleTimings {
    pub setup: Duration,
    pub dry_run: Duration,
    pub mutants: Duration,
    pub retry: Duration,
    /// The summed worker time of the survivor retries that ran each test
    /// of their test files.
    pub retry_wide_worker: Duration,
}

/// The result of a schedule.
#[derive(Debug, Clone)]
pub struct ScheduleOutcome {
    pub ready: Option<Ready>,
    pub results: FxHashMap<String, MutantOutcome>,
    /// Current baseline test files, keyed by opaque runner test id.
    pub test_files: FxHashMap<String, String>,
    /// The test ids that the dry run reports more than one time, sorted.
    pub duplicate_test_ids: Vec<String>,
    pub baseline_failures: Vec<BaselineFailure>,
    pub unloaded_paths: Vec<String>,
    /// The static test files that the runner does not list, in plan order. Their
    /// tests never ran, so the result of their mutants is not conclusive.
    pub undiscovered_test_files: Vec<String>,
    pub issues: Vec<ScheduleIssue>,
    /// The survivors and timeouts that kept their first status without a
    /// retry because the time budget ended, sorted.
    pub unretried: Vec<String>,
    pub timings: ScheduleTimings,
    pub workers: usize,
    /// The cache key of each mutant that ran or that the journal resumed.
    /// Empty without a cache and without a journal.
    pub cache_keys: FxHashMap<String, String>,
    /// The pending mutants that took the place of an uncovered planned
    /// mutant inside the `--max-mutants` limit, in rank order.
    pub promoted: Vec<String>,
    /// Each `--kill-matrix-for` target with the dry-run tests that it
    /// selects, in the given order. Empty outside target mode.
    pub targets: Vec<targets::ResolvedTarget>,
    /// The mutant runs and their time in each worker mode. `None` when no
    /// run stated a mode.
    pub worker_modes: Option<modes::WorkerModeCounts>,
    /// The test files whose dry-run facts came from the store.
    pub dry_run_reused: usize,
    /// The test files that the first dry run ran, because the store had no
    /// valid facts for them. Zero without a store.
    pub dry_run_missed: usize,
    /// The count of missed test files for each reason, see `dry_store`.
    pub dry_run_miss_reasons: BTreeMap<&'static str, usize>,
    /// The new dry-run facts of each test file, for the store.
    pub dry_run_entries: Vec<DryRunEntry>,
    /// The reach selection and the probe ids of the dry run.
    pub reach: Option<ReachOutcome>,
}

impl ScheduleOutcome {
    /// An outcome with no result.
    fn empty() -> Self {
        Self {
            ready: None,
            results: FxHashMap::default(),
            test_files: FxHashMap::default(),
            duplicate_test_ids: Vec::new(),
            baseline_failures: Vec::new(),
            unloaded_paths: Vec::new(),
            undiscovered_test_files: Vec::new(),
            issues: Vec::new(),
            unretried: Vec::new(),
            timings: ScheduleTimings::default(),
            workers: 0,
            cache_keys: FxHashMap::default(),
            promoted: Vec::new(),
            targets: Vec::new(),
            worker_modes: None,
            dry_run_reused: 0,
            dry_run_missed: 0,
            dry_run_miss_reasons: BTreeMap::new(),
            dry_run_entries: Vec::new(),
            reach: None,
        }
    }
}

/// Why the run stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScheduleError {
    /// A companion file changed after install (fail-closed rule 1).
    Integrity(String),
    /// The first companion did not start, load the runner, or dry-run.
    Setup {
        message: String,
        stderr_tail: Option<String>,
        /// The watchdog fired before the companion answered.
        timed_out: bool,
    },
    /// The runner version is outside the manifest range.
    RunnerVersion(String),
    /// A hit names a mutant outside the plan (fail-closed rule 9).
    UnknownHit(String),
    /// An error outside each test file failed the dry run.
    Baseline(Vec<String>),
    /// The control run of the self-check, with no active mutant, did not pass
    /// through the run path of a mutant run.
    RunPath { message: String, help: String },
    /// No `--kill-matrix-for` target selects a test of the dry run. Each
    /// target has the reason.
    UnmatchedTargets(Vec<targets::ResolvedTarget>),
    /// The runner cannot run a subset of the tests of a file, so the target
    /// tests cannot run apart from the other tests.
    TargetsNeedTestFilter,
    /// A `--kill-matrix-for` target selects a test with a duplicate test id.
    DuplicateTargetTestIds(Vec<String>),
}

impl std::fmt::Display for ScheduleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Integrity(message)
            | Self::RunnerVersion(message)
            | Self::Setup { message, .. }
            | Self::RunPath { message, .. } => f.write_str(message),
            Self::UnknownHit(id) => write!(
                f,
                "the runner reported a hit for mutant `{id}`, which is not in the plan; the instrumented files do not match the plan"
            ),
            Self::Baseline(messages) => write!(
                f,
                "the test suite fails with no active mutant: {}",
                messages.join("; ")
            ),
            Self::UnmatchedTargets(targets) => f.write_str(&targets::unmatched_message(targets)),
            Self::TargetsNeedTestFilter => f.write_str(
                "the runner cannot run single tests, so --kill-matrix-for cannot run the target tests apart from the other tests",
            ),
            Self::DuplicateTargetTestIds(ids) => f.write_str(&targets::duplicate_message(ids)),
        }
    }
}

/// One mutant to run.
#[derive(Debug, Clone)]
struct Job {
    id: String,
    /// The covering tests, fastest first.
    tests: Vec<String>,
    /// The covering `--kill-matrix-for` target tests, fastest first. Empty
    /// outside target mode.
    targets: Vec<String>,
    timeout_ms: u64,
    /// The test CPU time after which the companion ends a test that still
    /// evaluates instrumented code: `1.5 *` the covering-test time plus
    /// [`window::LOOP_BUDGET_BASE_MS`].
    loop_budget_ms: u64,
    /// `1.5 *` the setup time of the covering test files, rounded up. Only
    /// the watchdog waits for it (section 8).
    setup_ms: u64,
    static_mutant: bool,
    /// The sorted test files of `tests`.
    test_files: Vec<String>,
    /// The result cache key. `None` without a cache and without a journal.
    cache_key: Option<String>,
    /// The tests and the timeout of the retry of a survivor, when the retry
    /// runs more tests than the first run. A test can read a value that an
    /// earlier test of its file cached, so the dry run attributes no hit to
    /// it (issue #3670). `None` for a static mutant and for a run of whole
    /// files.
    confirmation: Option<(Vec<String>, u64)>,
    /// The tests of the first run, for the retry of a survivor with a
    /// confirmation set. A kill by a test outside this set is no
    /// disagreement with the first run. Empty for each other job.
    first_tests: Vec<String>,
    /// The dry-run time that the retry tests of a survivor add to `tests`.
    /// The time reserve of a budgeted first pass uses it (issue #3665).
    retry_extra_ms: u64,
    /// `1.5 *` the dry-run time of `tests` and the setup time of their
    /// files, rounded up. The split of a wide job compares it with the
    /// queue (issue #5422).
    estimate_ms: u64,
    /// The split job that this job is a part of. `None` for a whole job.
    part: Option<split::Part>,
}

impl Job {
    /// The test files that a companion must dry-run for this job. `None`
    /// means each file of the phase.
    ///
    /// A companion that runs a static mutant without fresh modules is
    /// discarded after the job when its reload restarts it, so a dry run of
    /// the other test files has no use (issue #3650).
    fn dry_scope(&self, caps: Capabilities, restarts_on_reload: bool) -> Option<&[String]> {
        let discarded = self.static_mutant && !caps.fresh_modules_per_run && restarts_on_reload;
        (discarded && !self.test_files.is_empty()).then_some(self.test_files.as_slice())
    }

    /// The watchdog wait: `timeout_ms`, the file setup time and the grace.
    fn watchdog_ms(&self) -> u64 {
        self.timeout_ms + self.setup_ms + WATCHDOG_GRACE_MS
    }

    /// The job for the retry of a timeout, with twice the test and setup
    /// time. Host load that slowed the first run often lasts into the retry,
    /// and a real infinite loop still times out.
    fn with_doubled_timeout(mut self) -> Self {
        self.timeout_ms = self.timeout_ms.saturating_mul(2);
        self.loop_budget_ms = self.loop_budget_ms.saturating_mul(2);
        self.setup_ms = self.setup_ms.saturating_mul(2);
        self
    }

    /// The job for the retry of a survivor: each test of the confirmation
    /// set, when the job has one. The test files and the cache key stay the
    /// same, because the set has only tests of the same files.
    fn for_confirmation(mut self) -> Self {
        if let Some((tests, timeout_ms)) = self.confirmation.take() {
            self.first_tests = std::mem::replace(&mut self.tests, tests);
            let extra = self.retry_extra_ms.saturating_mul(3).div_ceil(2);
            self.estimate_ms = self.estimate_ms.saturating_add(extra);
            self.loop_budget_ms = loop_budget_ms(timeout_ms);
            self.timeout_ms = timeout_ms;
        }
        self
    }

    /// True when `killed_by` is a test that the confirmation set added: the
    /// first run did not execute it.
    fn is_new_killer(&self, killed_by: Option<&String>) -> bool {
        !self.first_tests.is_empty()
            && killed_by.is_some_and(|test| !self.first_tests.contains(test))
    }
}

/// The decision of one attempt.
#[derive(Debug, Clone)]
struct Attempt {
    status: MutantStatus,
    killed_by: Option<String>,
    /// All observed failing tests, sorted. Timeouts do not count.
    killing_tests: Vec<String>,
    /// True only for exact requested membership with terminal pass or fail results.
    kill_matrix_complete: bool,
    duration_ms: Option<u64>,
    /// How the run detected the mutant, when the runner states it.
    failure_kind: Option<MutantFailureKind>,
    issue: Option<ScheduleIssue>,
    /// The covering tests that the companion left out, because they did not
    /// pass in its dry run or in a control run.
    dropped_tests: Vec<String>,
    /// A failed control run or a loop timeout of a short job decided the
    /// status, so a retry in a new companion cannot change it.
    settled: bool,
}

impl Attempt {
    fn runtime_error(job: &Job, message: String, stderr_tail: Option<String>) -> Self {
        Self {
            status: MutantStatus::RuntimeError,
            killed_by: None,
            killing_tests: Vec::new(),
            kill_matrix_complete: false,
            duration_ms: None,
            failure_kind: None,
            issue: Some(ScheduleIssue {
                code: "runtime-error",
                message,
                mutant_id: Some(job.id.clone()),
                stderr_tail,
            }),
            dropped_tests: Vec::new(),
            settled: false,
        }
    }

    /// Each covering test failed in a control run, so no test covers the
    /// mutant. The same rule as a self-check drop: `no_coverage`, no retry.
    const fn uncovered(dropped_tests: Vec<String>) -> Self {
        Self {
            status: MutantStatus::NoCoverage,
            killed_by: None,
            killing_tests: Vec::new(),
            kill_matrix_complete: false,
            duration_ms: None,
            failure_kind: None,
            issue: None,
            dropped_tests,
            settled: true,
        }
    }

    const fn decided(
        status: MutantStatus,
        killed_by: Option<String>,
        duration_ms: Option<u64>,
    ) -> Self {
        let failure_kind = match status {
            MutantStatus::Timeout => Some(MutantFailureKind::Timeout),
            _ => None,
        };
        Self {
            status,
            killed_by,
            killing_tests: Vec::new(),
            kill_matrix_complete: false,
            duration_ms,
            failure_kind,
            issue: None,
            dropped_tests: Vec::new(),
            settled: false,
        }
    }
}

/// A companion that has sent `ready`, with a dry run or a taken test index.
struct Worker<'a> {
    workers: usize,
    spawner: &'a dyn CompanionSpawner,
    config: &'a ScheduleConfig,
    dry_files: &'a [String],
    dry_run_timeout: Duration,
    link: Option<Box<dyn CompanionLink>>,
    known_tests: FxHashSet<String>,
    /// The test files of the dry run of `link`. `None` means `dry_files`.
    loaded_files: Option<Vec<String>>,
    /// A reload of a companion of this worker gave `restart_required`.
    restarts_on_reload: bool,
    memo: &'a ControlMemo,
    /// The test index of the first dry run, when the runner can take it.
    index: Option<&'a TestIndex>,
    /// The taken tests that did not yet pass in this companion, with the
    /// test file of each.
    unproven: FxHashMap<String, String>,
    /// The taken tests that failed in a control run of a companion of this
    /// worker. A new companion of the worker does not take them again.
    dropped: FxHashSet<String>,
}

enum EnsureError {
    Fatal(ScheduleError),
    Fault(String, Option<String>),
}

impl<'a> Worker<'a> {
    fn new(
        spawner: &'a dyn CompanionSpawner,
        config: &'a ScheduleConfig,
        dry_files: &'a [String],
        dry_run_timeout: Duration,
        memo: &'a ControlMemo,
    ) -> Self {
        Self {
            workers: 1,
            spawner,
            config,
            dry_files,
            dry_run_timeout,
            link: None,
            known_tests: FxHashSet::default(),
            loaded_files: None,
            restarts_on_reload: false,
            memo,
            index: None,
            unproven: FxHashMap::default(),
            dropped: FxHashSet::default(),
        }
    }

    fn reset(&mut self) {
        if let Some(mut link) = self.link.take() {
            link.shutdown();
        }
        self.known_tests.clear();
        self.unproven.clear();
        self.loaded_files = None;
    }

    /// True when the companion of this worker dry-ran each of `files`.
    fn holds(&self, files: &[String]) -> bool {
        self.link.is_some() && holds(self.loaded_files.as_deref(), files)
    }

    /// [`Self::ensure`] for the test files of `job`.
    fn ensure_for(
        &mut self,
        job: &Job,
        caps: Capabilities,
    ) -> Result<&mut Box<dyn CompanionLink>, EnsureError> {
        let scope = job.dry_scope(caps, self.restarts_on_reload);
        self.ensure(scope, &job.test_files)
    }

    /// Runs `job` one time. `Err` stops the run. A job with target tests
    /// runs in two phases, see [`Self::attempt_targets`].
    fn attempt(&mut self, job: &Job, caps: Capabilities) -> Result<Attempt, ScheduleError> {
        if job.targets.is_empty() {
            return self.attempt_tests(job, caps, !self.config.kill_matrix);
        }
        self.attempt_targets(job, caps)
    }

    /// Runs the tests of `job` one time. Without `bail`, the attempt keeps
    /// each killing test. `Err` stops the run.
    fn attempt_tests(
        &mut self,
        job: &Job,
        caps: Capabilities,
        bail: bool,
    ) -> Result<Attempt, ScheduleError> {
        let needs_reload = !caps.hot_switch || (job.static_mutant && !caps.fresh_modules_per_run);
        if needs_reload && self.link.is_some() {
            self.reload();
        }
        match self.ensure_for(job, caps) {
            Ok(_) => {}
            Err(EnsureError::Fatal(error)) => return Err(error),
            Err(EnsureError::Fault(message, tail)) => {
                self.reset();
                return Ok(Attempt::runtime_error(job, message, tail));
            }
        }
        let (tests, dropped_tests): (Vec<String>, Vec<String>) = job
            .tests
            .iter()
            .cloned()
            .partition(|test| self.known_tests.contains(test) && !self.memo.is_failed(test));
        if tests.is_empty() {
            return Ok(self.no_tests_left(job, dropped_tests));
        }
        let worker_mode = match self.worker_mode_for(job, &tests, caps) {
            worker_checks::ModeStep::Mode(mode) => mode,
            worker_checks::ModeStep::Restart => return self.attempt_tests(job, caps, bail),
        };
        let request = Request::Run(RunRequest {
            mutant_id: Some(job.id.clone()),
            test_ids: tests.clone(),
            timeout_ms: job.timeout_ms,
            loop_budget_ms: Some(job.loop_budget_ms),
            bail,
            worker_mode,
        });
        let watchdog = Duration::from_millis(job.watchdog_ms());
        let taken = self.taken_of(&tests);
        let Some(link) = self.link.as_mut() else {
            return Ok(Attempt::runtime_error(
                job,
                "the companion is gone".to_owned(),
                None,
            ));
        };
        let mut load_kill = false;
        let mut attempt = match link.request(&request, watchdog) {
            Ok(Response::Run(result)) => match self.memo.modes.classified(job, &tests, &result) {
                Ok((status, killed_by)) => {
                    load_kill = is_load_kill(&result, status, killed_by.as_ref());
                    let kind = kill_kind(&result, status, killed_by.as_deref());
                    let decided = Attempt::decided(status, killed_by, round_ms(result.duration_ms));
                    let (killing_tests, kill_matrix_complete) = if bail {
                        (Vec::new(), false)
                    } else {
                        matrix_evidence(&tests, &result)
                    };
                    Attempt {
                        killing_tests,
                        kill_matrix_complete: kill_matrix_complete && dropped_tests.is_empty(),
                        failure_kind: kind.or(decided.failure_kind),
                        settled: status == MutantStatus::Timeout
                            && result.loop_timeout
                            && settles_loop_timeout(job.loop_budget_ms),
                        ..decided
                    }
                }
                Err(reason) => {
                    self.reset();
                    Attempt::runtime_error(job, reason, None)
                }
            },
            Ok(_) => {
                self.reset();
                Attempt::runtime_error(job, "run gave no run_result".to_owned(), None)
            }
            Err(LinkFault::Timeout) => {
                self.reset();
                Attempt::decided(MutantStatus::Timeout, None, Some(job.watchdog_ms()))
            }
            Err(fault) => {
                let tail = fault.stderr_tail().map(str::to_owned);
                self.reset();
                Attempt::runtime_error(job, fault.to_string(), tail)
            }
        };
        attempt.dropped_tests = dropped_tests;
        if job.static_mutant && !caps.fresh_modules_per_run && self.link.is_some() {
            self.reload();
        }
        let attempt = match self.prove_taken(job, &tests, caps, attempt, &taken)? {
            Proven::Kept(attempt) => attempt,
            Proven::Rerun => return self.attempt_tests(job, caps, bail),
            Proven::Fault(fault) => return Ok(fault),
        };
        if load_kill {
            return self.confirm_load_kill(job, &tests, caps, attempt);
        }
        if attempt.status == MutantStatus::Killed && job.is_new_killer(attempt.killed_by.as_ref()) {
            return self.confirm_new_killer(job, &tests, caps, attempt);
        }
        Ok(attempt)
    }

    /// The attempt of a job with no covering test left in the companion:
    /// `no_coverage` when each test failed in a control run, otherwise a
    /// `runtime_error`.
    fn no_tests_left(&self, job: &Job, dropped_tests: Vec<String>) -> Attempt {
        if dropped_tests.iter().all(|test| self.memo.is_failed(test)) {
            return Attempt::uncovered(dropped_tests);
        }
        let mut attempt = Attempt::runtime_error(
            job,
            format!(
                "{}{}",
                no_covering_test_message(&dropped_tests),
                parallel_state_hint(self.workers)
            ),
            None,
        );
        attempt.dropped_tests = dropped_tests;
        attempt
    }

    /// Keeps a kill with no executed test only when `tests` pass in a control
    /// run with no active mutant: then the mutant broke the module load.
    /// Otherwise the run path executes no test, and the mutant is
    /// `runtime_error`. The memo gives each test set at most one control
    /// run. After the time budget, a control that did not run leaves the
    /// mutant `pending`.
    fn confirm_load_kill(
        &mut self,
        job: &Job,
        tests: &[String],
        caps: Capabilities,
        attempt: Attempt,
    ) -> Result<Attempt, ScheduleError> {
        // Another worker can mark a test as failed after this mutant ran. The
        // memo key is the set that the control run can still run.
        let tests: Vec<String> = tests
            .iter()
            .filter(|test| !self.memo.is_failed(test))
            .cloned()
            .collect();
        if tests.is_empty() {
            let mut dropped = attempt.dropped_tests;
            dropped.extend(
                job.tests
                    .iter()
                    .filter(|test| self.memo.is_failed(test))
                    .cloned(),
            );
            dropped.sort();
            dropped.dedup();
            return Ok(Attempt::uncovered(dropped));
        }
        let dropped_tests = attempt.dropped_tests.clone();
        let failed = |detail: &str, settled: bool| {
            let mut failed = Attempt::runtime_error(
                job,
                format!(
                    "the run executed no tests and reported an error, and the same tests do not pass in a control run with no active mutant: {detail}"
                ),
                None,
            );
            failed.dropped_tests.clone_from(&dropped_tests);
            failed.settled = settled;
            failed
        };
        Ok(match self.check_control(&tests, job, caps)? {
            ControlCheck::Passed => attempt,
            ControlCheck::Failed { detail, settled } => failed(&detail, settled),
            ControlCheck::NoTime => Attempt::decided(MutantStatus::Pending, None, None),
        })
    }

    /// Keeps the kill of a survivor retry by a test that the first run did
    /// not execute only when the retry tests pass in a control run with no
    /// active mutant. A test can fail in a run of part of its file because
    /// it depends on the order of the tests. When the control run does not
    /// pass, the first run decides: the mutant stays `survived`, and the
    /// tests that failed in the control run are baseline failures.
    fn confirm_new_killer(
        &mut self,
        job: &Job,
        tests: &[String],
        caps: Capabilities,
        attempt: Attempt,
    ) -> Result<Attempt, ScheduleError> {
        let survived = |worker: &Self, attempt: Attempt| {
            let mut survived = Attempt::decided(MutantStatus::Survived, None, attempt.duration_ms);
            survived.dropped_tests = attempt.dropped_tests;
            survived.dropped_tests.extend(
                job.tests
                    .iter()
                    .filter(|test| worker.memo.is_failed(test))
                    .cloned(),
            );
            survived.dropped_tests.sort();
            survived.dropped_tests.dedup();
            survived.settled = true;
            survived
        };
        // Another worker can mark the killer as failed after this mutant
        // ran. A test that fails with no active mutant decides nothing.
        if attempt
            .killed_by
            .as_ref()
            .is_some_and(|killer| self.memo.is_failed(killer))
        {
            return Ok(survived(self, attempt));
        }
        let tests: Vec<String> = tests
            .iter()
            .filter(|test| !self.memo.is_failed(test))
            .cloned()
            .collect();
        Ok(match self.check_control(&tests, job, caps)? {
            ControlCheck::Passed => attempt,
            ControlCheck::Failed { settled: true, .. } => survived(self, attempt),
            ControlCheck::Failed {
                detail,
                settled: false,
            } => {
                let mut fault = Attempt::runtime_error(
                    job,
                    format!(
                        "the run killed the mutant with a test that the first run did not execute, and the control run of the same tests gave no result: {detail}"
                    ),
                    None,
                );
                fault.dropped_tests = attempt.dropped_tests;
                fault
            }
            ControlCheck::NoTime => Attempt::decided(MutantStatus::Pending, None, None),
        })
    }

    /// The control evidence about `tests`. The memo gives each test set at
    /// most one control run. After the time budget, a test set with no
    /// stored result gives [`ControlCheck::NoTime`].
    fn check_control(
        &mut self,
        tests: &[String],
        job: &Job,
        caps: Capabilities,
    ) -> Result<ControlCheck, ScheduleError> {
        let memo = self.memo;
        let cell = memo.cell(tests);
        let Ok(mut known) = cell.lock() else {
            return Ok(ControlCheck::Passed);
        };
        let settled = |detail: String| ControlCheck::Failed {
            detail,
            settled: true,
        };
        match &*known {
            Some(ControlVerdict::Passed) => return Ok(ControlCheck::Passed),
            Some(ControlVerdict::Failed) => {
                return Ok(settled(
                    "an earlier control run of these tests failed".to_owned(),
                ));
            }
            Some(ControlVerdict::TimedOut(reason)) => return Ok(settled(reason.clone())),
            None => {}
        }
        if reserve::expired(self.config) {
            return Ok(ControlCheck::NoTime);
        }
        let (outcome, ran) = self.control_run(tests, job, caps)?;
        // The result belongs to the set that ran. When the control run left
        // out a test that another worker marked meanwhile, the cell of this
        // set stays open.
        let mut remember = |verdict: ControlVerdict| {
            if sorted_tests(&ran) == sorted_tests(tests) {
                *known = Some(verdict);
            } else if !ran.is_empty() {
                memo.store(&ran, verdict);
            }
        };
        Ok(match outcome {
            ControlOutcome::Passed => {
                remember(ControlVerdict::Passed);
                ControlCheck::Passed
            }
            ControlOutcome::Failed(fault) => {
                remember(ControlVerdict::Failed);
                settled(fault.detail)
            }
            // A timeout is stored and settles the mutant: a retry or a later
            // mutant with the same tests would wait as long again.
            ControlOutcome::TimedOut(detail) => {
                let reason = format!("the control run timed out ({detail})");
                remember(ControlVerdict::TimedOut(reason.clone()));
                settled(reason)
            }
            // A fault of the companion decides nothing, so the retry stays.
            ControlOutcome::Fault(message) => ControlCheck::Failed {
                detail: message,
                settled: false,
            },
        })
    }

    /// Runs `tests` with no active mutant through the run path of a mutant
    /// run. The result is never a mutant result, so the cache never stores
    /// it. Gives the outcome and the tests that the run requested.
    ///
    /// The timeout is the timeout of `job`, and at least the setup timeout:
    /// a control run decides about each later mutant, so host load must not
    /// end it early.
    fn control_run(
        &mut self,
        tests: &[String],
        job: &Job,
        caps: Capabilities,
    ) -> Result<(ControlOutcome, Vec<String>), ScheduleError> {
        if !caps.hot_switch && self.link.is_some() {
            self.reload();
        }
        // The control tests are a subset of the tests of `job`.
        match self.ensure_for(job, caps) {
            Ok(_) => {}
            Err(EnsureError::Fatal(error)) => return Err(error),
            Err(EnsureError::Fault(message, _)) => {
                self.reset();
                return Ok((ControlOutcome::Fault(message), Vec::new()));
            }
        }
        let tests: Vec<String> = tests
            .iter()
            .filter(|test| self.known_tests.contains(*test) && !self.memo.is_failed(test))
            .cloned()
            .collect();
        if tests.is_empty() {
            return Ok((
                ControlOutcome::Fault(
                    "no covering test passed in the dry run of the companion".to_owned(),
                ),
                tests,
            ));
        }
        let setup_ms = u64::try_from(self.config.runner_timeout.as_millis()).unwrap_or(u64::MAX);
        let timeout_ms = job.timeout_ms.max(setup_ms);
        // No bail: the run must name each covering test that fails without a
        // mutant, so that each later mutant run leaves it out.
        let request = Request::Run(RunRequest {
            mutant_id: None,
            test_ids: tests.clone(),
            timeout_ms,
            loop_budget_ms: None,
            bail: false,
            worker_mode: None,
        });
        let watchdog = Duration::from_millis(timeout_ms + job.setup_ms + WATCHDOG_GRACE_MS);
        let Some(link) = self.link.as_mut() else {
            return Ok((
                ControlOutcome::Fault("the companion is gone".to_owned()),
                tests,
            ));
        };
        let outcome = match link.request(&request, watchdog) {
            // A timeout is checked first: a test that the timeout stopped
            // is no evidence about the run path or about the test.
            Ok(Response::Run(result)) if result.status == RunStatus::Timeout => {
                ControlOutcome::TimedOut(format!("a test ran longer than {timeout_ms} ms"))
            }
            Ok(Response::Run(result)) => control_fault(&tests, &result)
                .map_or(ControlOutcome::Passed, ControlOutcome::Failed),
            Ok(_) => ControlOutcome::Fault("run gave no run_result".to_owned()),
            Err(LinkFault::Timeout) => ControlOutcome::TimedOut(format!(
                "the companion gave no result within {} ms",
                watchdog.as_millis()
            )),
            Err(fault) => ControlOutcome::Fault(fault.to_string()),
        };
        match &outcome {
            ControlOutcome::Passed => {
                for test in &tests {
                    self.unproven.remove(test);
                }
            }
            // Failed tests are a fact about the tests. The companion stays.
            ControlOutcome::Failed(fault) if !fault.failed_tests.is_empty() => {
                let message = if job.first_tests.is_empty() {
                    CONTROL_RUN_FAILURE
                } else {
                    CONFIRMATION_RUN_FAILURE
                };
                self.memo.mark_failed(&fault.failed_tests, message);
            }
            ControlOutcome::Failed(_) | ControlOutcome::Fault(_) | ControlOutcome::TimedOut(_) => {
                self.reset();
            }
        }
        Ok((outcome, tests))
    }

    /// Sends `reload`. A restart request or a fault replaces the companion.
    fn reload(&mut self) {
        let Some(link) = self.link.as_mut() else {
            return;
        };
        match link.request(&Request::Reload, self.config.runner_timeout) {
            Ok(Response::Reloaded(reloaded))
                if reloaded.method != ReloadMethod::RestartRequired => {}
            Ok(Response::Reloaded(_)) => {
                self.restarts_on_reload = true;
                self.reset();
            }
            _ => self.reset(),
        }
    }

    /// Runs `job`, with one retry in a new companion after a runtime error
    /// (section 10).
    fn run_with_retry(&mut self, job: &Job, caps: Capabilities) -> Result<Attempt, ScheduleError> {
        let first = self.attempt(job, caps)?;
        if first.status != MutantStatus::RuntimeError || first.settled {
            return Ok(first);
        }
        self.reset();
        let second = self.attempt(job, caps)?;
        Ok(second)
    }
}

/// The timeout of the dry run of a respawned companion: the first dry-run
/// time times [`RESPAWN_DRY_RUN_FACTOR`] times the worker count, and at least
/// `runner_timeout`. The first dry run has the host to itself, but a
/// respawned companion shares it with each other worker.
fn respawn_dry_run_timeout(
    runner_timeout: Duration,
    first_dry_run: Duration,
    workers: usize,
) -> Duration {
    let workers = u32::try_from(workers).unwrap_or(u32::MAX);
    let scaled = first_dry_run.saturating_mul(RESPAWN_DRY_RUN_FACTOR.saturating_mul(workers));
    runner_timeout.max(scaled)
}

fn version_error(range: &RunnerRange, version: &str) -> ScheduleError {
    ScheduleError::RunnerVersion(format!(
        "{} {version} is not supported; the supported range is >={} <{}",
        range.package, range.min_version, range.max_version_exclusive
    ))
}

/// The facts of the first companion that the jobs read.
struct JobContext<'a> {
    dry_files: &'a [String],
    kill_matrix: bool,
    /// True for the full mode of `--kill-matrix-for` (`--kill-matrix-full`).
    target_full: bool,
    /// The dry-run test ids that the `--kill-matrix-for` targets select.
    /// `None` outside target mode.
    targets: Option<&'a FxHashSet<String>>,
    ready: &'a Ready,
    cache: Option<&'a dyn ResultLookup>,
    journal: Option<&'a dyn OutcomeJournal>,
    /// True when a time budget is set.
    budgeted: bool,
}

impl JobContext<'_> {
    /// True when the run keeps the killing tests of each mutant.
    const fn collects_evidence(&self) -> bool {
        self.kill_matrix || self.targets.is_some()
    }
}

/// Records a mutant that gets no job and returns true for it: a mutant with
/// no covering test is `no_coverage`. Target mode runs only the mutants that
/// a target test covers, so each other mutant is `pruned`. Neither uses a
/// place inside the limit.
fn record_unrun(
    context: &JobContext<'_>,
    mutant: &ScheduledMutant,
    tests: &BTreeSet<String>,
    static_mutant: bool,
    outcome: &mut ScheduleOutcome,
) -> bool {
    let result = if tests.is_empty() {
        MutantOutcome {
            static_mutant,
            ..MutantOutcome::no_coverage()
        }
    } else if context
        .targets
        .is_some_and(|targets| !tests.iter().any(|test| targets.contains(test)))
    {
        MutantOutcome {
            status: MutantStatus::Pruned,
            covering_tests: tests.iter().cloned().collect(),
            static_mutant,
            ..MutantOutcome::no_coverage()
        }
    } else {
        return false;
    };
    outcome.results.insert(mutant.id.clone(), result);
    true
}

/// Where a mutant of [`build_jobs`] comes from.
#[derive(Clone, Copy, PartialEq, Eq)]
enum JobSource {
    /// Inside the `--max-mutants` limit.
    Planned,
    /// After the limit. It runs only to fill the place of an uncovered
    /// planned mutant.
    Pending,
    /// Statically `no_coverage`. It runs only on a dry-run hit, outside the
    /// limit.
    Unreached,
}

/// Builds the job of each covered mutant. A mutant with a cached result gets
/// that result and no job. The `--max-mutants` limit counts the covered
/// mutants only: an uncovered planned mutant leaves its place to the next
/// covered pending mutant in rank order. Under a time budget the regular jobs
/// run the cheapest first (the lowest dry-run time of their tests), so the
/// budget settles the most mutants. The sort is stable, so equal costs keep
/// the rank order. Static mutants stay last.
fn build_jobs(
    plan: &SchedulePlan,
    facts: &DryRunFacts,
    context: &JobContext<'_>,
    outcome: &mut ScheduleOutcome,
) -> Vec<Job> {
    let (dry_files, ready) = (context.dry_files, context.ready);
    let caps = ready.capabilities;
    let mut regular = Vec::new();
    let mut statics = Vec::new();
    // Without pending mutants the limit cut nothing, so each planned mutant
    // is inside it. With pending mutants the limit is the planned count.
    let limit = plan.mutants.len();
    let mut covered = 0;
    let planned = plan
        .mutants
        .iter()
        .map(|mutant| (mutant, JobSource::Planned));
    let pending = plan
        .pending
        .iter()
        .map(|mutant| (mutant, JobSource::Pending));
    let unreached = plan
        .unreached
        .iter()
        .map(|mutant| (mutant, JobSource::Unreached));
    for (mutant, source) in planned.chain(pending).chain(unreached) {
        if source == JobSource::Pending && covered >= limit {
            continue;
        }
        let map_miss = source == JobSource::Unreached;
        let static_mutant = facts.static_ids.contains(&mutant.id);
        let hit = static_mutant || facts.covering.contains_key(&mutant.id);
        if map_miss && !hit {
            continue;
        }
        let mut tests = covering_tests(plan, facts, dry_files, mutant, static_mutant, map_miss);
        if !caps.test_filter {
            tests = whole_files(facts, &tests);
        }
        if record_unrun(context, mutant, &tests, static_mutant, outcome) {
            continue;
        }
        match source {
            JobSource::Planned => covered += 1,
            JobSource::Pending => {
                covered += 1;
                outcome.promoted.push(mutant.id.clone());
            }
            JobSource::Unreached => {}
        }
        if map_miss {
            outcome.issues.push(static_map_miss_issue(mutant));
        }
        let confirmation = (!context.collects_evidence() && !static_mutant && caps.test_filter)
            .then(|| confirmation_tests(facts, &tests))
            .flatten();
        let (tests, total) = fastest_first(facts, tests);
        let retry_extra_ms = confirmation
            .as_ref()
            .map_or(0, |(_, retry_total)| retry_extra_ms(*retry_total, total));
        let confirmation = confirmation.map(|(set, _)| set);
        let setup = facts.setup_of(&tests);
        let setup_ms = scaled_ms(setup);
        let test_files = split::files_of(facts, &tests);
        let targets = job_targets(context, &tests);
        let cache_key = job_key(context, mutant, (&tests, &targets), &test_files);
        if record_stored(
            context,
            cache_key.as_ref(),
            mutant,
            &tests,
            static_mutant,
            outcome,
        ) {
            continue;
        }
        let job = Job {
            id: mutant.id.clone(),
            tests,
            targets,
            timeout_ms: timeout_ms(total),
            loop_budget_ms: loop_budget_ms(timeout_ms(total)),
            setup_ms,
            static_mutant,
            test_files,
            cache_key,
            confirmation,
            first_tests: Vec::new(),
            retry_extra_ms,
            estimate_ms: scaled_ms(total + setup),
            part: None,
        };
        if static_mutant {
            statics.push(job);
        } else {
            regular.push((total, job));
        }
    }
    if context.budgeted {
        regular.sort_by(|left, right| left.0.total_cmp(&right.0));
    }
    let mut jobs: Vec<Job> = regular.into_iter().map(|(_, job)| job).collect();
    jobs.extend(statics);
    jobs
}

/// Shared state of the worker threads of one phase.
struct Phase<'a> {
    workers: usize,
    jobs: Mutex<VecDeque<Job>>,
    /// The test files of the companion of each worker, while it holds a
    /// share of the dry run. `None` for each other worker.
    held: Mutex<Vec<Option<Vec<String>>>>,
    attempts: Mutex<Vec<(Job, Attempt)>>,
    fatal: Mutex<Option<ScheduleError>>,
    stop: AtomicBool,
    deadline: Option<PhaseDeadline>,
    caps: Capabilities,
    spawner: &'a dyn CompanionSpawner,
    config: &'a ScheduleConfig,
    dry_files: &'a [String],
    dry_run_timeout: Duration,
    memo: &'a ControlMemo,
    index: Option<&'a TestIndex>,
    /// The dry-run facts that divide a wide job into parts.
    facts: &'a DryRunFacts,
    /// Receives the count of finished jobs after each job.
    report: &'a (dyn Fn(usize) + Sync),
    /// Receives each finished job before the phase records it.
    finished: Finished<'a>,
}

/// Receives each finished job of a phase, on the worker thread.
type Finished<'a> = &'a (dyn Fn(&Job, &Attempt) + Sync);

impl Phase<'_> {
    /// Adds a finished job and reports the new count of finished jobs.
    fn push(&self, job: Job, attempt: Attempt) {
        (self.finished)(&job, &attempt);
        let done = self.attempts.lock().ok().map(|mut attempts| {
            attempts.push((job, attempt));
            attempts.len()
        });
        if let Some(done) = done {
            (self.report)(done);
        }
    }

    /// The next job of the worker in `slot`, in this order:
    ///
    /// 1. the first queued job whose test files its companion dry-ran;
    /// 2. the first queued job that the share of no other worker holds;
    /// 3. from the back of the queue, a job of another share that holds more
    ///    than one queued job.
    ///
    /// A job for another companion needs a new dry run, or the index entries
    /// of its files and a control run before a kill counts, so the worker
    /// leaves the last queued job of a share to its holder.
    ///
    /// The split of a wide job happens under the same lock, so no other
    /// worker sees an empty queue and ends before the parts are queued.
    fn next_job(&self, slot: usize, worker: &Worker<'_>) -> Option<Job> {
        let mut jobs = self.jobs.lock().ok()?;
        let job = self.pick(slot, worker, &mut jobs)?;
        let job = self.spread(&mut jobs, job);
        drop(jobs);
        Some(job)
    }

    /// Removes the next job of the worker in `slot` from `jobs`, see
    /// [`Self::next_job`].
    fn pick(&self, slot: usize, worker: &Worker<'_>, jobs: &mut VecDeque<Job>) -> Option<Job> {
        let others: Vec<Vec<String>> = {
            let mut held = self.held.lock().ok()?;
            held[slot] = worker
                .loaded_files
                .clone()
                .filter(|_| worker.link.is_some());
            held.iter()
                .enumerate()
                .filter(|(index, _)| *index != slot)
                .filter_map(|(_, loaded)| loaded.clone())
                .collect()
        };
        if let Some(index) = jobs.iter().position(|job| worker.holds(&job.test_files)) {
            return jobs.remove(index);
        }
        let holder = |job: &Job| {
            others
                .iter()
                .position(|loaded| holds(Some(loaded), &job.test_files))
        };
        if let Some(index) = jobs.iter().position(|job| holder(job).is_none()) {
            return jobs.remove(index);
        }
        let mut queued = vec![0_usize; others.len()];
        for share in jobs.iter().filter_map(holder) {
            queued[share] += 1;
        }
        let index = jobs
            .iter()
            .rposition(|job| holder(job).is_some_and(|share| queued[share] > 1))?;
        jobs.remove(index)
    }

    /// Ends the claim of the worker in `slot` on the jobs of its share.
    fn release(&self, slot: usize) {
        if let Ok(mut held) = self.held.lock() {
            held[slot] = None;
        }
    }

    /// Stops the phase with its first fatal error.
    fn fail(&self, error: ScheduleError) {
        self.stop.store(true, Ordering::SeqCst);
        if let Ok(mut fatal) = self.fatal.lock() {
            fatal.get_or_insert(error);
        }
    }

    fn work(&self, slot: usize, primary: Option<WarmLink>) {
        let mut worker = Worker::new(
            self.spawner,
            self.config,
            self.dry_files,
            self.dry_run_timeout,
            self.memo,
        );
        worker.workers = self.workers;
        worker.index = self.index;
        if let Some(warm) = primary {
            worker.link = Some(warm.link);
            worker.known_tests = warm.known;
            worker.loaded_files = warm.loaded;
            worker.unproven = warm.unproven;
        }
        loop {
            if self.stop.load(Ordering::SeqCst) {
                break;
            }
            let Some(job) = self.next_job(slot, &worker) else {
                break;
            };
            if let Some(part) = job.part.clone() {
                if let Err(error) = self.run_part(&mut worker, &job, &part) {
                    self.fail(error);
                    break;
                }
                continue;
            }
            if self.deadline.as_ref().is_some_and(PhaseDeadline::passed) {
                self.push(job, Attempt::decided(MutantStatus::Pending, None, None));
                continue;
            }
            match PhaseDeadline::timed(self.deadline.as_ref(), &job, || {
                worker.run_with_retry(&job, self.caps)
            }) {
                Ok(attempt) => self.push(job, attempt),
                Err(error) => {
                    self.fail(error);
                    break;
                }
            }
        }
        self.release(slot);
        worker.reset();
    }
}

/// The companion source and runner facts that each phase shares.
#[derive(Clone, Copy)]
struct PhaseRunner<'a> {
    spawner: &'a dyn CompanionSpawner,
    config: &'a ScheduleConfig,
    caps: Capabilities,
    /// The time of one dry run of each file: the sum of the dry-run times
    /// of the shares.
    first_dry_run: Duration,
    /// The control results of the run.
    memo: &'a ControlMemo,
    /// The test index of the first dry run, when the runner can take it.
    index: Option<&'a TestIndex>,
    /// Receives the phase events of the run.
    progress: MutationProgressSink<'a>,
}

/// Runs `jobs` on up to `concurrency` workers. Each worker takes one of
/// `primaries`, the companions that already did their dry run, and prefers
/// the jobs whose test files its companion dry-ran. `event` makes the
/// progress event of the phase from the finished and the total jobs.
/// `finished` receives each finished job on its worker thread. `facts`
/// divide a wide job into parts.
fn run_phase(
    runner: PhaseRunner<'_>,
    dry_files: &[String],
    jobs: Vec<Job>,
    deadline: Option<PhaseDeadline>,
    (primaries, finished, facts): (Vec<WarmLink>, Finished<'_>, &DryRunFacts),
    event: fn(usize, usize) -> MutationProgress,
) -> Result<(Vec<(Job, Attempt)>, usize), ScheduleError> {
    let workers = runner
        .config
        .concurrency
        .max(1)
        .min(split::capacity(&jobs, runner.index.is_some(), runner.caps).max(1));
    let total = jobs.len();
    let jobs: VecDeque<Job> = jobs.into();
    runner.memo.checks.count_jobs(&jobs);
    let mut primaries = primaries;
    shutdown_all(primaries.split_off(workers.min(primaries.len())));
    let mut held: Vec<Option<Vec<String>>> =
        primaries.iter().map(|warm| warm.loaded.clone()).collect();
    held.resize(workers, None);
    let report = |done: usize| (runner.progress)(event(done, total));
    report(0);
    let phase = Phase {
        workers,
        jobs: Mutex::new(jobs),
        held: Mutex::new(held),
        attempts: Mutex::new(Vec::new()),
        fatal: Mutex::new(None),
        stop: AtomicBool::new(false),
        deadline,
        caps: runner.caps,
        spawner: runner.spawner,
        config: runner.config,
        dry_files,
        dry_run_timeout: respawn_dry_run_timeout(
            runner.config.runner_timeout,
            runner.first_dry_run,
            workers,
        ),
        memo: runner.memo,
        index: runner.index,
        facts,
        report: &report,
        finished,
    };
    let mut primaries = primaries.into_iter();
    std::thread::scope(|scope| {
        for slot in 0..workers {
            let warm = primaries.next();
            let phase = &phase;
            scope.spawn(move || phase.work(slot, warm));
        }
    });
    if let Some(error) = phase.fatal.into_inner().ok().flatten() {
        return Err(error);
    }
    Ok((phase.attempts.into_inner().unwrap_or_default(), workers))
}

fn record(
    workers: usize,
    outcome: &mut ScheduleOutcome,
    facts: &DryRunFacts,
    memo: &ControlMemo,
    job: Job,
    attempt: Attempt,
) {
    if let Some(issue) = attempt.issue {
        outcome.issues.push(issue);
    }
    // The key covers each covering test. A survivor of a run without a
    // dropped test is not proof against that test, so it is not stored.
    let partial_survivor =
        attempt.status == MutantStatus::Survived && !attempt.dropped_tests.is_empty();
    for test in attempt.dropped_tests {
        note_restarted_baseline_failure(workers, outcome, facts, memo, test, &job.id);
    }
    // `job.tests` is in measured fastest-first order. The report must not
    // depend on durations, so it lists the covering tests by id. A survivor
    // retry runs more tests than the dry run attributes to the mutant, and
    // `covering_tests` stays the dry-run hits of the first run.
    let mut covering_tests = if attempt.status == MutantStatus::NoCoverage {
        Vec::new()
    } else if job.first_tests.is_empty() {
        job.tests
    } else {
        job.first_tests
    };
    covering_tests.sort_unstable();
    match job.cache_key {
        Some(key) if !partial_survivor => {
            outcome.cache_keys.insert(job.id.clone(), key);
        }
        _ => {
            outcome.cache_keys.remove(&job.id);
        }
    }
    outcome.results.insert(
        job.id,
        MutantOutcome {
            status: attempt.status,
            killing_tests: attempt.killing_tests,
            kill_matrix_complete: attempt.kill_matrix_complete,
            killed_by: attempt.killed_by,
            covering_tests,
            static_mutant: job.static_mutant,
            duration_ms: attempt.duration_ms,
            retried: false,
            attempts: Vec::new(),
            cached: false,
            resumed: false,
        },
    );
}

/// The final status of a retried job from its first status and the retry.
/// A kill by a test that the first run did not execute is no disagreement
/// with the first run. The worker already confirmed it with a control run.
fn retry_status(first: MutantStatus, job: &Job, retry: &Attempt) -> MutantStatus {
    if first == MutantStatus::Survived
        && retry.status == MutantStatus::Killed
        && job.is_new_killer(retry.killed_by.as_ref())
    {
        MutantStatus::Killed
    } else {
        retried_status(first, retry.status)
    }
}

/// The companion error code for a runner version outside the manifest range.
const RUNNER_VERSION_CODE: &str = "runner-version";

/// Maps a setup fault to its schedule error. A `runner-version` error of the
/// companion is a [`ScheduleError::RunnerVersion`], the same error as a
/// version that the scheduler refuses after `ready`.
fn setup_error(stage: &str, fault: &LinkFault) -> ScheduleError {
    if let LinkFault::Remote(error) = fault
        && error.code == RUNNER_VERSION_CODE
    {
        return ScheduleError::RunnerVersion(error.message.clone());
    }
    ScheduleError::Setup {
        message: format!("the mutation companion {stage} failed: {fault}"),
        stderr_tail: fault.stderr_tail().map(str::to_owned),
        timed_out: matches!(fault, LinkFault::Timeout),
    }
}

/// Sends one setup request on the first companion. A failure shuts it down.
fn setup_request(
    link: &mut Box<dyn CompanionLink>,
    request: &Request,
    timeout: Duration,
) -> Result<Response, ScheduleError> {
    let stage = request.type_name();
    match link.request(request, timeout) {
        Ok(response) => Ok(response),
        Err(fault) => {
            link.shutdown();
            Err(setup_error(stage, &fault))
        }
    }
}

/// The first companion after `ready`, with the test files to dry-run.
struct Primary {
    link: Box<dyn CompanionLink>,
    ready: Ready,
    dry_files: Vec<String>,
    /// The static test files that the runner does not list.
    undiscovered: Vec<String>,
}

/// Starts the first companion, sends `init`, and checks the runner version.
fn start_primary(
    spawner: &dyn CompanionSpawner,
    plan: &SchedulePlan,
    config: &ScheduleConfig,
) -> Result<Primary, ScheduleError> {
    let (link, ready) = start_link(spawner, config)?;
    let runner_files: FxHashSet<&String> = ready.test_files.iter().collect();
    let (dry_files, undiscovered) = plan
        .test_files
        .iter()
        .cloned()
        .partition(|file| runner_files.contains(file));
    Ok(Primary {
        link,
        ready,
        dry_files,
        undiscovered,
    })
}

/// Starts a companion for the dry run, sends `init`, and checks the runner
/// version.
fn start_link(
    spawner: &dyn CompanionSpawner,
    config: &ScheduleConfig,
) -> Result<(Box<dyn CompanionLink>, Ready), ScheduleError> {
    let mut link = spawner.spawn().map_err(|failure| match failure {
        SpawnFailure::Integrity(message) => ScheduleError::Integrity(message),
        SpawnFailure::Fault(fault) => setup_error("start", &fault),
    })?;
    let request = Request::Init(config.init.clone());
    let Response::Ready(ready) = setup_request(&mut link, &request, config.runner_timeout)? else {
        link.shutdown();
        return Err(setup_error(
            "init",
            &LinkFault::Protocol("no ready".to_owned()),
        ));
    };
    if !config.runner.contains(&ready.runner_version) {
        link.shutdown();
        return Err(version_error(&config.runner, &ready.runner_version));
    }
    Ok((link, ready))
}

/// Gives `no_coverage` to each pending mutant that has no passing dry-run
/// covering test. The test is the same as for a planned mutant, so a run
/// that `--max-mutants` limits reports the coverage of the full run.
fn mark_unreached_pending(
    plan: &SchedulePlan,
    facts: &DryRunFacts,
    dry_files: &[String],
    outcome: &mut ScheduleOutcome,
) {
    for mutant in plan.pending.iter().chain(&plan.sampled_out) {
        let static_mutant = facts.static_ids.contains(&mutant.id);
        if covering_tests(plan, facts, dry_files, mutant, static_mutant, false).is_empty() {
            outcome.results.insert(
                mutant.id.clone(),
                MutantOutcome {
                    static_mutant,
                    ..MutantOutcome::no_coverage()
                },
            );
        }
    }
}

/// Records the first run of each mutant. Gives the survivors and the
/// timeouts to run again, and the evidence of their first run.
fn record_first_runs(
    config: &ScheduleConfig,
    outcome: &mut ScheduleOutcome,
    facts: &DryRunFacts,
    memo: &ControlMemo,
    attempts: Vec<(Job, Attempt)>,
) -> (Vec<Job>, FxHashMap<String, MutantAttempt>) {
    let mut suspects = Vec::new();
    let mut first_runs = FxHashMap::default();
    for (job, attempt) in attempts {
        let suspect = match retry_of(config.target_full, &job, &attempt) {
            Retry::Confirmation => Some(job.clone().for_confirmation()),
            Retry::DoubledTimeout => Some(job.clone().with_doubled_timeout()),
            Retry::Same => Some(job.clone()),
            Retry::None => None,
        };
        if let Some(suspect) = suspect {
            first_runs.insert(job.id.clone(), attempt_record(&attempt));
            suspects.push(suspect);
        }
        record(outcome.workers, outcome, facts, memo, job, attempt);
    }
    (suspects, first_runs)
}

/// How the retry phase runs `job` again after its first `attempt`. The time
/// reserve of the first pass uses the same rule.
fn retry_of(target_full: bool, job: &Job, attempt: &Attempt) -> Retry {
    match attempt.status {
        MutantStatus::Survived => Retry::Confirmation,
        MutantStatus::Timeout if !attempt.settled => Retry::DoubledTimeout,
        MutantStatus::Killed if retries_unique_kill(target_full, job, attempt) => Retry::Same,
        _ => Retry::None,
    }
}

/// Runs the first pass of `jobs` on the companions of the dry run, inside
/// the time budget less the time reserve of the retries.
fn run_first_pass(
    runner: PhaseRunner<'_>,
    facts: &DryRunFacts,
    (jobs, phase_files, links): (Vec<Job>, &[String], Vec<WarmLink>),
    outcome: &mut ScheduleOutcome,
) -> Result<Vec<(Job, Attempt)>, ScheduleError> {
    let config = runner.config;
    let finished = |job: &Job, attempt: &Attempt| journal_first_run(config, job, attempt);
    let start = CompanionStart::new(
        outcome.timings.setup,
        runner.first_dry_run,
        facts,
        runner.index.is_some(),
    );
    let (attempts, workers) = run_phase(
        runner,
        phase_files,
        jobs,
        PhaseDeadline::first_pass(config, start),
        (links, &finished, facts),
        |done, total| MutationProgress::Mutants { done, total },
    )?;
    outcome.workers = workers;
    Ok(attempts)
}

/// Runs each survivor and each timeout again in a new companion (section
/// 10), see [`retried_status`]. Host load alone can make a test time out,
/// so a single timeout is not proof of detection. The retry stays inside the time budget: a
/// mutant that gets no retry before the deadline keeps its first status and
/// is listed in [`ScheduleOutcome::unretried`].
fn retry_suspects(
    runner: PhaseRunner<'_>,
    facts: &DryRunFacts,
    mut suspects: Vec<Job>,
    mut first_runs: FxHashMap<String, MutantAttempt>,
    outcome: &mut ScheduleOutcome,
) -> Result<(), ScheduleError> {
    retry_order(&mut suspects, runner.config.deadline.is_some(), |job| {
        outcome
            .results
            .get(&job.id)
            .is_some_and(|first| first.status == MutantStatus::Timeout)
    });
    let mut retry_files: Vec<String> = suspects
        .iter()
        .flat_map(|job| &job.tests)
        .filter_map(|test| facts.passing.get(test).map(|(file, _)| file.clone()))
        .collect();
    retry_files.sort();
    retry_files.dedup();
    let firsts: FirstRuns = suspects
        .iter()
        .filter_map(|job| {
            let first = outcome.results.get(&job.id)?;
            Some((
                job.id.clone(),
                FirstRun {
                    status: first.status,
                    duration_ms: first.duration_ms,
                    killing_tests: first.killing_tests.clone(),
                },
            ))
        })
        .collect();
    let wide = WideTime::default();
    let finished = |job: &Job, attempt: &Attempt| {
        wide.add(job, attempt);
        journal_retry(runner.config, &firsts, job, attempt);
    };
    let (retries, workers) = run_phase(
        runner,
        &retry_files,
        suspects,
        PhaseDeadline::retries(runner.config),
        (Vec::new(), &finished, facts),
        |done, total| MutationProgress::Retries { done, total },
    )?;
    outcome.timings.retry_wide_worker = wide.total();
    note_control_failures(outcome, facts, runner.memo);
    for (job, attempt) in retries {
        if attempt.status == MutantStatus::Pending {
            outcome.unretried.push(job.id);
            continue;
        }
        let first = outcome.results.get(&job.id);
        let first_status = first.map_or(MutantStatus::Survived, |first| first.status);
        let status = retry_status(first_status, &job, &attempt);
        let (status, attempt) = match first {
            Some(first) if first_status == MutantStatus::Killed => {
                confirmed_kill(first, status, attempt)
            }
            _ => (status, attempt),
        };
        // A kill reports the run that killed the mutant. Each other status
        // keeps the first run, so a retry never shortens a timeout.
        let duration_ms = if status == MutantStatus::Killed {
            attempt.duration_ms
        } else {
            first.and_then(|first| first.duration_ms)
        };
        let id = job.id.clone();
        let attempts: Vec<MutantAttempt> = first_runs
            .remove(&id)
            .into_iter()
            .chain([attempt_record(&attempt)])
            .collect();
        record(
            workers,
            outcome,
            facts,
            runner.memo,
            job,
            Attempt {
                status,
                duration_ms,
                ..attempt
            },
        );
        if let Some(result) = outcome.results.get_mut(&id) {
            result.retried = true;
            result.attempts = attempts;
        }
    }
    Ok(())
}

/// The dry-run test ids that the `--kill-matrix-for` targets select. `None`
/// outside target mode.
type TargetIds = Option<FxHashSet<String>>;

/// [`run_schedule_with_progress`] with no progress events.
///
/// # Errors
///
/// Returns a [`ScheduleError`] when the run cannot give trustworthy results.
#[cfg(test)]
pub fn run_schedule(
    spawner: &dyn CompanionSpawner,
    plan: &SchedulePlan,
    config: &ScheduleConfig,
) -> Result<ScheduleOutcome, ScheduleError> {
    run_schedule_with_progress(spawner, plan, config, &|_| {})
}

/// Runs the plan: dry run, mutant runs, and retries of survivors and
/// timeouts. `progress` receives the baseline, mutant and retry events.
///
/// # Errors
///
/// Returns a [`ScheduleError`] when the run cannot give trustworthy results.
pub fn run_schedule_with_progress(
    spawner: &dyn CompanionSpawner,
    plan: &SchedulePlan,
    config: &ScheduleConfig,
    progress: MutationProgressSink<'_>,
) -> Result<ScheduleOutcome, ScheduleError> {
    let mut outcome = ScheduleOutcome::empty();
    // In target mode, the runner starts also when no dry run happens. Its
    // list of test files gives the reason of each target.
    let no_dry_run =
        (plan.mutants.is_empty() && plan.unreached.is_empty()) || plan.test_files.is_empty();
    if no_dry_run && config.targets.is_empty() {
        no_coverage_for_all(plan, &mut outcome);
        return Ok(outcome);
    }

    let started = config.clock.now();
    let Primary {
        mut link,
        ready,
        dry_files,
        undiscovered,
    } = start_primary(spawner, plan, config)?;
    let dry_files = reach_select::narrow(config.reach.as_ref(), &ready, dry_files, &mut outcome);
    outcome.issues.extend(undiscovered_issue(&undiscovered));
    outcome.undiscovered_test_files = undiscovered;
    let memo = ControlMemo::default();
    let mut runner = PhaseRunner {
        spawner,
        config,
        caps: ready.capabilities,
        first_dry_run: Duration::ZERO,
        memo: &memo,
        index: None,
        progress,
    };
    outcome.ready = Some(ready.clone());
    outcome.timings.setup = config.clock.now().saturating_duration_since(started);
    let dry_ran: &[String] = if no_dry_run { &[] } else { &dry_files };
    let files = TargetFiles::new(&ready.test_files, dry_ran);
    check_targets(config, runner.caps, files).inspect_err(|_| link.shutdown())?;
    if dry_ran.is_empty() {
        link.shutdown();
        no_coverage_for_all(plan, &mut outcome);
        return Ok(outcome);
    }
    let inputs = (plan, &ready, dry_files.as_slice());
    let phase = joined_dry_phase(spawner, link, inputs, config, (progress, &mut outcome))?;
    runner.first_dry_run = phase.serial;
    let (mut facts, target_ids, links) = read_dry_phase(
        plan,
        config,
        (runner.caps, files),
        &phase.result,
        &mut outcome,
        phase.links,
    )?;
    let index = TestIndex::new(runner.caps, &phase.result);
    runner.index = index.as_ref();
    let context = JobContext {
        dry_files: &dry_files,
        kill_matrix: config.kill_matrix,
        target_full: config.target_full,
        targets: target_ids.as_ref(),
        ready: &ready,
        cache: config.cache.as_deref(),
        journal: config.journal.as_deref(),
        budgeted: config.deadline.is_some(),
    };
    let mutants_started = config.clock.now();
    let (jobs, phase_files, links) =
        checked_jobs(runner, plan, &mut facts, &context, links, &mut outcome)?;
    let attempts = run_first_pass(runner, &facts, (jobs, &phase_files, links), &mut outcome)?;
    outcome.timings.mutants = config
        .clock
        .now()
        .saturating_duration_since(mutants_started);
    note_control_failures(&mut outcome, &facts, &memo);
    let (suspects, first_runs) = record_first_runs(config, &mut outcome, &facts, &memo, attempts);
    if !suspects.is_empty() {
        let retry_started = config.clock.now();
        retry_suspects(runner, &facts, suspects, first_runs, &mut outcome)?;
        outcome.timings.retry = config.clock.now().saturating_duration_since(retry_started);
    }
    finish_outcome(&mut outcome, &memo);
    Ok(outcome)
}

/// Sorts the lists of a finished schedule and adds the worker-mode counts.
fn finish_outcome(outcome: &mut ScheduleOutcome, memo: &ControlMemo) {
    outcome
        .issues
        .sort_by(|left, right| left.mutant_id.cmp(&right.mutant_id));
    for failure in &mut outcome.baseline_failures {
        failure.mutant_ids.sort();
        failure.mutant_ids.dedup();
    }
    outcome.baseline_failures.sort();
    outcome.unretried.sort();
    outcome.worker_modes = memo.modes.counts();
}

mod reserve;
#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "tests unwrap to keep assertions short")]
mod tests;
