//! The dry run and the self-check before the mutant runs.
//!
//! The dry run divides the test files over up to `concurrency` companions,
//! and each companion dry-runs its share at the same time. Each companion
//! then starts the mutant phase with the tests of its share, so no companion
//! dry-runs a file again for a job that its share holds.

use super::*;

/// A companion after its dry run.
pub(super) struct WarmLink {
    pub(super) link: Box<dyn CompanionLink>,
    /// The tests that passed in its dry run.
    pub(super) known: FxHashSet<String>,
    /// The sorted test files of its dry run. `None` means each test file of
    /// the phase.
    pub(super) loaded: Option<Vec<String>>,
    /// The tests that it took from the first dry run and that did not yet
    /// pass in it, with the test file of each.
    pub(super) unproven: FxHashMap<String, String>,
}

/// Shuts down each companion of `links`.
pub(super) fn shutdown_all(links: Vec<WarmLink>) {
    for mut warm in links {
        warm.link.shutdown();
    }
}

/// True when the sorted `loaded` files hold each file of `files`. `None`
/// holds each file.
pub(super) fn holds(loaded: Option<&[String]>, files: &[String]) -> bool {
    loaded.is_none_or(|loaded| files.iter().all(|file| loaded.binary_search(file).is_ok()))
}

/// The sorted test files of each job.
pub(super) fn phase_files(jobs: &[Job]) -> Vec<String> {
    let mut files: Vec<String> = jobs
        .iter()
        .flat_map(|job| job.test_files.iter().cloned())
        .collect();
    files.sort_unstable();
    files.dedup();
    files
}

/// Divides `files` round robin into at most `count` shares, each sorted.
pub(super) fn share_files(files: &[String], count: usize) -> Vec<Vec<String>> {
    let count = count.clamp(1, files.len().max(1));
    let mut shares = vec![Vec::new(); count];
    for (index, file) in files.iter().enumerate() {
        shares[index % count].push(file.clone());
    }
    for share in &mut shares {
        share.sort_unstable();
    }
    shares
}

/// The test files of one dry-run share, with the facts that the runner gives
/// only for the whole share.
pub(super) struct ShareFacts {
    pub(super) files: Vec<String>,
    pub(super) static_hits: Vec<String>,
    pub(super) unloaded_paths: Vec<String>,
    /// The wall time of the dry run of the share.
    pub(super) took: Duration,
}

impl ShareFacts {
    fn of(files: &[String], dry: &DryRunResult, took: Duration) -> Self {
        Self {
            files: files.to_vec(),
            static_hits: dry.static_hits.clone(),
            unloaded_paths: dry.unloaded_paths.clone(),
            took,
        }
    }
}

/// The merged dry run of each share and the companion of each share.
pub(super) struct DryPhase {
    pub(super) result: DryRunResult,
    /// The companion of each share. Its `known` tests are empty until the
    /// scheduler reads the dry-run facts.
    pub(super) links: Vec<WarmLink>,
    /// The sum of the dry-run times of the shares: the time of one dry run
    /// of each file. The timeout of a later dry run scales with it.
    pub(super) serial: Duration,
    /// The facts of each share. A recheck of failed files gives one share
    /// of each file with the merged facts.
    pub(super) shares: Vec<ShareFacts>,
}

type ShareRun = Result<(Box<dyn CompanionLink>, DryRunResult, Duration), ScheduleError>;

/// The dry run of one share on `link`, or on a new companion for `None`.
fn run_share(
    spawner: &dyn CompanionSpawner,
    link: Option<Box<dyn CompanionLink>>,
    files: &[String],
    config: &ScheduleConfig,
) -> ShareRun {
    let mut link = match link {
        Some(link) => link,
        None => start_link(spawner, config)?.0,
    };
    let started = config.clock.now();
    let dry = first_dry_run(&mut link, files, config)?;
    Ok((
        link,
        dry,
        config.clock.now().saturating_duration_since(started),
    ))
}

