//! The results of earlier runs: the result cache lookup, the outcome
//! journal of an interrupted run, and the journal writes of a running
//! schedule.

use fallow_output::MutantStatus;
use rustc_hash::FxHashMap;

use super::super::protocol::Ready;
use super::{
    Attempt, Job, JobContext, MutantOutcome, ScheduleConfig, ScheduleOutcome, ScheduledMutant,
    retry_status,
};

/// A result of an earlier run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedResult {
    pub status: MutantStatus,
    pub killed_by: Option<String>,
    /// All observed failing tests, sorted. Timeouts do not count.
    pub killing_tests: Vec<String>,
    /// True only for exact requested membership with terminal pass or fail results.
    pub kill_matrix_complete: bool,
    pub duration_ms: Option<u64>,
}

/// Looks up the result of a mutant from an earlier run.
pub trait ResultLookup: std::fmt::Debug + Send + Sync {
    /// The cache key of `mutant` with these covering tests and their test
    /// files, for the runner that `ready` describes.
    fn key(
        &self,
        ready: &Ready,
        mutant: &ScheduledMutant,
        tests: &[String],
        test_files: &[String],
    ) -> String;

    /// The stored result of `mutant_id` under `key`.
    fn get(&self, mutant_id: &str, key: &str) -> Option<CachedResult>;
}

/// Keeps the final results of a run on disk while the run continues, so an
/// interrupted run does not lose them. Only a `killed` result and a
/// `survived` result that a retry confirmed are final.
pub trait OutcomeJournal: std::fmt::Debug + Send + Sync {
    /// The key of `mutant`: the same key as [`ResultLookup::key`].
    fn key(
        &self,
        ready: &Ready,
        mutant: &ScheduledMutant,
        tests: &[String],
        test_files: &[String],
    ) -> String;

    /// The result of `mutant_id` from the journal of an interrupted run, when
    /// its key is `key`. `None` when the run does not resume.
    fn resumed(&self, mutant_id: &str, key: &str) -> Option<CachedResult>;

    /// Adds one final result. The scheduler calls it from worker threads.
    fn append(&self, mutant_id: &str, key: &str, result: &CachedResult);
}

/// Where a result of an earlier run comes from.
#[derive(Clone, Copy, PartialEq, Eq)]
enum StoredSource {
    Cache,
    Journal,
}

/// The result of an earlier run for `mutant` under `key`: first the journal
/// of an interrupted run, then the result cache.
fn stored_result(
    context: &JobContext<'_>,
    key: Option<&String>,
    mutant: &ScheduledMutant,
) -> Option<(CachedResult, StoredSource)> {
    let key = key?;
    let resumed = context
        .journal
        .and_then(|journal| journal.resumed(&mutant.id, key))
        .map(|hit| (hit, StoredSource::Journal));
    resumed
        .or_else(|| {
            context
                .cache
                .and_then(|cache| cache.get(&mutant.id, key))
                .map(|hit| (hit, StoredSource::Cache))
        })
        .filter(|(hit, _)| !context.kill_matrix || hit.kill_matrix_complete)
}

/// The outcome of a result of an earlier run, with its covering `tests`.
fn stored_outcome(
    (hit, source): (CachedResult, StoredSource),
    tests: &[String],
    static_mutant: bool,
) -> MutantOutcome {
    let mut covering_tests = tests.to_vec();
    covering_tests.sort_unstable();
    MutantOutcome {
        status: hit.status,
        killing_tests: hit.killing_tests,
        kill_matrix_complete: hit.kill_matrix_complete,
        killed_by: hit.killed_by,
        covering_tests,
        static_mutant,
        duration_ms: hit.duration_ms,
        // The cache and the journal keep a survivor only after its retry.
        retried: hit.status == MutantStatus::Survived,
        attempts: Vec::new(),
        cached: source == StoredSource::Cache,
        resumed: source == StoredSource::Journal,
    }
}

