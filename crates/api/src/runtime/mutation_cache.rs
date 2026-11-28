//! The result cache and the dry-run cache of a `fallow mutation` run.

use std::path::Path;
use std::sync::Arc;

use fallow_engine::mutation::MutationPlan;
use fallow_output::MutationCacheOutput;

use crate::mutation::dry_run_cache::DryRunCache;
use crate::mutation::protocol::InitRequest;
use crate::mutation::result_cache::{KeyFiles, NOT_KEYED, ResultCache};
use crate::mutation::scheduler::{ScheduleIssue, ScheduleOutcome};

/// The result cache of a run. `None` with `--no-cache`.
pub(super) fn open_result_cache(
    no_cache: bool,
    mutation_cache: &Path,
    root: &Path,
    runner: &str,
    files: KeyFiles,
) -> Option<Arc<ResultCache>> {
    (!no_cache).then(|| Arc::new(ResultCache::open(mutation_cache, root, runner, files)))
}

/// The key files of the result cache. A selected runner config file is a
/// support file of the key, so a result of one config content is never the
/// result of another. The selected Vitest projects are a key part too.
pub(super) fn key_files(
    plan: &MutationPlan,
    runner_config: Option<&str>,
    runner_projects: &[String],
) -> KeyFiles {
    let mut files = KeyFiles {
        runner_projects: runner_projects.to_vec(),
        ..KeyFiles::from_plan(plan)
    };
    if let Some(config) = runner_config
        && !files.support_files.iter().any(|file| file == config)
    {
        files.support_files.push(config.to_owned());
    }
    files
}

/// Stores the new results and dry-run facts of `outcome` in `cache` and
/// `dry_cache`, and writes the cache files. A failed write adds a
/// `cache-write-failed` issue and keeps the run result. Gives `None` with no
/// cache (`--no-cache`) and when no companion started.
pub(super) fn store_results(
    (cache, dry_cache): (Option<&ResultCache>, Option<&DryRunCache>),
    options: &crate::mutation::MutationRunOptions,
    outcome: &mut ScheduleOutcome,
) -> Option<MutationCacheOutput> {
    let cache = cache.filter(|_| outcome.ready.is_some())?;
    let counts = cache.record(outcome);
    let mut failures = Vec::new();
    if let Err(message) = cache.save() {
        failures.push(format!(
            "the mutation result cache was not saved: {message}"
        ));
    }
    let reach = super::mutation_reach::ReachRequest::new(
        options.reach_selection,
        !options.kill_matrix_targets.is_empty(),
    );
    let reach_selection = super::mutation_reach::finish(reach, dry_cache, outcome);
    let dry_run_stored = dry_cache.map_or(0, |dry_cache| {
        let stored = dry_cache.record(&outcome.dry_run_entries);
        if let Err(message) = dry_cache.save() {
            failures.push(format!(
                "the mutation dry-run cache was not saved: {message}"
            ));
        }
        stored
    });
    outcome
        .issues
        .extend(failures.into_iter().map(|message| ScheduleIssue {
            code: "cache-write-failed",
            message,
            mutant_id: None,
            stderr_tail: None,
        }));
    Some(MutationCacheOutput {
        hits: counts.hits,
        stored: counts.stored,
        dry_run_hits: outcome.dry_run_reused,
        dry_run_misses: outcome.dry_run_missed,
        dry_run_miss_reasons: outcome
            .dry_run_miss_reasons
            .iter()
            .map(|(reason, count)| ((*reason).to_owned(), *count))
            .collect(),
        dry_run_stored,
        not_keyed: NOT_KEYED.iter().map(|&item| item.to_owned()).collect(),
        reach_selection,
    })
}

/// The dry-run cache of a run of `plan` at `root` that has the result cache
/// `keys`. It keys on the result cache key of each test file. A plan with
/// probes gives it the reach facts of the run.
pub(super) fn dry_run_cache(
    keys: &Arc<ResultCache>,
    mutation_cache: &Path,
    (plan, root): (&MutationPlan, &Path),
    operators: Option<&[String]>,
    init: &InitRequest,
) -> Arc<DryRunCache> {
    let instrumented = super::mutation_reach::run_files(plan);
    let cache = DryRunCache::open(
        mutation_cache,
        Arc::clone(keys),
        &instrumented,
        operators,
        init,
    );
    Arc::new(super::mutation_reach::with_facts(cache, root, plan))
}
