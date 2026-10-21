use std::sync::atomic::AtomicUsize;
use std::sync::{Arc, Mutex};

use super::super::protocol::{DryTest, Reloaded, RemoteError, RunTest, UnhandledError};
use super::*;

type Script = dyn Fn(usize, &Request) -> Result<Response, LinkFault> + Send + Sync;
type ControlScript = dyn Fn(usize, &RunRequest) -> Result<Response, LinkFault> + Send + Sync;

/// The answer of a passing control run: each requested test passes.
fn passing_control(run: &RunRequest) -> Response {
    run_result(run, RunStatus::Survived, None)
}

struct FakeLink {
    index: usize,
    script: Arc<Script>,
    log: Arc<Mutex<Vec<(usize, Request)>>>,
    waits: Arc<Mutex<Vec<Duration>>>,
    respawn_dry_run: Option<Duration>,
    control: Arc<ControlScript>,
    controls: Arc<Mutex<Vec<(usize, RunRequest)>>>,
}

impl CompanionLink for FakeLink {
    fn request(&mut self, request: &Request, timeout: Duration) -> Result<Response, LinkFault> {
        // A control run (no active mutant) goes to its own log and script, so
        // the mutant-run assertions of each test stay about mutant runs.
        if let Request::Run(run) = request
            && run.mutant_id.is_none()
        {
            if let Ok(mut controls) = self.controls.lock() {
                controls.push((self.index, run.clone()));
            }
            return (self.control)(self.index, run);
        }
        if let Ok(mut log) = self.log.lock() {
            log.push((self.index, request.clone()));
        }
        if let (Request::DryRun { .. }, Some(needed)) = (request, self.respawn_dry_run)
            && self.index > 0
            && timeout < needed
        {
            return Err(LinkFault::Timeout);
        }
        if let (Request::Run(_), Ok(mut waits)) = (request, self.waits.lock()) {
            waits.push(timeout);
        }
        (self.script)(self.index, request)
    }

    fn shutdown(&mut self) {}
}

struct FakeSpawner {
    script: Arc<Script>,
    spawns: AtomicUsize,
    log: Arc<Mutex<Vec<(usize, Request)>>>,
    /// The watchdog wait of each `run` request.
    waits: Arc<Mutex<Vec<Duration>>>,
    integrity: Option<String>,
    /// The time that the dry run of a respawned companion needs. A shorter
    /// timeout gives a watchdog timeout.
    respawn_dry_run: Option<Duration>,
    /// The answer of each control run. The default passes each test.
    control: Arc<ControlScript>,
    controls: Arc<Mutex<Vec<(usize, RunRequest)>>>,
}

impl FakeSpawner {
    fn new(
        script: impl Fn(usize, &Request) -> Result<Response, LinkFault> + Send + Sync + 'static,
    ) -> Self {
        Self {
            script: Arc::new(script),
            spawns: AtomicUsize::new(0),
            log: Arc::new(Mutex::new(Vec::new())),
            waits: Arc::new(Mutex::new(Vec::new())),
            integrity: None,
            respawn_dry_run: None,
            control: Arc::new(|_, run| Ok(passing_control(run))),
            controls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn with_control(
        mut self,
        control: impl Fn(usize, &RunRequest) -> Result<Response, LinkFault> + Send + Sync + 'static,
    ) -> Self {
        self.control = Arc::new(control);
        self
    }

    /// The control runs, with the companion index of each.
    fn controls(&self) -> Vec<(usize, RunRequest)> {
        self.controls.lock().expect("controls").clone()
    }

    fn runs(&self) -> Vec<(usize, RunRequest)> {
        self.log
            .lock()
            .expect("log")
            .iter()
            .filter_map(|(index, request)| match request {
                Request::Run(run) => Some((*index, run.clone())),
                _ => None,
            })
            .collect()
    }

    fn requests_of(&self, index: usize) -> Vec<&'static str> {
        self.log
            .lock()
            .expect("log")
            .iter()
            .filter(|(spawn, _)| *spawn == index)
            .map(|(_, request)| request.type_name())
            .collect()
    }
}

impl CompanionSpawner for FakeSpawner {
    fn spawn(&self) -> Result<Box<dyn CompanionLink>, SpawnFailure> {
        if let Some(message) = &self.integrity {
            return Err(SpawnFailure::Integrity(message.clone()));
        }
        let index = self.spawns.fetch_add(1, Ordering::SeqCst);
        Ok(Box::new(FakeLink {
            index,
            script: Arc::clone(&self.script),
            log: Arc::clone(&self.log),
            waits: Arc::clone(&self.waits),
            respawn_dry_run: self.respawn_dry_run,
            control: Arc::clone(&self.control),
            controls: Arc::clone(&self.controls),
        }))
    }
}

const VITEST_CAPS: Capabilities = Capabilities {
    per_test_attribution: true,
    test_filter: true,
    hot_switch: true,
    bail: true,
    fresh_modules_per_run: true,
    batch_mutants: false,
};

fn ready(caps: Capabilities, version: &str) -> Response {
    Response::Ready(Ready {
        runner_version: version.to_owned(),
        runtime_version: "v24.20.0".to_owned(),
        test_files: vec!["test/a.test.ts".to_owned(), "test/b.test.ts".to_owned()],
        capabilities: caps,
        notes: Vec::new(),
    })
}

fn test(id: &str, duration: f64, status: TestStatus) -> DryTest {
    DryTest {
        test_id: format!("test/a.test.ts::{id}"),
        file: "test/a.test.ts".to_owned(),
        duration_ms: Some(duration),
        status,
    }
}

fn tid(id: &str) -> String {
    format!("test/a.test.ts::{id}")
}

/// t1 (fast) hits m1, t2 (slow) and t1 hit m2, nothing hits m3.
fn standard_dry() -> DryRunResult {
    DryRunResult {
        tests: vec![
            test("t1", 2.0, TestStatus::Pass),
            test("t2", 100.0, TestStatus::Pass),
        ],
        hits: BTreeMap::from([
            (tid("t1"), vec!["m1".to_owned(), "m2".to_owned()]),
            (tid("t2"), vec!["m2".to_owned()]),
        ]),
        static_hits: Vec::new(),
        file_hits: None,
        unhandled_errors: Vec::new(),
        unloaded_paths: vec!["src/c.ts".to_owned()],
        file_setup_ms: BTreeMap::new(),
    }
}

fn run_result(run: &RunRequest, status: RunStatus, killed_by: Option<&str>) -> Response {
    let tests = match status {
        RunStatus::Survived => run
            .test_ids
            .iter()
            .map(|test| RunTest {
                test_id: test.clone(),
                status: TestStatus::Pass,
                failure_kind: None,
            })
            .collect(),
        RunStatus::Killed => killed_by
            .map(|test| RunTest {
                test_id: test.to_owned(),
                status: TestStatus::Fail,
                failure_kind: Some("assertion".to_owned()),
            })
            .into_iter()
            .collect(),
        _ => Vec::new(),
    };
    Response::Run(RunResult {
        mutant_id: run.mutant_id.clone(),
        status,
        killed_by: killed_by.map(str::to_owned),
        duration_ms: Some(12.4),
        tests_executed: tests.len(),
        tests,
        unhandled_errors: Vec::new(),
    })
}

fn script(
    dry: impl Fn(usize) -> DryRunResult + Send + Sync + 'static,
    run: impl Fn(usize, &RunRequest) -> Result<Response, LinkFault> + Send + Sync + 'static,
) -> impl Fn(usize, &Request) -> Result<Response, LinkFault> + Send + Sync + 'static {
    move |index, request| match request {
        Request::Init(_) => Ok(ready(VITEST_CAPS, "5.0.4")),
        Request::DryRun { .. } => Ok(Response::DryRun(dry(index))),
        Request::Run(body) => run(index, body),
        Request::Reload => Ok(Response::Reloaded(Reloaded {
            method: ReloadMethod::Noop,
        })),
        Request::Shutdown => Err(LinkFault::Protocol("no response".to_owned())),
    }
}

fn plan(ids: &[&str]) -> SchedulePlan {
    SchedulePlan {
        mutants: ids
            .iter()
            .map(|id| ScheduledMutant {
                id: (*id).to_owned(),
                file: "src/a.ts".to_owned(),
            })
            .collect(),
        unreached: Vec::new(),
        pending: Vec::new(),
        known_ids: ids
            .iter()
            .copied()
            .chain(["m0", "m1", "m2", "m3"])
            .map(str::to_owned)
            .collect(),
        test_files: vec![
            "test/a.test.ts".to_owned(),
            "test/types.test-d.ts".to_owned(),
        ],
        file_test_files: BTreeMap::from([(
            "src/a.ts".to_owned(),
            vec!["test/a.test.ts".to_owned()],
        )]),
    }
}

fn config(concurrency: usize) -> ScheduleConfig {
    ScheduleConfig {
        init: InitRequest {
            root: "/project".to_owned(),
            runner: "vitest".to_owned(),
            config_path: None,
            run_dir: "/cache/run".to_owned(),
            path_map: BTreeMap::new(),
            options: super::super::protocol::InitOptions::default(),
        },
        runner: RunnerRange {
            name: "vitest".to_owned(),
            package: "vitest".to_owned(),
            min_version: "4.0.0".to_owned(),
            max_version_exclusive: "6.0.0".to_owned(),
        },
        concurrency,
        runner_timeout: Duration::from_secs(5),
        deadline: None,
        cache: None,
    }
}

fn status(outcome: &ScheduleOutcome, id: &str) -> MutantStatus {
    outcome.results[id].status
}

#[test]
fn killed_survived_and_no_coverage_follow_the_dry_run_hits() {
    let spawner = FakeSpawner::new(script(
        |_| standard_dry(),
        |_, run| match run.mutant_id.as_deref().unwrap_or_default() {
            "m1" => Ok(run_result(run, RunStatus::Killed, Some(&tid("t1")))),
            _ => Ok(run_result(run, RunStatus::Survived, None)),
        },
    ));
    let outcome = run_schedule(&spawner, &plan(&["m1", "m2", "m3"]), &config(1)).expect("run");
    assert_eq!(status(&outcome, "m1"), MutantStatus::Killed);
    assert_eq!(outcome.results["m1"].killed_by, Some(tid("t1")));
    assert_eq!(status(&outcome, "m2"), MutantStatus::Survived);
    assert_eq!(status(&outcome, "m3"), MutantStatus::NoCoverage);
    assert_eq!(outcome.results["m1"].duration_ms, Some(12));
    assert_eq!(outcome.unloaded_paths, vec!["src/c.ts"]);
    assert_eq!(
        spawner.requests_of(0)[..2],
        ["init", "dry_run"],
        "the first companion sends init, then the dry run"
    );
    let dry_files =
        spawner
            .log
            .lock()
            .expect("log")
            .iter()
            .find_map(|(_, request)| match request {
                Request::DryRun { test_files } => Some(test_files.clone()),
                _ => None,
            });
    assert_eq!(
        dry_files,
        Some(vec!["test/a.test.ts".to_owned()]),
        "a static test file that the runner does not list is not dry-run"
    );
}

#[test]
fn the_progress_sink_gets_the_baseline_mutant_and_retry_counts_in_run_order() {
    let spawner = FakeSpawner::new(script(
        |_| standard_dry(),
        |_, run| match run.mutant_id.as_deref().unwrap_or_default() {
            "m1" => Ok(run_result(run, RunStatus::Killed, Some(&tid("t1")))),
            _ => Ok(run_result(run, RunStatus::Survived, None)),
        },
    ));
    let events = Mutex::new(Vec::new());
    let sink = |event: MutationProgress| events.lock().expect("events").push(event);
    run_schedule_with_progress(&spawner, &plan(&["m1", "m2", "m3"]), &config(1), &sink)
        .expect("run");
    assert_eq!(
        events.into_inner().expect("events"),
        [
            MutationProgress::Baseline { test_files: 1 },
            MutationProgress::Mutants { done: 0, total: 2 },
            MutationProgress::Mutants { done: 1, total: 2 },
            MutationProgress::Mutants { done: 2, total: 2 },
            MutationProgress::Retries { done: 0, total: 1 },
            MutationProgress::Retries { done: 1, total: 1 },
        ],
        "m3 has no covering test, so it is not a mutant job"
    );
}

