use super::*;

#[test]
fn restarted_setup_faults_suggest_serial_runs_only_for_parallel_phases() {
    for stage in ["init", "dry run", "timeout"] {
        for (concurrency, ids, workers) in [
            (2, vec!["m1", "m2"], 2),
            (1, vec!["m1", "m2"], 1),
            (8, vec!["m1"], 1),
        ] {
            let caps = Capabilities {
                fresh_modules_per_run: false,
                ..VITEST_CAPS
            };
            let spawner = FakeSpawner::new(move |index, request| {
                let fault = index > 0
                    && matches!(
                        (stage, request),
                        ("init" | "timeout", Request::Init(_))
                            | ("dry run", Request::DryRun { .. })
                    );
                if fault {
                    return Err(if stage == "timeout" {
                        LinkFault::Timeout
                    } else {
                        LinkFault::Remote(RemoteError {
                            code: "internal".to_owned(),
                            message: "UNIQUE constraint failed: users.id".to_owned(),
                            retryable: false,
                        })
                    });
                }
                match request {
                    Request::Init(_) => Ok(ready(caps, "5.0.4")),
                    Request::DryRun { .. } => Ok(Response::DryRun(DryRunResult {
                        static_hits: vec!["m1".to_owned(), "m2".to_owned()],
                        ..standard_dry()
                    })),
                    Request::Reload => Ok(Response::Reloaded(Reloaded {
                        method: ReloadMethod::RestartRequired,
                    })),
                    Request::Run(run) => Ok(passing_control(run)),
                    Request::Shutdown | Request::Replay(_) => {
                        Err(LinkFault::Protocol("no response".to_owned()))
                    }
                }
            });
            let outcome = run_schedule(&spawner, &plan(&ids), &config(concurrency)).expect("run");
            assert_eq!(outcome.workers, workers);
            for id in &ids {
                let result = &outcome.results[*id];
                assert_eq!(result.status, MutantStatus::RuntimeError);
                assert!(!result.retried);
                assert!(result.attempts.is_empty());
                let issue = outcome
                    .issues
                    .iter()
                    .find(|issue| issue.mutant_id.as_deref() == Some(*id))
                    .expect("runtime issue");
                assert_eq!(issue.code, "runtime-error");
                if stage == "timeout" {
                    assert!(
                        issue.message.contains("--setup-timeout"),
                        "{}",
                        issue.message
                    );
                } else {
                    assert!(
                        issue
                            .message
                            .contains("internal: UNIQUE constraint failed: users.id"),
                        "{}",
                        issue.message
                    );
                }
                assert_eq!(
                    issue.message.contains("--concurrency 1"),
                    workers > 1,
                    "{}",
                    issue.message
                );
                assert_eq!(
                    issue.message.contains("mutation.concurrency"),
                    workers > 1,
                    "{}",
                    issue.message
                );
            }
            assert!(outcome.baseline_failures.is_empty());
        }
    }
}

#[test]
fn restarted_baseline_mismatches_suggest_serial_runs_only_for_parallel_phases() {
    for missing in [false, true] {
        for (concurrency, ids, workers) in [
            (2, vec!["m1", "m2"], 2),
            (1, vec!["m1", "m2"], 1),
            (8, vec!["m1"], 1),
        ] {
            let spawner = FakeSpawner::new(script(
                move |index| {
                    let mut dry = standard_dry();
                    if index > 0 {
                        if missing {
                            dry.tests.remove(0);
                        } else {
                            dry.tests[0] = failed_test(
                                "t1",
                                "EEXIST: shared budget\n    at test/a.test.ts:1:1",
                            );
                        }
                    }
                    dry
                },
                |index, run| {
                    if index == 0 {
                        return Err(LinkFault::Protocol("restart before trial".to_owned()));
                    }
                    Ok(run_result(run, RunStatus::Killed, Some(&run.test_ids[0])))
                },
            ));
            let outcome = run_schedule(&spawner, &plan(&ids), &config(concurrency)).expect("run");
            assert_eq!(outcome.workers, workers);
            assert_eq!(status(&outcome, "m1"), MutantStatus::RuntimeError);
            if ids.contains(&"m2") {
                assert_eq!(status(&outcome, "m2"), MutantStatus::Killed);
                assert_eq!(outcome.results["m2"].killed_by, Some(tid("t2")));
            }
            let issue = outcome
                .issues
                .iter()
                .find(|issue| issue.mutant_id.as_deref() == Some("m1"))
                .expect("runtime issue");
            assert!(
                issue.message.starts_with("no covering test passed"),
                "{}",
                issue.message
            );
            assert_eq!(
                issue.message.contains("--concurrency 1"),
                workers > 1,
                "{}",
                issue.message
            );
            assert_eq!(outcome.baseline_failures.len(), 1);
            let failure = &outcome.baseline_failures[0];
            assert_eq!(failure.test_id, Some(tid("t1")));
            assert_eq!(failure.mutant_ids, ids);
            let message = failure.message.as_deref().expect("baseline message");
            assert!(
                message
                    .starts_with("the test did not pass in the dry run of a restarted companion"),
                "{message}"
            );
            assert_eq!(
                message.contains("--concurrency 1"),
                workers > 1,
                "{message}"
            );
            if !missing {
                let (reason, error) =
                    split_control_run_failure(message).expect("reason and test error");
                assert_eq!(reason.contains("--concurrency 1"), workers > 1, "{reason}");
                assert_eq!(error, "EEXIST: shared budget\n    at test/a.test.ts:1:1");
            }
        }
    }
}

