use super::super::dry_facts::DUPLICATE_TEST_IDS_CODE;
use super::*;

/// The mutant runs: the tests and the bail flag of each run with an active
/// mutant. Control runs are left out.
fn mutant_runs(spawner: &FakeSpawner) -> Vec<(Option<String>, Vec<String>, bool)> {
    spawner
        .runs()
        .into_iter()
        .filter(|(_, run)| run.mutant_id.is_some())
        .map(|(_, run)| (run.mutant_id, run.test_ids, run.bail))
        .collect()
}

/// A run in which `killers` fail. A control run, with no active mutant,
/// passes.
fn killed_by_each(run: &RunRequest, killers: &[String]) -> Response {
    if run.mutant_id.is_none() {
        return killed_by_each_always(run, &[]);
    }
    killed_by_each_always(run, killers)
}

/// A run in which `killers` fail, also with no active mutant.
fn killed_by_each_always(run: &RunRequest, killers: &[String]) -> Response {
    let tests: Vec<RunTest> = run
        .test_ids
        .iter()
        .map(|id| RunTest {
            test_id: id.clone(),
            status: if killers.contains(id) {
                TestStatus::Fail
            } else {
                TestStatus::Pass
            },
            failure_kind: killers.contains(id).then(|| "assertion".to_owned()),
            message: None,
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
    })
}

fn target_config(targets: &[String]) -> ScheduleConfig {
    let mut config = config(1);
    config.targets = targets.to_vec();
    config
}

/// m1: only t1 covers it. m2: t1 and t2 cover it.
fn run_targets(
    targets: &[String],
    killers: &'static [&'static str],
) -> (ScheduleOutcome, FakeSpawner) {
    let spawner = FakeSpawner::new(script(
        |_| standard_dry(),
        move |_, run| {
            let killers: Vec<String> = killers.iter().map(|id| tid(id)).collect();
            Ok(killed_by_each(run, &killers))
        },
    ));
    let outcome =
        run_schedule(&spawner, &plan(&["m1", "m2"]), &target_config(targets)).expect("schedule");
    (outcome, spawner)
}

#[test]
fn targets_run_without_bail_then_the_other_tests_run_with_bail() {
    let (outcome, spawner) = run_targets(&[tid("t2")], &["t2"]);
    assert_eq!(
        mutant_runs(&spawner),
        [
            (Some("m2".to_owned()), vec![tid("t2")], false),
            (Some("m2".to_owned()), vec![tid("t1")], true),
        ],
        "only the mutant that a target covers runs"
    );
    let m2 = &outcome.results["m2"];
    assert_eq!(m2.status, MutantStatus::Killed);
    assert_eq!(m2.killed_by, Some(tid("t2")));
    assert_eq!(m2.killing_tests, [tid("t2")]);
    assert!(m2.kill_matrix_complete);
    assert_eq!(outcome.results["m1"].status, MutantStatus::Pruned);
    assert_eq!(
        outcome.targets,
        [targets::ResolvedTarget {
            target: tid("t2"),
            test_ids: vec![tid("t2")],
        }]
    );
}

#[test]
fn the_first_other_killer_joins_the_target_killers() {
    let (outcome, _) = run_targets(&[tid("t2")], &["t1", "t2"]);
    let m2 = &outcome.results["m2"];
    assert_eq!(m2.status, MutantStatus::Killed);
    assert_eq!(m2.killed_by, Some(tid("t2")));
    assert_eq!(m2.killing_tests, [tid("t1"), tid("t2")]);
    assert!(
        m2.kill_matrix_complete,
        "the control run of the other tests passed"
    );
}

#[test]
fn an_other_killer_that_fails_without_the_mutant_leaves_incomplete_evidence() {
    // t1 passes with t2 in the dry run, but fails in a run without t2, also
    // with no active mutant: it depends on the order of the tests.
    let spawner = FakeSpawner::new(script(
        |_| standard_dry(),
        |_, run| Ok(killed_by_each(run, &[tid("t1"), tid("t2")])),
    ))
    .with_control(|_, run| {
        if run.test_ids == [tid("t1")] {
            Ok(killed_by_each_always(run, &[tid("t1")]))
        } else {
            Ok(passing_control(run))
        }
    });
    let outcome =
        run_schedule(&spawner, &plan(&["m2"]), &target_config(&[tid("t2")])).expect("schedule");
    let m2 = &outcome.results["m2"];
    assert_eq!(m2.status, MutantStatus::Killed);
    assert_eq!(m2.killed_by, Some(tid("t2")));
    assert!(
        !m2.kill_matrix_complete,
        "an unconfirmed other killer must not remove the uniqueness of a target kill"
    );
    assert!(
        spawner
            .controls()
            .iter()
            .any(|(_, run)| run.test_ids == [tid("t1")]),
        "the other tests get a control run"
    );
}

