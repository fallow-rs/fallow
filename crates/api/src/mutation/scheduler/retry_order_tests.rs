//! The order and the cost report of the survivor retries (issue #5488).

use super::*;

/// t1 hits m1 and m2. t2 and t3 hit neither: the retry of a survivor adds
/// them.
fn shared_dry() -> DryRunResult {
    DryRunResult {
        tests: vec![
            test("t1", 2.0, TestStatus::Pass),
            test("t2", 100.0, TestStatus::Pass),
            test("t3", 100.0, TestStatus::Pass),
        ],
        hits: BTreeMap::from([(tid("t1"), vec!["m1".to_owned(), "m2".to_owned()])]),
        ..standard_dry()
    }
}

/// The tests of each retry run of `id`.
fn retry_tests(spawner: &FakeSpawner, id: &str) -> Vec<Vec<String>> {
    spawner
        .runs()
        .into_iter()
        .filter(|(index, run)| *index > 0 && run.mutant_id.as_deref() == Some(id))
        .map(|(_, run)| run.test_ids)
        .collect()
}

/// m1 survives each run. m2 survives its first run, and its retry kills it
/// only with t2, a test that reads a value that t1 cached (issue #3670).
fn only_m2_reads_the_cache(index: usize, run: &RunRequest) -> Response {
    if index > 0 && run.mutant_id.as_deref() == Some("m2") && run.test_ids.contains(&tid("t2")) {
        run_result(run, RunStatus::Killed, Some(&tid("t2")))
    } else {
        run_result(run, RunStatus::Survived, None)
    }
}

#[test]
fn a_clean_wide_retry_of_one_survivor_does_not_narrow_the_retry_of_another() {
    // m1 and m2 have the same covering tests. The wide retry of m1 detects
    // nothing, and that proves nothing about m2: only the wide retry of m2
    // runs the test that detects it.
    let spawner = FakeSpawner::new(script(
        |_| shared_dry(),
        |index, run| Ok(only_m2_reads_the_cache(index, run)),
    ));
    let outcome = run_schedule(&spawner, &plan(&["m1", "m2"]), &config(1)).expect("run");
    let wide = vec![tid("t1"), tid("t2"), tid("t3")];
    assert_eq!(retry_tests(&spawner, "m1"), vec![wide.clone()]);
    assert_eq!(retry_tests(&spawner, "m2"), vec![wide]);
    assert_eq!(status(&outcome, "m1"), MutantStatus::Survived);
    assert_eq!(status(&outcome, "m2"), MutantStatus::Killed);
    assert_eq!(outcome.results["m2"].killed_by, Some(tid("t2")));
}

#[test]
fn the_outcome_reports_the_run_time_of_the_wide_retries() {
    let spawner = FakeSpawner::new(script(
        |_| shared_dry(),
        |_, run| Ok(run_result(run, RunStatus::Survived, None)),
    ));
    let outcome = run_schedule(&spawner, &plan(&["m1", "m2"]), &config(1)).expect("run");
    let retries: u64 = ["m1", "m2"]
        .iter()
        .map(|id| {
            outcome.results[*id].attempts[1]
                .duration_ms
                .expect("the retry has a duration")
        })
        .sum();
    assert!(retries > 0);
    assert_eq!(
        outcome.timings.retry_wide_worker,
        Duration::from_millis(retries),
        "the two survivor retries ran each test of their file"
    );
}

#[test]
fn a_retry_with_only_the_first_run_tests_adds_no_wide_time() {
    // m2 is covered by each test of its file, so its retry adds no test.
    let spawner = FakeSpawner::new(script(
        |_| standard_dry(),
        |_, run| Ok(run_result(run, RunStatus::Survived, None)),
    ));
    let outcome = run_schedule(&spawner, &plan(&["m2"]), &config(1)).expect("run");
    assert!(outcome.results["m2"].retried);
    assert_eq!(outcome.timings.retry_wide_worker, Duration::ZERO);
}

/// m2 times out in its first run, so its retry gets a doubled timeout.
/// m1 survives each run.
fn m2_times_out(index: usize, run: &RunRequest) -> Response {
    let status = if index == 0 && run.mutant_id.as_deref() == Some("m2") {
        RunStatus::Timeout
    } else {
        RunStatus::Survived
    };
    run_result(run, status, None)
}

/// The mutant ids of the retry runs, in run order.
fn retry_order_of(spawner: &FakeSpawner) -> Vec<String> {
    spawner
        .runs()
        .into_iter()
        .filter(|(index, _)| *index > 0)
        .filter_map(|(_, run)| run.mutant_id)
        .collect()
}

#[test]
fn a_budgeted_run_retries_a_survivor_before_a_slower_timeout() {
    let run = |config: &ScheduleConfig| {
        let spawner = FakeSpawner::new(script(
            |_| shared_dry(),
            |index, run| Ok(m2_times_out(index, run)),
        ));
        run_schedule(&spawner, &plan(&["m2", "m1"]), config).expect("run");
        retry_order_of(&spawner)
    };
    assert_eq!(
        run(&config(1)),
        ["m2", "m1"],
        "without a budget the order stays"
    );
    let mut budgeted = config(1);
    budgeted.deadline = Some(Instant::now() + Duration::from_secs(3600));
    assert_eq!(
        run(&budgeted),
        ["m1", "m2"],
        "the doubled timeout of m2 costs more than the wide retry of m1"
    );
}

#[test]
fn under_a_time_budget_the_cheapest_retries_run_first() {
    let job = |id: &str, estimate_ms| Job {
        id: id.to_owned(),
        estimate_ms,
        ..load_kill_job()
    };
    let queue = || vec![job("m1", 9_000), job("m2", 10), job("m3", 500)];
    let ids = |jobs: Vec<Job>| jobs.into_iter().map(|job| job.id).collect::<Vec<_>>();
    let mut budgeted = queue();
    retry_order(&mut budgeted, true, |_| false);
    assert_eq!(ids(budgeted), ["m2", "m3", "m1"]);
    let mut unbounded = queue();
    retry_order(&mut unbounded, false, |_| false);
    assert_eq!(
        ids(unbounded),
        ["m1", "m2", "m3"],
        "without a budget the order stays"
    );
}