#[test]
fn covering_tests_run_fastest_first_with_the_stryker_timeout() {
    let spawner = FakeSpawner::new(script(
        |_| standard_dry(),
        |_, run| Ok(run_result(run, RunStatus::Killed, Some(&run.test_ids[0]))),
    ));
    run_schedule(&spawner, &plan(&["m2"]), &config(1)).expect("run");
    let runs = spawner.runs();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].1.test_ids, vec![tid("t1"), tid("t2")]);
    assert_eq!(runs[0].1.timeout_ms, 5153, "ceil(1.5 * 102) + 5000");
    assert!(runs[0].1.bail);
}

#[test]
fn the_watchdog_adds_the_setup_time_of_each_covering_file() {
    let spawner = FakeSpawner::new(script(
        |_| DryRunResult {
            file_setup_ms: BTreeMap::from([
                ("test/a.test.ts".to_owned(), 8000.0),
                ("test/unrelated.test.ts".to_owned(), 60_000.0),
            ]),
            ..standard_dry()
        },
        |_, run| match run.mutant_id.as_deref().unwrap_or_default() {
            "m1" => Err(LinkFault::Timeout),
            _ => Ok(run_result(run, RunStatus::Killed, Some(&run.test_ids[0]))),
        },
    ));
    let outcome = run_schedule(&spawner, &plan(&["m1", "m2"]), &config(1)).expect("run");
    let runs = spawner.runs();
    assert_eq!(
        runs[1].1.timeout_ms, 5153,
        "the setup time is not part of timeout_ms"
    );
    let waits = spawner.waits.lock().expect("waits").clone();
    assert_eq!(
        waits,
        vec![
            Duration::from_millis(5003 + 12_000 + 2000),
            Duration::from_millis(5153 + 12_000 + 2000),
            Duration::from_millis(2 * (5003 + 12_000) + 2000),
        ],
        "timeout_ms + ceil(1.5 * 8000) + 2000, and twice the wait without the grace for the m1 retry"
    );
    let timed_out = &outcome.results["m1"];
    assert_eq!(timed_out.status, MutantStatus::Timeout);
    assert_eq!(timed_out.duration_ms, Some(19_003));
}

#[test]
fn a_survivor_reruns_in_a_new_companion_and_a_different_result_is_flaky() {
    let spawner = FakeSpawner::new(script(
        |_| standard_dry(),
        |index, run| {
            if index == 0 {
                Ok(run_result(run, RunStatus::Survived, None))
            } else {
                Ok(run_result(run, RunStatus::Killed, Some(&tid("t1"))))
            }
        },
    ));
    let outcome = run_schedule(&spawner, &plan(&["m1"]), &config(1)).expect("run");
    assert_eq!(status(&outcome, "m1"), MutantStatus::Flaky);
    let runs = spawner.runs();
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[0].0, 0);
    assert_eq!(runs[1].0, 1, "the retry uses a new companion");
    assert_eq!(spawner.requests_of(1)[..2], ["init", "dry_run"]);
}

#[test]
fn a_survivor_with_a_timed_out_retry_stays_survived() {
    let spawner = FakeSpawner::new(script(
        |_| standard_dry(),
        |index, run| {
            if index == 0 {
                Ok(run_result(run, RunStatus::Survived, None))
            } else {
                Err(LinkFault::Timeout)
            }
        },
    ));
    let outcome = run_schedule(&spawner, &plan(&["m1"]), &config(1)).expect("run");
    let result = &outcome.results["m1"];
    assert_eq!(
        result.status,
        MutantStatus::Survived,
        "the first run finished with the mutant active, and a timeout under host load is no detection"
    );
    assert!(result.retried);
    assert_eq!(
        result.duration_ms,
        Some(12),
        "a survivor keeps the duration of the first run"
    );
    assert_eq!(spawner.runs().len(), 2);
}

#[test]
fn fail_closed_a_survived_claim_without_full_evidence_is_a_runtime_error() {
    type Respond = fn(&RunRequest) -> Response;
    let cases: Vec<(&str, Respond)> = vec![
        ("zero tests", |run| {
            Response::Run(RunResult {
                mutant_id: run.mutant_id.clone(),
                status: RunStatus::Survived,
                killed_by: None,
                duration_ms: None,
                tests_executed: 0,
                tests: Vec::new(),
                unhandled_errors: Vec::new(),
            })
        }),
        ("one of two tests", |run| {
            Response::Run(RunResult {
                mutant_id: run.mutant_id.clone(),
                status: RunStatus::Survived,
                killed_by: None,
                duration_ms: None,
                tests_executed: 1,
                tests: vec![RunTest {
                    test_id: run.test_ids[0].clone(),
                    status: TestStatus::Pass,
                    failure_kind: None,
                }],
                unhandled_errors: Vec::new(),
            })
        }),
        ("a failed test", |run| {
            Response::Run(RunResult {
                mutant_id: run.mutant_id.clone(),
                status: RunStatus::Survived,
                killed_by: None,
                duration_ms: None,
                tests_executed: 2,
                tests: vec![
                    RunTest {
                        test_id: run.test_ids[0].clone(),
                        status: TestStatus::Pass,
                        failure_kind: None,
                    },
                    RunTest {
                        test_id: run.test_ids[1].clone(),
                        status: TestStatus::Fail,
                        failure_kind: None,
                    },
                ],
                unhandled_errors: Vec::new(),
            })
        }),
        ("another mutant", |run| {
            let mut other = run.clone();
            other.mutant_id = Some("m9".to_owned());
            run_result(&other, RunStatus::Survived, None)
        }),
        ("a kill with no evidence", |run| {
            Response::Run(RunResult {
                mutant_id: run.mutant_id.clone(),
                status: RunStatus::Killed,
                killed_by: None,
                duration_ms: None,
                tests_executed: 0,
                tests: Vec::new(),
                unhandled_errors: Vec::new(),
            })
        }),
    ];
    for (name, respond) in cases {
        let spawner = FakeSpawner::new(script(|_| standard_dry(), move |_, run| Ok(respond(run))));
        let outcome = run_schedule(&spawner, &plan(&["m2"]), &config(1)).expect("run");
        assert_eq!(status(&outcome, "m2"), MutantStatus::RuntimeError, "{name}");
        assert_eq!(
            spawner.runs().len(),
            2,
            "{name}: one retry in a new companion"
        );
        assert!(
            outcome
                .issues
                .iter()
                .any(|issue| issue.mutant_id.as_deref() == Some("m2")),
            "{name}: the reason is reported"
        );
    }
}

#[test]
fn a_kill_by_an_unhandled_error_has_no_killer() {
    let spawner = FakeSpawner::new(script(
        |_| standard_dry(),
        |_, run| {
            Ok(Response::Run(RunResult {
                mutant_id: run.mutant_id.clone(),
                status: RunStatus::Killed,
                killed_by: None,
                duration_ms: Some(3.0),
                tests_executed: 1,
                tests: vec![RunTest {
                    test_id: run.test_ids[0].clone(),
                    status: TestStatus::Pass,
                    failure_kind: None,
                }],
                unhandled_errors: vec![UnhandledError {
                    message: "late rejection".to_owned(),
                    file: Some("test/a.test.ts".to_owned()),
                }],
            }))
        },
    ));
    let outcome = run_schedule(&spawner, &plan(&["m1"]), &config(1)).expect("run");
    assert_eq!(status(&outcome, "m1"), MutantStatus::Killed);
    assert_eq!(outcome.results["m1"].killed_by, None);
}

#[test]
fn a_companion_exit_is_retried_once_in_a_new_companion() {
    let spawner = FakeSpawner::new(script(
        |_| standard_dry(),
        |index, run| {
            if index == 0 {
                Err(LinkFault::Exited {
                    stderr_tail: "segfault".to_owned(),
                })
            } else {
                Ok(run_result(run, RunStatus::Killed, Some(&tid("t1"))))
            }
        },
    ));
    let outcome = run_schedule(&spawner, &plan(&["m1"]), &config(1)).expect("run");
    assert_eq!(status(&outcome, "m1"), MutantStatus::Killed);
    assert_eq!(
        spawner.runs().iter().map(|run| run.0).collect::<Vec<_>>(),
        vec![0, 1]
    );
}

#[test]
fn the_dry_run_of_a_respawned_companion_waits_in_proportion_to_the_first_dry_run() {
    // On a loaded host the dry run of a respawned companion takes longer than
    // `runner_timeout`, although the first dry run took less.
    let first_dry_run = Duration::from_millis(200);
    let mut spawner = FakeSpawner::new(script(
        move |index| {
            if index == 0 {
                std::thread::sleep(first_dry_run);
            }
            standard_dry()
        },
        |index, run| {
            if index == 0 {
                Err(LinkFault::Exited {
                    stderr_tail: "segfault".to_owned(),
                })
            } else {
                Ok(run_result(run, RunStatus::Killed, Some(&tid("t1"))))
            }
        },
    ));
    spawner.respawn_dry_run = Some(Duration::from_millis(300));
    let config = ScheduleConfig {
        runner_timeout: Duration::from_millis(50),
        ..config(1)
    };
    let outcome = run_schedule(&spawner, &plan(&["m1"]), &config).expect("run");
    assert_eq!(status(&outcome, "m1"), MutantStatus::Killed);
    assert_eq!(spawner.requests_of(1), ["init", "dry_run", "run"]);
}

#[test]
fn the_respawn_dry_run_timeout_is_at_least_the_runner_timeout() {
    let floor = Duration::from_secs(120);
    assert_eq!(
        respawn_dry_run_timeout(floor, Duration::from_secs(10), 2),
        floor,
        "a fast first dry run keeps the runner timeout"
    );
    assert_eq!(
        respawn_dry_run_timeout(floor, Duration::from_secs(26), 5),
        Duration::from_secs(26 * 2 * 5),
        "each worker shares the host, so the wait grows with the worker count"
    );
    assert_eq!(
        respawn_dry_run_timeout(floor, Duration::MAX, usize::MAX),
        Duration::MAX
    );
}

#[test]
fn two_runtime_errors_stay_a_runtime_error_with_the_stderr_tail() {
    let spawner = FakeSpawner::new(script(
        |_| standard_dry(),
        |_, _| {
            Err(LinkFault::Exited {
                stderr_tail: "out of memory".to_owned(),
            })
        },
    ));
    let outcome = run_schedule(&spawner, &plan(&["m1"]), &config(1)).expect("run");
    assert_eq!(status(&outcome, "m1"), MutantStatus::RuntimeError);
    assert_eq!(spawner.runs().len(), 2);
    let issue = outcome
        .issues
        .iter()
        .find(|issue| issue.mutant_id.as_deref() == Some("m1"))
        .expect("the m1 issue");
    assert_eq!(issue.stderr_tail.as_deref(), Some("out of memory"));
}

#[test]
fn a_watchdog_timeout_is_timeout_and_the_next_mutant_gets_a_new_companion() {
    let spawner = FakeSpawner::new(script(
        |_| standard_dry(),
        |_, run| match run.mutant_id.as_deref().unwrap_or_default() {
            "m1" => Err(LinkFault::Timeout),
            _ => Ok(run_result(run, RunStatus::Killed, Some(&tid("t1")))),
        },
    ));
    let outcome = run_schedule(&spawner, &plan(&["m1", "m2"]), &config(1)).expect("run");
    assert_eq!(status(&outcome, "m1"), MutantStatus::Timeout);
    assert_eq!(status(&outcome, "m2"), MutantStatus::Killed);
    assert!(outcome.results["m1"].retried, "a confirmed timeout");
    assert!(!outcome.results["m2"].retried, "a kill gets no retry");
    let runs = spawner.runs();
    assert_eq!(runs.len(), 3, "a timeout gets one retry");
    assert_eq!((runs[0].0, runs[1].0), (0, 1));
    assert_eq!(spawner.requests_of(1)[..3], ["init", "dry_run", "run"]);
    assert_eq!(
        runs[2].1.mutant_id.as_deref(),
        Some("m1"),
        "the retry runs the timed-out mutant"
    );
}

#[test]
fn a_timeout_reruns_in_a_new_companion_and_a_survived_retry_is_flaky() {
    let spawner = FakeSpawner::new(script(
        |_| standard_dry(),
        |index, run| {
            if index == 0 {
                Ok(run_result(run, RunStatus::Timeout, None))
            } else {
                Ok(run_result(run, RunStatus::Survived, None))
            }
        },
    ));
    let outcome = run_schedule(&spawner, &plan(&["m1"]), &config(1)).expect("run");
    assert_eq!(status(&outcome, "m1"), MutantStatus::Flaky);
    assert!(outcome.results["m1"].retried);
    let runs = spawner.runs();
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[1].0, 1, "the retry uses a new companion");
    assert_eq!(
        runs[1].1.timeout_ms,
        2 * runs[0].1.timeout_ms,
        "host load that slowed the first run can last, so the retry waits twice as long"
    );
}

