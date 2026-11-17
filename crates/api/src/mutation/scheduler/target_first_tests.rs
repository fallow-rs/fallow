//! The target-first runs of `--kill-matrix-for`: a mutant that no target
//! kills runs no other test and gets no retry, and a unique target kill gets
//! one retry.

use super::*;

const TARGET: &str = "target";

/// The covering tests of each mutant, and the tests that kill it:
/// - `m_unique`: only the target kills it.
/// - `m_shared`: the target and `o2` kill it.
/// - `m_others`: only `o2` kills it.
/// - `m_survivor`: no test kills it.
const MUTANTS: [(&str, &[&str], &[&str]); 4] = [
    ("m_unique", &[TARGET, "o1"], &[TARGET]),
    ("m_shared", &[TARGET, "o1", "o2"], &[TARGET, "o2"]),
    ("m_others", &[TARGET, "o1", "o2"], &["o2"]),
    ("m_survivor", &[TARGET, "o1", "o3"], &[]),
];

fn dry() -> DryRunResult {
    let mut hits: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (mutant, covering, _) in MUTANTS {
        for test in covering {
            hits.entry(tid(test)).or_default().push(mutant.to_owned());
        }
    }
    DryRunResult {
        tests: vec![
            test(TARGET, 5.0, TestStatus::Pass),
            test("o1", 1.0, TestStatus::Pass),
            test("o2", 2.0, TestStatus::Pass),
            test("o3", 3.0, TestStatus::Pass),
        ],
        hits,
        static_hits: Vec::new(),
        file_hits: None,
        unhandled_errors: Vec::new(),
        unloaded_paths: Vec::new(),
        file_setup_ms: BTreeMap::new(),
        duplicate_test_ids: Vec::new(),
    }
}

fn killers_of(mutant: &str) -> Vec<String> {
    MUTANTS
        .iter()
        .find(|(id, _, _)| *id == mutant)
        .map(|(_, _, killers)| killers.iter().map(|test| tid(test)).collect())
        .unwrap_or_default()
}

/// A run in which the killers of the active mutant fail. `flaky` names a
/// mutant whose target kill holds only in its first run.
fn respond(run: &RunRequest, flaky: Option<&str>, seen: &Mutex<Vec<String>>) -> Response {
    let Some(mutant) = run.mutant_id.as_deref() else {
        return passing_control(run);
    };
    let mut killers = killers_of(mutant);
    if run.test_ids.contains(&tid(TARGET)) && flaky == Some(mutant) {
        let mut seen = seen.lock().expect("seen");
        if seen.iter().any(|id| id == mutant) {
            killers.clear();
        }
        seen.push(mutant.to_owned());
    }
    killed_by(run, &killers)
}

/// The run result in which each test of `killers` fails.
fn killed_by(run: &RunRequest, killers: &[String]) -> Response {
    let tests: Vec<RunTest> = run
        .test_ids
        .iter()
        .map(|id| {
            let fails = killers.contains(id);
            RunTest {
                test_id: id.clone(),
                status: if fails {
                    TestStatus::Fail
                } else {
                    TestStatus::Pass
                },
                failure_kind: fails.then(|| "assertion".to_owned()),
                message: None,
            }
        })
        .collect();
    let first = run.test_ids.iter().find(|id| killers.contains(id)).cloned();
    Response::Run(RunResult {
        mutant_id: run.mutant_id.clone(),
        status: if first.is_some() {
            RunStatus::Killed
        } else {
            RunStatus::Survived
        },
        killed_by: first,
        duration_ms: Some(3.0),
        tests_executed: tests.len(),
        tests,
        unhandled_errors: Vec::new(),
        loop_timeout: false,
    })
}

fn run_mode(full: bool, flaky: Option<&'static str>) -> (ScheduleOutcome, FakeSpawner) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let spawner = FakeSpawner::new(script(
        |_| dry(),
        move |_, run| Ok(respond(run, flaky, &seen)),
    ));
    let mut config = config(1);
    config.targets = vec![tid(TARGET)];
    config.target_full = full;
    let ids: Vec<&str> = MUTANTS.iter().map(|(id, _, _)| *id).collect();
    let outcome = run_schedule(&spawner, &plan(&ids), &config).expect("schedule");
    (outcome, spawner)
}

