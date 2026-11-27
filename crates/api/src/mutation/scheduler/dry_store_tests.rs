//! The dry run of a run with stored dry-run facts.

use super::adopt_tests::{ADOPT_CAPS, Taken, adopt_plan, adopt_spawner, count_of};
use super::shard_tests::{FILES, dry_of, dry_runs, shard_plan, shard_spawner, verdicts};
use super::*;

/// A store in memory. The key of a file is its name and its version, so a
/// test changes the key of a file with [`MemoryStore::change`].
#[derive(Debug, Default)]
struct MemoryStore {
    versions: Mutex<FxHashMap<String, u32>>,
    entries: Mutex<FxHashMap<String, (String, StoredDryRun)>>,
}

impl MemoryStore {
    fn change(&self, file: &str) {
        *self
            .versions
            .lock()
            .unwrap()
            .entry(file.to_owned())
            .or_default() += 1;
    }

    fn put(&self, entries: &[DryRunEntry]) {
        let mut stored = self.entries.lock().unwrap();
        for entry in entries {
            stored.insert(
                entry.test_file.clone(),
                (entry.key.clone(), entry.facts.clone()),
            );
        }
    }

    fn edit(&self, file: &str, edit: impl FnOnce(&mut StoredDryRun)) {
        let mut stored = self.entries.lock().unwrap();
        edit(&mut stored.get_mut(file).expect("stored file").1);
    }
}

impl DryRunLookup for MemoryStore {
    fn key(&self, ready: &Ready, test_file: &str) -> String {
        let version = self
            .versions
            .lock()
            .unwrap()
            .get(test_file)
            .copied()
            .unwrap_or(0);
        format!("{test_file}@{version}@{}", ready.runner_version)
    }

    fn get(&self, test_file: &str, key: &str) -> Option<StoredDryRun> {
        self.entries
            .lock()
            .unwrap()
            .get(test_file)
            .filter(|(stored, _)| stored == key)
            .map(|(_, facts)| facts.clone())
    }
}

fn stored_config(store: &Arc<MemoryStore>, concurrency: usize) -> ScheduleConfig {
    let mut config = config(concurrency);
    config.dry_runs = Some(Arc::clone(store) as Arc<dyn DryRunLookup>);
    config.init.path_map = ["src/a.ts", "src/never.ts"]
        .into_iter()
        .map(|path| (path.to_owned(), format!("run/{path}")))
        .collect();
    config
}

/// The plan of `ids` in which `a` and `b` reach `src/a.ts`, and `c` reaches
/// no source file. The static test files of `src/a.ts` stay each file, so
/// the holder rule must use the reach.
fn reach_plan(ids: &[&str]) -> SchedulePlan {
    SchedulePlan {
        reaching_test_files: BTreeMap::from([(
            "src/a.ts".to_owned(),
            vec!["test/a.test.ts".to_owned(), "test/b.test.ts".to_owned()],
        )]),
        ..shard_plan(ids)
    }
}

/// Runs the plan of `ids` with the store and adds the new entries to the
/// store.
fn stored_run(
    store: &Arc<MemoryStore>,
    ids: &[&str],
    concurrency: usize,
) -> (ScheduleOutcome, FakeSpawner) {
    let spawner = shard_spawner(None);
    let outcome = run_schedule(
        &spawner,
        &reach_plan(ids),
        &stored_config(store, concurrency),
    )
    .expect("run");
    store.put(&outcome.dry_run_entries);
    (outcome, spawner)
}

fn cold(ids: &[&str]) -> ScheduleOutcome {
    run_schedule(&shard_spawner(None), &reach_plan(ids), &config(1)).expect("run")
}

fn dry_files(spawner: &FakeSpawner) -> Vec<String> {
    let mut files: Vec<String> = dry_runs(spawner)
        .into_iter()
        .flat_map(|(_, files)| files)
        .collect();
    files.sort();
    files
}