/// Runs m1 with `first` in the first companion and `retry` in the next one.
fn retried_m1(
    first: impl Fn(&RunRequest) -> Result<Response, LinkFault> + Send + Sync + 'static,
    retry: impl Fn(&RunRequest) -> Result<Response, LinkFault> + Send + Sync + 'static,
) -> MutantOutcome {
    let spawner = FakeSpawner::new(script(
        |_| standard_dry(),
        move |index, run| if index == 0 { first(run) } else { retry(run) },
    ));
    let outcome = run_schedule(&spawner, &plan(&["m1"]), &config(1)).expect("run");
    outcome.results["m1"].clone()
}

#[test]
fn a_flaky_result_keeps_the_history_of_each_attempt() {
    let timeout_then_survived = retried_m1(
        |_| Err(LinkFault::Timeout),
        |run| Ok(run_result(run, RunStatus::Survived, None)),
    );
    let survived_then_killed = retried_m1(
        |run| Ok(run_result(run, RunStatus::Survived, None)),
        |run| Ok(run_result(run, RunStatus::Killed, Some(&tid("t1")))),
    );
    assert_eq!(timeout_then_survived.status, MutantStatus::Flaky);
    assert_eq!(survived_then_killed.status, MutantStatus::Flaky);
    let watchdog_ms = timeout_then_survived
        .attempts
        .first()
        .and_then(|attempt| attempt.duration_ms);
    assert!(watchdog_ms.is_some_and(|ms| ms > 12), "{watchdog_ms:?}");
    assert_eq!(
        timeout_then_survived.attempts,
        [
            MutantAttempt {
                status: MutantStatus::Timeout,
                duration_ms: watchdog_ms,
                killed_by: None,
                failure_kind: Some(MutantFailureKind::Timeout),
                detail: None,
            },
            MutantAttempt {
                status: MutantStatus::Survived,
                duration_ms: Some(12),
                killed_by: None,
                failure_kind: None,
                detail: None,
            },
        ]
    );
    assert_eq!(
        survived_then_killed.attempts,
        [
            MutantAttempt {
                status: MutantStatus::Survived,
                duration_ms: Some(12),
                killed_by: None,
                failure_kind: None,
                detail: None,
            },
            MutantAttempt {
                status: MutantStatus::Killed,
                duration_ms: Some(12),
                killed_by: Some(tid("t1")),
                failure_kind: Some(MutantFailureKind::Assertion),
                detail: None,
            },
        ]
    );
}

#[test]
fn a_timeout_from_the_companion_keeps_its_failure_kind() {
    let timeout_then_survived = retried_m1(
        |run| Ok(run_result(run, RunStatus::Timeout, None)),
        |run| Ok(run_result(run, RunStatus::Survived, None)),
    );
    assert_eq!(timeout_then_survived.status, MutantStatus::Flaky);
    assert_eq!(
        timeout_then_survived.attempts.first(),
        Some(&MutantAttempt {
            status: MutantStatus::Timeout,
            duration_ms: Some(12),
            killed_by: None,
            failure_kind: Some(MutantFailureKind::Timeout),
            detail: None,
        }),
        "a runner timeout carries the duration of the run result"
    );
}

#[test]
fn a_runtime_error_retry_keeps_its_reason_and_a_single_run_has_no_attempts() {
    let survived_then_error = retried_m1(
        |run| Ok(run_result(run, RunStatus::Survived, None)),
        |run| Ok(run_result(run, RunStatus::RuntimeError, None)),
    );
    assert_eq!(survived_then_error.status, MutantStatus::RuntimeError);
    let statuses: Vec<_> = survived_then_error
        .attempts
        .iter()
        .map(|attempt| (attempt.status, attempt.detail.as_deref()))
        .collect();
    assert_eq!(
        statuses,
        [
            (MutantStatus::Survived, None),
            (
                MutantStatus::RuntimeError,
                Some("the companion reported a runtime error")
            ),
        ]
    );
    let killed = retried_m1(
        |run| Ok(run_result(run, RunStatus::Killed, Some(&tid("t1")))),
        |run| Ok(run_result(run, RunStatus::Survived, None)),
    );
    assert_eq!(killed.status, MutantStatus::Killed);
    assert!(!killed.retried);
    assert!(killed.attempts.is_empty(), "a kill gets no retry");
}

#[test]
fn a_kill_names_the_failure_kind_of_its_killing_test_or_an_error() {
    let request = RunRequest {
        mutant_id: Some("m1".to_owned()),
        test_ids: vec![tid("t1")],
        timeout_ms: 1000,
        bail: true,
    };
    let Response::Run(mut result) = run_result(&request, RunStatus::Killed, Some(&tid("t1")))
    else {
        unreachable!("run_result gives a run response")
    };
    let killed = MutantStatus::Killed;
    let killer = tid("t1");
    assert_eq!(
        kill_kind(&result, killed, Some(&killer)),
        Some(MutantFailureKind::Assertion)
    );
    assert_eq!(
        kill_kind(&result, killed, None),
        Some(MutantFailureKind::Error)
    );
    assert_eq!(
        kill_kind(&result, MutantStatus::Survived, Some(&killer)),
        None
    );
    result.tests[0].failure_kind = Some("segfault".to_owned());
    assert_eq!(kill_kind(&result, killed, Some(&killer)), None);
}

#[test]
fn a_timeout_with_a_killed_retry_is_killed_with_the_retry_duration() {
    let spawner = FakeSpawner::new(script(
        |_| standard_dry(),
        |index, run| {
            if index == 0 {
                Err(LinkFault::Timeout)
            } else {
                Ok(run_result(run, RunStatus::Killed, Some(&tid("t1"))))
            }
        },
    ));
    let outcome = run_schedule(&spawner, &plan(&["m1"]), &config(1)).expect("run");
    let result = &outcome.results["m1"];
    assert_eq!(result.status, MutantStatus::Killed);
    assert_eq!(result.killed_by.as_deref(), Some(tid("t1").as_str()));
    assert_eq!(
        result.duration_ms,
        Some(12),
        "the kill reports the duration of the run that killed it, not the watchdog wait"
    );
}

#[test]
fn rule_9_a_hit_outside_the_plan_stops_the_run() {
    let spawner = FakeSpawner::new(script(
        |_| {
            let mut dry = standard_dry();
            dry.static_hits.push("m-stale".to_owned());
            dry
        },
        |_, run| Ok(run_result(run, RunStatus::Killed, Some(&tid("t1")))),
    ));
    let error = run_schedule(&spawner, &plan(&["m1", "m2"]), &config(1)).unwrap_err();
    assert_eq!(error, ScheduleError::UnknownHit("m-stale".to_owned()));
    assert!(spawner.runs().is_empty());
}

#[test]
fn a_hash_mismatch_stops_the_run_before_the_spawn() {
    let mut spawner = FakeSpawner::new(script(
        |_| standard_dry(),
        |_, run| Ok(run_result(run, RunStatus::Killed, Some(&tid("t1")))),
    ));
    spawner.integrity = Some("companion.mjs changed".to_owned());
    let error = run_schedule(&spawner, &plan(&["m1"]), &config(1)).unwrap_err();
    assert_eq!(
        error,
        ScheduleError::Integrity("companion.mjs changed".to_owned())
    );
    assert!(spawner.log.lock().expect("log").is_empty());
}

#[test]
fn a_runner_version_outside_the_manifest_range_stops_the_run() {
    let spawner = FakeSpawner::new(|_, request| match request {
        Request::Init(_) => Ok(ready(VITEST_CAPS, "3.2.4")),
        _ => Err(LinkFault::Protocol("unexpected".to_owned())),
    });
    let error = run_schedule(&spawner, &plan(&["m1"]), &config(1)).unwrap_err();
    assert!(
        matches!(&error, ScheduleError::RunnerVersion(message) if message.contains("3.2.4")),
        "{error:?}"
    );
}

#[test]
fn a_runner_version_error_of_the_companion_is_a_runner_version_error() {
    let message = "vitest 4.0.18 is not supported; the supported range is >=4.1.0 <6.0.0";
    let spawner = FakeSpawner::new(move |_, _| {
        Err(LinkFault::Remote(RemoteError {
            code: "runner-version".to_owned(),
            message: message.to_owned(),
            retryable: false,
        }))
    });
    let error = run_schedule(&spawner, &plan(&["m1"]), &config(1)).unwrap_err();
    assert_eq!(error, ScheduleError::RunnerVersion(message.to_owned()));
}

#[test]
fn an_init_error_response_stops_the_run_with_its_message() {
    let spawner = FakeSpawner::new(|_, _| {
        Err(LinkFault::Remote(RemoteError {
            code: "runner-not-found".to_owned(),
            message: "cannot resolve vitest".to_owned(),
            retryable: false,
        }))
    });
    let error = run_schedule(&spawner, &plan(&["m1"]), &config(1)).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("runner-not-found: cannot resolve vitest"),
        "{error}"
    );
}

#[test]
fn baseline_failures_never_decide_a_mutant() {
    let spawner = FakeSpawner::new(script(
        |_| DryRunResult {
            tests: vec![
                test("t1", 2.0, TestStatus::Fail),
                test("t2", 3.0, TestStatus::Pass),
                DryTest {
                    test_id: "test/b.test.ts::t3".to_owned(),
                    file: "test/b.test.ts".to_owned(),
                    duration_ms: Some(1.0),
                    status: TestStatus::Pass,
                },
            ],
            hits: BTreeMap::from([
                (tid("t1"), vec!["m1".to_owned()]),
                (tid("t2"), vec!["m2".to_owned()]),
                (
                    "test/b.test.ts::t3".to_owned(),
                    vec!["m2".to_owned(), "m3".to_owned()],
                ),
            ]),
            static_hits: Vec::new(),
            file_hits: None,
            unhandled_errors: vec![UnhandledError {
                message: "suite failed".to_owned(),
                file: Some("test/b.test.ts".to_owned()),
            }],
            unloaded_paths: Vec::new(),
            file_setup_ms: BTreeMap::new(),
        },
        |_, run| Ok(run_result(run, RunStatus::Killed, Some(&run.test_ids[0]))),
    ));
    let mut plan = plan(&["m1", "m2", "m3"]);
    plan.test_files.push("test/b.test.ts".to_owned());
    let outcome = run_schedule(&spawner, &plan, &config(1)).expect("run");
    assert_eq!(
        status(&outcome, "m1"),
        MutantStatus::NoCoverage,
        "only a failing test hit m1"
    );
    assert_eq!(
        status(&outcome, "m3"),
        MutantStatus::NoCoverage,
        "only an errored file hit m3"
    );
    assert_eq!(status(&outcome, "m2"), MutantStatus::Killed);
    assert_eq!(spawner.runs()[0].1.test_ids, vec![tid("t2")]);
    assert_eq!(
        outcome.baseline_failures,
        vec![
            BaselineFailure {
                file: Some("test/a.test.ts".to_owned()),
                test_id: Some(tid("t1")),
                message: None,
                mutant_ids: Vec::new(),
            },
            BaselineFailure {
                file: Some("test/b.test.ts".to_owned()),
                test_id: None,
                message: Some("suite failed".to_owned()),
                mutant_ids: Vec::new(),
            },
        ]
    );
}

#[test]
fn an_error_outside_each_test_file_stops_the_run() {
    let spawner = FakeSpawner::new(script(
        |_| {
            let mut dry = standard_dry();
            dry.unhandled_errors.push(UnhandledError {
                message: "global setup threw".to_owned(),
                file: None,
            });
            dry
        },
        |_, run| Ok(run_result(run, RunStatus::Killed, Some(&tid("t1")))),
    ));
    let error = run_schedule(&spawner, &plan(&["m1"]), &config(1)).unwrap_err();
    assert_eq!(
        error,
        ScheduleError::Baseline(vec!["global setup threw".to_owned()])
    );
}