#[test]
fn primary_setup_faults_do_not_suggest_reducing_concurrency() {
    for dry_run in [false, true] {
        let spawner = FakeSpawner::new(move |_, request| {
            if dry_run && matches!(request, Request::Init(_)) {
                return Ok(ready(VITEST_CAPS, "5.0.4"));
            }
            Err(LinkFault::Remote(RemoteError {
                code: "internal".to_owned(),
                message: "UNIQUE constraint failed: users.id".to_owned(),
                retryable: false,
            }))
        });
        let error =
            run_schedule(&spawner, &plan(&["m1", "m2"]), &config(8)).expect_err("setup failure");
        assert!(matches!(
            error,
            ScheduleError::Setup {
                timed_out: false,
                ..
            }
        ));
        let message = error.to_string();
        assert!(
            message.contains("internal: UNIQUE constraint failed: users.id"),
            "{message}"
        );
        assert!(!message.contains("--concurrency 1"), "{message}");
    }
}

#[test]
fn retry_diagnostics_use_the_retry_phase_worker_count() {
    for all_survive in [false, true] {
        for baseline_mismatch in [false, true] {
            let runs = AtomicUsize::new(0);
            let spawner = FakeSpawner::new(move |_, request| {
                let retry = runs.load(Ordering::SeqCst) >= 2;
                match request {
                    Request::Init(_) if retry && !baseline_mismatch => {
                        Err(LinkFault::Remote(RemoteError {
                            code: "internal".to_owned(),
                            message: "EEXIST: shared budget".to_owned(),
                            retryable: false,
                        }))
                    }
                    Request::Init(_) => Ok(ready(VITEST_CAPS, "5.0.4")),
                    Request::DryRun { .. } => {
                        let mut dry = standard_dry();
                        if retry {
                            dry.tests = vec![
                                failed_test("t1", "EEXIST: shared budget"),
                                failed_test("t2", "EEXIST: shared budget"),
                            ];
                        }
                        Ok(Response::DryRun(dry))
                    }
                    Request::Run(run) => {
                        runs.fetch_add(1, Ordering::SeqCst);
                        if !all_survive && run.mutant_id.as_deref() == Some("m2") {
                            Ok(run_result(run, RunStatus::Killed, Some(&run.test_ids[0])))
                        } else {
                            Ok(run_result(run, RunStatus::Survived, None))
                        }
                    }
                    Request::Reload => Ok(Response::Reloaded(Reloaded {
                        method: ReloadMethod::Noop,
                    })),
                    Request::Shutdown | Request::Replay(_) => {
                        Err(LinkFault::Protocol("no response".to_owned()))
                    }
                }
            });
            let outcome = run_schedule(&spawner, &plan(&["m1", "m2"]), &config(8)).expect("run");
            assert_eq!(outcome.workers, 2);
            assert_eq!(status(&outcome, "m1"), MutantStatus::RuntimeError);
            assert!(outcome.results["m1"].retried);
            assert_eq!(
                status(&outcome, "m2"),
                if all_survive {
                    MutantStatus::RuntimeError
                } else {
                    MutantStatus::Killed
                }
            );
            let issue = outcome
                .issues
                .iter()
                .find(|issue| issue.mutant_id.as_deref() == Some("m1"))
                .expect("retry issue");
            assert_eq!(
                issue.message.contains("--concurrency 1"),
                all_survive,
                "{}",
                issue.message
            );
            if baseline_mismatch {
                let message = outcome.baseline_failures[0]
                    .message
                    .as_deref()
                    .expect("baseline message");
                assert!(message.contains("EEXIST: shared budget"), "{message}");
                assert_eq!(
                    message.contains("--concurrency 1"),
                    all_survive,
                    "{message}"
                );
            }
        }
    }
}
