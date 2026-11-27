use super::super::super::protocol::RunWorkerMode;
use super::*;

const FILES: [&str; 3] = ["test/a.test.ts", "test/b.test.ts", "test/c.test.ts"];

fn file_test(file: &str, id: &str, status: TestStatus) -> DryTest {
    DryTest {
        index: None,
        test_id: format!("{file}::{id}"),
        file: file.to_owned(),
        duration_ms: Some(5.0),
        status,
        failure_kind: None,
        message: None,
    }
}

/// The dry run of `files`: `a::t1` hits m0, m1 and m3, `b::u1` hits m2 and m3,
/// `c::v1` hits nothing. Each file reports the sources that it does not
/// load, so a path that no file loads stays in each report.
fn dry_of(files: &[String], failing: Option<&str>) -> DryRunResult {
    let mut dry = DryRunResult {
        worker_mode: None,
        tests: Vec::new(),
        hits: BTreeMap::new(),
        static_hits: Vec::new(),
        file_hits: None,
        unhandled_errors: Vec::new(),
        unloaded_paths: vec!["src/never.ts".to_owned()],
        file_setup_ms: BTreeMap::new(),
        duplicate_test_ids: Vec::new(),
    };
    for file in files {
        let (id, hits): (&str, &[&str]) = match file.as_str() {
            "test/a.test.ts" => ("t1", &["m0", "m1", "m3"]),
            "test/b.test.ts" => ("u1", &["m2", "m3"]),
            _ => ("v1", &[]),
        };
        let status = if failing == Some(file.as_str()) {
            TestStatus::Fail
        } else {
            TestStatus::Pass
        };
        let test = file_test(file, id, status);
        if !hits.is_empty() {
            dry.hits.insert(
                test.test_id.clone(),
                hits.iter().map(|hit| (*hit).to_owned()).collect(),
            );
        }
        dry.file_setup_ms.insert(file.clone(), 1.0);
        dry.tests.push(test);
    }
    dry
}

fn shard_spawner(failing: Option<&'static str>) -> FakeSpawner {
    FakeSpawner::new(move |_, request| match request {
        Request::Init(_) => {
            let Response::Ready(mut ready) = ready(VITEST_CAPS, "5.0.4") else {
                unreachable!("ready gives a ready response");
            };
            ready.test_files = FILES.map(str::to_owned).to_vec();
            Ok(Response::Ready(ready))
        }
        Request::DryRun { test_files } => Ok(Response::DryRun(dry_of(test_files, failing))),
        Request::Run(run) => Ok(run_result(run, RunStatus::Killed, Some(&run.test_ids[0]))),
        Request::Reload => Ok(Response::Reloaded(Reloaded {
            method: ReloadMethod::Noop,
        })),
        Request::Shutdown | Request::Replay(_) | Request::Record { .. } | Request::Adopt(_) => {
            Err(LinkFault::Protocol("no response".to_owned()))
        }
    })
}

fn shard_plan(ids: &[&str]) -> SchedulePlan {
    SchedulePlan {
        test_files: FILES.map(str::to_owned).to_vec(),
        file_test_files: BTreeMap::from([(
            "src/a.ts".to_owned(),
            FILES.map(str::to_owned).to_vec(),
        )]),
        ..plan(ids)
    }
}