#[test]
fn static_mutants_run_last_with_a_reload_around_each_when_modules_stay_loaded() {
    let caps = Capabilities {
        fresh_modules_per_run: false,
        ..VITEST_CAPS
    };
    let spawner = FakeSpawner::new(move |index, request| match request {
        Request::Init(_) => Ok(ready(caps, "4.1.11")),
        Request::DryRun { .. } => Ok(Response::DryRun(DryRunResult {
            static_hits: vec!["m0".to_owned()],
            ..standard_dry()
        })),
        Request::Run(run) => Ok(run_result(run, RunStatus::Killed, Some(&run.test_ids[0]))),
        Request::Reload => Ok(Response::Reloaded(Reloaded {
            method: if index == 0 {
                ReloadMethod::RestartRequired
            } else {
                ReloadMethod::Invalidate
            },
        })),
        Request::Shutdown => Err(LinkFault::Protocol("no response".to_owned())),
    });
    let outcome = run_schedule(&spawner, &plan(&["m0", "m1"]), &config(1)).expect("run");
    let runs = spawner.runs();
    assert_eq!(
        runs.iter()
            .map(|run| run.1.mutant_id.as_deref().unwrap_or_default())
            .collect::<Vec<_>>(),
        vec!["m1", "m0"],
        "the static mutant runs last although it ranks first"
    );
    assert_eq!(runs[1].0, 1, "restart_required replaces the companion");
    assert_eq!(
        runs[1].1.test_ids,
        vec![tid("t1"), tid("t2")],
        "a static mutant runs each test of the files that load its module"
    );
    assert!(outcome.results["m0"].static_mutant);
    assert_eq!(
        spawner.requests_of(1),
        vec!["init", "dry_run", "run", "reload"],
        "the reload after the static mutant"
    );
}

#[test]
fn a_module_level_mutant_with_static_and_test_hits_runs_static_importer_tests() {
    let dyn_test = "test/b.test.ts::dyn".to_owned();
    let dry_test = dyn_test.clone();
    let spawner = FakeSpawner::new(script(
        move |_| {
            let mut dry = standard_dry();
            dry.tests.push(DryTest {
                test_id: dry_test.clone(),
                file: "test/b.test.ts".to_owned(),
                duration_ms: Some(5.0),
                status: TestStatus::Pass,
            });
            dry.hits.insert(dry_test.clone(), vec!["m0".to_owned()]);
            dry.static_hits = vec!["m0".to_owned()];
            dry
        },
        |_, run| {
            if run.test_ids.contains(&tid("t1")) {
                Ok(run_result(run, RunStatus::Killed, Some(&tid("t1"))))
            } else {
                Ok(run_result(run, RunStatus::Survived, None))
            }
        },
    ));
    let outcome = run_schedule(&spawner, &plan(&["m0"]), &config(1)).expect("run");
    let runs = spawner.runs();
    let mut test_ids = runs[0].1.test_ids.clone();
    test_ids.sort();
    assert_eq!(
        test_ids,
        vec![tid("t1"), tid("t2"), dyn_test],
        "a module that loads at collection time and inside a test runs the tests of both files"
    );
    assert!(outcome.results["m0"].static_mutant);
    assert_eq!(status(&outcome, "m0"), MutantStatus::Killed);
}

#[test]
fn file_hits_cover_each_passing_test_of_the_file_without_per_test_attribution() {
    let caps = Capabilities {
        per_test_attribution: false,
        ..VITEST_CAPS
    };
    let spawner = FakeSpawner::new(move |_, request| match request {
        Request::Init(_) => Ok(ready(caps, "5.0.4")),
        Request::DryRun { .. } => Ok(Response::DryRun(DryRunResult {
            hits: BTreeMap::new(),
            file_hits: Some(BTreeMap::from([(
                "test/a.test.ts".to_owned(),
                vec!["m1".to_owned()],
            )])),
            ..standard_dry()
        })),
        Request::Run(run) => Ok(run_result(run, RunStatus::Killed, Some(&run.test_ids[0]))),
        _ => Err(LinkFault::Protocol("unexpected".to_owned())),
    });
    let outcome = run_schedule(&spawner, &plan(&["m1", "m2"]), &config(1)).expect("run");
    assert_eq!(spawner.runs()[0].1.test_ids, vec![tid("t1"), tid("t2")]);
    assert_eq!(status(&outcome, "m2"), MutantStatus::NoCoverage);
}

#[test]
fn an_ended_time_budget_leaves_each_mutant_pending() {
    let spawner = FakeSpawner::new(script(
        |_| standard_dry(),
        |_, run| Ok(run_result(run, RunStatus::Killed, Some(&tid("t1")))),
    ));
    let mut config = config(2);
    config.deadline = Some(Instant::now());
    let outcome = run_schedule(&spawner, &plan(&["m1", "m2", "m3"]), &config).expect("run");
    assert_eq!(status(&outcome, "m1"), MutantStatus::Pending);
    assert_eq!(status(&outcome, "m2"), MutantStatus::Pending);
    assert_eq!(status(&outcome, "m3"), MutantStatus::NoCoverage);
    assert!(spawner.runs().is_empty());
}

#[test]
fn a_capped_run_gives_no_coverage_to_the_same_mutants_as_a_full_run() {
    let killer =
        |_: usize, run: &RunRequest| Ok(run_result(run, RunStatus::Killed, Some(&tid("t1"))));
    let full_spawner = FakeSpawner::new(script(|_| standard_dry(), killer));
    let full = run_schedule(&full_spawner, &plan(&["m1", "m2", "m3"]), &config(1)).expect("full");

    let capped_spawner = FakeSpawner::new(script(|_| standard_dry(), killer));
    let mut capped_plan = plan(&["m1"]);
    capped_plan.pending = ["m2", "m3"]
        .iter()
        .map(|id| ScheduledMutant {
            id: (*id).to_owned(),
            file: "src/a.ts".to_owned(),
        })
        .collect();
    let capped = run_schedule(&capped_spawner, &capped_plan, &config(1)).expect("capped");

    let no_coverage = |outcome: &ScheduleOutcome| -> BTreeSet<String> {
        outcome
            .results
            .iter()
            .filter(|(_, result)| result.status == MutantStatus::NoCoverage)
            .map(|(id, _)| id.clone())
            .collect()
    };
    assert_eq!(no_coverage(&capped), no_coverage(&full));
    assert_eq!(no_coverage(&capped), BTreeSet::from(["m3".to_owned()]));
    assert!(
        !capped.results.contains_key("m2"),
        "a pending mutant with a dry-run hit has no result and stays pending"
    );
    let ran: Vec<String> = capped_spawner
        .runs()
        .into_iter()
        .filter_map(|(_, run)| run.mutant_id)
        .collect();
    assert_eq!(ran, vec!["m1".to_owned()], "a pending mutant never runs");
}

#[test]
fn covered_pending_mutants_fill_the_limit_that_uncovered_planned_mutants_leave() {
    let spawner = FakeSpawner::new(script(
        |_| standard_dry(),
        |_, run| Ok(run_result(run, RunStatus::Killed, Some(&tid("t1")))),
    ));
    // The limit is one mutant. The planned mutant has no dry-run hit, so the
    // first covered pending mutant in rank order takes its place.
    let mut plan = plan(&["m3", "m4"]);
    plan.mutants.truncate(1);
    plan.pending = ["m4", "m1", "m2"]
        .iter()
        .map(|id| ScheduledMutant {
            id: (*id).to_owned(),
            file: "src/a.ts".to_owned(),
        })
        .collect();
    let outcome = run_schedule(&spawner, &plan, &config(1)).expect("run");
    let ran: Vec<String> = spawner
        .runs()
        .into_iter()
        .filter_map(|(_, run)| run.mutant_id)
        .collect();
    assert_eq!(ran, vec!["m1".to_owned()]);
    assert_eq!(status(&outcome, "m1"), MutantStatus::Killed);
    assert_eq!(status(&outcome, "m3"), MutantStatus::NoCoverage);
    assert_eq!(status(&outcome, "m4"), MutantStatus::NoCoverage);
    assert!(
        !outcome.results.contains_key("m2"),
        "a covered pending mutant after the filled limit stays pending"
    );
    assert_eq!(outcome.promoted, vec!["m1".to_owned()]);
}

#[test]
fn without_a_dry_run_each_pending_mutant_is_no_coverage() {
    let spawner = FakeSpawner::new(|_, _| Err(LinkFault::Protocol("unexpected".to_owned())));
    let mut plan = plan(&["m1"]);
    plan.pending = vec![ScheduledMutant {
        id: "m2".to_owned(),
        file: "src/a.ts".to_owned(),
    }];
    plan.test_files.clear();
    let outcome = run_schedule(&spawner, &plan, &config(1)).expect("run");
    assert_eq!(status(&outcome, "m2"), MutantStatus::NoCoverage);
}

#[test]
fn concurrent_workers_run_each_mutant_once() {
    let ids: Vec<String> = (0..12).map(|index| format!("m{index}")).collect();
    let spawner = FakeSpawner::new(script(
        move |_| DryRunResult {
            hits: BTreeMap::from([(
                tid("t1"),
                (0..12).map(|index| format!("m{index}")).collect(),
            )]),
            ..standard_dry()
        },
        |_, run| Ok(run_result(run, RunStatus::Killed, Some(&tid("t1")))),
    ));
    let id_refs: Vec<&str> = ids.iter().map(String::as_str).collect();
    let outcome = run_schedule(&spawner, &plan(&id_refs), &config(3)).expect("run");
    assert_eq!(outcome.workers, 3);
    let mut ran: Vec<String> = spawner
        .runs()
        .into_iter()
        .filter_map(|run| run.1.mutant_id)
        .collect();
    ran.sort();
    let mut expected = ids.clone();
    expected.sort();
    assert_eq!(ran, expected);
    assert!(
        ids.iter()
            .all(|id| status(&outcome, id) == MutantStatus::Killed)
    );
}

#[test]
fn no_test_files_give_no_coverage_without_a_spawn() {
    let spawner = FakeSpawner::new(|_, _| Err(LinkFault::Protocol("unexpected".to_owned())));
    let mut plan = plan(&["m1"]);
    plan.test_files.clear();
    let outcome = run_schedule(&spawner, &plan, &config(1)).expect("run");
    assert_eq!(status(&outcome, "m1"), MutantStatus::NoCoverage);
    assert_eq!(spawner.spawns.load(Ordering::SeqCst), 0);
}

#[test]
fn covering_tests_sort_by_id_while_the_run_order_follows_the_duration() {
    let swapped = |_| DryRunResult {
        tests: vec![
            test("t1", 100.0, TestStatus::Pass),
            test("t2", 2.0, TestStatus::Pass),
        ],
        ..standard_dry()
    };
    let spawner = FakeSpawner::new(script(swapped, |_, run| {
        Ok(run_result(run, RunStatus::Survived, None))
    }));
    let outcome = run_schedule(&spawner, &plan(&["m2"]), &config(1)).expect("run");
    assert_eq!(
        spawner.runs()[0].1.test_ids,
        vec![tid("t2"), tid("t1")],
        "the run request keeps the fastest test first"
    );
    assert_eq!(
        outcome.results["m2"].covering_tests,
        vec![tid("t1"), tid("t2")],
        "the report order does not depend on measured durations"
    );
}

#[test]
fn an_ended_time_budget_skips_the_survivor_retry_and_keeps_survived() {
    let spawner = FakeSpawner::new(script(
        |_| standard_dry(),
        |_, run| {
            std::thread::sleep(Duration::from_millis(300));
            Ok(run_result(run, RunStatus::Survived, None))
        },
    ));
    let mut config = config(1);
    config.deadline = Some(Instant::now() + Duration::from_millis(150));
    let outcome = run_schedule(&spawner, &plan(&["m1"]), &config).expect("run");
    assert_eq!(status(&outcome, "m1"), MutantStatus::Survived);
    assert_eq!(outcome.unretried, vec!["m1".to_owned()]);
    assert_eq!(
        spawner.spawns.load(Ordering::SeqCst),
        1,
        "no retry companion starts after the budget ends"
    );
}

#[test]
fn an_ended_time_budget_skips_the_timeout_retry_and_keeps_timeout() {
    let spawner = FakeSpawner::new(script(
        |_| standard_dry(),
        |_, run| {
            std::thread::sleep(Duration::from_millis(300));
            Ok(run_result(run, RunStatus::Timeout, None))
        },
    ));
    let mut config = config(1);
    config.deadline = Some(Instant::now() + Duration::from_millis(150));
    let outcome = run_schedule(&spawner, &plan(&["m1"]), &config).expect("run");
    assert_eq!(status(&outcome, "m1"), MutantStatus::Timeout);
    assert!(!outcome.results["m1"].retried, "an unconfirmed timeout");
    assert_eq!(outcome.unretried, vec!["m1".to_owned()]);
    assert_eq!(spawner.spawns.load(Ordering::SeqCst), 1);
}

