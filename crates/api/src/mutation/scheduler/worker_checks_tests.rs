use super::super::super::protocol::{RunWorkerMode, WorkersChecked};
use super::*;

const FILES: [&str; 3] = ["test/a.test.ts", "test/b.test.ts", "test/c.test.ts"];
/// The mutants that only `a::t1` covers: enough jobs for a check.
const SHARED_SET: [&str; 4] = ["m0", "m1", "m2", "m3"];
/// The mutant that only `b::u1` covers: too few jobs for a check.
const RARE_SET: &str = "m4";
/// The mutants that only `c::v1` covers: a second set with a check.
const OTHER_SET: [&str; 3] = ["m5", "m6", "m7"];

const CHECKING_CAPS: Capabilities = Capabilities {
    worker_check: true,
    ..VITEST_CAPS
};

fn test_of(file: &str, id: &str) -> DryTest {
    DryTest {
        index: None,
        test_id: format!("{file}::{id}"),
        file: file.to_owned(),
        duration_ms: Some(5.0),
        status: TestStatus::Pass,
        failure_kind: None,
        message: None,
    }
}

fn dry_of(files: &[String]) -> DryRunResult {
    let mut dry = DryRunResult {
        tests: Vec::new(),
        hits: BTreeMap::new(),
        static_hits: Vec::new(),
        file_hits: None,
        unhandled_errors: Vec::new(),
        unloaded_paths: Vec::new(),
        file_setup_ms: BTreeMap::new(),
        duplicate_test_ids: Vec::new(),
    };
    for file in files {
        let (id, hits): (&str, Vec<&str>) = match file.as_str() {
            "test/a.test.ts" => ("t1", SHARED_SET.to_vec()),
            "test/b.test.ts" => ("u1", vec![RARE_SET]),
            _ => ("v1", OTHER_SET.to_vec()),
        };
        let test = test_of(file, id);
        dry.hits.insert(
            test.test_id.clone(),
            hits.iter().map(|hit| (*hit).to_owned()).collect(),
        );
        dry.file_setup_ms.insert(file.clone(), 1.0);
        dry.tests.push(test);
    }
    dry
}

/// A companion with the `worker_check` capability. `check` answers each
/// `check_workers` request.
fn checking_spawner(
    check: impl Fn(&[String]) -> Result<Response, LinkFault> + Send + Sync + 'static,
) -> FakeSpawner {
    FakeSpawner::new(move |_, request| match request {
        Request::Init(_) => {
            let Response::Ready(mut ready) = ready(CHECKING_CAPS, "5.0.4") else {
                unreachable!("ready gives a ready response");
            };
            ready.test_files = FILES.map(str::to_owned).to_vec();
            Ok(Response::Ready(ready))
        }
        Request::DryRun { test_files } => Ok(Response::DryRun(dry_of(test_files))),
        Request::CheckWorkers(body) => check(&body.test_ids),
        Request::Run(run) => Ok(run_result(run, RunStatus::Survived, None)),
        Request::Reload => Ok(Response::Reloaded(Reloaded {
            method: ReloadMethod::Noop,
        })),
        Request::Shutdown | Request::Replay(_) | Request::Record { .. } | Request::Adopt(_) => {
            Err(LinkFault::Protocol("no response".to_owned()))
        }
    })
}

const fn checked(mode: RunWorkerMode) -> Response {
    Response::WorkersChecked(WorkersChecked { worker_mode: mode })
}

fn checking_plan() -> SchedulePlan {
    let ids: Vec<&str> = SHARED_SET
        .iter()
        .copied()
        .chain([RARE_SET])
        .chain(OTHER_SET)
        .collect();
    SchedulePlan {
        test_files: FILES.map(str::to_owned).to_vec(),
        file_test_files: BTreeMap::from([(
            "src/a.ts".to_owned(),
            FILES.map(str::to_owned).to_vec(),
        )]),
        ..plan(&ids)
    }
}

/// The `check_workers` requests, as the test ids of each.
fn checks(spawner: &FakeSpawner) -> Vec<Vec<String>> {
    spawner
        .log
        .lock()
        .expect("log")
        .iter()
        .filter_map(|(_, request)| match request {
            Request::CheckWorkers(body) => Some(body.test_ids.clone()),
            _ => None,
        })
        .collect()
}

/// The worker mode of the mutant runs of `mutant`.
fn modes_of(spawner: &FakeSpawner, mutant: &str) -> Vec<Option<RunWorkerMode>> {
    spawner
        .runs()
        .iter()
        .filter(|(_, run)| run.mutant_id.as_deref() == Some(mutant))
        .map(|(_, run)| run.worker_mode)
        .collect()
}

fn verdicts(outcome: &ScheduleOutcome) -> Vec<(String, MutantStatus)> {
    let mut verdicts: Vec<_> = outcome
        .results
        .iter()
        .map(|(id, result)| (id.clone(), result.status))
        .collect();
    verdicts.sort();
    verdicts
}