/// Dry-runs `dry_files` on `primary` and on up to `concurrency - 1` more
/// companions. A fault of any share stops the run, as a fault of the first
/// companion does. With `partial`, `dry_files` are not each test file of the
/// run, so a single share records its files as its loaded files.
pub(super) fn dry_phase(
    spawner: &dyn CompanionSpawner,
    primary: Box<dyn CompanionLink>,
    dry_files: &[String],
    config: &ScheduleConfig,
    partial: bool,
) -> Result<DryPhase, ScheduleError> {
    let shares = share_files(dry_files, config.concurrency);
    if shares.len() == 1 {
        let (link, result, serial) = run_share(spawner, Some(primary), dry_files, config)?;
        let mut loaded = partial.then(|| dry_files.to_vec());
        if let Some(loaded) = &mut loaded {
            loaded.sort_unstable();
        }
        let links = vec![WarmLink {
            link,
            known: FxHashSet::default(),
            loaded,
            unproven: FxHashMap::default(),
        }];
        let shares = vec![ShareFacts::of(dry_files, &result, serial)];
        return Ok(DryPhase {
            result,
            links,
            serial,
            shares,
        });
    }
    let mut primary = Some(primary);
    let runs: Vec<ShareRun> = std::thread::scope(|scope| {
        let handles: Vec<_> = shares
            .iter()
            .map(|files| {
                let link = primary.take();
                scope.spawn(move || run_share(spawner, link, files, config))
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| {
                handle.join().unwrap_or_else(|_| {
                    Err(setup_error(
                        "dry_run",
                        &LinkFault::Protocol("the dry-run thread stopped".to_owned()),
                    ))
                })
            })
            .collect()
    });
    let mut links = Vec::new();
    let mut results = Vec::new();
    let mut facts = Vec::new();
    let mut serial = Duration::ZERO;
    let mut error = None;
    for (run, files) in runs.into_iter().zip(shares) {
        match run {
            Ok((link, dry, took)) => {
                facts.push(ShareFacts::of(&files, &dry, took));
                links.push(WarmLink {
                    link,
                    known: FxHashSet::default(),
                    loaded: Some(files),
                    unproven: FxHashMap::default(),
                });
                results.push(dry);
                serial += took;
            }
            Err(fault) => {
                error.get_or_insert(fault);
            }
        }
    }
    if let Some(error) = error {
        shutdown_all(links);
        return Err(error);
    }
    let rechecked = match recheck_failures(spawner, config, dry_files, &mut results) {
        Ok(took) => took,
        Err(error) => {
            shutdown_all(links);
            return Err(error);
        }
    };
    let result = merge_dry_runs(results, dry_files);
    if let Some(took) = rechecked {
        serial += took;
        facts = vec![ShareFacts::of(dry_files, &result, serial)];
    }
    Ok(DryPhase {
        result,
        links,
        serial,
        shares: facts,
    })
}

/// Runs the failures of the shares again in a new companion, after each
/// share ended, and keeps that result. Shares that run at the same time can
/// fail on shared outside state, for example a port or a fixed file, that a
/// serial dry run never shows. A test file with a failed test or an error is
/// dry-run again. An error outside each test file runs each file again. Gives
/// the time of the new dry run, or `None` when no file failed.
fn recheck_failures(
    spawner: &dyn CompanionSpawner,
    config: &ScheduleConfig,
    dry_files: &[String],
    results: &mut Vec<DryRunResult>,
) -> Result<Option<Duration>, ScheduleError> {
    let global = results
        .iter()
        .flat_map(|dry| &dry.unhandled_errors)
        .any(|error| error.file.is_none());
    let failed: BTreeSet<String> = if global {
        dry_files.iter().cloned().collect()
    } else {
        results
            .iter()
            .flat_map(|dry| {
                let tests = dry
                    .tests
                    .iter()
                    .filter(|test| test.status == TestStatus::Fail)
                    .map(|test| test.file.clone());
                let errors = dry
                    .unhandled_errors
                    .iter()
                    .filter_map(|error| error.file.clone());
                tests.chain(errors).collect::<Vec<_>>()
            })
            .collect()
    };
    if failed.is_empty() {
        return Ok(None);
    }
    let files: Vec<String> = dry_files
        .iter()
        .filter(|file| failed.contains(*file))
        .cloned()
        .collect();
    let (mut link, recheck, took) = run_share(spawner, None, &files, config)?;
    link.shutdown();
    for dry in results.iter_mut() {
        without_files(dry, &failed, global);
    }
    results.push(recheck);
    Ok(Some(took))
}

/// Removes the facts of `files` from a dry run. With `all`, it also removes
/// each error outside a test file and each static hit.
fn without_files(dry: &mut DryRunResult, files: &BTreeSet<String>, all: bool) {
    let removed: FxHashSet<String> = dry
        .tests
        .iter()
        .filter(|test| files.contains(&test.file))
        .map(|test| test.test_id.clone())
        .collect();
    dry.tests.retain(|test| !files.contains(&test.file));
    dry.hits.retain(|test, _| !removed.contains(test));
    dry.duplicate_test_ids
        .retain(|test| !removed.contains(test));
    dry.file_setup_ms.retain(|file, _| !files.contains(file));
    if let Some(file_hits) = &mut dry.file_hits {
        file_hits.retain(|file, _| !files.contains(file));
    }
    dry.unhandled_errors.retain(|error| {
        error
            .file
            .as_ref()
            .is_some_and(|file| !files.contains(file))
    });
    if all {
        dry.static_hits.clear();
    }
}

/// Merges the dry runs of disjoint shares into the dry run of each file.
/// A path is unloaded when no share loads it. The tests keep the order of
/// `dry_files`, and inside a file the order of the runner.
fn merge_dry_runs(results: Vec<DryRunResult>, dry_files: &[String]) -> DryRunResult {
    let mut merged = DryRunResult {
        tests: Vec::new(),
        hits: BTreeMap::new(),
        static_hits: Vec::new(),
        file_hits: None,
        unhandled_errors: Vec::new(),
        unloaded_paths: Vec::new(),
        file_setup_ms: BTreeMap::new(),
        duplicate_test_ids: Vec::new(),
    };
    let mut unloaded: Option<BTreeSet<String>> = None;
    for dry in results {
        merged.tests.extend(dry.tests);
        merged.hits.extend(dry.hits);
        merged.static_hits.extend(dry.static_hits);
        if let Some(file_hits) = dry.file_hits {
            merged
                .file_hits
                .get_or_insert_with(BTreeMap::new)
                .extend(file_hits);
        }
        merged.unhandled_errors.extend(dry.unhandled_errors);
        merged.file_setup_ms.extend(dry.file_setup_ms);
        merged.duplicate_test_ids.extend(dry.duplicate_test_ids);
        let paths: BTreeSet<String> = dry.unloaded_paths.into_iter().collect();
        unloaded = Some(match unloaded {
            None => paths,
            Some(kept) => kept.intersection(&paths).cloned().collect(),
        });
    }
    let order: FxHashMap<&str, usize> = dry_files
        .iter()
        .enumerate()
        .map(|(index, file)| (file.as_str(), index))
        .collect();
    merged
        .tests
        .sort_by_key(|test| order.get(test.file.as_str()).copied().unwrap_or(usize::MAX));
    merged.static_hits.sort_unstable();
    merged.static_hits.dedup();
    merged.duplicate_test_ids.sort_unstable();
    merged.duplicate_test_ids.dedup();
    merged.unloaded_paths = unloaded.unwrap_or_default().into_iter().collect();
    merged
}

/// Sends the dry run on a companion that has sent `ready`. A failure shuts
/// it down.
fn first_dry_run(
    link: &mut Box<dyn CompanionLink>,
    dry_files: &[String],
    config: &ScheduleConfig,
) -> Result<DryRunResult, ScheduleError> {
    let request = Request::DryRun {
        test_files: dry_files.to_vec(),
    };
    let Response::DryRun(dry) = setup_request(link, &request, config.runner_timeout)? else {
        link.shutdown();
        return Err(setup_error(
            "dry_run",
            &LinkFault::Protocol("no dry_run_result".to_owned()),
        ));
    };
    Ok(dry)
}

/// Makes each test of `failed` a baseline failure that covers no mutant, the
/// same as a test that fails in the dry run.
fn drop_failed_tests(facts: &mut DryRunFacts, failed: &[FailedTest]) {
    for test in failed {
        let file = facts.passing.remove(&test.test_id).map(|(file, _)| file);
        for tests in facts.covering.values_mut() {
            tests.remove(&test.test_id);
        }
        facts.baseline.push(BaselineFailure {
            file,
            test_id: Some(test.test_id.clone()),
            message: Some(test.message(CONTROL_RUN_FAILURE)),
            mutant_ids: Vec::new(),
        });
    }
    facts.covering.retain(|_, tests| !tests.is_empty());
    facts.baseline.sort();
}

/// Gives each companion the passing tests of its share. A test that the
/// dry-run facts do not hold (an excluded file or a dropped test) is not
/// known.
fn with_known(links: Vec<WarmLink>, facts: &DryRunFacts) -> Vec<WarmLink> {
    links
        .into_iter()
        .map(|warm| {
            let known = facts
                .passing
                .iter()
                .filter(|(_, (file, _))| holds(warm.loaded.as_deref(), std::slice::from_ref(file)))
                .map(|(test, _)| test.clone())
                .collect();
            WarmLink { known, ..warm }
        })
        .collect()
}

/// The issue tracker that the self-check error names.
const ISSUE_TRACKER: &str = "https://github.com/fallow-rs/fallow/issues";

/// The self-check of the run path. Before the first mutant job, the tests of
/// the first job run with no active mutant, through the same request as a
/// mutant run. The dry run selects whole test files, so it does not use this
/// path. When a runner change breaks the path, each mutant run fails with an
/// error and no executed test, and each mutant looks killed.
///
/// A control run that executes no test, or that gives a result with no
/// failed test that is still not a pass, stops the run. A test that ran and
/// failed is a fact about the tests, for example a test that depends on
/// another test of its file. The check gives these tests back, and the run
/// continues without them. The check needs no run when no job exists (no
/// covered mutant, or each result comes from the cache) or when the time
/// budget already ended.
///
/// The check runs on a companion whose share holds each test file of the
/// job. When no share holds them, a new companion dry-runs only those files.
/// Gives the companions with the companion of the check first.
fn self_check(
    runner: PhaseRunner<'_>,
    files: &[String],
    ready: &Ready,
    jobs: &[Job],
    mut links: Vec<WarmLink>,
) -> Result<(Vec<WarmLink>, Vec<FailedTest>), ScheduleError> {
    let expired = reserve::expired(runner.config);
    let Some(job) = jobs.first().filter(|_| !expired) else {
        return Ok((links, Vec::new()));
    };
    let holder = links
        .iter()
        .position(|warm| holds(warm.loaded.as_deref(), &job.test_files));
    let (warm, scope) = match holder {
        Some(index) => (Some(links.remove(index)), files),
        None => (None, job.test_files.as_slice()),
    };
    let mut worker = Worker::new(
        runner.spawner,
        runner.config,
        scope,
        runner.config.runner_timeout,
        runner.memo,
    );
    worker.index = runner.index;
    if let Some(warm) = warm {
        worker.link = Some(warm.link);
        worker.known_tests = warm.known;
        worker.loaded_files = warm.loaded;
        worker.unproven = warm.unproven;
    }
    let outcome = worker
        .control_run(&job.tests, job, runner.caps)
        .map(|(outcome, _)| outcome);
    let runner_name = format!("{} {}", runner.config.runner.package, ready.runner_version);
    let report =
        format!("report this at {ISSUE_TRACKER} with the runner name and version ({runner_name})");
    let stopped = ScheduleError::RunPath {
        message: format!("the companion of {runner_name} stopped after the self-check"),
        help: report.clone(),
    };
    let kept = |worker: &mut Worker<'_>| {
        let link = worker.link.take()?;
        let loaded = worker
            .loaded_files
            .take()
            .or_else(|| holder.is_none().then(|| job.test_files.clone()));
        Some(WarmLink {
            link,
            known: std::mem::take(&mut worker.known_tests),
            loaded,
            unproven: std::mem::take(&mut worker.unproven),
        })
    };
    let error = match outcome {
        Err(error) => error,
        Ok(ControlOutcome::Passed) => {
            runner.memo.store(&job.tests, ControlVerdict::Passed);
            if let Some(warm) = kept(&mut worker) {
                links.insert(0, warm);
                return Ok((links, Vec::new()));
            }
            stopped
        }
        Ok(ControlOutcome::Failed(fault)) if !fault.failed_tests.is_empty() => {
            runner.memo.store(&job.tests, ControlVerdict::Failed);
            if let Some(warm) = kept(&mut worker) {
                links.insert(0, warm);
                return Ok((links, fault.failed_tests));
            }
            stopped
        }
        Ok(ControlOutcome::Failed(fault)) if fault.no_tests => ScheduleError::RunPath {
            message: format!(
                "{runner_name} executes no tests in a mutant run: a control run of the tests of a covered mutant, with no active mutant, gave {}; each mutant would look killed",
                fault.detail
            ),
            help: report,
        },
        Ok(ControlOutcome::Failed(fault)) => ScheduleError::RunPath {
            message: format!(
                "{runner_name} gives no trustworthy result in a mutant run: a control run of the tests of a covered mutant, with no active mutant, gave {}, but no test failed",
                fault.detail
            ),
            help: report,
        },
        Ok(ControlOutcome::TimedOut(detail)) => ScheduleError::RunPath {
            message: format!(
                "the self-check of {runner_name} timed out: a control run of the tests of a covered mutant, with no active mutant, did not finish ({detail})"
            ),
            help: self_check_help(runner.config.runner_timeout, job.timeout_ms),
        },
        Ok(ControlOutcome::Fault(message)) => ScheduleError::RunPath {
            message: format!(
                "the self-check of {runner_name} failed: a control run with no active mutant gave no result: {message}"
            ),
            help: report,
        },
    };
    worker.reset();
    shutdown_all(links);
    Err(error)
}