/// The key of `mutant` with these tests. The cache and the journal compute
/// the same key. `None` without a cache and without a journal.
pub(super) fn job_key(
    context: &JobContext<'_>,
    mutant: &ScheduledMutant,
    tests: &[String],
    test_files: &[String],
) -> Option<String> {
    match (context.cache, context.journal) {
        (Some(cache), _) => Some(cache.key(context.ready, mutant, tests, test_files)),
        (None, Some(journal)) => Some(journal.key(context.ready, mutant, tests, test_files)),
        (None, None) => None,
    }
    .map(|key| {
        if context.kill_matrix {
            format!("matrix:{key}")
        } else {
            key
        }
    })
}

/// Records the result of an earlier run for `mutant`, and returns true when
/// one exists. The mutant then needs no job.
pub(super) fn record_stored(
    context: &JobContext<'_>,
    key: Option<&String>,
    mutant: &ScheduledMutant,
    tests: &[String],
    static_mutant: bool,
    outcome: &mut ScheduleOutcome,
) -> bool {
    let Some(stored) = stored_result(context, key, mutant) else {
        return false;
    };
    let stored = stored_outcome(stored, tests, static_mutant);
    // The result cache stores a resumed result, because the interrupted run
    // never saved its cache.
    if let (true, Some(key)) = (stored.resumed, key) {
        outcome.cache_keys.insert(mutant.id.clone(), key.clone());
    }
    outcome.results.insert(mutant.id.clone(), stored);
    true
}

/// Adds a final result of `job` to the journal of `config`. A job without a
/// key has no journal.
fn journal_result(config: &ScheduleConfig, job: &Job, result: &CachedResult) {
    if config.kill_matrix && !result.kill_matrix_complete {
        return;
    }
    if let (Some(journal), Some(key)) = (&config.journal, &job.cache_key) {
        journal.append(&job.id, key, result);
    }
}

/// Journals the first run of a job when its status is final: a kill. A
/// survivor and a timeout get a retry, and each other status runs again in a
/// later run.
pub(super) fn journal_first_run(config: &ScheduleConfig, job: &Job, attempt: &Attempt) {
    if attempt.status != MutantStatus::Killed {
        return;
    }
    journal_result(
        config,
        job,
        &CachedResult {
            status: MutantStatus::Killed,
            killing_tests: attempt.killing_tests.clone(),
            kill_matrix_complete: attempt.kill_matrix_complete,
            killed_by: attempt.killed_by.clone(),
            duration_ms: attempt.duration_ms,
        },
    );
}

/// The status and the duration of the first run of each retried mutant.
pub(super) type FirstRuns = FxHashMap<String, (MutantStatus, Option<u64>)>;

/// Journals a retried job when the retry makes it final: a kill, or a
/// survivor that the retry confirmed without a dropped covering test. The
/// result cache keeps the same results.
pub(super) fn journal_retry(
    config: &ScheduleConfig,
    firsts: &FirstRuns,
    job: &Job,
    retry: &Attempt,
) {
    let (first, first_ms) = firsts
        .get(&job.id)
        .copied()
        .unwrap_or((MutantStatus::Survived, None));
    let result = match retry_status(first, job, retry) {
        MutantStatus::Killed => CachedResult {
            status: MutantStatus::Killed,
            killing_tests: retry.killing_tests.clone(),
            kill_matrix_complete: retry.kill_matrix_complete,
            killed_by: retry.killed_by.clone(),
            duration_ms: retry.duration_ms,
        },
        MutantStatus::Survived if retry.dropped_tests.is_empty() => CachedResult {
            status: MutantStatus::Survived,
            killing_tests: Vec::new(),
            kill_matrix_complete: retry.kill_matrix_complete,
            killed_by: None,
            duration_ms: first_ms,
        },
        _ => return,
    };
    journal_result(config, job, &result);
}