/// The test runs with an active mutant, as (mutant, short test names, bail).
fn mutant_runs(spawner: &FakeSpawner) -> Vec<(String, Vec<String>, bool)> {
    spawner
        .runs()
        .into_iter()
        .filter_map(|(_, run)| {
            let mutant = run.mutant_id?;
            let tests = run
                .test_ids
                .iter()
                .map(|test| test.trim_start_matches("test/a.test.ts::").to_owned())
                .collect();
            Some((mutant, tests, run.bail))
        })
        .collect()
}

fn runs_of<'a>(runs: &'a [(String, Vec<String>, bool)], mutant: &str) -> Vec<(&'a [String], bool)> {
    runs.iter()
        .filter(|(id, _, _)| id == mutant)
        .map(|(_, tests, bail)| (tests.as_slice(), *bail))
        .collect()
}

fn names(tests: &[&str]) -> Vec<String> {
    tests.iter().map(|test| (*test).to_owned()).collect()
}

#[test]
fn a_mutant_that_no_target_kills_runs_only_the_target_tests() {
    let (outcome, spawner) = run_mode(false, None);
    let runs = mutant_runs(&spawner);
    for mutant in ["m_others", "m_survivor"] {
        let result = &outcome.results[mutant];
        assert_eq!(result.status, MutantStatus::NotKilledByTarget, "{mutant}");
        assert!(result.kill_matrix_complete, "{mutant}");
        assert!(result.killing_tests.is_empty(), "{mutant}");
        assert!(!result.retried, "{mutant} gets no survivor retry");
        assert_eq!(
            runs_of(&runs, mutant),
            [(names(&[TARGET]).as_slice(), false)],
            "{mutant} runs no other covering test"
        );
    }
}

#[test]
fn a_shared_target_kill_stops_at_the_cheapest_other_killer_and_gets_no_retry() {
    let (outcome, spawner) = run_mode(false, None);
    let runs = mutant_runs(&spawner);
    let shared = &outcome.results["m_shared"];
    assert_eq!(shared.status, MutantStatus::Killed);
    assert_eq!(shared.killing_tests, [tid("o2"), tid(TARGET)]);
    assert!(shared.kill_matrix_complete);
    assert!(!shared.retried);
    assert_eq!(
        runs_of(&runs, "m_shared"),
        [
            (names(&[TARGET]).as_slice(), false),
            (names(&["o1", "o2"]).as_slice(), true),
        ],
        "the other tests run cheapest first with bail"
    );
}

#[test]
fn a_unique_target_kill_gets_one_retry() {
    let (outcome, spawner) = run_mode(false, None);
    let runs = mutant_runs(&spawner);
    let unique = &outcome.results["m_unique"];
    assert_eq!(unique.status, MutantStatus::Killed);
    assert_eq!(unique.killing_tests, [tid(TARGET)]);
    assert!(unique.kill_matrix_complete);
    assert!(unique.retried);
    assert_eq!(unique.attempts.len(), 2);
    assert_eq!(
        runs_of(&runs, "m_unique"),
        [
            (names(&[TARGET]).as_slice(), false),
            (names(&["o1"]).as_slice(), true),
            (names(&[TARGET]).as_slice(), false),
            (names(&["o1"]).as_slice(), true),
        ]
    );
    assert_eq!(runs.len(), 8, "target-first runs: {runs:?}");
}

#[test]
fn a_unique_target_kill_that_the_retry_does_not_repeat_is_flaky() {
    let (outcome, _) = run_mode(false, Some("m_unique"));
    let unique = &outcome.results["m_unique"];
    assert_eq!(unique.status, MutantStatus::Flaky);
    assert!(
        !unique.kill_matrix_complete,
        "a flaky kill must not give a unique kill"
    );
}

