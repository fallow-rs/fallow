//! The mutant scheduler (`protocol.md` sections 4, 8, 9 and 10).
//!
//! The scheduler owns every decision: the dry run, the mutant-to-test map,
//! the test order, bail, timeouts, retries and the final status. It talks to
//! companions only through [`CompanionLink`], so tests drive it with a fake
//! transport.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use fallow_output::MutantStatus;
use rustc_hash::{FxHashMap, FxHashSet};

use super::manifest::RunnerRange;
use super::protocol::{
    Capabilities, DryRunResult, InitRequest, LinkFault, Ready, ReloadMethod, Request, Response,
    RunRequest, RunResult, RunStatus, TestStatus,
};
use super::transport::{CompanionLink, CompanionSpawner, SpawnFailure};

/// The extra wait of the watchdog after `timeout_ms` (section 8).
const WATCHDOG_GRACE_MS: u64 = 2000;
/// The fixed part of `timeout_ms` (section 8, the Stryker default).
const TIMEOUT_BASE_MS: u64 = 5000;
/// The factor on the covering-test duration (section 8, the Stryker default).
const TIMEOUT_FACTOR: f64 = 1.5;

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
    /// The timeout of `init` and `dry_run`.
    pub runner_timeout: Duration,
    /// The end of the time budget.
    pub deadline: Option<Instant>,
}

/// The result of one mutant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MutantOutcome {
    pub status: MutantStatus,
    pub killed_by: Option<String>,
    pub covering_tests: Vec<String>,
    pub static_mutant: bool,
    pub duration_ms: Option<u64>,
}

impl MutantOutcome {
    fn no_coverage() -> Self {
        Self {
            status: MutantStatus::NoCoverage,
            killed_by: None,
            covering_tests: Vec::new(),
            static_mutant: false,
            duration_ms: None,
        }
    }
}

/// A test or test file that failed with no active mutant.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct BaselineFailure {
    pub file: Option<String>,
    pub test_id: Option<String>,
    pub message: Option<String>,
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
}

/// The result of a schedule.
#[derive(Debug, Clone)]
pub struct ScheduleOutcome {
    pub ready: Option<Ready>,
    pub results: FxHashMap<String, MutantOutcome>,
    pub baseline_failures: Vec<BaselineFailure>,
    pub unloaded_paths: Vec<String>,
    pub issues: Vec<ScheduleIssue>,
    /// The survivors that kept `survived` without a retry because the time
    /// budget ended, sorted.
    pub unretried_survivors: Vec<String>,
    pub timings: ScheduleTimings,
    pub workers: usize,
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
    },
    /// The runner version is outside the manifest range.
    RunnerVersion(String),
    /// A hit names a mutant outside the plan (fail-closed rule 9).
    UnknownHit(String),
    /// An error outside each test file failed the dry run.
    Baseline(Vec<String>),
}

impl std::fmt::Display for ScheduleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Integrity(message)
            | Self::RunnerVersion(message)
            | Self::Setup { message, .. } => f.write_str(message),
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
}

impl Job {
    /// The watchdog wait: `timeout_ms`, the file setup time and the grace.
    fn watchdog_ms(&self) -> u64 {
        self.timeout_ms + self.setup_ms + WATCHDOG_GRACE_MS
    }
}

/// The decision of one attempt.
#[derive(Debug, Clone)]
struct Attempt {
    status: MutantStatus,
    killed_by: Option<String>,
    duration_ms: Option<u64>,
    issue: Option<ScheduleIssue>,
}

