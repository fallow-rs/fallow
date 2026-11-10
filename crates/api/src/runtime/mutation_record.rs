//! The record pass of `fallow mutation --record-args`: a new companion runs
//! the test files once more with the wraps of the record plan, after the
//! mutant runs, and the inputs go into the cache.

use std::path::Path;
use std::time::{Duration, Instant};

use super::mutation::{plan_with_wraps, run_files};
use super::mutation_witness::WitnessSearch;
use crate::mutation::MutationRunOptions;
use crate::mutation::RecordArgsMode;
use crate::mutation::protocol::InitRequest;
use crate::mutation::recorded_args::RecordPass;
use crate::mutation::run_dir::write_run_dir;
use crate::mutation::scheduler::{ScheduleConfig, ScheduleOutcome};
use crate::mutation::transport::ProcessSpawner;

/// Starts a new companion and sends `record`: the test files run once more
/// with the recorder on. The pass runs after the mutant runs, so its test
/// results never change a result of the run.
pub(super) fn record_pass(
    options: &MutationRunOptions,
    outcome: &ScheduleOutcome,
    spawner: &ProcessSpawner,
    config: &ScheduleConfig,
    mutation_cache: &Path,
) -> Option<RecordPass> {
    if options.record_args == RecordArgsMode::Off || outcome.ready.is_none() {
        return None;
    }
    let failed = |error: &str| {
        Some(RecordPass {
            functions: Vec::new(),
            error: Some(error.to_owned()),
        })
    };
    // The same scope with the wraps gives the same mutants and files.
    let Ok(wrapped) = plan_with_wraps(options, |_, _| Ok(()), true) else {
        return failed("the plan of the record pass failed");
    };
    let files = run_files(&wrapped.plan);
    let Ok(run_dir) = write_run_dir(mutation_cache, &files, Some(&spawner.companion)) else {
        return failed("the run directory of the record pass cannot be written");
    };
    let init = InitRequest {
        run_dir: run_dir.dir.to_string_lossy().into_owned(),
        path_map: run_dir.path_map,
        ..config.init.clone()
    };
    let timeout = config.deadline.map_or(config.runner_timeout, |deadline| {
        config
            .runner_timeout
            .min(deadline.saturating_duration_since(Instant::now()))
    });
    Some(record_pass_with(spawner, &init, timeout, config.deadline))
}

fn record_pass_with(
    spawner: &ProcessSpawner,
    init: &InitRequest,
    timeout: Duration,
    deadline: Option<Instant>,
) -> RecordPass {
    use crate::mutation::protocol::{Request, Response};
    use crate::mutation::transport::CompanionSpawner as _;
    let failed = |error: &str| RecordPass {
        functions: Vec::new(),
        error: Some(error.to_owned()),
    };
    if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
        return failed("the time budget ended before the record pass");
    }
    let Ok(mut link) = spawner.spawn() else {
        return failed("the record companion did not start");
    };
    let pass = match link.request(&Request::Init(init.clone()), timeout) {
        Ok(Response::Ready(ready)) if ready.capabilities.record => {
            match link.request(
                &Request::Record {
                    test_files: Vec::new(),
                },
                timeout,
            ) {
                Ok(Response::Record(result)) => RecordPass {
                    functions: result.recorded_args,
                    error: None,
                },
                Ok(_) => failed("the companion answered record with another response"),
                Err(crate::mutation::protocol::LinkFault::Timeout) => {
                    failed("the record pass did not end before the setup timeout")
                }
                Err(_) => failed("the record pass failed"),
            }
        }
        Ok(Response::Ready(_)) => failed("the runner records no arguments"),
        _ => failed("the record companion did not start"),
    };
    link.shutdown();
    pass
}

/// The record pass, and the companion facts of the witness search: only a
/// `values` recording with a record pass searches.
pub(super) fn record_and_search(
    options: &MutationRunOptions,
    outcome: &ScheduleOutcome,
    spawner: ProcessSpawner,
    config: &ScheduleConfig,
    mutation_cache: &Path,
) -> (Option<RecordPass>, Option<WitnessSearch>) {
    let record = record_pass(options, outcome, &spawner, config, mutation_cache);
    let search = (options.record_args == RecordArgsMode::Values && record.is_some()).then(|| {
        WitnessSearch {
            spawner,
            init: config.init.clone(),
            runner_timeout: config.runner_timeout,
            deadline: config.deadline,
        }
    });
    (record, search)
}

/// Removes the expired recorded inputs, also when this run does not record,
/// and stores the inputs of the record pass.
pub(super) fn settle_recording(
    options: &MutationRunOptions,
    cache_dir: &Path,
    pass: Option<&RecordPass>,
) -> Option<fallow_output::MutationRecordingOutput> {
    use crate::mutation::recorded_args::witnesses::prune_witnesses;
    use crate::mutation::recorded_args::{now_secs, prune, save_recording};
    prune(cache_dir, now_secs());
    prune_witnesses(cache_dir, now_secs());
    save_recording(cache_dir, options.record_args, pass)
}
