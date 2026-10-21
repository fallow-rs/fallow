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
    Capabilities, DryRunResult, InitRequest, LinkFault, Ready, ReloadMethod, Request, Response,
    RunRequest, RunResult, RunStatus, TestStatus,
};
use super::transport::{CompanionLink, CompanionSpawner, SpawnFailure};
use super::{MutationProgress, MutationProgressSink};

mod control;

use control::{
    CONFIRMATION_RUN_FAILURE, CONTROL_RUN_FAILURE, ControlCheck, ControlMemo, ControlOutcome,
    ControlVerdict, control_fault, first_line, sorted_tests,
};

/// The extra wait of the watchdog after `timeout_ms` (section 8).
const WATCHDOG_GRACE_MS: u64 = 2000;
/// The fixed part of `timeout_ms` (section 8, the Stryker default).
const TIMEOUT_BASE_MS: u64 = 5000;
/// The factor on the covering-test duration (section 8, the Stryker default).
const TIMEOUT_FACTOR: f64 = 1.5;
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
    /// Each mutant id of the plan. A hit outside this set stops the run.
    pub known_ids: FxHashSet<String>,
    /// The static test files of the plan.
    pub test_files: Vec<String>,
    /// The static test files of each source file.
    pub file_test_files: BTreeMap<String, Vec<String>>,
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
    /// The timeout of `init` and the first `dry_run`. It is also the minimum
    /// dry-run timeout of a respawned companion.
    pub runner_timeout: Duration,
    /// The end of the time budget.
    pub deadline: Option<Instant>,
    /// The result cache. `None` runs each covered mutant.
    pub cache: Option<Arc<dyn ResultLookup>>,
}

/// A result of an earlier run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedResult {
    pub status: MutantStatus,
    pub killed_by: Option<String>,
    pub duration_ms: Option<u64>,
}

/// Looks up the result of a mutant from an earlier run.
pub trait ResultLookup: std::fmt::Debug + Send + Sync {
    /// The cache key of `mutant` with these covering tests and their test
    /// files, for the runner that `ready` describes.
    fn key(
        &self,
        ready: &Ready,
        mutant: &ScheduledMutant,
        tests: &[String],
        test_files: &[String],
    ) -> String;

    /// The stored result of `mutant_id` under `key`.
    fn get(&self, mutant_id: &str, key: &str) -> Option<CachedResult>;
}

/// The result of one mutant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MutantOutcome {
    pub status: MutantStatus,
    pub killed_by: Option<String>,
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
}