#[test]
fn a_time_budget_keeps_time_to_confirm_the_survivors_of_the_first_pass() {
    const IDS: [&str; 8] = ["m1", "m2", "m3", "m4", "m5", "m6", "m7", "m8"];
    let spawner = FakeSpawner::new(script(
        |_| DryRunResult {
            hits: BTreeMap::from([(tid("t1"), IDS.iter().map(|id| (*id).to_owned()).collect())]),
            ..standard_dry()
        },
        |_, run| {
            std::thread::sleep(Duration::from_millis(100));
            Ok(run_result(run, RunStatus::Survived, None))
        },
    ));
    let mut config = config(1);
    // Without a reserve, the first pass fills this budget with about six
    // runs, and no survivor gets its confirmation.
    config.deadline = Some(Instant::now() + Duration::from_millis(550));
    let outcome = run_schedule(&spawner, &plan(&IDS), &config).expect("run");
    assert!(
        outcome.unretried.is_empty(),
        "each survivor of the first pass gets its confirmation: {:?}",
        outcome.unretried
    );
    let confirmed = IDS
        .iter()
        .filter(|id| outcome.results[**id].status == MutantStatus::Survived)
        .inspect(|id| assert!(outcome.results[**id].retried, "{id} is confirmed"))
        .count();
    assert!(confirmed > 0, "the run confirms at least one survivor");
    assert_eq!(
        status(&outcome, "m8"),
        MutantStatus::Pending,
        "the tail of the plan stays pending"
    );
}

#[test]
fn a_dry_run_hit_on_a_statically_unreached_mutant_runs_it_and_reports_the_miss() {
    let spawner = FakeSpawner::new(script(
        |_| DryRunResult {
            hits: BTreeMap::from([
                (tid("t1"), vec!["m1".to_owned()]),
                (tid("t2"), vec!["m3".to_owned()]),
            ]),
            static_hits: vec!["m4".to_owned()],
            ..standard_dry()
        },
        |_, run| Ok(run_result(run, RunStatus::Killed, Some(&run.test_ids[0]))),
    ));
    let mut plan = plan(&["m1"]);
    plan.unreached = ["m3", "m4", "m5"]
        .into_iter()
        .map(|id| ScheduledMutant {
            id: id.to_owned(),
            file: "src/lazy.ts".to_owned(),
        })
        .collect();
    plan.known_ids.extend(["m4".to_owned(), "m5".to_owned()]);
    let outcome = run_schedule(&spawner, &plan, &config(1)).expect("run");
    let runs: BTreeMap<String, Vec<String>> = spawner
        .runs()
        .into_iter()
        .filter_map(|(_, run)| Some((run.mutant_id?, run.test_ids)))
        .collect();
    assert_eq!(
        runs.get("m3"),
        Some(&vec![tid("t2")]),
        "the dry-run map wins"
    );
    assert_eq!(
        runs.get("m4"),
        Some(&vec![tid("t1"), tid("t2")]),
        "a static hit with no static test file runs each passing dry-run test"
    );
    assert!(!runs.contains_key("m5"), "no hit, so no run");
    assert_eq!(status(&outcome, "m3"), MutantStatus::Killed);
    assert_eq!(status(&outcome, "m4"), MutantStatus::Killed);
    assert!(!outcome.results.contains_key("m5"));
    let missed: Vec<&str> = outcome
        .issues
        .iter()
        .filter(|issue| issue.code == "static-map-miss")
        .filter_map(|issue| issue.mutant_id.as_deref())
        .collect();
    assert_eq!(missed, vec!["m3", "m4"]);
    for issue in outcome
        .issues
        .iter()
        .filter(|issue| issue.code == "static-map-miss")
    {
        let id = issue.mutant_id.as_deref().unwrap_or_default();
        assert!(
            !issue.message.contains(&format!("`{id}`")),
            "the issue carries the id, so the message leaves it out: {}",
            issue.message
        );
    }
}

#[test]
fn without_a_test_filter_a_mutant_runs_each_passing_test_of_its_covering_files() {
    let caps = Capabilities {
        test_filter: false,
        ..VITEST_CAPS
    };
    let spawner = FakeSpawner::new(move |_, request| match request {
        Request::Init(_) => Ok(ready(caps, "5.0.4")),
        Request::DryRun { .. } => Ok(Response::DryRun(standard_dry())),
        Request::Run(run) => Ok(run_result(run, RunStatus::Survived, None)),
        Request::Reload => Ok(Response::Reloaded(Reloaded {
            method: ReloadMethod::Noop,
        })),
        Request::Shutdown => Err(LinkFault::Protocol("no response".to_owned())),
    });
    let outcome = run_schedule(&spawner, &plan(&["m1"]), &config(1)).expect("run");
    assert_eq!(
        spawner.runs()[0].1.test_ids,
        vec![tid("t1"), tid("t2")],
        "t1 alone hits m1, but the runner runs the whole file"
    );
    assert_eq!(status(&outcome, "m1"), MutantStatus::Survived);
}

#[test]
fn a_planned_test_file_that_the_runner_does_not_list_is_a_run_issue() {
    let spawner = FakeSpawner::new(script(
        |_| standard_dry(),
        |_, run| Ok(run_result(run, RunStatus::Killed, Some(&run.test_ids[0]))),
    ));
    let outcome = run_schedule(&spawner, &plan(&["m1"]), &config(1)).expect("run");
    let issues: Vec<(&str, &str)> = outcome
        .issues
        .iter()
        .map(|issue| (issue.code, issue.message.as_str()))
        .collect();
    assert_eq!(issues.len(), 1, "{issues:?}");
    assert_eq!(issues[0].0, "test-files-not-discovered");
    assert!(issues[0].1.contains("test/types.test-d.ts"), "{issues:?}");
    assert!(!issues[0].1.contains("test/a.test.ts"), "{issues:?}");
    assert_eq!(
        outcome.undiscovered_test_files,
        vec!["test/types.test-d.ts".to_owned()],
        "the completion reads the undiscovered files from the outcome"
    );
}

#[test]
fn undiscovered_issue_uses_the_singular_for_one_file_and_the_plural_for_more() {
    let one = undiscovered_issue(&["test/a.test-d.ts".to_owned()]).expect("one file");
    assert_eq!(
        one.message,
        "the runner does not list 1 static test file, so it did not run: test/a.test-d.ts"
    );
    let two = undiscovered_issue(&["test/a.test-d.ts".to_owned(), "test/b.test-d.ts".to_owned()])
        .expect("two files");
    assert_eq!(
        two.message,
        "the runner does not list 2 static test files, so they did not run: \
         test/a.test-d.ts, test/b.test-d.ts"
    );
    assert!(undiscovered_issue(&[]).is_none());
}

#[test]
fn no_covering_test_message_gives_a_count_and_the_first_test_only() {
    let many: Vec<String> = (1..=28)
        .map(|index| tid(&format!("case {index}")))
        .collect();
    assert_eq!(
        no_covering_test_message(&many),
        "no covering test passed in the dry run of the restarted companion \
         (28 tests, first: test/a.test.ts::case 1)"
    );
    let long = vec![format!("test/a.test.ts::{}", "x".repeat(200))];
    let message = no_covering_test_message(&long);
    assert!(message.starts_with("no covering test passed"), "{message}");
    assert!(message.ends_with("...)"), "{message}");
    assert!(message.chars().count() < 140, "{message}");
}

/// A covering test that fails in the dry run of a respawned companion is a
/// baseline failure of that companion. The mutant runs with the other
/// covering tests, and only a mutant with no covering test left is a
/// runtime error.
#[test]
fn a_covering_test_that_fails_in_a_respawned_dry_run_is_a_baseline_failure() {
    let dry = |index: usize| DryRunResult {
        tests: vec![
            test("t1", 2.0, TestStatus::Pass),
            test(
                "t2",
                3.0,
                if index == 0 {
                    TestStatus::Pass
                } else {
                    TestStatus::Fail
                },
            ),
        ],
        hits: BTreeMap::from([
            (tid("t1"), vec!["m1".to_owned(), "m2".to_owned()]),
            (tid("t2"), vec!["m2".to_owned(), "m3".to_owned()]),
        ]),
        static_hits: Vec::new(),
        file_hits: None,
        unhandled_errors: Vec::new(),
        unloaded_paths: Vec::new(),
        file_setup_ms: BTreeMap::new(),
    };
    let spawner = FakeSpawner::new(script(dry, |_, run| {
        match run.mutant_id.as_deref().unwrap_or_default() {
            "m1" => Err(LinkFault::Timeout),
            _ => Ok(run_result(run, RunStatus::Killed, Some(&run.test_ids[0]))),
        }
    }));
    let outcome = run_schedule(&spawner, &plan(&["m1", "m2", "m3"]), &config(1)).expect("run");

    assert_eq!(status(&outcome, "m1"), MutantStatus::Timeout);
    assert_eq!(status(&outcome, "m2"), MutantStatus::Killed);
    let m2_runs: Vec<Vec<String>> = spawner
        .runs()
        .into_iter()
        .filter(|(_, run)| run.mutant_id.as_deref() == Some("m2"))
        .map(|(_, run)| run.test_ids)
        .collect();
    assert_eq!(m2_runs, vec![vec![tid("t1")]], "m2 runs without t2");
    assert_eq!(
        outcome.results["m2"].covering_tests,
        vec![tid("t1"), tid("t2")],
        "the report keeps each covering test of the first dry run"
    );
    assert_eq!(status(&outcome, "m3"), MutantStatus::RuntimeError);
    let m3_issue = outcome
        .issues
        .iter()
        .find(|issue| issue.mutant_id.as_deref() == Some("m3"))
        .expect("m3 issue");
    assert_eq!(
        m3_issue.message,
        format!(
            "no covering test passed in the dry run of the restarted companion (1 test: {})",
            tid("t2")
        ),
        "the human footer adds the mutant id, so the message leaves it out"
    );
    assert_eq!(
        outcome.baseline_failures,
        vec![BaselineFailure {
            file: Some("test/a.test.ts".to_owned()),
            test_id: Some(tid("t2")),
            message: Some(
                "the test did not pass in the dry run of a restarted companion".to_owned()
            ),
            mutant_ids: vec!["m2".to_owned(), "m3".to_owned()],
        }]
    );
}

/// A survivor that runs without a dropped covering test survives. The run
/// requests only the tests that passed in the dry run of the restarted
/// companion, so the count check uses that list, not the first dry run. The
/// result cache does not store such a survivor.
#[test]
fn a_survivor_without_a_dropped_covering_test_is_not_a_runtime_error() {
    let dry = |index: usize| DryRunResult {
        tests: vec![
            test("t1", 2.0, TestStatus::Pass),
            test(
                "t2",
                3.0,
                if index == 0 {
                    TestStatus::Pass
                } else {
                    TestStatus::Fail
                },
            ),
        ],
        hits: BTreeMap::from([
            (tid("t1"), vec!["m3".to_owned(), "m2".to_owned()]),
            (tid("t2"), vec!["m2".to_owned()]),
        ]),
        static_hits: Vec::new(),
        file_hits: None,
        unhandled_errors: Vec::new(),
        unloaded_paths: Vec::new(),
        file_setup_ms: BTreeMap::new(),
    };
    let spawner = FakeSpawner::new(script(dry, |_, run| {
        match run.mutant_id.as_deref().unwrap_or_default() {
            "m3" => Err(LinkFault::Timeout),
            _ => Ok(run_result(run, RunStatus::Survived, None)),
        }
    }));
    let mut config = config(1);
    config.cache = Some(Arc::new(FakeCache::default()) as Arc<dyn ResultLookup>);
    let outcome = run_schedule(&spawner, &plan(&["m3", "m2"]), &config).expect("run");

    let m2_runs: Vec<Vec<String>> = spawner
        .runs()
        .into_iter()
        .filter(|(_, run)| run.mutant_id.as_deref() == Some("m2"))
        .map(|(_, run)| run.test_ids)
        .collect();
    assert!(
        m2_runs.iter().all(|tests| tests == &vec![tid("t1")]),
        "m2 runs without t2: {m2_runs:?}"
    );
    assert_eq!(
        status(&outcome, "m2"),
        MutantStatus::Survived,
        "{:?}",
        outcome.issues
    );
    assert!(
        outcome
            .issues
            .iter()
            .all(|issue| issue.mutant_id.as_deref() != Some("m2")),
        "{:?}",
        outcome.issues
    );
    assert!(
        !outcome.cache_keys.contains_key("m2"),
        "t2 did not run against m2, so the key over t1 and t2 must not store the survivor"
    );
}

