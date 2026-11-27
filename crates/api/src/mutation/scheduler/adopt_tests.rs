use super::super::super::protocol::{AdoptRequest, Adopted, WorkerMode};
use super::*;

const FILES: [&str; 3] = ["test/a.test.ts", "test/b.test.ts", "test/c.test.ts"];

pub(super) const ADOPT_CAPS: Capabilities = Capabilities {
    adopt: true,
    ..VITEST_CAPS
};

/// The taken tests of each companion, by companion index.
pub(super) type Taken = Arc<Mutex<FxHashMap<usize, Vec<String>>>>;

/// The dry run of `files`: `a::t1` hits m3, `b::u1` hits m2 and m3, and
/// `c::v1` hits m1. Each passing test has an index entry, and the dry run
/// proves a shared worker for its files. With two companions the shares are
/// `[a, c]` and `[b]`, so m1 always runs in the companion that dry-ran its
/// file, and the tests that another companion takes never cover it.
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
        worker_mode: Some(WorkerMode {
            files: files.to_vec(),
            reused: false,
        }),
    };
    for file in files {
        let (id, hits): (&str, &[&str]) = match file.as_str() {
            "test/a.test.ts" => ("t1", &["m3"]),
            "test/b.test.ts" => ("u1", &["m2", "m3"]),
            _ => ("v1", &["m1"]),
        };
        let test_id = format!("{file}::{id}");
        if !hits.is_empty() {
            dry.hits.insert(
                test_id.clone(),
                hits.iter().map(|hit| (*hit).to_owned()).collect(),
            );
        }
        dry.tests.push(DryTest {
            test_id: test_id.clone(),
            file: file.clone(),
            duration_ms: Some(5.0),
            status: TestStatus::Pass,
            failure_kind: None,
            message: None,
            index: Some(serde_json::json!({ "task": test_id })),
        });
    }
    dry
}

/// A runner with the `adopt` capability. A mutant run gives `status`; a
/// kill names a test that the companion took when it took one, else the
/// first test.
pub(super) fn adopt_spawner(caps: Capabilities, status: RunStatus, taken: &Taken) -> FakeSpawner {
    let taken = Arc::clone(taken);
    FakeSpawner::new(move |index, request| match request {
        Request::Init(_) => {
            let Response::Ready(mut ready) = ready(caps, "5.0.4") else {
                unreachable!("ready gives a ready response");
            };
            ready.test_files = FILES.map(str::to_owned).to_vec();
            Ok(Response::Ready(ready))
        }
        Request::DryRun { test_files } => Ok(Response::DryRun(dry_of(test_files))),
        Request::Adopt(AdoptRequest { tests, .. }) => {
            taken
                .lock()
                .expect("taken")
                .entry(index)
                .or_default()
                .extend(tests.iter().map(|test| test.test_id.clone()));
            Ok(Response::Adopted(Adopted { tests: tests.len() }))
        }
        Request::Run(run) => {
            let taken = taken.lock().expect("taken");
            let killer = run
                .test_ids
                .iter()
                .find(|test| taken.get(&index).is_some_and(|ids| ids.contains(test)))
                .unwrap_or(&run.test_ids[0]);
            let killed_by = (status == RunStatus::Killed).then_some(killer.as_str());
            Ok(run_result(run, status, killed_by))
        }
        Request::Reload => Ok(Response::Reloaded(Reloaded {
            method: ReloadMethod::Noop,
        })),
        Request::Shutdown | Request::Replay(_) | Request::Record { .. } => {
            Err(LinkFault::Protocol("no response".to_owned()))
        }
    })
}

pub(super) fn adopt_plan(ids: &[&str]) -> SchedulePlan {
    SchedulePlan {
        test_files: FILES.map(str::to_owned).to_vec(),
        file_test_files: BTreeMap::from([(
            "src/a.ts".to_owned(),
            FILES.map(str::to_owned).to_vec(),
        )]),
        ..plan(ids)
    }
}

pub(super) fn count_of(spawner: &FakeSpawner, kind: &str) -> usize {
    spawner
        .log
        .lock()
        .expect("log")
        .iter()
        .filter(|(_, request)| request.type_name() == kind)
        .count()
}