#[test]
fn a_repeat_run_with_each_file_stored_sends_no_dry_run() {
    for concurrency in [1, 3] {
        let store = Arc::new(MemoryStore::default());
        // No test hits `m9`, so the run has no mutant job, as a run in
        // which each result comes from the result cache.
        let (first, _) = stored_run(&store, &["m9"], concurrency);
        assert_eq!(first.dry_run_reused, 0);
        assert_eq!(first.dry_run_missed, FILES.len());
        assert_eq!(first.dry_run_entries.len(), FILES.len());

        let (second, spawner) = stored_run(&store, &["m9"], concurrency);
        assert_eq!(second.dry_run_reused, FILES.len());
        assert_eq!(second.dry_run_missed, 0);
        assert!(
            dry_runs(&spawner).is_empty(),
            "no companion dry-runs a file: {:?}",
            dry_runs(&spawner)
        );
        assert_eq!(spawner.spawns.load(Ordering::SeqCst), 1);
        assert!(second.dry_run_entries.is_empty());
        let expected = cold(&["m9"]);
        assert_eq!(verdicts(&second), verdicts(&expected));
        assert_eq!(second.test_files, expected.test_files);
        assert_eq!(second.unloaded_paths, vec!["src/never.ts".to_owned()]);
    }
}

#[test]
fn stored_facts_give_the_verdicts_of_a_run_without_the_store() {
    for concurrency in [1, 3] {
        let store = Arc::new(MemoryStore::default());
        stored_run(&store, &["m1"], concurrency);
        let (warm, _) = stored_run(&store, &["m0", "m1", "m2", "m3"], concurrency);
        assert_eq!(warm.dry_run_reused, FILES.len());
        let expected = cold(&["m0", "m1", "m2", "m3"]);
        assert_eq!(verdicts(&warm), verdicts(&expected));
        assert_eq!(warm.baseline_failures, expected.baseline_failures);
        for id in ["m0", "m1", "m2", "m3"] {
            assert_eq!(
                warm.results[id].covering_tests, expected.results[id].covering_tests,
                "{id}"
            );
        }
    }
}

#[test]
fn only_a_file_with_a_changed_key_dry_runs_again() {
    let store = Arc::new(MemoryStore::default());
    stored_run(&store, &["m1"], 1);
    store.change("test/c.test.ts");
    let (warm, spawner) = stored_run(&store, &["m1", "m2"], 1);
    assert_eq!(dry_runs(&spawner)[0].1, vec!["test/c.test.ts".to_owned()]);
    assert_eq!((warm.dry_run_reused, warm.dry_run_missed), (2, 1));
    assert_eq!(
        warm.dry_run_entries
            .iter()
            .map(|entry| entry.test_file.as_str())
            .collect::<Vec<_>>(),
        vec!["test/c.test.ts"]
    );
    assert_eq!(verdicts(&warm), verdicts(&cold(&["m1", "m2"])));
    let runs = spawner.runs();
    assert!(!runs.is_empty());
    assert!(
        dry_runs(&spawner)
            .iter()
            .skip(1)
            .all(|(_, files)| !files.contains(&"test/c.test.ts".to_owned())),
        "a companion of the mutant phase dry-runs the files of its jobs, not the new file again"
    );
}

#[test]
fn a_file_whose_share_facts_another_changed_file_can_give_dry_runs_again() {
    // `a` and `b` reach `src/a.ts`, and their share loaded it. `c` reaches
    // no source file. A change of `b` can remove the load that `a` keeps.
    let store = Arc::new(MemoryStore::default());
    stored_run(&store, &["m1"], 1);
    store.change("test/b.test.ts");
    let (warm, spawner) = stored_run(&store, &["m1"], 1);
    assert_eq!(
        dry_runs(&spawner)[0].1,
        vec!["test/a.test.ts".to_owned(), "test/b.test.ts".to_owned()]
    );
    assert_eq!(warm.dry_run_reused, 1);
}

#[test]
fn a_static_hit_of_a_share_stays_with_the_files_that_reach_its_source() {
    let spawner = FakeSpawner::new(|index, request| match request {
        Request::DryRun { test_files } => {
            let mut dry = dry_of(test_files, None);
            if test_files.iter().any(|file| file == "test/b.test.ts") {
                dry.static_hits = vec!["m1".to_owned()];
            }
            Ok(Response::DryRun(dry))
        }
        _ => (shard_spawner(None).script)(index, request),
    });
    let store = Arc::new(MemoryStore::default());
    let outcome =
        run_schedule(&spawner, &reach_plan(&["m1"]), &stored_config(&store, 1)).expect("run");
    let facts = |file: &str| {
        outcome
            .dry_run_entries
            .iter()
            .find(|entry| entry.test_file == file)
            .map(|entry| entry.facts.clone())
            .expect("entry")
    };
    let a = facts("test/a.test.ts");
    assert_eq!(a.static_hits, vec!["m1".to_owned()]);
    assert_eq!(a.bound, vec!["test/b.test.ts".to_owned()]);
    assert_eq!(a.loaded_paths, vec!["src/a.ts".to_owned()]);
    let c = facts("test/c.test.ts");
    assert!(c.static_hits.is_empty() && c.loaded_paths.is_empty() && c.bound.is_empty());
}

