use super::*;

#[test]
fn kill_matrix_runs_all_tests_without_bail() {
    let spawner = FakeSpawner::new(script(
        |_| standard_dry(),
        |_, run| Ok(run_result(run, RunStatus::Killed, Some(&tid("t1")))),
    ));
    let mut config = config(1);
    config.kill_matrix = true;
    let outcome = run_schedule(&spawner, &plan(&["m2"]), &config).expect("schedule");
    assert_eq!(outcome.results["m2"].status, MutantStatus::Killed);
    assert!(
        !spawner.runs()[0].1.bail,
        "matrix mode must execute every covering test"
    );
}

fn all_failed(run: &RunRequest) -> Response {
    Response::Run(RunResult {
        mutant_id: run.mutant_id.clone(),
        status: RunStatus::Killed,
        killed_by: run.test_ids.first().cloned(),
        duration_ms: Some(1.0),
        tests_executed: run.test_ids.len(),
        tests: run
            .test_ids
            .iter()
            .map(|id| RunTest {
                test_id: id.clone(),
                status: TestStatus::Fail,
                failure_kind: Some("assertion".to_owned()),
                message: None,
            })
            .collect(),
        unhandled_errors: Vec::new(),
    })
}

#[test]
fn a_normal_cache_entry_cannot_satisfy_matrix_mode() {
    let spawner = FakeSpawner::new(script(|_| standard_dry(), |_, run| Ok(all_failed(run))));
    let mut config = config(1);
    config.kill_matrix = true;
    config.cache = Some(Arc::new(FakeCache::default()));
    let outcome = run_schedule(&spawner, &plan(&["m1"]), &config).expect("schedule");
    assert!(
        !outcome.results["m1"].cached,
        "a first-killer entry cannot provide complete evidence"
    );
    assert_eq!(outcome.results["m1"].killing_tests, [tid("t1")]);
}

#[test]
fn exact_matrix_membership_is_required_and_all_killers_are_retained() {
    for defect in [
        "none",
        "missing",
        "duplicate",
        "unexpected",
        "skipped",
        "timeout",
        "unhandled",
        "unsettled",
    ] {
        let spawner = FakeSpawner::new(script(
            |_| standard_dry(),
            move |_, run| {
                let Response::Run(mut response) = all_failed(run) else {
                    unreachable!()
                };
                match defect {
                    "missing" => {
                        response.tests.pop();
                        response.tests_executed -= 1;
                    }
                    "duplicate" => {
                        response.tests[1] = response.tests[0].clone();
                    }
                    "unexpected" => {
                        response.tests[1].test_id = tid("unexpected");
                    }
                    "skipped" => {
                        response.tests[1].status = TestStatus::Skip;
                    }
                    "timeout" => {
                        response.tests[1].failure_kind = Some("timeout".to_owned());
                    }
                    "unhandled" => {
                        response.unhandled_errors.push(UnhandledError {
                            file: None,
                            message: "unsettled error".to_owned(),
                        });
                    }
                    "unsettled" => {
                        response.tests_executed = 1;
                    }
                    _ => {}
                }
                Ok(Response::Run(response))
            },
        ));
        let mut config = config(1);
        config.kill_matrix = true;
        let outcome = run_schedule(&spawner, &plan(&["m2"]), &config).expect("schedule");
        let result = &outcome.results["m2"];
        assert_eq!(result.kill_matrix_complete, defect == "none", "{defect}");
        if defect == "none" {
            assert_eq!(result.killing_tests, [tid("t1"), tid("t2")]);
            assert_eq!(result.killed_by, Some(tid("t1")));
        }
        if defect == "timeout" {
            assert_eq!(result.killing_tests, [tid("t1")]);
        }
    }
}

#[test]
fn matrix_survivor_retry_keeps_the_exact_covering_test_set() {
    let spawner = FakeSpawner::new(script(
        |_| standard_dry(),
        |_, run| Ok(run_result(run, RunStatus::Survived, None)),
    ));
    let mut config = config(1);
    config.kill_matrix = true;
    let outcome = run_schedule(&spawner, &plan(&["m1"]), &config).expect("schedule");
    let result = &outcome.results["m1"];
    assert_eq!(result.status, MutantStatus::Survived);
    assert!(result.retried && result.kill_matrix_complete);
    assert_eq!(
        result
            .attempts
            .iter()
            .map(|attempt| attempt.kill_matrix_complete)
            .collect::<Vec<_>>(),
        [true, true]
    );
    assert_eq!(
        spawner
            .runs()
            .iter()
            .map(|(_, run)| run.test_ids.clone())
            .collect::<Vec<_>>(),
        [vec![tid("t1")], vec![tid("t1")]]
    );
}

#[test]
fn matrix_timeout_retry_uses_only_the_complete_retry_killers() {
    let spawner = FakeSpawner::new(script(
        |_| standard_dry(),
        |index, run| {
            if index == 0 {
                Ok(run_result(run, RunStatus::Timeout, None))
            } else {
                Ok(all_failed(run))
            }
        },
    ));
    let mut config = config(1);
    config.kill_matrix = true;
    let outcome = run_schedule(&spawner, &plan(&["m2"]), &config).expect("schedule");
    let result = &outcome.results["m2"];
    assert_eq!(result.status, MutantStatus::Killed);
    assert_eq!(result.killing_tests, [tid("t1"), tid("t2")]);
    assert_eq!(result.attempts[0].status, MutantStatus::Timeout);
    assert!(result.attempts[0].killing_tests.is_empty());
    assert!(!result.attempts[0].kill_matrix_complete);
    assert_eq!(result.attempts[1].killing_tests, [tid("t1"), tid("t2")]);
    assert!(result.kill_matrix_complete);
}

#[test]
fn a_late_baseline_failure_leaves_matrix_evidence_incomplete() {
    let spawner = FakeSpawner::new(script(
        |index| {
            let mut dry = standard_dry();
            if index > 0 {
                dry.tests[0].status = TestStatus::Fail;
            }
            dry
        },
        |index, run| {
            if index == 0 {
                Ok(run_result(run, RunStatus::Timeout, None))
            } else {
                Ok(all_failed(run))
            }
        },
    ));
    let mut config = config(1);
    config.kill_matrix = true;
    let outcome = run_schedule(&spawner, &plan(&["m2"]), &config).expect("schedule");
    let result = &outcome.results["m2"];
    assert_eq!(result.status, MutantStatus::Killed);
    assert_eq!(result.killing_tests, [tid("t2")]);
    assert!(!result.kill_matrix_complete);
    assert_eq!(
        outcome
            .baseline_failures
            .iter()
            .filter_map(|failure| failure.test_id.clone())
            .collect::<Vec<_>>(),
        [tid("t1")]
    );
}