/// A dropped test was not requested, so a result that names it as the
/// killer is not trusted.
#[test]
fn a_dropped_test_cannot_be_the_killer() {
    let dry = |index: usize| DryRunResult {
        tests: vec![
            test("t1", 2.0, TestStatus::Pass),
            test(
                "t2",
                3.0,
                if index == 0 {
                    TestStatus::Pass
                } else {
                    TestStatus::Fail
                },
            ),
        ],
        hits: BTreeMap::from([
            (tid("t1"), vec!["m3".to_owned(), "m2".to_owned()]),
            (tid("t2"), vec!["m2".to_owned()]),
        ]),
        static_hits: Vec::new(),
        file_hits: None,
        unhandled_errors: Vec::new(),
        unloaded_paths: Vec::new(),
        file_setup_ms: BTreeMap::new(),
    };
    let spawner = FakeSpawner::new(script(dry, |_, run| {
        match run.mutant_id.as_deref().unwrap_or_default() {
            "m3" => Err(LinkFault::Timeout),
            _ => Ok(run_result(run, RunStatus::Killed, Some(&tid("t2")))),
        }
    }));
    let outcome = run_schedule(&spawner, &plan(&["m3", "m2"]), &config(1)).expect("run");
    assert_eq!(status(&outcome, "m2"), MutantStatus::RuntimeError);
}

#[test]
fn a_setup_watchdog_fault_is_marked_as_a_timeout() {
    let timed_out = setup_error("dry_run", &LinkFault::Timeout);
    assert!(matches!(
        timed_out,
        ScheduleError::Setup {
            timed_out: true,
            ..
        }
    ));
    let protocol = setup_error("dry_run", &LinkFault::Protocol("bad line".to_owned()));
    assert!(matches!(
        protocol,
        ScheduleError::Setup {
            timed_out: false,
            ..
        }
    ));
}

#[test]
fn a_restarted_companion_setup_timeout_names_the_setup_timeout_flag() {
    let normal = script(|_| standard_dry(), |_, _| Err(LinkFault::Timeout));
    let spawner = FakeSpawner::new(move |index, request| match request {
        Request::Init(_) if index > 0 => Err(LinkFault::Timeout),
        _ => normal(index, request),
    });
    let outcome = run_schedule(&spawner, &plan(&["m1"]), &config(1)).expect("run");
    assert_eq!(status(&outcome, "m1"), MutantStatus::RuntimeError);
    let issue = outcome
        .issues
        .iter()
        .find(|issue| issue.mutant_id.as_deref() == Some("m1"))
        .expect("the m1 issue");
    assert!(
        issue.message.contains("--setup-timeout"),
        "{}",
        issue.message
    );
}

/// The query of one key: the mutant, the tests, the test files and the
/// runner version.
type KeyQuery = (String, Vec<String>, Vec<String>, String);

/// A cache that holds a killed result for `m1` and records each query.
#[derive(Debug, Default)]
struct FakeCache {
    queries: Mutex<Vec<KeyQuery>>,
}

impl ResultLookup for FakeCache {
    fn key(
        &self,
        ready: &Ready,
        mutant: &ScheduledMutant,
        tests: &[String],
        test_files: &[String],
    ) -> String {
        self.queries.lock().expect("queries").push((
            mutant.id.clone(),
            tests.to_vec(),
            test_files.to_vec(),
            ready.runner_version.clone(),
        ));
        format!("key-{}", mutant.id)
    }

    fn get(&self, mutant_id: &str, key: &str) -> Option<CachedResult> {
        (mutant_id == "m1" && key == "key-m1").then(|| CachedResult {
            status: MutantStatus::Killed,
            killed_by: Some(tid("t1")),
            duration_ms: Some(3),
        })
    }
}

#[test]
fn a_cache_hit_skips_the_mutant_run_and_a_miss_reports_its_key() {
    let spawner = FakeSpawner::new(script(
        |_| standard_dry(),
        |_, run| Ok(run_result(run, RunStatus::Killed, Some(&run.test_ids[0]))),
    ));
    let cache = Arc::new(FakeCache::default());
    let mut config = config(1);
    config.cache = Some(Arc::clone(&cache) as Arc<dyn ResultLookup>);
    let outcome = run_schedule(&spawner, &plan(&["m1", "m2", "m3"]), &config).expect("run");

    let ran: Vec<String> = spawner
        .runs()
        .into_iter()
        .filter_map(|(_, run)| run.mutant_id)
        .collect();
    assert_eq!(ran, vec!["m2".to_owned()], "the cached mutant does not run");
    let m1 = &outcome.results["m1"];
    assert!(m1.cached);
    assert_eq!(m1.status, MutantStatus::Killed);
    assert_eq!(m1.killed_by, Some(tid("t1")));
    assert_eq!(m1.duration_ms, Some(3));
    assert_eq!(m1.covering_tests, vec![tid("t1")]);
    assert!(!outcome.results["m2"].cached);
    assert!(
        !outcome.results["m3"].cached,
        "no_coverage never asks the cache"
    );
    assert_eq!(
        outcome.cache_keys,
        FxHashMap::from_iter([("m2".to_owned(), "key-m2".to_owned())]),
        "only an executed mutant reports its key"
    );
    let queries = cache.queries.lock().expect("queries").clone();
    assert_eq!(
        queries,
        vec![
            (
                "m1".to_owned(),
                vec![tid("t1")],
                vec!["test/a.test.ts".to_owned()],
                "5.0.4".to_owned()
            ),
            (
                "m2".to_owned(),
                vec![tid("t1"), tid("t2")],
                vec!["test/a.test.ts".to_owned()],
                "5.0.4".to_owned()
            ),
        ],
        "the key covers the covering tests and their files"
    );
}

#[test]
fn a_cached_result_uses_a_place_in_the_mutant_limit() {
    let spawner = FakeSpawner::new(script(
        |_| standard_dry(),
        |_, run| Ok(run_result(run, RunStatus::Killed, Some(&run.test_ids[0]))),
    ));
    // The limit is one mutant. The planned mutant `m1` is covered and has a
    // cached result, so it fills the limit without a run.
    let mut plan = plan(&["m1"]);
    plan.pending = vec![ScheduledMutant {
        id: "m2".to_owned(),
        file: "src/a.ts".to_owned(),
    }];
    let mut config = config(1);
    config.cache = Some(Arc::new(FakeCache::default()) as Arc<dyn ResultLookup>);
    let outcome = run_schedule(&spawner, &plan, &config).expect("run");

    assert!(
        spawner.runs().is_empty(),
        "the cached mutant fills the limit"
    );
    assert!(outcome.results["m1"].cached);
    assert!(
        !outcome.results.contains_key("m2"),
        "a covered pending mutant after the filled limit stays pending"
    );
    assert!(outcome.promoted.is_empty());
}

#[test]
fn without_a_cache_each_covered_mutant_runs_and_no_key_is_reported() {
    let spawner = FakeSpawner::new(script(
        |_| standard_dry(),
        |_, run| Ok(run_result(run, RunStatus::Killed, Some(&run.test_ids[0]))),
    ));
    let outcome = run_schedule(&spawner, &plan(&["m1", "m2"]), &config(1)).expect("run");
    assert_eq!(spawner.runs().len(), 2);
    assert!(outcome.cache_keys.is_empty());
    assert!(outcome.results.values().all(|result| !result.cached));
}

/// A run result with no executed test and an error outside each test: the
/// shape of a mutant that throws at module load, and of a run path that
/// executes no test.
fn load_failure(run: &RunRequest) -> Response {
    Response::Run(RunResult {
        mutant_id: run.mutant_id.clone(),
        status: RunStatus::Killed,
        killed_by: None,
        duration_ms: Some(4.0),
        tests_executed: 0,
        tests: Vec::new(),
        unhandled_errors: vec![UnhandledError {
            message: "createSpecification failed\n    at stack".to_owned(),
            file: None,
        }],
    })
}

/// t1 hits m1, m2 and m3. t2 hits m2 and m3. So m1 runs t1, and m2 and m3
/// run t1 and t2.
fn shared_set_dry() -> DryRunResult {
    DryRunResult {
        hits: BTreeMap::from([
            (
                tid("t1"),
                vec!["m1".to_owned(), "m2".to_owned(), "m3".to_owned()],
            ),
            (tid("t2"), vec!["m2".to_owned(), "m3".to_owned()]),
        ]),
        ..standard_dry()
    }
}

fn sorted(mut tests: Vec<String>) -> Vec<String> {
    tests.sort();
    tests
}

/// m1 is killed by t1. Each other mutant breaks the module load.
fn m1_killed_others_fail_at_load(run: &RunRequest) -> Response {
    match run.mutant_id.as_deref().unwrap_or_default() {
        "m1" => run_result(run, RunStatus::Killed, Some(&tid("t1"))),
        _ => load_failure(run),
    }
}

#[test]
fn the_self_check_stops_a_run_whose_run_path_executes_no_tests() {
    // The run path of the runner fails each run before a test starts, as
    // Vitest 4.0 did. Each mutant run would look killed.
    let spawner = FakeSpawner::new(script(|_| standard_dry(), |_, run| Ok(load_failure(run))))
        .with_control(|_, run| Ok(load_failure(run)));
    let error = run_schedule(&spawner, &plan(&["m1", "m2"]), &config(2)).unwrap_err();
    let ScheduleError::RunPath { message, help } = &error else {
        panic!("expected a run-path error, got {error:?}");
    };
    assert!(
        message.starts_with("vitest 5.0.4 executes no tests in a mutant run"),
        "{message}"
    );
    assert!(
        message.contains("0 of 1 requested tests ran; first error: createSpecification failed"),
        "{message}"
    );
    assert!(help.contains("report this at"), "{help}");
    assert!(help.contains("(vitest 5.0.4)"), "{help}");
    assert!(
        spawner.runs().is_empty(),
        "no mutant runs after the self-check fails"
    );
    let controls = spawner.controls();
    assert_eq!(controls.len(), 1, "the self-check is one run");
    assert_eq!(controls[0].0, 0, "the first companion runs the self-check");
    assert_eq!(
        controls[0].1.test_ids,
        vec![tid("t1")],
        "the tests of the first job"
    );
}

/// A control run in which `tests/a.test.ts::t2` fails and each other test
/// passes.
fn t2_fails_in_control(run: &RunRequest) -> Response {
    let tests: Vec<RunTest> = run
        .test_ids
        .iter()
        .map(|test| RunTest {
            test_id: test.clone(),
            status: if *test == tid("t2") {
                TestStatus::Fail
            } else {
                TestStatus::Pass
            },
            failure_kind: None,
        })
        .collect();
    let failed = run.test_ids.contains(&tid("t2"));
    Response::Run(RunResult {
        mutant_id: None,
        status: if failed {
            RunStatus::Killed
        } else {
            RunStatus::Survived
        },
        killed_by: failed.then(|| tid("t2")),
        duration_ms: Some(4.0),
        tests_executed: tests.len(),
        tests,
        unhandled_errors: Vec::new(),
    })
}

/// t1 hits m1, and t2 hits m2 only.
fn split_dry() -> DryRunResult {
    DryRunResult {
        hits: BTreeMap::from([
            (tid("t1"), vec!["m1".to_owned()]),
            (tid("t2"), vec!["m2".to_owned()]),
        ]),
        ..standard_dry()
    }
}