/// The jobs of the mutant phase, the test files of the phase, and the
/// companions that already did their dry run.
pub(super) type CheckedJobs = (Vec<Job>, Vec<String>, Vec<WarmLink>);

/// Builds the jobs and runs the self-check on the first job. A test that
/// fails in the self-check is a baseline failure, the same as a test that
/// fails in the dry run: it covers no mutant, so the jobs are built again
/// without it. The test files of the phase are the test files of the jobs:
/// a companion that starts later dry-runs only these files.
pub(super) fn checked_jobs(
    runner: PhaseRunner<'_>,
    plan: &SchedulePlan,
    facts: &mut DryRunFacts,
    context: &JobContext<'_>,
    links: Vec<WarmLink>,
    outcome: &mut ScheduleOutcome,
) -> Result<CheckedJobs, ScheduleError> {
    let mut first_jobs = ScheduleOutcome::empty();
    let jobs = build_jobs(plan, facts, context, &mut first_jobs);
    let links = with_known(links, facts);
    let (links, failed) = self_check(runner, &phase_files(&jobs), context.ready, &jobs, links)?;
    let (jobs, links) = if failed.is_empty() {
        outcome.results.extend(first_jobs.results);
        outcome.cache_keys.extend(first_jobs.cache_keys);
        outcome.issues.extend(first_jobs.issues);
        outcome.promoted = first_jobs.promoted;
        (jobs, links)
    } else {
        drop_failed_tests(facts, &failed);
        let jobs = build_jobs(plan, facts, context, outcome);
        let links = links
            .into_iter()
            .map(|mut warm| {
                warm.known.retain(|test| facts.passing.contains_key(test));
                warm
            })
            .collect();
        (jobs, links)
    };
    outcome.baseline_failures.clone_from(&facts.baseline);
    mark_unreached_pending(plan, facts, context.dry_files, outcome);
    let files = phase_files(&jobs);
    Ok((jobs, files, links))
}