/// The test files of each dry run, with the companion index, in log order.
fn dry_runs(spawner: &FakeSpawner) -> Vec<(usize, Vec<String>)> {
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

fn verdicts(outcome: &ScheduleOutcome) -> Vec<(String, MutantStatus, Option<String>)> {
    let mut verdicts: Vec<_> = outcome
        .results
        .iter()
        .map(|(id, result)| (id.clone(), result.status, result.killed_by.clone()))
        .collect();
    verdicts.sort();
    verdicts
}

#[test]
fn the_dry_run_divides_the_test_files_over_the_companions() {
    let spawner = shard_spawner(None);
    let outcome = run_schedule(&spawner, &shard_plan(&["m1", "m2"]), &config(3)).expect("run");

    let mut dry = dry_runs(&spawner);
    dry.sort_by(|left, right| left.1.cmp(&right.1));
    let files: Vec<Vec<String>> = dry.iter().map(|(_, files)| files.clone()).collect();
    assert_eq!(
        files,
        FILES.map(|file| vec![file.to_owned()]).to_vec(),
        "each companion dry-runs one share of the files, and no companion dry-runs a file again"
    );
    assert_eq!(spawner.spawns.load(Ordering::SeqCst), 3);
    let shard_of = |file: &str| {
        dry.iter()
            .find(|(_, files)| files.iter().any(|loaded| loaded == file))
            .map(|(index, _)| *index)
    };
    for (index, run) in spawner.runs() {
        let file = run.test_ids[0].split("::").next().map(str::to_owned);
        assert_eq!(Some(index), file.as_deref().and_then(shard_of), "{run:?}");
    }

    let serial = shard_spawner(None);
    let expected = run_schedule(&serial, &shard_plan(&["m1", "m2"]), &config(1)).expect("run");
    assert_eq!(verdicts(&outcome), verdicts(&expected));
    assert_eq!(status(&outcome, "m1"), MutantStatus::Killed);
    assert_eq!(
        outcome.results["m2"].killed_by.as_deref(),
        Some("test/b.test.ts::u1")
    );
    assert_eq!(outcome.test_files, expected.test_files);
    assert_eq!(outcome.unloaded_paths, vec!["src/never.ts".to_owned()]);
}

#[test]
fn a_job_over_two_shares_gets_a_companion_for_the_files_of_the_jobs() {
    let spawner = shard_spawner(None);
    let outcome = run_schedule(&spawner, &shard_plan(&["m3"]), &config(2)).expect("run");

    assert_eq!(status(&outcome, "m3"), MutantStatus::Killed);
    let dry = dry_runs(&spawner);
    let later: Vec<Vec<String>> = dry.iter().skip(2).map(|(_, files)| files.clone()).collect();
    assert_eq!(
        later,
        vec![vec![
            "test/a.test.ts".to_owned(),
            "test/b.test.ts".to_owned()
        ]],
        "the new companion dry-runs only the files of the covering tests"
    );
}

#[test]
fn another_worker_takes_a_backlog_job_with_a_dry_run_of_the_phase_files_only() {
    let spawner = shard_spawner(None);
    let outcome = run_schedule(&spawner, &shard_plan(&["m0", "m1"]), &config(3)).expect("run");

    assert_eq!(status(&outcome, "m0"), MutantStatus::Killed);
    assert_eq!(status(&outcome, "m1"), MutantStatus::Killed);
    assert_eq!(outcome.workers, 2);
    for (_, files) in dry_runs(&spawner).iter().skip(3) {
        assert_eq!(
            files,
            &vec!["test/a.test.ts".to_owned()],
            "a new companion of the mutant phase dry-runs only the files of the jobs"
        );
    }
}

#[test]
fn a_failing_test_of_one_share_is_a_baseline_failure() {
    let spawner = shard_spawner(Some("test/b.test.ts"));
    let outcome = run_schedule(&spawner, &shard_plan(&["m1", "m2"]), &config(3)).expect("run");

    assert_eq!(status(&outcome, "m1"), MutantStatus::Killed);
    assert_eq!(status(&outcome, "m2"), MutantStatus::NoCoverage);
    assert_eq!(outcome.baseline_failures.len(), 1);
    assert_eq!(
        outcome.baseline_failures[0].test_id.as_deref(),
        Some("test/b.test.ts::u1")
    );
}

#[test]
fn a_setup_fault_of_another_share_stops_the_run() {
    let spawner = FakeSpawner::new(|index, request| match request {
        Request::Init(_) if index > 0 => Err(LinkFault::Remote(RemoteError {
            code: "internal".to_owned(),
            message: "config load failed".to_owned(),
            retryable: false,
        })),
        _ => (shard_spawner(None).script)(index, request),
    });
    let error =
        run_schedule(&spawner, &shard_plan(&["m1", "m2"]), &config(3)).expect_err("setup failure");
    assert!(matches!(error, ScheduleError::Setup { .. }), "{error:?}");
    assert!(error.to_string().contains("config load failed"), "{error}");
}

/// A spawner whose first `shares` companions see `fault` in the dry run of
/// `test/b.test.ts`, as a test does that fails on outside state that another
/// share uses at the same time. A later companion dry-runs it cleanly.
fn contended_spawner(shares: usize, fault: fn(&mut DryRunResult), always: bool) -> FakeSpawner {
    FakeSpawner::new(move |index, request| match request {
        Request::DryRun { test_files } => {
            let mut dry = dry_of(test_files, None);
            let contended = always || index < shares;
            if contended && test_files.iter().any(|file| file == "test/b.test.ts") {
                fault(&mut dry);
            }
            Ok(Response::DryRun(dry))
        }
        _ => (shard_spawner(None).script)(index, request),
    })
}

fn fail_b(dry: &mut DryRunResult) {
    for test in &mut dry.tests {
        if test.file == "test/b.test.ts" {
            test.status = TestStatus::Fail;
        }
    }
}

fn global_error(dry: &mut DryRunResult) {
    dry.unhandled_errors.push(UnhandledError {
        file: None,
        message: "EADDRINUSE: port 4000".to_owned(),
    });
}

#[test]
fn a_failure_of_a_share_runs_again_alone_and_that_result_counts() {
    let spawner = contended_spawner(3, fail_b, false);
    let outcome = run_schedule(&spawner, &shard_plan(&["m1", "m2"]), &config(3)).expect("run");

    assert!(
        outcome.baseline_failures.is_empty(),
        "{:?}",
        outcome.baseline_failures
    );
    assert_eq!(status(&outcome, "m2"), MutantStatus::Killed);
    let rechecks: Vec<Vec<String>> = dry_runs(&spawner)
        .into_iter()
        .skip(3)
        .map(|(_, files)| files)
        .collect();
    assert_eq!(rechecks, vec![vec!["test/b.test.ts".to_owned()]]);
}

#[test]
fn an_error_outside_each_file_of_a_share_runs_each_file_again() {
    let spawner = contended_spawner(3, global_error, false);
    let outcome = run_schedule(&spawner, &shard_plan(&["m1", "m2"]), &config(3)).expect("run");
    assert_eq!(status(&outcome, "m1"), MutantStatus::Killed);
    assert_eq!(status(&outcome, "m2"), MutantStatus::Killed);
    let rechecks: Vec<Vec<String>> = dry_runs(&spawner)
        .into_iter()
        .skip(3)
        .map(|(_, files)| files)
        .collect();
    assert_eq!(rechecks, vec![FILES.map(str::to_owned).to_vec()]);

    let lasting = contended_spawner(3, global_error, true);
    let error = run_schedule(&lasting, &shard_plan(&["m1", "m2"]), &config(3))
        .expect_err("a lasting error outside each file stops the run");
    assert_eq!(
        error,
        ScheduleError::Baseline(vec!["EADDRINUSE: port 4000".to_owned()])
    );
}

#[test]
fn the_outcome_counts_the_worker_mode_of_each_mutant_run() {
    let plain = shard_spawner(None);
    let spawner = FakeSpawner::new(move |index, request| match request {
        Request::Run(run) => {
            let Response::Run(mut result) = run_result(run, RunStatus::Survived, None) else {
                unreachable!("run_result gives a run response");
            };
            result.worker_mode = Some(if run.mutant_id.as_deref() == Some("m1") {
                RunWorkerMode::Reused
            } else {
                RunWorkerMode::Isolated
            });
            Ok(Response::Run(result))
        }
        other => (plain.script)(index, other),
    });
    let outcome = run_schedule(&spawner, &shard_plan(&["m1", "m2"]), &config(1)).expect("run");

    let runs_of = |id: &str| {
        spawner
            .runs()
            .iter()
            .filter(|(_, run)| run.mutant_id.as_deref() == Some(id))
            .count()
    };
    assert!(runs_of("m1") > 0 && runs_of("m2") > 0);
    let counts = outcome.worker_modes.expect("the runs stated a mode");
    assert_eq!(
        (counts.reused.runs, counts.isolated.runs, counts.shared.runs),
        (runs_of("m1"), runs_of("m2"), 0)
    );
}

#[test]
fn a_runner_without_a_worker_mode_gives_no_counts() {
    let spawner = shard_spawner(None);
    let outcome = run_schedule(&spawner, &shard_plan(&["m1"]), &config(1)).expect("run");
    assert_eq!(outcome.worker_modes, None);
}
