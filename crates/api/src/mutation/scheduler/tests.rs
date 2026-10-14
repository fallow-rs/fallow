use std::sync::atomic::AtomicUsize;
use std::sync::{Arc, Mutex};

use super::super::protocol::{DryTest, Reloaded, RemoteError, RunTest, UnhandledError};
use super::*;

type Script = dyn Fn(usize, &Request) -> Result<Response, LinkFault> + Send + Sync;

struct FakeLink {
    index: usize,
    script: Arc<Script>,
    log: Arc<Mutex<Vec<(usize, Request)>>>,
    waits: Arc<Mutex<Vec<Duration>>>,
}

impl CompanionLink for FakeLink {
    fn request(&mut self, request: &Request, timeout: Duration) -> Result<Response, LinkFault> {
        if let Ok(mut log) = self.log.lock() {
            log.push((self.index, request.clone()));
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
        }
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
            })
            .collect(),
        RunStatus::Killed => killed_by
            .map(|test| RunTest {
                test_id: test.to_owned(),
                status: TestStatus::Fail,
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
    }
}

fn status(outcome: &ScheduleOutcome, id: &str) -> MutantStatus {
    outcome.results[id].status
}

#[test]
fn killed_survived_and_no_coverage_follow_the_dry_run_hits() {
    let spawner = FakeSpawner::new(script(
        |_| standard_dry(),
        |_, run| match run.mutant_id.as_str() {
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
        |_, run| match run.mutant_id.as_str() {
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
            Duration::from_millis(5003 + 12_000 + 2000),
        ],
        "timeout_ms + ceil(1.5 * 8000) + 2000, also for the m1 retry"
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
                    },
                    RunTest {
                        test_id: run.test_ids[1].clone(),
                        status: TestStatus::Fail,
                    },
                ],
                unhandled_errors: Vec::new(),
            })
        }),
        ("another mutant", |run| {
            let mut other = run.clone();
            other.mutant_id = "m9".to_owned();
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
        |_, run| match run.mutant_id.as_str() {
            "m1" => Err(LinkFault::Timeout),
            _ => Ok(run_result(run, RunStatus::Killed, Some(&tid("t1")))),
        },
    ));
    let outcome = run_schedule(&spawner, &plan(&["m1", "m2"]), &config(1)).expect("run");
    assert_eq!(status(&outcome, "m1"), MutantStatus::Timeout);
    assert_eq!(status(&outcome, "m2"), MutantStatus::Killed);
    let runs = spawner.runs();
    assert_eq!(
        runs.iter()
            .map(|(index, run)| (*index, run.mutant_id.as_str()))
            .collect::<Vec<_>>(),
        vec![(0, "m1"), (1, "m2"), (2, "m1")],
        "m2 and the m1 retry each get a new companion"
    );
    assert_eq!(spawner.requests_of(1)[..3], ["init", "dry_run", "run"]);
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
            .map(|run| run.1.mutant_id.as_str())
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
        .map(|(_, run)| run.mutant_id)
        .collect();
    assert_eq!(ran, vec!["m1".to_owned()], "a pending mutant never runs");
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
        .map(|run| run.1.mutant_id)
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
    assert_eq!(outcome.unretried, vec!["m1".to_owned()]);
    assert_eq!(spawner.runs().len(), 1);
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
        .map(|(_, run)| (run.mutant_id, run.test_ids))
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
    let spawner = FakeSpawner::new(script(dry, |_, run| match run.mutant_id.as_str() {
        "m1" => Err(LinkFault::Timeout),
        _ => Ok(run_result(run, RunStatus::Killed, Some(&run.test_ids[0]))),
    }));
    let outcome = run_schedule(&spawner, &plan(&["m1", "m2", "m3"]), &config(1)).expect("run");

    assert_eq!(status(&outcome, "m1"), MutantStatus::Timeout);
    assert_eq!(status(&outcome, "m2"), MutantStatus::Killed);
    let m2_runs: Vec<Vec<String>> = spawner
        .runs()
        .into_iter()
        .filter(|(_, run)| run.mutant_id == "m2")
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

#[test]
fn a_timeout_reruns_in_a_new_companion_and_a_pass_is_flaky() {
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
    let runs = spawner.runs();
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[1].0, 1, "the retry uses a new companion");
    assert_eq!(
        outcome.results["m1"].duration_ms,
        Some(12),
        "a flaky mutant keeps the duration of its first run"
    );
}

#[test]
fn a_timeout_that_a_retry_kills_is_killed_by_the_failed_test() {
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
    assert_eq!(status(&outcome, "m1"), MutantStatus::Killed);
    assert_eq!(outcome.results["m1"].killed_by, Some(tid("t1")));
    assert_eq!(outcome.results["m1"].duration_ms, Some(12));
}

#[test]
fn a_timeout_that_times_out_again_stays_timeout() {
    let spawner = FakeSpawner::new(script(
        |_| standard_dry(),
        |_, run| Ok(run_result(run, RunStatus::Timeout, None)),
    ));
    let outcome = run_schedule(&spawner, &plan(&["m1"]), &config(1)).expect("run");
    assert_eq!(status(&outcome, "m1"), MutantStatus::Timeout);
    let runs = spawner.runs();
    assert_eq!(runs.len(), 2, "one retry, no more");
    assert_eq!(runs[1].0, 1, "the retry uses a new companion");
}