/// The dry-run facts and the `--kill-matrix-for` test ids of the dry run,
/// and the companions of the dry run. A failure shuts down each companion.
pub(super) fn read_dry_phase(
    plan: &SchedulePlan,
    config: &ScheduleConfig,
    (caps, files): (Capabilities, TargetFiles<'_>),
    dry: &DryRunResult,
    outcome: &mut ScheduleOutcome,
    links: Vec<WarmLink>,
) -> Result<(DryRunFacts, TargetIds, Vec<WarmLink>), ScheduleError> {
    let read = read_dry_run(plan, caps, dry).and_then(|facts| {
        record_dry_run(outcome, dry);
        config_targets(config, dry, caps, files, outcome).map(|ids| (facts, ids))
    });
    match read {
        Ok((facts, ids)) => Ok((facts, ids, links)),
        Err(error) => {
            shutdown_all(links);
            Err(error)
        }
    }
}

/// Gives `no_coverage` to each planned and pending mutant of a run with no
/// dry run.
pub(super) fn no_coverage_for_all(plan: &SchedulePlan, outcome: &mut ScheduleOutcome) {
    for mutant in plan.mutants.iter().chain(&plan.pending) {
        outcome
            .results
            .insert(mutant.id.clone(), MutantOutcome::no_coverage());
    }
}