#[test]
fn a_test_that_fails_in_the_self_check_is_a_baseline_failure_and_the_run_continues() {
    // m2 is the first job, and its only covering test t2 fails in the
    // self-check, for example because it depends on another test of its
    // file. t2 covers no mutant, so m2 is `no_coverage`, and m1 runs.
    let spawner = FakeSpawner::new(script(
        |_| split_dry(),
        |_, run| Ok(run_result(run, RunStatus::Killed, Some(&run.test_ids[0]))),
    ))
    .with_control(|_, run| Ok(t2_fails_in_control(run)));
    let outcome = run_schedule(&spawner, &plan(&["m2", "m1"]), &config(1)).expect("run");
    assert_eq!(status(&outcome, "m2"), MutantStatus::NoCoverage);
    assert_eq!(status(&outcome, "m1"), MutantStatus::Killed);
    assert_eq!(
        outcome.baseline_failures,
        vec![BaselineFailure {
            file: Some("test/a.test.ts".to_owned()),
            test_id: Some(tid("t2")),
            message: Some(CONTROL_RUN_FAILURE.to_owned()),
            mutant_ids: Vec::new(),
        }]
    );
    let runs: Vec<(String, Vec<String>)> = spawner
        .runs()
        .into_iter()
        .filter_map(|(_, run)| Some((run.mutant_id?, run.test_ids)))
        .collect();
    assert_eq!(runs, vec![("m1".to_owned(), vec![tid("t1")])]);
    assert_eq!(spawner.controls().len(), 1, "the self-check is one run");

    // m2 of the standard dry run has t1 and t2. It runs with t1 only.
    let spawner = FakeSpawner::new(script(
        |_| standard_dry(),
        |_, run| Ok(run_result(run, RunStatus::Killed, Some(&run.test_ids[0]))),
    ))
    .with_control(|_, run| Ok(t2_fails_in_control(run)));
    let outcome = run_schedule(&spawner, &plan(&["m2"]), &config(1)).expect("run");
    assert_eq!(status(&outcome, "m2"), MutantStatus::Killed);
    assert_eq!(outcome.results["m2"].covering_tests, vec![tid("t1")]);
    assert_eq!(spawner.runs()[0].1.test_ids, vec![tid("t1")]);
    assert_eq!(outcome.baseline_failures.len(), 1);
}

#[test]
fn the_self_check_stops_a_run_whose_result_has_an_error_but_no_failed_test() {
    let spawner = FakeSpawner::new(script(
        |_| standard_dry(),
        |_, run| Ok(run_result(run, RunStatus::Killed, Some(&run.test_ids[0]))),
    ))
    .with_control(|_, run| {
        let Response::Run(mut result) = run_result(run, RunStatus::Survived, None) else {
            unreachable!("a run result");
        };
        result.status = RunStatus::Killed;
        result.unhandled_errors = vec![UnhandledError {
            message: "worker crashed".to_owned(),
            file: None,
        }];
        Ok(Response::Run(result))
    });
    let error = run_schedule(&spawner, &plan(&["m2"]), &config(1)).unwrap_err();
    let ScheduleError::RunPath { message, .. } = &error else {
        panic!("expected a run-path error, got {error:?}");
    };
    assert!(
        message.starts_with("vitest 5.0.4 gives no trustworthy result in a mutant run"),
        "{message}"
    );
    assert!(message.contains("worker crashed"), "{message}");
    assert!(spawner.runs().is_empty());
}

#[test]
fn a_passing_self_check_runs_the_tests_of_the_first_job_once_with_no_mutant() {
    let spawner = FakeSpawner::new(script(
        |_| standard_dry(),
        |_, run| Ok(run_result(run, RunStatus::Killed, Some(&run.test_ids[0]))),
    ));
    let outcome = run_schedule(&spawner, &plan(&["m1", "m2", "m3"]), &config(1)).expect("run");
    assert_eq!(status(&outcome, "m1"), MutantStatus::Killed);
    assert_eq!(status(&outcome, "m2"), MutantStatus::Killed);
    let controls = spawner.controls();
    assert_eq!(controls.len(), 1, "{controls:?}");
    let control = &controls[0].1;
    assert_eq!(control.mutant_id, None);
    assert_eq!(control.test_ids, vec![tid("t1")]);
    assert!(!control.bail, "a control run names each failed test");
}

#[test]
fn the_self_check_needs_no_run_without_a_covered_mutant() {
    let spawner = FakeSpawner::new(script(|_| standard_dry(), |_, run| Ok(load_failure(run))))
        .with_control(|_, run| Ok(load_failure(run)));
    let outcome = run_schedule(&spawner, &plan(&["m3"]), &config(1)).expect("run");
    assert_eq!(status(&outcome, "m3"), MutantStatus::NoCoverage);
    assert!(spawner.controls().is_empty());
}

#[test]
fn a_mutant_that_throws_at_module_load_stays_killed_when_its_tests_pass_without_it() {
    // m2 and m3 break the module load. Their tests (t1, t2) pass in a
    // control run with no active mutant, so each kill stands.
    let spawner = FakeSpawner::new(script(
        |_| shared_set_dry(),
        |_, run| Ok(m1_killed_others_fail_at_load(run)),
    ));
    let outcome = run_schedule(&spawner, &plan(&["m1", "m2", "m3"]), &config(1)).expect("run");
    for id in ["m2", "m3"] {
        assert_eq!(status(&outcome, id), MutantStatus::Killed, "{id}");
        assert_eq!(outcome.results[id].killed_by, None, "{id}");
    }
    let controls: Vec<Vec<String>> = spawner
        .controls()
        .into_iter()
        .map(|(_, run)| sorted(run.test_ids))
        .collect();
    assert_eq!(
        controls,
        vec![vec![tid("t1")], vec![tid("t1"), tid("t2")]],
        "the self-check, then one control run for the shared test set of m2 and m3"
    );
    assert!(
        outcome.issues.iter().all(|issue| issue.mutant_id.is_none()),
        "{:?}",
        outcome.issues
    );
}

#[test]
fn a_zero_test_kill_whose_tests_fail_without_the_mutant_is_a_runtime_error() {
    // The self-check of t1 passes. m2 and m3 break the module load. The
    // control run of their tests (t1, t2) fails t2, so the kill of m2 is no
    // evidence: m2 is `runtime_error`, with no retry. t2 is a baseline
    // failure for the rest of the run, so m3 runs with t1 only. t1 passes
    // without the mutant, so the kill of m3 stands.
    let spawner = FakeSpawner::new(script(
        |_| shared_set_dry(),
        |_, run| Ok(m1_killed_others_fail_at_load(run)),
    ))
    .with_control(|_, run| Ok(t2_fails_in_control(run)));
    let outcome = run_schedule(&spawner, &plan(&["m1", "m2", "m3"]), &config(1)).expect("run");
    assert_eq!(status(&outcome, "m1"), MutantStatus::Killed);
    assert_eq!(status(&outcome, "m2"), MutantStatus::RuntimeError);
    assert_eq!(status(&outcome, "m3"), MutantStatus::Killed);
    let issue = outcome
        .issues
        .iter()
        .find(|issue| issue.mutant_id.as_deref() == Some("m2"))
        .expect("an issue");
    assert_eq!(issue.code, "runtime-error");
    assert!(
        issue
            .message
            .contains("the same tests do not pass in a control run with no active mutant"),
        "{}",
        issue.message
    );
    let m2_runs = spawner
        .runs()
        .iter()
        .filter(|(_, run)| run.mutant_id.as_deref() == Some("m2"))
        .count();
    assert_eq!(m2_runs, 1, "a failed control run decides; no retry");
    let m3_tests: Vec<Vec<String>> = spawner
        .runs()
        .into_iter()
        .filter(|(_, run)| run.mutant_id.as_deref() == Some("m3"))
        .map(|(_, run)| run.test_ids)
        .collect();
    assert_eq!(m3_tests, vec![vec![tid("t1")]]);
    assert_eq!(
        outcome.baseline_failures,
        vec![BaselineFailure {
            file: Some("test/a.test.ts".to_owned()),
            test_id: Some(tid("t2")),
            message: Some(CONTROL_RUN_FAILURE.to_owned()),
            mutant_ids: vec!["m3".to_owned()],
        }]
    );
    let controls: Vec<Vec<String>> = spawner
        .controls()
        .into_iter()
        .map(|(_, run)| sorted(run.test_ids))
        .collect();
    assert_eq!(
        controls,
        vec![vec![tid("t1")], vec![tid("t1"), tid("t2")]],
        "the memo answers the control of m3 (t1) from the self-check"
    );
}

#[test]
fn a_zero_test_kill_after_the_time_budget_stays_pending_without_a_control_run() {
    let spawner = FakeSpawner::new(script(
        |_| shared_set_dry(),
        |_, run| {
            if run.mutant_id.as_deref() == Some("m2") {
                std::thread::sleep(Duration::from_millis(400));
            }
            Ok(m1_killed_others_fail_at_load(run))
        },
    ));
    let mut config = config(1);
    config.deadline = Some(Instant::now() + Duration::from_millis(250));
    let outcome = run_schedule(&spawner, &plan(&["m1", "m2"]), &config).expect("run");
    assert_eq!(status(&outcome, "m1"), MutantStatus::Killed);
    assert_eq!(status(&outcome, "m2"), MutantStatus::Pending);
    assert!(
        spawner
            .controls()
            .iter()
            .all(|(_, run)| run.test_ids.len() == 1),
        "no control run starts after the deadline"
    );
}

#[test]
fn a_timeout_of_the_self_check_is_reported_as_a_timeout_not_as_a_broken_run_path() {
    type Control = fn(&RunRequest) -> Result<Response, LinkFault>;
    let cases: Vec<(&str, Control)> = vec![
        ("a timeout result with no executed test", |_| {
            Ok(Response::Run(RunResult {
                mutant_id: None,
                status: RunStatus::Timeout,
                killed_by: None,
                duration_ms: Some(6000.0),
                tests_executed: 0,
                tests: Vec::new(),
                unhandled_errors: Vec::new(),
            }))
        }),
        ("a watchdog timeout", |_| Err(LinkFault::Timeout)),
    ];
    for (name, control) in cases {
        let spawner = FakeSpawner::new(script(
            |_| standard_dry(),
            |_, run| Ok(run_result(run, RunStatus::Killed, Some(&run.test_ids[0]))),
        ))
        .with_control(move |_, run| control(run));
        let error = run_schedule(&spawner, &plan(&["m1"]), &config(1)).unwrap_err();
        let ScheduleError::RunPath { message, help } = &error else {
            panic!("{name}: expected a run-path error, got {error:?}");
        };
        assert!(
            message.starts_with("the self-check of vitest 5.0.4 timed out"),
            "{name}: {message}"
        );
        assert!(!message.contains("executes no tests"), "{name}: {message}");
        assert!(help.contains("--setup-timeout"), "{name}: {help}");
        assert!(spawner.runs().is_empty(), "{name}");
    }
}

#[test]
fn a_control_run_waits_at_least_the_setup_timeout() {
    let spawner = FakeSpawner::new(script(
        |_| standard_dry(),
        |_, run| Ok(run_result(run, RunStatus::Killed, Some(&run.test_ids[0]))),
    ));
    let mut config = config(1);
    config.runner_timeout = Duration::from_secs(60);
    run_schedule(&spawner, &plan(&["m1"]), &config).expect("run");
    let controls = spawner.controls();
    assert_eq!(
        controls[0].1.timeout_ms, 60_000,
        "the setup timeout, not the 5003 ms of the mutant job"
    );
}

/// t1 hits m1. t2 and t3 hit m2 and m3.
fn three_test_dry() -> DryRunResult {
    let mut dry = standard_dry();
    dry.tests.push(test("t3", 150.0, TestStatus::Pass));
    dry.hits = BTreeMap::from([
        (tid("t1"), vec!["m1".to_owned()]),
        (tid("t2"), vec!["m2".to_owned(), "m3".to_owned()]),
        (tid("t3"), vec!["m2".to_owned(), "m3".to_owned()]),
    ]);
    dry
}

/// A control run in which each test fails, except t1.
fn only_t1_passes_in_control(run: &RunRequest) -> Response {
    let tests: Vec<RunTest> = run
        .test_ids
        .iter()
        .map(|test| RunTest {
            test_id: test.clone(),
            status: if *test == tid("t1") {
                TestStatus::Pass
            } else {
                TestStatus::Fail
            },
            failure_kind: None,
        })
        .collect();
    let failed = tests.iter().any(|test| test.status == TestStatus::Fail);
    Response::Run(RunResult {
        mutant_id: None,
        status: if failed {
            RunStatus::Killed
        } else {
            RunStatus::Survived
        },
        killed_by: None,
        duration_ms: Some(4.0),
        tests_executed: tests.len(),
        tests,
        unhandled_errors: Vec::new(),
    })
}