#[test]
fn stored_facts_with_a_mutant_id_outside_the_plan_dry_run_again() {
    let store = Arc::new(MemoryStore::default());
    stored_run(&store, &["m1"], 1);
    store.edit("test/c.test.ts", |facts| {
        facts
            .hits
            .insert("test/c.test.ts::v1".to_owned(), vec!["gone".to_owned()]);
    });
    let (warm, spawner) = stored_run(&store, &["m1"], 1);
    assert_eq!(dry_runs(&spawner)[0].1, vec!["test/c.test.ts".to_owned()]);
    assert_eq!(warm.dry_run_reused, 2);
}

#[test]
fn a_file_with_a_failed_test_or_an_error_is_not_stored() {
    let spawner = shard_spawner(Some("test/b.test.ts"));
    let store = Arc::new(MemoryStore::default());
    let outcome =
        run_schedule(&spawner, &shard_plan(&["m1"]), &stored_config(&store, 1)).expect("run");
    let files: Vec<&str> = outcome
        .dry_run_entries
        .iter()
        .map(|entry| entry.test_file.as_str())
        .collect();
    assert_eq!(files, vec!["test/a.test.ts", "test/c.test.ts"]);
}

#[test]
fn without_a_store_each_file_dry_runs_and_no_entry_comes_back() {
    let spawner = shard_spawner(None);
    let outcome = run_schedule(&spawner, &shard_plan(&["m1"]), &config(1)).expect("run");
    assert_eq!(dry_files(&spawner)[..3], FILES.map(str::to_owned));
    assert!(outcome.dry_run_entries.is_empty());
    assert_eq!((outcome.dry_run_reused, outcome.dry_run_missed), (0, 0));
}

#[test]
fn a_stored_file_keeps_its_part_of_the_measured_dry_run_time() {
    let clock = FakeClock::new();
    let ticking = Arc::clone(&clock);
    let spawner = FakeSpawner::new(move |index, request| {
        if let Request::DryRun { .. } = request {
            ticking.advance(Duration::from_secs(9));
        }
        (shard_spawner(None).script)(index, request)
    });
    let store = Arc::new(MemoryStore::default());
    let config = ScheduleConfig {
        clock: Arc::clone(&clock) as Arc<dyn Clock>,
        ..stored_config(&store, 1)
    };
    let outcome = run_schedule(&spawner, &reach_plan(&["m9"]), &config).expect("run");
    let total: f64 = outcome
        .dry_run_entries
        .iter()
        .filter_map(|entry| entry.facts.dry_run_ms)
        .sum();
    assert!(
        (total - 9000.0).abs() < 1.0,
        "the stored parts give the measured time of the dry run, not only the test time: {total}"
    );
}

#[test]
fn a_warm_run_gives_new_companions_the_stored_test_index() {
    let store = Arc::new(MemoryStore::default());
    let config = || ScheduleConfig {
        concurrency: 2,
        ..stored_config(&store, 2)
    };
    let cold_taken = Taken::default();
    let cold = adopt_spawner(ADOPT_CAPS, RunStatus::Killed, &cold_taken);
    let first = run_schedule(&cold, &adopt_plan(&["m1", "m2"]), &config()).expect("run");
    store.put(&first.dry_run_entries);
    assert!(
        first.dry_run_entries.iter().all(|entry| entry
            .facts
            .tests
            .iter()
            .all(|test| test.index.is_some())),
        "each stored test keeps its index entry"
    );

    let warm_taken = Taken::default();
    let warm = adopt_spawner(ADOPT_CAPS, RunStatus::Killed, &warm_taken);
    let second = run_schedule(&warm, &adopt_plan(&["m1", "m2"]), &config()).expect("run");
    assert_eq!(second.dry_run_reused, FILES.len());
    assert_eq!(
        count_of(&warm, "dry_run"),
        0,
        "no companion dry-runs a file"
    );
    assert!(
        count_of(&warm, "adopt") > 0,
        "the companions take the stored index"
    );
    assert_eq!(verdicts(&second), verdicts(&first));
}