#[test]
fn a_test_set_of_enough_jobs_gets_one_check_and_its_runs_use_the_mode() {
    let spawner = checking_spawner(|_| Ok(checked(RunWorkerMode::Reused)));
    run_schedule(&spawner, &checking_plan(), &config(1)).expect("run");

    assert_eq!(
        checks(&spawner),
        vec![vec![
            "test/a.test.ts::t1".to_owned(),
            "test/c.test.ts::v1".to_owned()
        ]],
        "one check holds each set with enough jobs"
    );
    for mutant in SHARED_SET {
        let modes = modes_of(&spawner, mutant);
        assert!(!modes.is_empty(), "{mutant} ran");
        assert!(
            modes
                .iter()
                .all(|mode| *mode == Some(RunWorkerMode::Reused)),
            "{mutant}: {modes:?}"
        );
    }
    assert!(
        modes_of(&spawner, RARE_SET).iter().all(Option::is_none),
        "a set of one job gets no check and no mode"
    );
}

#[test]
fn a_set_that_differs_in_one_worker_keeps_isolated_runs() {
    let spawner = checking_spawner(|_| Ok(checked(RunWorkerMode::Isolated)));
    run_schedule(&spawner, &checking_plan(), &config(1)).expect("run");

    assert_eq!(
        checks(&spawner).len(),
        3,
        "the check of both sets, then one check of each set, and no more"
    );
    for mutant in SHARED_SET {
        assert!(modes_of(&spawner, mutant).iter().all(Option::is_none));
    }
}

#[test]
fn a_check_with_no_answer_restarts_the_companion_and_keeps_isolated_runs() {
    let spawner = checking_spawner(|_| Err(LinkFault::Timeout));
    let outcome = run_schedule(&spawner, &checking_plan(), &config(1)).expect("run");

    assert_eq!(
        checks(&spawner).len(),
        3,
        "the check of both sets, then one check of each set, and no more"
    );
    for mutant in SHARED_SET {
        assert!(modes_of(&spawner, mutant).iter().all(Option::is_none));
    }
    let plain = checking_spawner(|_| Ok(checked(RunWorkerMode::Reused)));
    let expected = run_schedule(&plain, &checking_plan(), &config(1)).expect("run");
    assert_eq!(verdicts(&outcome), verdicts(&expected));
    assert!(
        spawner.spawns.load(Ordering::SeqCst) > 1,
        "the companion restarted"
    );
}

#[test]
fn concurrent_workers_check_a_set_one_time() {
    let spawner = checking_spawner(|_| Ok(checked(RunWorkerMode::Shared)));
    run_schedule(&spawner, &checking_plan(), &config(3)).expect("run");

    assert_eq!(checks(&spawner).len(), 1);
    let shared = SHARED_SET
        .iter()
        .flat_map(|mutant| modes_of(&spawner, mutant))
        .filter(|mode| *mode == Some(RunWorkerMode::Shared))
        .count();
    assert!(shared > 0, "the runs after the check use its mode");
}

#[test]
fn a_runner_without_the_capability_gets_no_check() {
    let spawner = FakeSpawner::new(move |index, request| match request {
        Request::Init(_) => {
            let Response::Ready(mut ready) = ready(VITEST_CAPS, "5.0.4") else {
                unreachable!("ready gives a ready response");
            };
            ready.test_files = FILES.map(str::to_owned).to_vec();
            Ok(Response::Ready(ready))
        }
        other => (checking_spawner(|_| Ok(checked(RunWorkerMode::Reused))).script)(index, other),
    });
    run_schedule(&spawner, &checking_plan(), &config(1)).expect("run");

    assert!(checks(&spawner).is_empty());
    assert!(
        spawner
            .runs()
            .iter()
            .all(|(_, run)| run.worker_mode.is_none())
    );
}

#[test]
fn a_failed_check_of_two_sets_gives_each_set_its_own_check() {
    // The two files differ in one worker, and each passes alone.
    let spawner = checking_spawner(|tests| {
        Ok(checked(if tests.len() > 1 {
            RunWorkerMode::Isolated
        } else {
            RunWorkerMode::Reused
        }))
    });
    let outcome = run_schedule(&spawner, &checking_plan(), &config(1)).expect("run");

    let mut seen = checks(&spawner);
    assert_eq!(seen.len(), 3, "{seen:?}");
    assert_eq!(seen.remove(0).len(), 2, "the first check holds both sets");
    seen.sort();
    assert_eq!(
        seen,
        vec![
            vec!["test/a.test.ts::t1".to_owned()],
            vec!["test/c.test.ts::v1".to_owned()]
        ]
    );
    let reused = SHARED_SET
        .iter()
        .chain(OTHER_SET.iter())
        .flat_map(|mutant| modes_of(&spawner, mutant))
        .filter(|mode| *mode == Some(RunWorkerMode::Reused))
        .count();
    assert!(
        reused > 0,
        "the runs after the single checks use their mode"
    );
    let plain = checking_spawner(|_| Ok(checked(RunWorkerMode::Isolated)));
    let expected = run_schedule(&plain, &checking_plan(), &config(1)).expect("run");
    assert_eq!(verdicts(&outcome), verdicts(&expected));
}
