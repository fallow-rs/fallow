use super::super::super::protocol::Adopted;
use super::*;

const FILES: [&str; 4] = [
    "test/a.test.ts",
    "test/b.test.ts",
    "test/c.test.ts",
    "test/d.test.ts",
];

const ADOPT_CAPS: Capabilities = Capabilities {
    adopt: true,
    ..VITEST_CAPS
};

/// The test of `file`.
fn file_test_id(file: &str) -> String {
    format!("{file}::t")
}

/// The dry run of `files`: each file has one test that hits m1. The test of
/// file `i` of [`FILES`] takes `(i + 1) * 2000` ms, so the fixed test order
/// of m1 is a, b, c, d.
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
        let rank = FILES.iter().position(|known| known == file).unwrap_or(0);
        let test_id = file_test_id(file);
        dry.hits.insert(test_id.clone(), vec!["m1".to_owned()]);
        dry.file_setup_ms.insert(file.clone(), 100.0);
        dry.tests.push(DryTest {
            test_id: test_id.clone(),
            file: file.clone(),
            duration_ms: Some(2000.0 * (rank as f64 + 1.0)),
            status: TestStatus::Pass,
            failure_kind: None,
            message: None,
            index: Some(serde_json::json!({ "task": test_id })),
        });
    }
    dry
}

/// The answer of a runner with each capability of `caps`. `run` answers
/// each mutant run.
fn wide_script(
    caps: Capabilities,
    request: &Request,
    run: &dyn Fn(&RunRequest) -> Response,
) -> Result<Response, LinkFault> {
    match request {
        Request::Init(_) => {
            let Response::Ready(mut ready) = ready(caps, "5.0.4") else {
                unreachable!("ready gives a ready response");
            };
            ready.test_files = FILES.map(str::to_owned).to_vec();
            Ok(Response::Ready(ready))
        }
        Request::DryRun { test_files } => Ok(Response::DryRun(dry_of(test_files))),
        Request::Adopt(body) => Ok(Response::Adopted(Adopted {
            tests: body.tests.len(),
        })),
        Request::Run(body) => Ok(run(body)),
        Request::Reload => Ok(Response::Reloaded(Reloaded {
            method: ReloadMethod::Noop,
        })),
        Request::Shutdown
        | Request::Replay(_)
        | Request::Record { .. }
        | Request::CheckWorkers(_) => Err(LinkFault::Protocol("no response".to_owned())),
    }
}

/// A runner with each capability of `caps`. `run` answers each mutant run.
fn wide_spawner(
    caps: Capabilities,
    run: impl Fn(&RunRequest) -> Response + Send + Sync + 'static,
) -> FakeSpawner {
    FakeSpawner::new(move |_, request| wide_script(caps, request, &run))
}

fn wide_plan() -> SchedulePlan {
    SchedulePlan {
        test_files: FILES.map(str::to_owned).to_vec(),
        file_test_files: BTreeMap::from([(
            "src/a.ts".to_owned(),
            FILES.map(str::to_owned).to_vec(),
        )]),
        ..plan(&["m1"])
    }
}

/// The mutant runs of m1, in the order that the companions received them.
fn mutant_runs(spawner: &FakeSpawner) -> Vec<(usize, Vec<String>)> {
    spawner
        .runs()
        .into_iter()
        .filter(|(_, run)| run.mutant_id.as_deref() == Some("m1"))
        .map(|(index, run)| (index, run.test_ids))
        .collect()
}

/// A kill by the test of `file` when the run holds it, else a survivor.
fn kill_by(files: &'static [&'static str]) -> impl Fn(&RunRequest) -> Response + Send + Sync {
    move |run| {
        let killer = files
            .iter()
            .map(|file| file_test_id(file))
            .find(|test| run.test_ids.contains(test));
        match killer {
            Some(test) => run_result(run, RunStatus::Killed, Some(&test)),
            None => run_result(run, RunStatus::Survived, None),
        }
    }
}