#[test]
fn the_full_mode_runs_each_covering_test_and_retries_each_survivor() {
    let (full, spawner) = run_mode(true, None);
    let runs = mutant_runs(&spawner);
    assert_eq!(full.results["m_others"].status, MutantStatus::Killed);
    assert_eq!(full.results["m_others"].killing_tests, [tid("o2")]);
    let survivor = &full.results["m_survivor"];
    assert_eq!(survivor.status, MutantStatus::Survived);
    assert!(survivor.retried);
    // `o2` is in the same test file but does not cover the survivor. Target
    // mode never widens the retry to whole files, so the retry runs the
    // tests of the first run, and both modes see the same target tests.
    assert_eq!(
        runs_of(&runs, "m_survivor"),
        [
            (names(&[TARGET]).as_slice(), false),
            (names(&["o1", "o3"]).as_slice(), true),
            (names(&[TARGET]).as_slice(), false),
            (names(&["o1", "o3"]).as_slice(), true),
        ]
    );
    assert!(!full.results["m_unique"].retried);
    assert_eq!(runs.len(), 10, "full-mode runs: {runs:?}");
    let (first, _) = run_mode(false, None);
    for mutant in ["m_unique", "m_shared"] {
        assert_eq!(
            (
                full.results[mutant].status,
                &full.results[mutant].killing_tests
            ),
            (
                first.results[mutant].status,
                &first.results[mutant].killing_tests
            ),
            "{mutant}: both modes give the same target evidence"
        );
    }
}

#[test]
fn the_two_target_modes_never_share_a_cache_key() {
    let keys = |full: bool| -> String {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let spawner = FakeSpawner::new(script(
            |_| dry(),
            move |_, run| Ok(respond(run, None, &seen)),
        ));
        let mut config = config(1);
        config.targets = vec![tid(TARGET)];
        config.target_full = full;
        config.cache = Some(Arc::new(FakeCache::default()) as Arc<dyn ResultLookup>);
        let ids: Vec<&str> = MUTANTS.iter().map(|(id, _, _)| *id).collect();
        let outcome = run_schedule(&spawner, &plan(&ids), &config).expect("schedule");
        outcome.cache_keys["m_unique"].clone()
    };
    let target_first = keys(false);
    let full = keys(true);
    assert!(target_first.starts_with("targets-first:"), "{target_first}");
    assert!(full.starts_with("targets:"), "{full}");
}

#[test]
fn a_retry_that_adds_an_other_killer_makes_a_unique_kill_flaky() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let other_runs = Arc::new(Mutex::new(0_usize));
    let spawner = FakeSpawner::new(script(
        |_| dry(),
        move |_, run| {
            if run.mutant_id.as_deref() == Some("m_unique") && run.test_ids == [tid("o1")] {
                let mut count = other_runs.lock().expect("other runs");
                *count += 1;
                // `o1` passes in the first run and fails in the retry.
                if *count == 2 {
                    return Ok(killed_by(run, &[tid("o1")]));
                }
            }
            Ok(respond(run, None, &seen))
        },
    ));
    let journal = Arc::new(FakeJournal {
        resume: FxHashMap::default(),
        appended: Mutex::new(Vec::new()),
    });
    let mut config = config(1);
    config.targets = vec![tid(TARGET)];
    config.journal = Some(Arc::clone(&journal) as Arc<dyn OutcomeJournal>);
    let ids: Vec<&str> = MUTANTS.iter().map(|(id, _, _)| *id).collect();
    let outcome = run_schedule(&spawner, &plan(&ids), &config).expect("schedule");
    let unique = &outcome.results["m_unique"];
    assert_eq!(unique.status, MutantStatus::Flaky);
    assert!(
        !journal.ids().contains(&"m_unique".to_owned()),
        "a flaky result runs again on resume"
    );
    assert!(
        !unique.kill_matrix_complete,
        "the two runs disagree about the other killers"
    );
    assert!(unique.retried);
}

#[test]
fn the_journal_records_a_confirmed_unique_kill() {
    let journal = Arc::new(FakeJournal {
        resume: FxHashMap::default(),
        appended: Mutex::new(Vec::new()),
    });
    let seen = Arc::new(Mutex::new(Vec::new()));
    let spawner = FakeSpawner::new(script(
        |_| dry(),
        move |_, run| Ok(respond(run, None, &seen)),
    ));
    let mut config = config(1);
    config.targets = vec![tid(TARGET)];
    config.journal = Some(Arc::clone(&journal) as Arc<dyn OutcomeJournal>);
    let ids: Vec<&str> = MUTANTS.iter().map(|(id, _, _)| *id).collect();
    let outcome = run_schedule(&spawner, &plan(&ids), &config).expect("schedule");
    assert!(outcome.results["m_unique"].retried);
    let mut journaled = journal.ids();
    journaled.sort();
    assert_eq!(
        journaled,
        ["m_others", "m_shared", "m_survivor", "m_unique"],
        "a resumed run does not retry a confirmed unique kill again"
    );
}