impl MutantOutcome {
    fn no_coverage() -> Self {
        Self {
            status: MutantStatus::NoCoverage,
            killed_by: None,
            covering_tests: Vec::new(),
            static_mutant: false,
            duration_ms: None,
            retried: false,
            attempts: Vec::new(),
            cached: false,
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

/// The message of a covering test that did not pass in the dry run of a
/// restarted companion.
const RESTARTED_DRY_RUN_FAILURE: &str =
    "the test did not pass in the dry run of a restarted companion";

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
}

/// The result of a schedule.
#[derive(Debug, Clone)]
pub struct ScheduleOutcome {
    pub ready: Option<Ready>,
    pub results: FxHashMap<String, MutantOutcome>,
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
    /// The cache key of each mutant that ran. Empty without a cache.
    pub cache_keys: FxHashMap<String, String>,
    /// The pending mutants that took the place of an uncovered planned
    /// mutant inside the `--max-mutants` limit, in rank order.
    pub promoted: Vec<String>,
}

impl ScheduleOutcome {
    /// An outcome with no result.
    fn empty() -> Self {
        Self {
            ready: None,
            results: FxHashMap::default(),
            baseline_failures: Vec::new(),
            unloaded_paths: Vec::new(),
            undiscovered_test_files: Vec::new(),
            issues: Vec::new(),
            unretried: Vec::new(),
            timings: ScheduleTimings::default(),
            workers: 0,
            cache_keys: FxHashMap::default(),
            promoted: Vec::new(),
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
        }
    }
}

/// One mutant to run.
#[derive(Debug, Clone)]
struct Job {
    id: String,
    /// The covering tests, fastest first.
    tests: Vec<String>,
    timeout_ms: u64,
    /// `1.5 *` the setup time of the covering test files, rounded up. Only
    /// the watchdog waits for it (section 8).
    setup_ms: u64,
    static_mutant: bool,
    /// The sorted test files of `tests`.
    test_files: Vec<String>,
    /// The result cache key. `None` without a cache.
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
        self.setup_ms = self.setup_ms.saturating_mul(2);
        self
    }

    /// The job for the retry of a survivor: each test of the confirmation
    /// set, when the job has one. The test files and the cache key stay the
    /// same, because the set has only tests of the same files.
    fn for_confirmation(mut self) -> Self {
        if let Some((tests, timeout_ms)) = self.confirmation.take() {
            self.first_tests = std::mem::replace(&mut self.tests, tests);
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
    duration_ms: Option<u64>,
    /// How the run detected the mutant, when the runner states it.
    failure_kind: Option<MutantFailureKind>,
    issue: Option<ScheduleIssue>,
    /// The covering tests that the companion left out, because they did not
    /// pass in its dry run or in a control run.
    dropped_tests: Vec<String>,
    /// A failed control run decided the status, so a retry in a new
    /// companion cannot change it.
    settled: bool,
}

impl Attempt {
    fn runtime_error(job: &Job, message: String, stderr_tail: Option<String>) -> Self {
        Self {
            status: MutantStatus::RuntimeError,
            killed_by: None,
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
            duration_ms,
            failure_kind,
            issue: None,
            dropped_tests: Vec::new(),
            settled: false,
        }
    }
}

/// `1.5 * duration`, rounded up. A duration that is not finite gives 0.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the duration sum is finite and non-negative"
)]
fn scaled_ms(duration_ms: f64) -> u64 {
    let scaled = (duration_ms.max(0.0) * TIMEOUT_FACTOR).ceil();
    if scaled.is_finite() { scaled as u64 } else { 0 }
}

/// `timeout_ms = 1.5 * covering-test duration + 5000`, rounded up.
fn timeout_ms(total_duration_ms: f64) -> u64 {
    scaled_ms(total_duration_ms) + TIMEOUT_BASE_MS
}

/// Maps a `run_result` to a status with the fail-closed rules 5 and 6.
/// `tests` are the test ids of the request: the covering tests of `job`
/// without the tests that a restarted companion dropped.
///
/// An `Err` is a runtime error with its reason.
fn classify(
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
fn kill_kind(
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
fn attempt_record(attempt: &Attempt) -> MutantAttempt {
    let detail = (attempt.status == MutantStatus::RuntimeError)
        .then_some(attempt.issue.as_ref())
        .flatten()
        .map(|issue| first_line(&issue.message).to_owned());
    MutantAttempt {
        status: attempt.status,
        duration_ms: attempt.duration_ms,
        killed_by: attempt.killed_by.clone(),
        failure_kind: attempt.failure_kind,
        detail,
    }
}

/// True for a kill with no executed test: an error outside each test is the
/// only evidence. A mutant that throws at module load gives this result, and
/// so does a run path that executes no test at all.
fn is_load_kill(result: &RunResult, status: MutantStatus, killed_by: Option<&String>) -> bool {
    status == MutantStatus::Killed && killed_by.is_none() && result.tests_executed == 0
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "durations are finite and non-negative milliseconds"
)]
fn round_ms(value: Option<f64>) -> Option<u64> {
    value
        .filter(|value| value.is_finite() && *value >= 0.0)
        .map(|value| value.round() as u64)
}

/// A companion that has sent `ready` and a dry run.
struct Worker<'a> {
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
            spawner,
            config,
            dry_files,
            dry_run_timeout,
            link: None,
            known_tests: FxHashSet::default(),
            loaded_files: None,
            restarts_on_reload: false,
            memo,
        }
    }

    fn reset(&mut self) {
        if let Some(mut link) = self.link.take() {
            link.shutdown();
        }
        self.known_tests.clear();
        self.loaded_files = None;
    }

    /// Gives a companion whose dry run covers `scope`, or each file of the
    /// phase for `None`. A companion with a narrower dry run is replaced:
    /// the protocol has no second dry run.
    fn ensure(
        &mut self,
        scope: Option<&[String]>,
    ) -> Result<&mut Box<dyn CompanionLink>, EnsureError> {
        if let Some(loaded) = &self.loaded_files
            && self.link.is_some()
            && scope.is_none_or(|scope| scope.iter().any(|file| !loaded.contains(file)))
        {
            self.reset();
        }
        if self.link.is_none() {
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
            let fault = |stage: &str, fault: &LinkFault| {
                EnsureError::Fault(
                    format!(
                        "the companion {stage} failed: {fault}{}",
                        setup_timeout_hint(fault)
                    ),
                    fault.stderr_tail().map(str::to_owned),
                )
            };
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
                }
                Ok(_) => return Err(EnsureError::Fault("init gave no ready".to_owned(), None)),
                Err(error) => return Err(fault("init", &error)),
            }
            let request = Request::DryRun {
                test_files: scope.unwrap_or(self.dry_files).to_vec(),
            };
            match link.request(&request, self.dry_run_timeout) {
                Ok(Response::DryRun(result)) => {
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
                Err(error) => return Err(fault("dry run", &error)),
            }
            self.link = Some(link);
            self.loaded_files = scope.map(<[String]>::to_vec);
        }
        self.link
            .as_mut()
            .ok_or_else(|| EnsureError::Fault("the companion is gone".to_owned(), None))
    }

    /// Runs `job` one time. `Err` stops the run.
    fn attempt(&mut self, job: &Job, caps: Capabilities) -> Result<Attempt, ScheduleError> {
        let needs_reload = !caps.hot_switch || (job.static_mutant && !caps.fresh_modules_per_run);
        if needs_reload && self.link.is_some() {
            self.reload();
        }
        match self.ensure(job.dry_scope(caps, self.restarts_on_reload)) {
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
            if dropped_tests.iter().all(|test| self.memo.is_failed(test)) {
                return Ok(Attempt::uncovered(dropped_tests));
            }
            let mut attempt =
                Attempt::runtime_error(job, no_covering_test_message(&dropped_tests), None);
            attempt.dropped_tests = dropped_tests;
            return Ok(attempt);
        }
        let request = Request::Run(RunRequest {
            mutant_id: Some(job.id.clone()),
            test_ids: tests.clone(),
            timeout_ms: job.timeout_ms,
            bail: true,
        });
        let watchdog = Duration::from_millis(job.watchdog_ms());
        let Some(link) = self.link.as_mut() else {
            return Ok(Attempt::runtime_error(
                job,
                "the companion is gone".to_owned(),
                None,
            ));
        };
        let mut load_kill = false;
        let mut attempt = match link.request(&request, watchdog) {
            Ok(Response::Run(result)) => match classify(job, &tests, &result) {
                Ok((status, killed_by)) => {
                    load_kill = is_load_kill(&result, status, killed_by.as_ref());
                    let kind = kill_kind(&result, status, killed_by.as_deref());
                    let decided = Attempt::decided(status, killed_by, round_ms(result.duration_ms));
                    Attempt {
                        failure_kind: kind.or(decided.failure_kind),
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
        if load_kill {
            return self.confirm_load_kill(job, &tests, caps, attempt);
        }
        if attempt.status == MutantStatus::Killed && job.is_new_killer(attempt.killed_by.as_ref()) {
            return self.confirm_new_killer(job, &tests, caps, attempt);
        }
        Ok(attempt)
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
        if self
            .config
            .deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
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
        match self.ensure(job.dry_scope(caps, self.restarts_on_reload)) {
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
            bail: false,
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
            ControlOutcome::Passed => {}
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

/// The facts of the dry run that the scheduler uses.
struct DryRunFacts {
    /// Each passing test that is not in an excluded file, with its duration.
    passing: BTreeMap<String, (String, f64)>,
    covering: FxHashMap<String, BTreeSet<String>>,
    static_ids: FxHashSet<String>,
    baseline: Vec<BaselineFailure>,
    /// The setup time of each test file outside its tests.
    file_setup: BTreeMap<String, f64>,
}

impl DryRunFacts {
    /// The setup time of the distinct test files of `tests`.
    fn setup_of(&self, tests: &[String]) -> f64 {
        let files: BTreeSet<&str> = tests
            .iter()
            .filter_map(|test| self.passing.get(test).map(|(file, _)| file.as_str()))
            .collect();
        files
            .into_iter()
            .filter_map(|file| self.file_setup.get(file))
            .sum()
    }
}

fn read_dry_run(
    plan: &SchedulePlan,
    caps: Capabilities,
    result: &DryRunResult,
) -> Result<DryRunFacts, ScheduleError> {
    let reported = result
        .hits
        .values()
        .flatten()
        .chain(&result.static_hits)
        .chain(
            result
                .file_hits
                .iter()
                .flat_map(|hits| hits.values().flatten()),
        );
    for id in reported {
        if !plan.known_ids.contains(id) {
            return Err(ScheduleError::UnknownHit(id.clone()));
        }
    }
    let mut baseline = Vec::new();
    let mut excluded_files = FxHashSet::default();
    let mut global_errors = Vec::new();
    for error in &result.unhandled_errors {
        match &error.file {
            Some(file) => {
                excluded_files.insert(file.clone());
                baseline.push(BaselineFailure {
                    file: Some(file.clone()),
                    test_id: None,
                    message: Some(error.message.clone()),
                    mutant_ids: Vec::new(),
                });
            }
            None => global_errors.push(error.message.clone()),
        }
    }
    if !global_errors.is_empty() {
        return Err(ScheduleError::Baseline(global_errors));
    }
    let mut passing = BTreeMap::new();
    for test in &result.tests {
        match test.status {
            TestStatus::Fail => baseline.push(BaselineFailure {
                file: Some(test.file.clone()),
                test_id: Some(test.test_id.clone()),
                message: None,
                mutant_ids: Vec::new(),
            }),
            TestStatus::Pass if !excluded_files.contains(&test.file) => {
                passing.insert(
                    test.test_id.clone(),
                    (test.file.clone(), test.duration_ms.unwrap_or(0.0)),
                );
            }
            _ => {}
        }
    }
    baseline.sort();
    let mut covering: FxHashMap<String, BTreeSet<String>> = FxHashMap::default();
    if caps.per_test_attribution {
        for (test, ids) in &result.hits {
            if !passing.contains_key(test) {
                continue;
            }
            for id in ids {
                covering.entry(id.clone()).or_default().insert(test.clone());
            }
        }
    } else if let Some(file_hits) = &result.file_hits {
        for (file, ids) in file_hits {
            let tests: Vec<&String> = passing
                .iter()
                .filter(|(_, (test_file, _))| test_file == file)
                .map(|(test, _)| test)
                .collect();
            for id in ids {
                covering
                    .entry(id.clone())
                    .or_default()
                    .extend(tests.iter().map(|test| (*test).clone()));
            }
        }
    }
    let static_ids = result.static_hits.iter().cloned().collect();
    Ok(DryRunFacts {
        passing,
        covering,
        static_ids,
        baseline,
        file_setup: result.file_setup_ms.clone(),
    })
}

/// The covering tests of `mutant` from the dry run. A static mutant runs
/// each passing test of the dry-run files that load its module, and also the
/// tests that hit it at run time (for example through a dynamic `import()`).
/// A static map miss has no static test file, so it runs each passing
/// dry-run test.
fn covering_tests(
    plan: &SchedulePlan,
    facts: &DryRunFacts,
    dry_files: &[String],
    mutant: &ScheduledMutant,
    static_mutant: bool,
    map_miss: bool,
) -> BTreeSet<String> {
    if !static_mutant {
        return facts.covering.get(&mutant.id).cloned().unwrap_or_default();
    }
    let files: FxHashSet<&String> = plan
        .file_test_files
        .get(&mutant.file)
        .into_iter()
        .flatten()
        .filter(|file| dry_files.contains(file))
        .collect();
    let mut tests: BTreeSet<String> = facts
        .passing
        .iter()
        .filter(|(_, (file, _))| map_miss || files.contains(file))
        .map(|(test, _)| test.clone())
        .collect();
    if let Some(runtime) = facts.covering.get(&mutant.id) {
        tests.extend(runtime.iter().cloned());
    }
    tests
}

/// Widens `tests` to each passing test of their files. Without
/// `test_filter` the runner runs whole test files (`protocol.md` section 5).
fn whole_files(facts: &DryRunFacts, tests: &BTreeSet<String>) -> BTreeSet<String> {
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
fn fastest_first(facts: &DryRunFacts, tests: BTreeSet<String>) -> (Vec<String>, f64) {
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
/// `None` when the files have no other test.
fn confirmation_tests(facts: &DryRunFacts, tests: &BTreeSet<String>) -> Option<(Vec<String>, u64)> {
    let widened = whole_files(facts, tests);
    (widened.len() > tests.len()).then(|| {
        let (widened, total) = fastest_first(facts, widened);
        (widened, timeout_ms(total))
    })
}

/// The facts of the first companion that the jobs read.
struct JobContext<'a> {
    dry_files: &'a [String],
    ready: &'a Ready,
    cache: Option<&'a dyn ResultLookup>,
}

/// The issue of a mutant that the dry run hit, while the static map reaches
/// no test of its file.
fn static_map_miss_issue(mutant: &ScheduledMutant) -> ScheduleIssue {
    ScheduleIssue {
        code: "static-map-miss",
        message: format!(
            "the dry run hit this mutant in `{}`, but the static map reaches no test of that file; the module graph misses a test that loads it",
            mutant.file
        ),
        mutant_id: Some(mutant.id.clone()),
        stderr_tail: None,
    }
}

/// The stored result of `mutant` under `key`, with its covering `tests`.
fn cached_outcome(
    cache: Option<&dyn ResultLookup>,
    key: Option<&String>,
    mutant: &ScheduledMutant,
    tests: &[String],
    static_mutant: bool,
) -> Option<MutantOutcome> {
    let hit = cache?.get(&mutant.id, key?)?;
    let mut covering_tests = tests.to_vec();
    covering_tests.sort_unstable();
    Some(MutantOutcome {
        status: hit.status,
        killed_by: hit.killed_by,
        covering_tests,
        static_mutant,
        duration_ms: hit.duration_ms,
        // The cache stores a survivor only after its retry.
        retried: hit.status == MutantStatus::Survived,
        attempts: Vec::new(),
        cached: true,
    })
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
/// covered pending mutant in rank order.
fn build_jobs(
    plan: &SchedulePlan,
    facts: &DryRunFacts,
    context: &JobContext<'_>,
    outcome: &mut ScheduleOutcome,
) -> Vec<Job> {
    let JobContext {
        dry_files,
        ready,
        cache,
    } = *context;
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
        if tests.is_empty() {
            outcome.results.insert(
                mutant.id.clone(),
                MutantOutcome {
                    static_mutant,
                    ..MutantOutcome::no_coverage()
                },
            );
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
        let confirmation = (!static_mutant && caps.test_filter)
            .then(|| confirmation_tests(facts, &tests))
            .flatten();
        let (tests, total) = fastest_first(facts, tests);
        let setup_ms = scaled_ms(facts.setup_of(&tests));
        let mut test_files: Vec<String> = tests
            .iter()
            .filter_map(|test| facts.passing.get(test).map(|(file, _)| file.clone()))
            .collect();
        test_files.sort_unstable();
        test_files.dedup();
        let cache_key = cache.map(|cache| cache.key(ready, mutant, &tests, &test_files));
        if let Some(cached) =
            cached_outcome(cache, cache_key.as_ref(), mutant, &tests, static_mutant)
        {
            outcome.results.insert(mutant.id.clone(), cached);
            continue;
        }
        let job = Job {
            id: mutant.id.clone(),
            tests,
            timeout_ms: timeout_ms(total),
            setup_ms,
            static_mutant,
            test_files,
            cache_key,
            confirmation,
            first_tests: Vec::new(),
        };
        if static_mutant {
            statics.push(job);
        } else {
            regular.push(job);
        }
    }
    regular.extend(statics);
    regular
}

/// Shared state of the worker threads of one phase.
struct Phase<'a> {
    jobs: Mutex<VecDeque<Job>>,
    attempts: Mutex<Vec<(Job, Attempt)>>,
    fatal: Mutex<Option<ScheduleError>>,
    stop: AtomicBool,
    deadline: Option<Instant>,
    caps: Capabilities,
    spawner: &'a dyn CompanionSpawner,
    config: &'a ScheduleConfig,
    dry_files: &'a [String],
    dry_run_timeout: Duration,
    memo: &'a ControlMemo,
    /// Receives the count of finished jobs after each job.
    report: &'a (dyn Fn(usize) + Sync),
}

impl Phase<'_> {
    /// Adds a finished job and reports the new count of finished jobs.
    fn push(&self, job: Job, attempt: Attempt) {
        let done = self.attempts.lock().ok().map(|mut attempts| {
            attempts.push((job, attempt));
            attempts.len()
        });
        if let Some(done) = done {
            (self.report)(done);
        }
    }

    fn work(&self, primary: Option<Box<dyn CompanionLink>>, known: FxHashSet<String>) {
        let mut worker = Worker::new(
            self.spawner,
            self.config,
            self.dry_files,
            self.dry_run_timeout,
            self.memo,
        );
        if let Some(link) = primary {
            worker.link = Some(link);
            worker.known_tests = known;
        }
        loop {
            if self.stop.load(Ordering::SeqCst) {
                break;
            }
            let Some(job) = self.jobs.lock().ok().and_then(|mut jobs| jobs.pop_front()) else {
                break;
            };
            if self
                .deadline
                .is_some_and(|deadline| Instant::now() >= deadline)
            {
                self.push(job, Attempt::decided(MutantStatus::Pending, None, None));
                continue;
            }
            match worker.run_with_retry(&job, self.caps) {
                Ok(attempt) => self.push(job, attempt),
                Err(error) => {
                    self.stop.store(true, Ordering::SeqCst);
                    if let Ok(mut fatal) = self.fatal.lock() {
                        fatal.get_or_insert(error);
                    }
                    break;
                }
            }
        }
        worker.reset();
    }
}

/// The companion source and runner facts that each phase shares.
#[derive(Clone, Copy)]
struct PhaseRunner<'a> {
    spawner: &'a dyn CompanionSpawner,
    config: &'a ScheduleConfig,
    caps: Capabilities,
    /// The wall time of the first dry run.
    first_dry_run: Duration,
    /// The control results of the run.
    memo: &'a ControlMemo,
    /// Receives the phase events of the run.
    progress: MutationProgressSink<'a>,
}

/// A companion that already did its dry run, and the tests that passed in it.
type WarmLink = (Box<dyn CompanionLink>, FxHashSet<String>);

/// Runs `jobs` on up to `concurrency` workers. The first worker takes
/// `primary`, a companion that already did its dry run. `event` makes the
/// progress event of the phase from the finished and the total jobs.
fn run_phase(
    runner: PhaseRunner<'_>,
    dry_files: &[String],
    jobs: Vec<Job>,
    deadline: Option<Instant>,
    primary: Option<WarmLink>,
    event: fn(usize, usize) -> MutationProgress,
) -> Result<(Vec<(Job, Attempt)>, usize), ScheduleError> {
    let workers = runner.config.concurrency.max(1).min(jobs.len().max(1));
    let total = jobs.len();
    let report = |done: usize| (runner.progress)(event(done, total));
    report(0);
    let phase = Phase {
        jobs: Mutex::new(jobs.into()),
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
        report: &report,
    };
    let mut primary = primary;
    std::thread::scope(|scope| {
        for _ in 0..workers {
            let (link, known) = primary
                .take()
                .map_or((None, FxHashSet::default()), |(link, known)| {
                    (Some(link), known)
                });
            let phase = &phase;
            scope.spawn(move || phase.work(link, known));
        }
    });
    if let Some((mut link, _)) = primary {
        link.shutdown();
    }
    if let Some(error) = phase.fatal.into_inner().ok().flatten() {
        return Err(error);
    }
    Ok((phase.attempts.into_inner().unwrap_or_default(), workers))
}

fn record(outcome: &mut ScheduleOutcome, facts: &DryRunFacts, job: Job, attempt: Attempt) {
    if let Some(issue) = attempt.issue {
        outcome.issues.push(issue);
    }
    // The key covers each covering test. A survivor of a run without a
    // dropped test is not proof against that test, so it is not stored.
    let partial_survivor =
        attempt.status == MutantStatus::Survived && !attempt.dropped_tests.is_empty();
    for test in attempt.dropped_tests {
        note_restarted_baseline_failure(outcome, facts, test, &job.id);
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
            killed_by: attempt.killed_by,
            covering_tests,
            static_mutant: job.static_mutant,
            duration_ms: attempt.duration_ms,
            retried: false,
            attempts: Vec::new(),
            cached: false,
        },
    );
}

/// Records `test` as a baseline failure of a restarted companion that
/// affected `mutant_id`. The mutant ran with its other covering tests.
fn note_restarted_baseline_failure(
    outcome: &mut ScheduleOutcome,
    facts: &DryRunFacts,
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
        test_id: Some(test),
        message: Some(RESTARTED_DRY_RUN_FAILURE.to_owned()),
        mutant_ids: vec![mutant_id.to_owned()],
    });
}

/// The hint for a setup watchdog fault of a restarted companion. The run
/// continues, so the hint goes into the mutant error.
fn setup_timeout_hint(fault: &LinkFault) -> &'static str {
    if matches!(fault, LinkFault::Timeout) {
        "; on a slow or busy host, pass --setup-timeout with a longer duration"
    } else {
        ""
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

/// The longest test name that [`no_covering_test_message`] shows.
const MAX_TEST_NAME_CHARS: usize = 60;
const ELLIPSIS: &str = "...";

/// The `runtime_error` message for a mutant with no covering test left after
/// the dry run of a respawned companion. It gives the count and the first
/// test only: `baseline_failures` holds the full list. The message leaves out
/// the mutant id because the issue carries it.
fn no_covering_test_message(dropped: &[String]) -> String {
    let first = dropped.first().map_or_else(String::new, |name| {
        if name.chars().count() <= MAX_TEST_NAME_CHARS {
            return name.clone();
        }
        let kept: String = name
            .chars()
            .take(MAX_TEST_NAME_CHARS - ELLIPSIS.len())
            .collect();
        format!("{kept}{ELLIPSIS}")
    });
    let tests = if dropped.len() == 1 {
        format!("1 test: {first}")
    } else {
        format!("{} tests, first: {first}", dropped.len())
    };
    format!("no covering test passed in the dry run of the restarted companion ({tests})")
}

/// The issue for static test files that the runner does not list. Their
/// tests never run, so a mutant that only they reach stays `no_coverage`.
fn undiscovered_issue(files: &[String]) -> Option<ScheduleIssue> {
    (!files.is_empty()).then(|| ScheduleIssue {
        code: "test-files-not-discovered",
        message: {
            let (noun, pronoun) = if files.len() == 1 {
                ("file", "it")
            } else {
                ("files", "they")
            };
            format!(
                "the runner does not list {} static test {noun}, so {pronoun} did not run: {}",
                files.len(),
                files.join(", ")
            )
        },
        mutant_id: None,
        stderr_tail: None,
    })
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
    for mutant in &plan.pending {
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

/// Gives the final status from the first and the retry status. A retry that
/// changes whether the tests detect the mutant gives `flaky`. A timeout with
/// a killed retry stays detected and is `killed`. A survivor with a timed-out
/// retry stays `survived`: the first run finished all tests with the mutant
/// active, and host load alone can make the retry time out.
const fn retried_status(first: MutantStatus, retry: MutantStatus) -> MutantStatus {
    match (first, retry) {
        (_, MutantStatus::RuntimeError) => MutantStatus::RuntimeError,
        (_, MutantStatus::NoCoverage) => MutantStatus::NoCoverage,
        (MutantStatus::Survived, MutantStatus::Survived | MutantStatus::Timeout) => {
            MutantStatus::Survived
        }
        (MutantStatus::Timeout, MutantStatus::Timeout) => MutantStatus::Timeout,
        (MutantStatus::Timeout, MutantStatus::Killed) => MutantStatus::Killed,
        _ => MutantStatus::Flaky,
    }
}

/// Records the first run of each mutant. Gives the survivors and the
/// timeouts to run again, and the evidence of their first run.
fn record_first_runs(
    outcome: &mut ScheduleOutcome,
    facts: &DryRunFacts,
    attempts: Vec<(Job, Attempt)>,
) -> (Vec<Job>, FxHashMap<String, MutantAttempt>) {
    let mut suspects = Vec::new();
    let mut first_runs = FxHashMap::default();
    for (job, attempt) in attempts {
        let suspect = match attempt.status {
            MutantStatus::Survived => Some(job.clone().for_confirmation()),
            MutantStatus::Timeout => Some(job.clone().with_doubled_timeout()),
            _ => None,
        };
        if let Some(suspect) = suspect {
            first_runs.insert(job.id.clone(), attempt_record(&attempt));
            suspects.push(suspect);
        }
        record(outcome, facts, job, attempt);
    }
    (suspects, first_runs)
}

/// Runs each survivor and each timeout again in a new companion (section
/// 10), see [`retried_status`]. Host load alone can make a test time out,
/// so a single timeout is not proof of detection. The retry stays inside the time budget: a
/// mutant that gets no retry before the deadline keeps its first status and
/// is listed in [`ScheduleOutcome::unretried`].
fn retry_suspects(
    runner: PhaseRunner<'_>,
    facts: &DryRunFacts,
    suspects: Vec<Job>,
    mut first_runs: FxHashMap<String, MutantAttempt>,
    outcome: &mut ScheduleOutcome,
) -> Result<(), ScheduleError> {
    let mut retry_files: Vec<String> = suspects
        .iter()
        .flat_map(|job| &job.tests)
        .filter_map(|test| facts.passing.get(test).map(|(file, _)| file.clone()))
        .collect();
    retry_files.sort();
    retry_files.dedup();
    let (retries, _) = run_phase(
        runner,
        &retry_files,
        suspects,
        runner.config.deadline,
        None,
        |done, total| MutationProgress::Retries { done, total },
    )?;
    note_control_failures(outcome, facts, runner.memo);
    for (job, attempt) in retries {
        if attempt.status == MutantStatus::Pending {
            outcome.unretried.push(job.id);
            continue;
        }
        let first = outcome.results.get(&job.id);
        let first_status = first.map_or(MutantStatus::Survived, |first| first.status);
        // A kill by a test that the first run did not execute is no
        // disagreement with the first run. The worker already confirmed it
        // with a control run.
        let status = if first_status == MutantStatus::Survived
            && attempt.status == MutantStatus::Killed
            && job.is_new_killer(attempt.killed_by.as_ref())
        {
            MutantStatus::Killed
        } else {
            retried_status(first_status, attempt.status)
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
            outcome,
            facts,
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

/// Sends the dry run on the first companion. A failure shuts it down.
fn first_dry_run(
    link: &mut Box<dyn CompanionLink>,
    dry_files: &[String],
    timeout: Duration,
) -> Result<DryRunResult, ScheduleError> {
    let request = Request::DryRun {
        test_files: dry_files.to_vec(),
    };
    let Response::DryRun(dry) = setup_request(link, &request, timeout)? else {
        link.shutdown();
        return Err(setup_error(
            "dry_run",
            &LinkFault::Protocol("no dry_run_result".to_owned()),
        ));
    };
    Ok(dry)
}

/// Makes each test of `failed` a baseline failure that covers no mutant, the
/// same as a test that fails in the dry run.
fn drop_failed_tests(facts: &mut DryRunFacts, failed: &[String]) {
    for test in failed {
        let file = facts.passing.remove(test).map(|(file, _)| file);
        for tests in facts.covering.values_mut() {
            tests.remove(test);
        }
        facts.baseline.push(BaselineFailure {
            file,
            test_id: Some(test.clone()),
            message: Some(CONTROL_RUN_FAILURE.to_owned()),
            mutant_ids: Vec::new(),
        });
    }
    facts.covering.retain(|_, tests| !tests.is_empty());
    facts.baseline.sort();
}

/// Adds each test that failed in a control run during the mutant runs as a
/// baseline failure. [`record`] then adds the mutants that ran without it.
fn note_control_failures(outcome: &mut ScheduleOutcome, facts: &DryRunFacts, memo: &ControlMemo) {
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
            message: Some(message.to_owned()),
            mutant_ids: Vec::new(),
        });
    }
}

/// The issue tracker that the self-check error names.
const ISSUE_TRACKER: &str = "https://github.com/fallow-rs/fallow/issues";

/// The self-check of the run path. Before the first mutant job, the tests of
/// the first job run with no active mutant, through the same request as a
/// mutant run. The dry run selects whole test files, so it does not use this
/// path. When a runner change breaks the path, each mutant run fails with an
/// error and no executed test, and each mutant looks killed.
///
/// A control run that executes no test, or that gives a result with no
/// failed test that is still not a pass, stops the run. A test that ran and
/// failed is a fact about the tests, for example a test that depends on
/// another test of its file. The check gives these tests back, and the run
/// continues without them. The check needs no run when no job exists (no
/// covered mutant, or each result comes from the cache) or when the time
/// budget already ended.
fn self_check(
    runner: PhaseRunner<'_>,
    dry_files: &[String],
    ready: &Ready,
    jobs: &[Job],
    link: Box<dyn CompanionLink>,
    known: &FxHashSet<String>,
) -> Result<(Box<dyn CompanionLink>, Vec<String>), ScheduleError> {
    let expired = runner
        .config
        .deadline
        .is_some_and(|deadline| Instant::now() >= deadline);
    let Some(job) = jobs.first().filter(|_| !expired) else {
        return Ok((link, Vec::new()));
    };
    let mut worker = Worker::new(
        runner.spawner,
        runner.config,
        dry_files,
        runner.config.runner_timeout,
        runner.memo,
    );
    worker.link = Some(link);
    worker.known_tests.clone_from(known);
    let outcome = worker
        .control_run(&job.tests, job, runner.caps)
        .map(|(outcome, _)| outcome);
    let runner_name = format!("{} {}", runner.config.runner.package, ready.runner_version);
    let report =
        format!("report this at {ISSUE_TRACKER} with the runner name and version ({runner_name})");
    let stopped = ScheduleError::RunPath {
        message: format!("the companion of {runner_name} stopped after the self-check"),
        help: report.clone(),
    };
    let error = match outcome {
        Err(error) => error,
        Ok(ControlOutcome::Passed) => {
            runner.memo.store(&job.tests, ControlVerdict::Passed);
            if let Some(passed) = worker.link.take() {
                return Ok((passed, Vec::new()));
            }
            stopped
        }
        Ok(ControlOutcome::Failed(fault)) if !fault.failed_tests.is_empty() => {
            runner.memo.store(&job.tests, ControlVerdict::Failed);
            if let Some(kept) = worker.link.take() {
                return Ok((kept, fault.failed_tests));
            }
            stopped
        }
        Ok(ControlOutcome::Failed(fault)) if fault.no_tests => ScheduleError::RunPath {
            message: format!(
                "{runner_name} executes no tests in a mutant run: a control run of the tests of a covered mutant, with no active mutant, gave {}; each mutant would look killed",
                fault.detail
            ),
            help: report,
        },
        Ok(ControlOutcome::Failed(fault)) => ScheduleError::RunPath {
            message: format!(
                "{runner_name} gives no trustworthy result in a mutant run: a control run of the tests of a covered mutant, with no active mutant, gave {}, but no test failed",
                fault.detail
            ),
            help: report,
        },
        Ok(ControlOutcome::TimedOut(detail)) => ScheduleError::RunPath {
            message: format!(
                "the self-check of {runner_name} timed out: a control run of the tests of a covered mutant, with no active mutant, did not finish ({detail})"
            ),
            help: "on a slow or busy host, pass --setup-timeout with a longer duration, for example 5m, or run on a less loaded host".to_owned(),
        },
        Ok(ControlOutcome::Fault(message)) => ScheduleError::RunPath {
            message: format!(
                "the self-check of {runner_name} failed: a control run with no active mutant gave no result: {message}"
            ),
            help: report,
        },
    };
    worker.reset();
    Err(error)
}

/// Builds the jobs and runs the self-check on the first job. A test that
/// fails in the self-check is a baseline failure, the same as a test that
/// fails in the dry run: it covers no mutant, so the jobs are built again
/// without it. Gives the jobs, the first companion and its passing tests.
fn checked_jobs(
    runner: PhaseRunner<'_>,
    plan: &SchedulePlan,
    facts: &mut DryRunFacts,
    context: &JobContext<'_>,
    link: Box<dyn CompanionLink>,
    outcome: &mut ScheduleOutcome,
) -> Result<(Vec<Job>, WarmLink), ScheduleError> {
    let mut first_jobs = ScheduleOutcome::empty();
    let jobs = build_jobs(plan, facts, context, &mut first_jobs);
    let known: FxHashSet<String> = facts.passing.keys().cloned().collect();
    let (link, failed) = self_check(
        runner,
        context.dry_files,
        context.ready,
        &jobs,
        link,
        &known,
    )?;
    let (jobs, known) = if failed.is_empty() {
        outcome.results.extend(first_jobs.results);
        outcome.issues.extend(first_jobs.issues);
        outcome.promoted = first_jobs.promoted;
        (jobs, known)
    } else {
        drop_failed_tests(facts, &failed);
        let jobs = build_jobs(plan, facts, context, outcome);
        (jobs, facts.passing.keys().cloned().collect())
    };
    outcome.baseline_failures.clone_from(&facts.baseline);
    mark_unreached_pending(plan, facts, context.dry_files, outcome);
    Ok((jobs, (link, known)))
}

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
    let no_coverage_for_all = |outcome: &mut ScheduleOutcome| {
        for mutant in plan.mutants.iter().chain(&plan.pending) {
            outcome
                .results
                .insert(mutant.id.clone(), MutantOutcome::no_coverage());
        }
    };
    if (plan.mutants.is_empty() && plan.unreached.is_empty()) || plan.test_files.is_empty() {
        no_coverage_for_all(&mut outcome);
        return Ok(outcome);
    }

    let started = Instant::now();
    let Primary {
        mut link,
        ready,
        dry_files,
        undiscovered,
    } = start_primary(spawner, plan, config)?;
    outcome.issues.extend(undiscovered_issue(&undiscovered));
    outcome.undiscovered_test_files = undiscovered;
    let memo = ControlMemo::default();
    let mut runner = PhaseRunner {
        spawner,
        config,
        caps: ready.capabilities,
        first_dry_run: Duration::ZERO,
        memo: &memo,
        progress,
    };
    outcome.ready = Some(ready.clone());
    outcome.timings.setup = started.elapsed();
    if dry_files.is_empty() {
        link.shutdown();
        no_coverage_for_all(&mut outcome);
        return Ok(outcome);
    }

    progress(MutationProgress::Baseline {
        test_files: dry_files.len(),
    });
    let dry_started = Instant::now();
    let dry = first_dry_run(&mut link, &dry_files, config.runner_timeout)?;
    outcome.timings.dry_run = dry_started.elapsed();
    runner.first_dry_run = outcome.timings.dry_run;
    let mut facts = match read_dry_run(plan, runner.caps, &dry) {
        Ok(facts) => facts,
        Err(error) => {
            link.shutdown();
            return Err(error);
        }
    };
    outcome.unloaded_paths = dry.unloaded_paths;
    let context = JobContext {
        dry_files: &dry_files,
        ready: &ready,
        cache: config.cache.as_deref(),
    };
    let mutants_started = Instant::now();
    let (jobs, (link, known)) =
        checked_jobs(runner, plan, &mut facts, &context, link, &mut outcome)?;
    let (attempts, workers) = run_phase(
        runner,
        &dry_files,
        jobs,
        config.deadline,
        Some((link, known)),
        |done, total| MutationProgress::Mutants { done, total },
    )?;
    outcome.workers = workers;
    outcome.timings.mutants = mutants_started.elapsed();
    note_control_failures(&mut outcome, &facts, &memo);
    let (suspects, first_runs) = record_first_runs(&mut outcome, &facts, attempts);
    if !suspects.is_empty() {
        let retry_started = Instant::now();
        retry_suspects(runner, &facts, suspects, first_runs, &mut outcome)?;
        outcome.timings.retry = retry_started.elapsed();
    }
    outcome
        .issues
        .sort_by(|left, right| left.mutant_id.cmp(&right.mutant_id));
    for failure in &mut outcome.baseline_failures {
        failure.mutant_ids.sort();
        failure.mutant_ids.dedup();
    }
    outcome.baseline_failures.sort();
    outcome.unretried.sort();
    Ok(outcome)
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "tests unwrap to keep assertions short")]
mod tests;