/// The companions that run m1 at the same time. A run of m1 waits up to
/// [`MEET_WAIT`] for a run of m1 on a second companion, so the test sees two
/// companions at once only when the mutant runs on two workers.
#[derive(Default)]
struct Meeting {
    companions: Mutex<FxHashSet<usize>>,
    met: std::sync::Condvar,
}

const MEET_WAIT: Duration = Duration::from_secs(5);

impl Meeting {
    fn join(&self, companion: usize) {
        let mut companions = self.companions.lock().expect("companions");
        companions.insert(companion);
        self.met.notify_all();
        let _met = self
            .met
            .wait_timeout_while(companions, MEET_WAIT, |companions| companions.len() < 2)
            .expect("companions");
    }

    fn count(&self) -> usize {
        self.companions.lock().expect("companions").len()
    }
}

#[test]
fn the_test_files_of_a_wide_mutant_run_on_more_than_one_idle_worker() {
    let meeting = Arc::new(Meeting::default());
    let met = Arc::clone(&meeting);
    let spawner = FakeSpawner::new(move |index, request| {
        if let Request::Run(run) = request
            && run.mutant_id.as_deref() == Some("m1")
        {
            met.join(index);
        }
        wide_script(ADOPT_CAPS, request, &|run| {
            run_result(run, RunStatus::Survived, None)
        })
    });
    let outcome = run_schedule(&spawner, &wide_plan(), &config(4)).expect("run");

    assert!(
        meeting.count() > 1,
        "two companions run the mutant at the same time"
    );
    let result = &outcome.results["m1"];
    assert_eq!(result.status, MutantStatus::Survived);
    assert!(result.retried, "a split survivor still gets its retry");
    let all: Vec<String> = FILES.iter().map(|file| file_test_id(file)).collect();
    assert_eq!(result.covering_tests, all);

    let runs = mutant_runs(&spawner);
    assert!(
        runs.iter().all(|(_, tests)| tests.len() == 1),
        "each part holds whole test files: {runs:?}"
    );
    let mut ran: Vec<String> = runs.iter().flat_map(|(_, tests)| tests.clone()).collect();
    ran.sort();
    let mut twice: Vec<String> = all.iter().chain(&all).cloned().collect();
    twice.sort();
    assert_eq!(
        ran, twice,
        "the first run and the retry each run each test one time"
    );
}

#[test]
fn a_split_mutant_names_the_first_killer_in_the_fixed_test_order() {
    // c and d kill. One run of each test names d, so a run that does not
    // split shows the order of the answer, not the test order.
    let spawner = wide_spawner(ADOPT_CAPS, kill_by(&["test/d.test.ts", "test/c.test.ts"]));
    let outcome = run_schedule(&spawner, &wide_plan(), &config(4)).expect("run");

    let result = &outcome.results["m1"];
    assert_eq!(result.status, MutantStatus::Killed);
    assert_eq!(
        result.killed_by.as_deref(),
        Some(file_test_id("test/c.test.ts").as_str())
    );
    assert!(result.duration_ms.is_some());
}

#[test]
fn a_split_kill_matrix_keeps_each_killing_test() {
    let spawner = wide_spawner(ADOPT_CAPS, |run| {
        let killer = run.test_ids[0].clone();
        run_result(run, RunStatus::Killed, Some(&killer))
    });
    let config = ScheduleConfig {
        kill_matrix: true,
        ..config(4)
    };
    let outcome = run_schedule(&spawner, &wide_plan(), &config).expect("run");

    let runs = mutant_runs(&spawner);
    assert!(runs.len() > 1, "the matrix run splits: {runs:?}");
    assert!(runs.iter().all(|(_, tests)| tests.len() == 1));
    let result = &outcome.results["m1"];
    assert_eq!(result.status, MutantStatus::Killed);
    let all: Vec<String> = FILES.iter().map(|file| file_test_id(file)).collect();
    assert_eq!(result.killing_tests, all);
    assert!(result.kill_matrix_complete);
    assert_eq!(result.killed_by.as_deref(), Some(all[0].as_str()));
}