fn adopt_requests(spawner: &FakeSpawner) -> Vec<(usize, AdoptRequest)> {
    spawner
        .log
        .lock()
        .expect("log")
        .iter()
        .filter_map(|(index, request)| match request {
            Request::Adopt(body) => Some((*index, body.clone())),
            _ => None,
        })
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
fn a_job_of_two_shares_and_each_retry_take_the_test_index_with_no_second_dry_run() {
    let taken = Taken::default();
    let spawner = adopt_spawner(ADOPT_CAPS, RunStatus::Survived, &taken);
    let outcome =
        run_schedule(&spawner, &adopt_plan(&["m1", "m2", "m3"]), &config(3)).expect("run");

    assert_eq!(
        count_of(&spawner, "dry_run"),
        3,
        "each share dry-runs its files one time; no later companion dry-runs"
    );
    assert!(
        spawner.spawns.load(Ordering::SeqCst) > 3,
        "the retries run in new companions"
    );
    let adopts = adopt_requests(&spawner);
    assert!(
        adopts.iter().any(|(index, _)| *index >= 3),
        "a retry companion takes the index: {adopts:?}"
    );
    for (_, request) in &adopts {
        assert!(
            request
                .tests
                .iter()
                .all(|test| test.index["task"] == test.test_id.as_str())
        );
        for mode in &request.worker_modes {
            assert!(
                request
                    .tests
                    .iter()
                    .any(|test| mode.files.iter().any(|file| test.test_id.starts_with(file))),
                "a worker mode goes only with the tests of its files: {request:?}"
            );
        }
    }

    let serial_taken = Taken::default();
    let plain = adopt_spawner(VITEST_CAPS, RunStatus::Survived, &serial_taken);
    let expected = run_schedule(&plain, &adopt_plan(&["m1", "m2", "m3"]), &config(3)).expect("run");
    assert!(
        count_of(&plain, "dry_run") > 3,
        "a runner without the capability keeps the dry run of each new companion"
    );
    assert_eq!(count_of(&plain, "adopt"), 0);
    assert_eq!(verdicts(&outcome), verdicts(&expected));
    assert_eq!(status(&outcome, "m3"), MutantStatus::Survived);
}

#[test]
fn a_kill_by_a_taken_test_counts_after_a_control_run_of_its_file() {
    let taken = Taken::default();
    let spawner = adopt_spawner(ADOPT_CAPS, RunStatus::Killed, &taken);
    let outcome = run_schedule(&spawner, &adopt_plan(&["m1", "m3"]), &config(2)).expect("run");

    assert_eq!(status(&outcome, "m1"), MutantStatus::Killed);
    assert_eq!(status(&outcome, "m3"), MutantStatus::Killed);
    assert!(outcome.baseline_failures.is_empty());
    let taken = taken.lock().expect("taken").clone();
    let (index, ids) = taken.iter().next().expect("a companion took a test");
    let proofs: Vec<RunRequest> = spawner
        .controls()
        .into_iter()
        .filter(|(control, _)| control == index)
        .map(|(_, run)| run)
        .collect();
    assert!(
        proofs.iter().any(|run| run.test_ids == *ids && !run.bail),
        "the taken killer passes a control run first: {proofs:?}"
    );
}

#[test]
fn a_taken_test_that_fails_in_the_control_run_leaves_the_companion() {
    let taken = Taken::default();
    let failing = Arc::clone(&taken);
    let spawner =
        adopt_spawner(ADOPT_CAPS, RunStatus::Killed, &taken).with_control(move |index, run| {
            let taken = failing.lock().expect("taken");
            let bad = run
                .test_ids
                .iter()
                .find(|test| taken.get(&index).is_some_and(|ids| ids.contains(test)));
            Ok(match bad {
                Some(test) => run_result(run, RunStatus::Killed, Some(test)),
                None => passing_control(run),
            })
        });
    let outcome = run_schedule(&spawner, &adopt_plan(&["m1", "m3"]), &config(2)).expect("run");

    let taken = taken.lock().expect("taken").clone();
    let dropped: Vec<&String> = taken.values().flatten().collect();
    assert_eq!(dropped.len(), 1, "{taken:?}");
    let failure = outcome
        .baseline_failures
        .iter()
        .find(|failure| failure.test_id.as_ref() == Some(dropped[0]))
        .expect("the dropped test is a baseline failure");
    // The companion that took the test can run `m1` before the control run
    // drops the test, so `m1` can also be a mutant that ran without it.
    assert!(
        failure.mutant_ids.contains(&"m3".to_owned())
            && failure.mutant_ids.iter().all(|id| id == "m1" || id == "m3"),
        "{:?}",
        failure.mutant_ids
    );
    let message = failure.message.as_deref().unwrap_or_default();
    assert!(
        message.starts_with(super::super::control::ADOPTED_TEST_FAILURE),
        "{message}"
    );
    assert_eq!(
        status(&outcome, "m3"),
        MutantStatus::Killed,
        "m3 runs again without the dropped test, and its other test kills it"
    );
    assert_ne!(outcome.results["m3"].killed_by.as_ref(), Some(dropped[0]));
}

#[test]
fn a_watchdog_timeout_of_a_taken_test_counts_only_after_a_control_run_in_a_new_companion() {
    let taken = Taken::default();
    let took = Arc::clone(&taken);
    let failing = Arc::clone(&taken);
    let base = adopt_spawner(ADOPT_CAPS, RunStatus::Survived, &taken);
    let script = Arc::clone(&base.script);
    let spawner = FakeSpawner {
        script: Arc::new(move |index, request| {
            let took_any = took
                .lock()
                .expect("taken")
                .get(&index)
                .is_some_and(|ids| !ids.is_empty());
            match request {
                Request::Run(run) if run.mutant_id.is_some() && took_any => Err(LinkFault::Timeout),
                _ => script(index, request),
            }
        }),
        ..base
    }
    .with_control(move |index, run| {
        let taken = failing.lock().expect("taken");
        let bad = run
            .test_ids
            .iter()
            .find(|test| taken.get(&index).is_some_and(|ids| ids.contains(test)));
        Ok(match bad {
            Some(test) => run_result(run, RunStatus::Killed, Some(test)),
            None => passing_control(run),
        })
    });
    let outcome = run_schedule(&spawner, &adopt_plan(&["m1", "m3"]), &config(2)).expect("run");

    assert_ne!(
        status(&outcome, "m3"),
        MutantStatus::Timeout,
        "a taken test that fails with no active mutant decides no timeout"
    );
    assert!(
        outcome
            .baseline_failures
            .iter()
            .any(|failure| failure.message.as_deref().is_some_and(
                |message| message.starts_with(super::super::control::ADOPTED_TEST_FAILURE)
            )),
        "{:?}",
        outcome.baseline_failures
    );
}