impl Attempt {
    fn runtime_error(job: &Job, message: String, stderr_tail: Option<String>) -> Self {
        Self {
            status: MutantStatus::RuntimeError,
            killed_by: None,
            duration_ms: None,
            issue: Some(ScheduleIssue {
                code: "runtime-error",
                message,
                mutant_id: Some(job.id.clone()),
                stderr_tail,
            }),
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
///
/// An `Err` is a runtime error with its reason.
fn classify(job: &Job, result: &RunResult) -> Result<(MutantStatus, Option<String>), String> {
    if result.mutant_id != job.id {
        return Err(format!(
            "the run result names mutant `{}`, expected `{}`",
            result.mutant_id, job.id
        ));
    }
    let requested: FxHashSet<&str> = job.tests.iter().map(String::as_str).collect();
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
            if result.tests_executed != job.tests.len() {
                return Err(format!(
                    "{} of {} requested tests ran",
                    result.tests_executed,
                    job.tests.len()
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
    link: Option<Box<dyn CompanionLink>>,
    known_tests: FxHashSet<String>,
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
    ) -> Self {
        Self {
            spawner,
            config,
            dry_files,
            link: None,
            known_tests: FxHashSet::default(),
        }
    }

    fn reset(&mut self) {
        if let Some(mut link) = self.link.take() {
            link.shutdown();
        }
        self.known_tests.clear();
    }

    fn ensure(&mut self) -> Result<&mut Box<dyn CompanionLink>, EnsureError> {
        if self.link.is_none() {
            let mut link = self.spawner.spawn().map_err(|failure| match failure {
                SpawnFailure::Integrity(message) => {
                    EnsureError::Fatal(ScheduleError::Integrity(message))
                }
                SpawnFailure::Fault(fault) => EnsureError::Fault(
                    format!("the companion did not start: {fault}"),
                    fault.stderr_tail().map(str::to_owned),
                ),
            })?;
            let fault = |stage: &str, fault: &LinkFault| {
                EnsureError::Fault(
                    format!("the companion {stage} failed: {fault}"),
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
                test_files: self.dry_files.to_vec(),
            };
            match link.request(&request, self.config.runner_timeout) {
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
        match self.ensure() {
            Ok(_) => {}
            Err(EnsureError::Fatal(error)) => return Err(error),
            Err(EnsureError::Fault(message, tail)) => {
                self.reset();
                return Ok(Attempt::runtime_error(job, message, tail));
            }
        }
        if let Some(missing) = job
            .tests
            .iter()
            .find(|test| !self.known_tests.contains(*test))
        {
            return Ok(Attempt::runtime_error(
                job,
                format!("the test `{missing}` did not pass in the dry run of this companion"),
                None,
            ));
        }
        let request = Request::Run(RunRequest {
            mutant_id: job.id.clone(),
            test_ids: job.tests.clone(),
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
        let attempt = match link.request(&request, watchdog) {
            Ok(Response::Run(result)) => match classify(job, &result) {
                Ok((status, killed_by)) => Attempt {
                    status,
                    killed_by,
                    duration_ms: round_ms(result.duration_ms),
                    issue: None,
                },
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
                Attempt {
                    status: MutantStatus::Timeout,
                    killed_by: None,
                    duration_ms: Some(job.watchdog_ms()),
                    issue: None,
                }
            }
            Err(fault) => {
                let tail = fault.stderr_tail().map(str::to_owned);
                self.reset();
                Attempt::runtime_error(job, fault.to_string(), tail)
            }
        };
        if job.static_mutant && !caps.fresh_modules_per_run && self.link.is_some() {
            self.reload();
        }
        Ok(attempt)
    }

    /// Sends `reload`. A restart request or a fault replaces the companion.
    fn reload(&mut self) {
        let Some(link) = self.link.as_mut() else {
            return;
        };
        match link.request(&Request::Reload, self.config.runner_timeout) {
            Ok(Response::Reloaded(reloaded))
                if reloaded.method != ReloadMethod::RestartRequired => {}
            _ => self.reset(),
        }
    }

    /// Runs `job`, with one retry in a new companion after a runtime error
    /// (section 10).
    fn run_with_retry(&mut self, job: &Job, caps: Capabilities) -> Result<Attempt, ScheduleError> {
        let first = self.attempt(job, caps)?;
        if first.status != MutantStatus::RuntimeError {
            return Ok(first);
        }
        self.reset();
        let second = self.attempt(job, caps)?;
        Ok(second)
    }
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
    fn setup_of(&self, tests: &[(f64, String)]) -> f64 {
        let files: BTreeSet<&str> = tests
            .iter()
            .filter_map(|(_, test)| self.passing.get(test).map(|(file, _)| file.as_str()))
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
    let test_hit: FxHashSet<&String> = result.hits.values().flatten().collect();
    let static_ids = result
        .static_hits
        .iter()
        .filter(|id| !test_hit.contains(id))
        .cloned()
        .collect();
    Ok(DryRunFacts {
        passing,
        covering,
        static_ids,
        baseline,
        file_setup: result.file_setup_ms.clone(),
    })
}

/// The covering tests of `mutant` from the dry run. A static mutant runs
/// each passing test of the dry-run files that load its module. A static
/// map miss has no static test file, so it runs each passing dry-run test.
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
    facts
        .passing
        .iter()
        .filter(|(_, (file, _))| map_miss || files.contains(file))
        .map(|(test, _)| test.clone())
        .collect()
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

fn build_jobs(
    plan: &SchedulePlan,
    facts: &DryRunFacts,
    dry_files: &[String],
    caps: Capabilities,
    outcome: &mut ScheduleOutcome,
) -> Vec<Job> {
    let mut regular = Vec::new();
    let mut statics = Vec::new();
    let planned = plan.mutants.iter().map(|mutant| (mutant, false));
    let unreached = plan.unreached.iter().map(|mutant| (mutant, true));
    for (mutant, map_miss) in planned.chain(unreached) {
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
        if map_miss {
            outcome.issues.push(ScheduleIssue {
                code: "static-map-miss",
                message: format!(
                    "the dry run hit mutant `{}` in `{}`, but the static map reaches no test of that file; the module graph misses a test that loads it",
                    mutant.id, mutant.file
                ),
                mutant_id: Some(mutant.id.clone()),
                stderr_tail: None,
            });
        }
        let mut tests: Vec<(f64, String)> = tests
            .into_iter()
            .map(|test| (facts.passing.get(&test).map_or(0.0, |entry| entry.1), test))
            .collect();
        tests.sort_by(|left, right| {
            left.0
                .total_cmp(&right.0)
                .then_with(|| left.1.cmp(&right.1))
        });
        let total: f64 = tests.iter().map(|(duration, _)| duration).sum();
        let setup_ms = scaled_ms(facts.setup_of(&tests));
        let job = Job {
            id: mutant.id.clone(),
            tests: tests.into_iter().map(|(_, test)| test).collect(),
            timeout_ms: timeout_ms(total),
            setup_ms,
            static_mutant,
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
}

impl Phase<'_> {
    fn work(&self, primary: Option<Box<dyn CompanionLink>>, known: FxHashSet<String>) {
        let mut worker = Worker::new(self.spawner, self.config, self.dry_files);
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
                let attempt = Attempt {
                    status: MutantStatus::Pending,
                    killed_by: None,
                    duration_ms: None,
                    issue: None,
                };
                if let Ok(mut attempts) = self.attempts.lock() {
                    attempts.push((job, attempt));
                }
                continue;
            }
            match worker.run_with_retry(&job, self.caps) {
                Ok(attempt) => {
                    if let Ok(mut attempts) = self.attempts.lock() {
                        attempts.push((job, attempt));
                    }
                }
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
}

/// A companion that already did its dry run, and the tests that passed in it.
type WarmLink = (Box<dyn CompanionLink>, FxHashSet<String>);

/// Runs `jobs` on up to `concurrency` workers. The first worker takes
/// `primary`, a companion that already did its dry run.
fn run_phase(
    runner: PhaseRunner<'_>,
    dry_files: &[String],
    jobs: Vec<Job>,
    deadline: Option<Instant>,
    primary: Option<WarmLink>,
) -> Result<(Vec<(Job, Attempt)>, usize), ScheduleError> {
    let workers = runner.config.concurrency.max(1).min(jobs.len().max(1));
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

fn record(
    results: &mut FxHashMap<String, MutantOutcome>,
    issues: &mut Vec<ScheduleIssue>,
    job: Job,
    attempt: Attempt,
) {
    if let Some(issue) = attempt.issue {
        issues.push(issue);
    }
    // `job.tests` is in measured fastest-first order. The report must not
    // depend on durations, so it lists the covering tests by id.
    let mut covering_tests = job.tests;
    covering_tests.sort_unstable();
    results.insert(
        job.id,
        MutantOutcome {
            status: attempt.status,
            killed_by: attempt.killed_by,
            covering_tests,
            static_mutant: job.static_mutant,
            duration_ms: attempt.duration_ms,
        },
    );
}

fn setup_error(stage: &str, fault: &LinkFault) -> ScheduleError {
    ScheduleError::Setup {
        message: format!("the mutation companion {stage} failed: {fault}"),
        stderr_tail: fault.stderr_tail().map(str::to_owned),
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

/// Runs each survivor again in a new companion (section 10). A different
/// second result is `flaky`. The retry stays inside the time budget: a
/// survivor that gets no retry before the deadline keeps `survived` and is
/// listed in [`ScheduleOutcome::unretried_survivors`].
fn retry_survivors(
    runner: PhaseRunner<'_>,
    facts: &DryRunFacts,
    survivors: Vec<Job>,
    outcome: &mut ScheduleOutcome,
) -> Result<(), ScheduleError> {
    let mut retry_files: Vec<String> = survivors
        .iter()
        .flat_map(|job| &job.tests)
        .filter_map(|test| facts.passing.get(test).map(|(file, _)| file.clone()))
        .collect();
    retry_files.sort();
    retry_files.dedup();
    let (retries, _) = run_phase(
        runner,
        &retry_files,
        survivors,
        runner.config.deadline,
        None,
    )?;
    for (job, attempt) in retries {
        if attempt.status == MutantStatus::Pending {
            outcome.unretried_survivors.push(job.id);
            continue;
        }
        let status = match attempt.status {
            MutantStatus::Survived => MutantStatus::Survived,
            MutantStatus::RuntimeError => MutantStatus::RuntimeError,
            _ => MutantStatus::Flaky,
        };
        let first_duration = outcome
            .results
            .get(&job.id)
            .and_then(|first| first.duration_ms);
        record(
            &mut outcome.results,
            &mut outcome.issues,
            job,
            Attempt {
                status,
                killed_by: attempt.killed_by,
                duration_ms: first_duration,
                issue: attempt.issue,
            },
        );
    }
    Ok(())
}

/// Runs the plan: dry run, mutant runs, and survivor retries.
///
/// # Errors
///
/// Returns a [`ScheduleError`] when the run cannot give trustworthy results.
pub fn run_schedule(
    spawner: &dyn CompanionSpawner,
    plan: &SchedulePlan,
    config: &ScheduleConfig,
) -> Result<ScheduleOutcome, ScheduleError> {
    let mut outcome = ScheduleOutcome {
        ready: None,
        results: FxHashMap::default(),
        baseline_failures: Vec::new(),
        unloaded_paths: Vec::new(),
        issues: Vec::new(),
        unretried_survivors: Vec::new(),
        timings: ScheduleTimings::default(),
        workers: 0,
    };
    let no_coverage_for_all = |outcome: &mut ScheduleOutcome| {
        for mutant in &plan.mutants {
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
    let runner = PhaseRunner {
        spawner,
        config,
        caps: ready.capabilities,
    };
    outcome.ready = Some(ready);
    outcome.timings.setup = started.elapsed();
    if dry_files.is_empty() {
        link.shutdown();
        no_coverage_for_all(&mut outcome);
        return Ok(outcome);
    }

    let dry_started = Instant::now();
    let request = Request::DryRun {
        test_files: dry_files.clone(),
    };
    let Response::DryRun(dry) = setup_request(&mut link, &request, config.runner_timeout)? else {
        link.shutdown();
        return Err(setup_error(
            "dry_run",
            &LinkFault::Protocol("no dry_run_result".to_owned()),
        ));
    };
    outcome.timings.dry_run = dry_started.elapsed();
    let facts = match read_dry_run(plan, runner.caps, &dry) {
        Ok(facts) => facts,
        Err(error) => {
            link.shutdown();
            return Err(error);
        }
    };
    outcome.unloaded_paths = dry.unloaded_paths;
    outcome.baseline_failures.clone_from(&facts.baseline);
    let jobs = build_jobs(plan, &facts, &dry_files, runner.caps, &mut outcome);
    let known: FxHashSet<String> = facts.passing.keys().cloned().collect();

    let mutants_started = Instant::now();
    let (attempts, workers) = run_phase(
        runner,
        &dry_files,
        jobs,
        config.deadline,
        Some((link, known)),
    )?;
    outcome.workers = workers;
    outcome.timings.mutants = mutants_started.elapsed();
    let mut survivors = Vec::new();
    for (job, attempt) in attempts {
        if attempt.status == MutantStatus::Survived {
            survivors.push(job.clone());
        }
        record(&mut outcome.results, &mut outcome.issues, job, attempt);
    }
    if !survivors.is_empty() {
        let retry_started = Instant::now();
        retry_survivors(runner, &facts, survivors, &mut outcome)?;
        outcome.timings.retry = retry_started.elapsed();
    }
    outcome
        .issues
        .sort_by(|left, right| left.mutant_id.cmp(&right.mutant_id));
    outcome.unretried_survivors.sort();
    Ok(outcome)
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "tests unwrap to keep assertions short")]
mod tests;