#[test]
fn a_runner_without_the_test_index_keeps_one_run_for_each_mutant() {
    let spawner = wide_spawner(VITEST_CAPS, |run| {
        run_result(run, RunStatus::Survived, None)
    });
    let outcome = run_schedule(&spawner, &wide_plan(), &config(4)).expect("run");

    assert_eq!(status(&outcome, "m1"), MutantStatus::Survived);
    let runs = mutant_runs(&spawner);
    assert_eq!(runs.len(), 2, "one first run and one retry: {runs:?}");
}

#[test]
fn a_one_worker_run_does_not_split() {
    let spawner = wide_spawner(ADOPT_CAPS, |run| run_result(run, RunStatus::Survived, None));
    let outcome = run_schedule(&spawner, &wide_plan(), &config(1)).expect("run");

    assert_eq!(status(&outcome, "m1"), MutantStatus::Survived);
    assert_eq!(mutant_runs(&spawner).len(), 2);
}

/// The mutant, the tests and the test files of one key query.
type KeyQuery = (String, Vec<String>, Vec<String>);

/// A cache with no stored result that records the key of each mutant.
#[derive(Debug, Default)]
struct KeyLog {
    keys: Mutex<Vec<KeyQuery>>,
}

impl ResultLookup for KeyLog {
    fn key(
        &self,
        _: &Ready,
        mutant: &ScheduledMutant,
        tests: &[String],
        files: &[String],
    ) -> String {
        self.keys
            .lock()
            .expect("keys")
            .push((mutant.id.clone(), tests.to_vec(), files.to_vec()));
        format!("key-{}", mutant.id)
    }

    fn get(&self, _: &str, _: &str) -> Option<CachedResult> {
        None
    }
}

#[test]
fn the_cache_key_of_a_split_mutant_is_the_key_of_the_whole_mutant() {
    let keys_of = |concurrency: usize| {
        let spawner = wide_spawner(ADOPT_CAPS, |run| run_result(run, RunStatus::Survived, None));
        let cache = Arc::new(KeyLog::default());
        let config = ScheduleConfig {
            cache: Some(Arc::clone(&cache) as Arc<dyn ResultLookup>),
            ..config(concurrency)
        };
        let outcome = run_schedule(&spawner, &wide_plan(), &config).expect("run");
        let runs = mutant_runs(&spawner).len();
        let keys = cache.keys.lock().expect("keys").clone();
        (outcome.cache_keys.get("m1").cloned(), keys, runs)
    };
    let (split_key, split_queries, split_runs) = keys_of(4);
    let (whole_key, whole_queries, whole_runs) = keys_of(1);
    assert!(split_runs > whole_runs, "the run with 4 workers splits");
    assert_eq!(split_key, Some("key-m1".to_owned()));
    assert_eq!(split_key, whole_key);
    assert_eq!(split_queries, whole_queries);
}

#[test]
fn a_static_mutant_splits_only_with_fresh_modules_for_each_run() {
    for fresh in [true, false] {
        let caps = Capabilities {
            fresh_modules_per_run: fresh,
            ..ADOPT_CAPS
        };
        let spawner = FakeSpawner::new(move |_, request| match request {
            Request::DryRun { test_files } => Ok(Response::DryRun(DryRunResult {
                static_hits: vec!["m1".to_owned()],
                hits: BTreeMap::new(),
                ..dry_of(test_files)
            })),
            _ => wide_script(caps, request, &|run| {
                run_result(run, RunStatus::Survived, None)
            }),
        });
        let outcome = run_schedule(&spawner, &wide_plan(), &config(4)).expect("run");

        assert_eq!(status(&outcome, "m1"), MutantStatus::Survived);
        assert!(outcome.results["m1"].static_mutant);
        let runs = mutant_runs(&spawner);
        let whole = runs.iter().all(|(_, tests)| tests.len() == FILES.len());
        assert_eq!(whole, !fresh, "fresh modules {fresh}: {runs:?}");
    }
}