#[test]
fn a_target_survivor_gets_its_status_from_the_other_tests() {
    let (outcome, _) = run_targets(&[tid("t2")], &["t1"]);
    let m2 = &outcome.results["m2"];
    assert_eq!(m2.status, MutantStatus::Killed);
    assert_eq!(m2.killed_by, Some(tid("t1")));
    assert_eq!(m2.killing_tests, [tid("t1")]);
    assert!(m2.kill_matrix_complete);
}

#[test]
fn a_file_target_selects_each_test_of_the_file_and_needs_no_other_run() {
    let (outcome, spawner) = run_targets(&["test/a.test.ts".to_owned()], &["t1", "t2"]);
    assert_eq!(
        mutant_runs(&spawner),
        [
            (Some("m1".to_owned()), vec![tid("t1")], false),
            (Some("m2".to_owned()), vec![tid("t1"), tid("t2")], false),
        ]
    );
    assert_eq!(outcome.results["m2"].killing_tests, [tid("t1"), tid("t2")]);
    assert!(outcome.results["m2"].kill_matrix_complete);
    assert_eq!(outcome.targets[0].test_ids, [tid("t1"), tid("t2")]);
}

#[test]
fn a_target_that_selects_no_dry_run_test_stops_the_run() {
    let spawner = FakeSpawner::new(script(
        |_| standard_dry(),
        |_, run| Ok(run_result(run, RunStatus::Survived, None)),
    ));
    let error = run_schedule(
        &spawner,
        &plan(&["m1"]),
        &target_config(&[tid("t1"), "test/missing.test.ts".to_owned()]),
    )
    .expect_err("an unmatched target is an error");
    assert_eq!(
        error,
        ScheduleError::UnmatchedTargets(vec!["test/missing.test.ts".to_owned()])
    );
}

#[test]
fn a_pruned_mutant_leaves_its_place_inside_the_limit() {
    let spawner = FakeSpawner::new(script(
        |_| standard_dry(),
        |_, run| Ok(killed_by_each(run, &[tid("t2")])),
    ));
    let mut plan = plan(&["m1"]);
    plan.pending = vec![ScheduledMutant {
        id: "m2".to_owned(),
        file: "src/a.ts".to_owned(),
    }];
    let outcome = run_schedule(&spawner, &plan, &target_config(&[tid("t2")])).expect("schedule");
    assert_eq!(outcome.results["m1"].status, MutantStatus::Pruned);
    assert_eq!(outcome.results["m2"].status, MutantStatus::Killed);
    assert_eq!(outcome.promoted, ["m2"]);
}

#[test]
fn a_target_timeout_skips_the_other_tests_and_leaves_incomplete_evidence() {
    let spawner = FakeSpawner::new(script(
        |_| standard_dry(),
        |_, run| {
            if run.test_ids == [tid("t2")] {
                Ok(run_result(run, RunStatus::Timeout, None))
            } else {
                Ok(run_result(run, RunStatus::Survived, None))
            }
        },
    ));
    let outcome =
        run_schedule(&spawner, &plan(&["m2"]), &target_config(&[tid("t2")])).expect("schedule");
    let m2 = &outcome.results["m2"];
    assert_eq!(m2.status, MutantStatus::Timeout);
    assert!(!m2.kill_matrix_complete);
    assert!(
        mutant_runs(&spawner)
            .iter()
            .all(|(_, tests, _)| *tests == [tid("t2")]),
        "a timeout of the targets runs no other test"
    );
}