#[test]
fn a_mutant_whose_covering_tests_all_fail_in_a_mid_run_control_is_no_coverage_without_a_retry() {
    // The self-check of t1 passes. m2 breaks the module load, and its
    // control run fails t2 and t3: m2 is `runtime_error`. m3 has only t2
    // and t3, so it is `no_coverage`, the same as after a self-check drop,
    // and it never runs.
    let spawner = FakeSpawner::new(script(
        |_| three_test_dry(),
        |_, run| Ok(m1_killed_others_fail_at_load(run)),
    ))
    .with_control(|_, run| Ok(only_t1_passes_in_control(run)));
    let outcome = run_schedule(&spawner, &plan(&["m1", "m2", "m3"]), &config(1)).expect("run");
    assert_eq!(status(&outcome, "m1"), MutantStatus::Killed);
    assert_eq!(status(&outcome, "m2"), MutantStatus::RuntimeError);
    assert_eq!(status(&outcome, "m3"), MutantStatus::NoCoverage);
    assert!(outcome.results["m3"].covering_tests.is_empty());
    assert!(
        outcome
            .issues
            .iter()
            .all(|issue| issue.mutant_id.as_deref() != Some("m3")),
        "{:?}",
        outcome.issues
    );
    let ran: Vec<String> = spawner
        .runs()
        .into_iter()
        .filter_map(|(_, run)| run.mutant_id)
        .collect();
    assert_eq!(
        ran,
        vec!["m1".to_owned(), "m2".to_owned()],
        "no retry, no m3 run"
    );
    let failures: Vec<(Option<String>, Vec<String>)> = outcome
        .baseline_failures
        .iter()
        .map(|failure| (failure.test_id.clone(), failure.mutant_ids.clone()))
        .collect();
    assert_eq!(
        failures,
        vec![
            (Some(tid("t2")), vec!["m3".to_owned()]),
            (Some(tid("t3")), vec!["m3".to_owned()]),
        ]
    );
}

fn load_kill_job() -> Job {
    Job {
        id: "m2".to_owned(),
        tests: vec![tid("t1"), tid("t2")],
        timeout_ms: 6000,
        setup_ms: 0,
        static_mutant: false,
        test_files: vec!["test/a.test.ts".to_owned()],
        cache_key: None,
    }
}

fn memo_value(memo: &ControlMemo, tests: &[String]) -> Option<bool> {
    memo.cell(tests)
        .lock()
        .expect("cell")
        .as_ref()
        .map(|verdict| *verdict == ControlVerdict::Passed)
}

#[test]
fn a_load_kill_control_keys_the_memo_on_the_tests_that_are_not_marked_failed() {
    // Another worker marked t2 as failed after m2 ran with t1 and t2.
    let spawner = FakeSpawner::new(script(|_| standard_dry(), |_, run| Ok(load_failure(run))));
    let config = config(1);
    let memo = ControlMemo::default();
    memo.mark_failed(&[tid("t2")]);
    let dry_files = vec!["test/a.test.ts".to_owned()];
    let mut worker = Worker::new(&spawner, &config, &dry_files, Duration::from_secs(5), &memo);
    worker.link = Some(spawner.spawn().expect("link"));
    worker.known_tests = [tid("t1"), tid("t2")].into_iter().collect();
    let job = load_kill_job();
    let attempt = Attempt::decided(MutantStatus::Killed, None, Some(4));
    let confirmed = worker
        .confirm_load_kill(&job, &job.tests, VITEST_CAPS, attempt)
        .expect("confirm");
    assert_eq!(confirmed.status, MutantStatus::Killed);
    assert_eq!(spawner.controls()[0].1.test_ids, vec![tid("t1")]);
    assert_eq!(memo_value(&memo, &[tid("t1")]), Some(true));
    assert_eq!(
        memo_value(&memo, &[tid("t1"), tid("t2")]),
        None,
        "the unfiltered set has no control result"
    );

    // When each test is marked failed, the mutant has no covering test.
    memo.mark_failed(&[tid("t1")]);
    let attempt = Attempt::decided(MutantStatus::Killed, None, Some(4));
    let uncovered = worker
        .confirm_load_kill(&job, &job.tests, VITEST_CAPS, attempt)
        .expect("confirm");
    assert_eq!(uncovered.status, MutantStatus::NoCoverage);
    assert!(uncovered.settled);
    assert_eq!(spawner.controls().len(), 1, "no control run without a test");
}

#[test]
fn a_self_check_drop_inside_the_mutant_limit_gives_the_place_to_a_covered_pending_mutant() {
    // The limit is one mutant: m2. Its only test t2 fails in the self-check,
    // so m2 is `no_coverage`. The jobs are built again, and the pending m1
    // takes the free place.
    let spawner = FakeSpawner::new(script(
        |_| split_dry(),
        |_, run| Ok(run_result(run, RunStatus::Killed, Some(&run.test_ids[0]))),
    ))
    .with_control(|_, run| Ok(t2_fails_in_control(run)));
    let mut plan = plan(&["m2"]);
    plan.pending = vec![ScheduledMutant {
        id: "m1".to_owned(),
        file: "src/a.ts".to_owned(),
    }];
    let outcome = run_schedule(&spawner, &plan, &config(1)).expect("run");
    assert_eq!(status(&outcome, "m2"), MutantStatus::NoCoverage);
    assert_eq!(status(&outcome, "m1"), MutantStatus::Killed);
    assert_eq!(outcome.promoted, vec!["m1".to_owned()]);
}

#[test]
fn a_hanging_control_set_costs_one_control_run_for_each_load_kill_mutant_with_it() {
    // The self-check of t1 passes. m2 and m3 break the module load, and the
    // control run of their tests (t1, t2) times out. The timeout is stored:
    // m3 gets the same reason at once, and neither mutant retries into a
    // second control run.
    let spawner = FakeSpawner::new(script(
        |_| shared_set_dry(),
        |_, run| Ok(m1_killed_others_fail_at_load(run)),
    ))
    .with_control(|_, run| {
        if run.test_ids.len() == 1 {
            Ok(run_result(run, RunStatus::Survived, None))
        } else {
            Err(LinkFault::Timeout)
        }
    });
    let outcome = run_schedule(&spawner, &plan(&["m1", "m2", "m3"]), &config(1)).expect("run");
    assert_eq!(status(&outcome, "m1"), MutantStatus::Killed);
    let mut reasons = Vec::new();
    for id in ["m2", "m3"] {
        assert_eq!(status(&outcome, id), MutantStatus::RuntimeError, "{id}");
        let issue = outcome
            .issues
            .iter()
            .find(|issue| issue.mutant_id.as_deref() == Some(id))
            .expect("an issue");
        assert!(
            issue.message.contains("the control run timed out"),
            "{}",
            issue.message
        );
        reasons.push(issue.message.clone());
    }
    assert_eq!(reasons[0], reasons[1], "the same reason");
    let set_controls = spawner
        .controls()
        .iter()
        .filter(|(_, run)| run.test_ids.len() == 2)
        .count();
    assert_eq!(set_controls, 1, "one control run for the hanging set");
    let mutant_runs = spawner
        .runs()
        .iter()
        .filter(|(_, run)| run.mutant_id.as_deref() != Some("m1"))
        .count();
    assert_eq!(mutant_runs, 2, "no retry of m2 or m3");
}

/// t1 in `test/a.test.ts` hits m1. t2 in `test/b.test.ts` hits nothing.
/// `src/a.ts` (m1, m_a) loads at import time in `test/a.test.ts` only, and
/// `src/b.ts` (m_b) in `test/b.test.ts` only.
fn two_file_dry() -> DryRunResult {
    DryRunResult {
        tests: vec![
            test("t1", 2.0, TestStatus::Pass),
            DryTest {
                test_id: "test/b.test.ts::t2".to_owned(),
                file: "test/b.test.ts".to_owned(),
                duration_ms: Some(3.0),
                status: TestStatus::Pass,
            },
        ],
        hits: BTreeMap::from([(tid("t1"), vec!["m1".to_owned()])]),
        static_hits: vec!["m_a".to_owned(), "m_b".to_owned()],
        ..standard_dry()
    }
}

fn two_file_plan(mutants: &[(&str, &str)]) -> SchedulePlan {
    let mut plan = plan(&[]);
    plan.mutants = mutants
        .iter()
        .map(|(id, file)| ScheduledMutant {
            id: (*id).to_owned(),
            file: (*file).to_owned(),
        })
        .collect();
    plan.known_ids.extend(["m_a", "m_b"].map(str::to_owned));
    plan.test_files = vec!["test/a.test.ts".to_owned(), "test/b.test.ts".to_owned()];
    plan.file_test_files = BTreeMap::from([
        ("src/a.ts".to_owned(), vec!["test/a.test.ts".to_owned()]),
        ("src/b.ts".to_owned(), vec!["test/b.test.ts".to_owned()]),
    ]);
    plan
}

fn dry_runs_of(spawner: &FakeSpawner) -> Vec<(usize, Vec<String>)> {
    spawner
        .log
        .lock()
        .expect("log")
        .iter()
        .filter_map(|(index, request)| match request {
            Request::DryRun { test_files } => Some((*index, test_files.clone())),
            _ => None,
        })
        .collect()
}

#[test]
fn a_companion_restarted_for_a_static_mutant_dry_runs_only_the_files_of_the_mutant() {
    let caps = Capabilities {
        fresh_modules_per_run: false,
        ..VITEST_CAPS
    };
    let spawner = FakeSpawner::new(move |_, request| match request {
        Request::Init(_) => Ok(ready(caps, "4.1.11")),
        Request::DryRun { .. } => Ok(Response::DryRun(two_file_dry())),
        Request::Run(run) => Ok(run_result(run, RunStatus::Killed, Some(&run.test_ids[0]))),
        Request::Reload => Ok(Response::Reloaded(Reloaded {
            method: ReloadMethod::RestartRequired,
        })),
        Request::Shutdown => Err(LinkFault::Protocol("no response".to_owned())),
    });
    let plan = two_file_plan(&[("m1", "src/a.ts"), ("m_a", "src/a.ts")]);
    let outcome = run_schedule(&spawner, &plan, &config(1)).expect("run");
    assert_eq!(status(&outcome, "m1"), MutantStatus::Killed);
    assert_eq!(status(&outcome, "m_a"), MutantStatus::Killed);
    let both = vec!["test/a.test.ts".to_owned(), "test/b.test.ts".to_owned()];
    assert_eq!(
        dry_runs_of(&spawner),
        vec![(0, both), (1, vec!["test/a.test.ts".to_owned()])],
        "the first companion dry-runs each file, the restarted one only the files of m_a"
    );
    let runs = spawner.runs();
    assert_eq!(runs[1].0, 1);
    assert_eq!(runs[1].1.test_ids, vec![tid("t1")]);
}

#[test]
fn a_kept_companion_with_a_narrow_dry_run_restarts_for_tests_outside_it() {
    // The reload restarts only the first companion. Later companions keep
    // their narrow dry run, and a job with other test files needs a new one.
    let caps = Capabilities {
        fresh_modules_per_run: false,
        ..VITEST_CAPS
    };
    let spawner = FakeSpawner::new(move |index, request| match request {
        Request::Init(_) => Ok(ready(caps, "4.1.11")),
        Request::DryRun { .. } => Ok(Response::DryRun(two_file_dry())),
        Request::Run(run) => Ok(run_result(run, RunStatus::Killed, Some(&run.test_ids[0]))),
        Request::Reload => Ok(Response::Reloaded(Reloaded {
            method: if index == 0 {
                ReloadMethod::RestartRequired
            } else {
                ReloadMethod::Invalidate
            },
        })),
        Request::Shutdown => Err(LinkFault::Protocol("no response".to_owned())),
    });
    let plan = two_file_plan(&[("m1", "src/a.ts"), ("m_a", "src/a.ts"), ("m_b", "src/b.ts")]);
    let outcome = run_schedule(&spawner, &plan, &config(1)).expect("run");
    for id in ["m1", "m_a", "m_b"] {
        assert_eq!(status(&outcome, id), MutantStatus::Killed, "{id}");
    }
    assert_eq!(
        dry_runs_of(&spawner),
        vec![
            (
                0,
                vec!["test/a.test.ts".to_owned(), "test/b.test.ts".to_owned()]
            ),
            (1, vec!["test/a.test.ts".to_owned()]),
            (2, vec!["test/b.test.ts".to_owned()]),
        ]
    );
    let runs = spawner.runs();
    assert_eq!(runs[2].0, 2);
    assert_eq!(runs[2].1.test_ids, vec!["test/b.test.ts::t2".to_owned()]);
}