#[test]
fn a_target_killer_that_fails_without_the_other_tests_leaves_incomplete_evidence() {
    // t2 passes with t1 in the dry run, but fails in a run without t1, also
    // with no active mutant: it depends on the order of the tests. Its kill
    // is no evidence, so it must not count as a unique or a shared kill.
    let spawner = FakeSpawner::new(script(
        |_| standard_dry(),
        |_, run| Ok(killed_by_each(run, &[tid("t2")])),
    ))
    .with_control(|_, run| {
        if run.test_ids == [tid("t2")] {
            Ok(killed_by_each_always(run, &[tid("t2")]))
        } else {
            Ok(passing_control(run))
        }
    });
    let outcome =
        run_schedule(&spawner, &plan(&["m2"]), &target_config(&[tid("t2")])).expect("schedule");
    assert!(
        !outcome.results["m2"].kill_matrix_complete,
        "an unconfirmed target killer must not give complete evidence"
    );
    assert!(
        spawner
            .controls()
            .iter()
            .any(|(_, run)| run.test_ids == [tid("t2")]),
        "the target tests get a control run when the other tests are left out"
    );
}

/// A dry run in which t2 shares its title with another test.
fn dry_with_duplicate_t2() -> DryRunResult {
    DryRunResult {
        duplicate_test_ids: vec![tid("t2")],
        ..standard_dry()
    }
}

#[test]
fn a_duplicate_test_id_is_reported_and_the_run_goes_on() {
    let spawner = FakeSpawner::new(script(
        |_| dry_with_duplicate_t2(),
        |_, run| Ok(run_result(run, RunStatus::Survived, None)),
    ));
    let outcome = run_schedule(&spawner, &plan(&["m1"]), &config(1)).expect("schedule");
    let issue = outcome
        .issues
        .iter()
        .find(|issue| issue.code == DUPLICATE_TEST_IDS_CODE)
        .expect("a duplicate test id is reported");
    assert!(issue.message.contains(&tid("t2")), "{}", issue.message);
    assert_eq!(outcome.results["m1"].status, MutantStatus::Survived);
}

#[test]
fn a_dry_run_without_duplicate_test_ids_reports_no_duplicate_issue() {
    let spawner = FakeSpawner::new(script(
        |_| standard_dry(),
        |_, run| Ok(run_result(run, RunStatus::Survived, None)),
    ));
    let outcome = run_schedule(&spawner, &plan(&["m1"]), &config(1)).expect("schedule");
    assert!(
        outcome
            .issues
            .iter()
            .all(|issue| issue.code != DUPLICATE_TEST_IDS_CODE)
    );
}

#[test]
fn a_target_that_names_a_duplicate_test_id_stops_the_run() {
    let spawner = FakeSpawner::new(script(
        |_| dry_with_duplicate_t2(),
        |_, run| Ok(run_result(run, RunStatus::Survived, None)),
    ));
    let error = run_schedule(&spawner, &plan(&["m1"]), &target_config(&[tid("t2")]))
        .expect_err("a duplicate target id is an error");
    assert_eq!(
        error,
        ScheduleError::DuplicateTargetTestIds(vec![tid("t2")])
    );
    let message = error.to_string();
    assert!(message.contains(&tid("t2")), "{message}");
    assert!(message.contains("not reliable"), "{message}");
}

#[test]
fn a_target_that_names_a_file_with_a_duplicate_test_id_stops_the_run() {
    let spawner = FakeSpawner::new(script(
        |_| dry_with_duplicate_t2(),
        |_, run| Ok(run_result(run, RunStatus::Survived, None)),
    ));
    let file = test("t2", 1.0, TestStatus::Pass).file;
    let error = run_schedule(&spawner, &plan(&["m1"]), &target_config(&[file]))
        .expect_err("a file target that selects a duplicate id is an error");
    assert_eq!(
        error,
        ScheduleError::DuplicateTargetTestIds(vec![tid("t2")])
    );
}

#[test]
fn a_target_that_avoids_the_duplicate_test_id_runs() {
    let spawner = FakeSpawner::new(script(
        |_| dry_with_duplicate_t2(),
        |_, run| Ok(run_result(run, RunStatus::Survived, None)),
    ));
    let outcome =
        run_schedule(&spawner, &plan(&["m1"]), &target_config(&[tid("t1")])).expect("schedule");
    assert_eq!(outcome.targets[0].test_ids, [tid("t1")]);
}
