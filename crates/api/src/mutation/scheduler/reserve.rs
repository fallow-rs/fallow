//! The deadline of a scheduler phase. The first pass of a budgeted run keeps
//! time free for the confirmation of its survivors and timeouts. Without that
//! time, the first pass can use the full budget, no suspect gets its retry,
//! and the result cache stores none of them, so a repeat run with the same
//! budget does the same work again (issue #3665).
//!
//! The reserve holds the start of the retry companions, which dry-run only
//! the test files of the suspects, not the time of the first dry run. A
//! reserve with the first dry run leaves about that time of the budget
//! unused (issue #5421).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use std::sync::PoisonError;

use rustc_hash::{FxHashMap, FxHashSet};

use super::clock::Clock;
use super::{Attempt, DryRunFacts, Job, ScheduleConfig, retry_of};

/// The margin on the estimated retry phase, as a divisor: a quarter. The
/// estimate comes from the first runs and the dry run, but each retry runs
/// in a new companion, which starts cold and adds time that the first runs
/// do not show.
const RETRY_MARGIN_DIVISOR: u64 = 4;

/// How the retry phase runs a first-pass job again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Retry {
    /// The job gets no retry.
    None,
    /// A survivor runs again with its confirmation set.
    Confirmation,
    /// A timeout runs again with a doubled timeout.
    DoubledTimeout,
    /// A unique kill of a `--kill-matrix-for` target runs again unchanged.
    Same,
}

/// The deadline of a phase. Only the first pass has a reserve.
pub(super) struct PhaseDeadline {
    at: Instant,
    clock: Arc<dyn Clock>,
    reserve: Option<ConfirmationReserve>,
}

impl PhaseDeadline {
    /// The deadline of the first pass, with a reserve for the retries.
    pub(super) fn first_pass(config: &ScheduleConfig, start: CompanionStart) -> Option<Self> {
        config.deadline.map(|at| Self {
            at,
            clock: Arc::clone(&config.clock),
            reserve: Some(ConfirmationReserve::new(
                start,
                config.concurrency,
                config.target_full,
            )),
        })
    }

    /// The deadline of the retry phase. The retries use the real deadline.
    pub(super) fn retries(config: &ScheduleConfig) -> Option<Self> {
        config.deadline.map(|at| Self {
            at,
            clock: Arc::clone(&config.clock),
            reserve: None,
        })
    }

    /// Whether a new job must stay pending.
    pub(super) fn passed(&self) -> bool {
        let needed = self
            .reserve
            .as_ref()
            .map_or(Duration::ZERO, ConfirmationReserve::needed);
        self.clock
            .now()
            .checked_add(needed)
            .is_none_or(|end| end >= self.at)
    }

    /// Runs `job` and adds its wall time and status to the reserve, when
    /// the phase has one.
    pub(super) fn timed<E>(
        deadline: Option<&Self>,
        job: &Job,
        run: impl FnOnce() -> Result<Attempt, E>,
    ) -> Result<Attempt, E> {
        let Some((deadline, reserve)) =
            deadline.and_then(|deadline| Some((deadline, deadline.reserve.as_ref()?)))
        else {
            return run();
        };
        let started = deadline.clock.now();
        let attempt = run()?;
        let wall = deadline.clock.now().saturating_duration_since(started);
        let retry = retry_of(reserve.target_full, job, &attempt);
        reserve.note(retry, wall, job.retry_extra_ms, &job.test_files);
        Ok(attempt)
    }

    /// Adds a job that ran outside [`Self::timed`] to the reserve: a split
    /// job counts one time, with the wall time of all its parts.
    pub(super) fn note(&self, job: &Job, attempt: &Attempt, wall: Duration) {
        if let Some(reserve) = &self.reserve {
            let retry = retry_of(reserve.target_full, job, attempt);
            reserve.note(retry, wall, job.retry_extra_ms, &job.test_files);
        }
    }
}

/// Whether the time budget of `config` has ended.
pub(super) fn expired(config: &ScheduleConfig) -> bool {
    config.deadline.is_some_and(|at| config.clock.now() >= at)
}

/// The start time of a retry companion: the setup of the run and the dry run
/// of the test files of the suspects. Each retry worker starts its own
/// companion at the same time and dry-runs each of these files, so the dry
/// run does not get shorter with more workers. A companion that takes the
/// test index of the first dry run runs no test before its first job, but it
/// still loads each test file, so only the setup time of each file counts.
pub(super) struct CompanionStart {
    /// The setup time of the run, in milliseconds.
    setup: u64,
    /// The start time of each passing test file, in milliseconds: the
    /// durations of its passing tests, unless the companion takes the test
    /// index, plus its setup time.
    files: FxHashMap<String, u64>,
    /// The time of one dry run of each test file, in milliseconds. A runner
    /// can dry-run files in parallel, so the sum of `files` can be more than
    /// a real dry run.
    dry_run_cap: u64,
}

impl CompanionStart {
    /// The start of a retry companion from the `setup` time of the run, the
    /// time of one dry run of each test file, and the dry-run facts.
    /// `adopts` is true when the companion takes the test index.
    pub(super) fn new(
        setup: Duration,
        first_dry_run: Duration,
        facts: &DryRunFacts,
        adopts: bool,
    ) -> Self {
        let mut file_ms: FxHashMap<String, f64> = FxHashMap::default();
        for (file, duration) in facts.passing.values() {
            let tests = if adopts { 0.0 } else { *duration };
            *file_ms.entry(file.clone()).or_default() += tests;
        }
        for (file, setup) in &facts.file_setup {
            if let Some(total) = file_ms.get_mut(file) {
                *total += setup;
            }
        }
        Self {
            setup: duration_ms(setup),
            files: file_ms
                .into_iter()
                .map(|(file, total)| (file, ceil_ms(total)))
                .collect(),
            dry_run_cap: duration_ms(first_dry_run),
        }
    }

    /// The dry-run time that `file` adds to the start of a retry companion.
    fn dry_run_ms(&self, file: &str) -> u64 {
        self.files.get(file).copied().unwrap_or(0)
    }

    /// The start time of a retry companion that dry-runs files of
    /// `dry_run_ms` in total.
    fn ms(&self, dry_run_ms: u64) -> u64 {
        self.setup.saturating_add(dry_run_ms.min(self.dry_run_cap))
    }
}

struct ConfirmationReserve {
    start: CompanionStart,
    /// Whether the run reports each kill of the targets, so that a unique
    /// kill gets no retry.
    target_full: bool,
    /// The test files of the suspects found so far.
    suspect_files: Mutex<FxHashSet<String>>,
    /// The dry-run time of `suspect_files` in a retry companion.
    suspect_dry_run_ms: AtomicU64,
    /// The worker count of the run. The retry phase starts no more workers
    /// than suspects.
    workers: u64,
    /// The estimated time of the retries of the suspects found so far.
    suspect_ms: AtomicU64,
    /// The count of the suspects found so far.
    suspects: AtomicU64,
    /// The wall time of the finished first-pass jobs.
    run_ms: AtomicU64,
    /// The count of the finished first-pass jobs.
    runs: AtomicU64,
}

impl ConfirmationReserve {
    fn new(start: CompanionStart, workers: usize, target_full: bool) -> Self {
        Self {
            start,
            target_full,
            suspect_files: Mutex::new(FxHashSet::default()),
            suspect_dry_run_ms: AtomicU64::new(0),
            workers: u64::try_from(workers.max(1)).unwrap_or(u64::MAX),
            suspect_ms: AtomicU64::new(0),
            suspects: AtomicU64::new(0),
            run_ms: AtomicU64::new(0),
            runs: AtomicU64::new(0),
        }
    }

    /// Adds a finished first-pass job. A survivor reserves its run time plus
    /// `retry_extra_ms` for its confirmation. A timeout reserves twice its
    /// run time, because its retry gets a doubled timeout. A unique kill
    /// reserves its run time. The `test_files` of a suspect add their dry
    /// run to the start of the retry companions.
    fn note(&self, retry: Retry, wall: Duration, retry_extra_ms: u64, test_files: &[String]) {
        let wall_ms = duration_ms(wall);
        self.run_ms.fetch_add(wall_ms, Ordering::SeqCst);
        self.runs.fetch_add(1, Ordering::SeqCst);
        let retry_ms = match retry {
            Retry::None => return,
            Retry::Confirmation => wall_ms.saturating_add(retry_extra_ms),
            Retry::DoubledTimeout => wall_ms.saturating_mul(2),
            Retry::Same => wall_ms,
        };
        let mut files = self
            .suspect_files
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        for file in test_files {
            if files.insert(file.clone()) {
                self.suspect_dry_run_ms
                    .fetch_add(self.start.dry_run_ms(file), Ordering::SeqCst);
            }
        }
        drop(files);
        self.suspect_ms.fetch_add(retry_ms, Ordering::SeqCst);
        self.suspects.fetch_add(1, Ordering::SeqCst);
    }

    /// The time that the next first-pass job needs before the deadline: the
    /// retries of the known suspects and the start of their companions, with
    /// [`RETRY_MARGIN_DIVISOR`] on top, and the mean run time twice, for the
    /// job and its possible retry.
    fn needed(&self) -> Duration {
        let runs = self.runs.load(Ordering::SeqCst);
        let mean_ms = self
            .run_ms
            .load(Ordering::SeqCst)
            .checked_div(runs)
            .unwrap_or(0);
        let suspects = self.suspects.load(Ordering::SeqCst);
        let suspect_ms = self.suspect_ms.load(Ordering::SeqCst);
        let startup_ms = if suspects > 0 {
            self.start
                .ms(self.suspect_dry_run_ms.load(Ordering::SeqCst))
        } else {
            0
        };
        let workers = self.workers.min(suspects).max(1);
        let retry_phase_ms = startup_ms.saturating_add(suspect_ms.div_ceil(workers));
        Duration::from_millis(
            retry_phase_ms
                .saturating_add(retry_phase_ms.div_ceil(RETRY_MARGIN_DIVISOR))
                .saturating_add(mean_ms.saturating_mul(2)),
        )
    }
}

fn duration_ms(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a reserve estimate in milliseconds; the value is clamped first"
)]
fn ceil_ms(ms: f64) -> u64 {
    let ms = ms.max(0.0).ceil();
    if ms.is_finite() { ms as u64 } else { 0 }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    const FILE: &str = "test/a.test.ts";

    /// A companion start of `setup_ms` with a dry run of `dry_ms` for
    /// [`FILE`] and of 9,000 ms for a slow file that no suspect needs.
    fn start(setup_ms: u64, dry_ms: u64) -> CompanionStart {
        CompanionStart {
            setup: setup_ms,
            files: FxHashMap::from_iter([
                (FILE.to_owned(), dry_ms),
                ("test/slow.test.ts".to_owned(), 9_000),
            ]),
            dry_run_cap: 20_000,
        }
    }

    fn files() -> Vec<String> {
        vec![FILE.to_owned()]
    }

    #[test]
    fn the_reserve_is_empty_before_the_first_job() {
        let reserve = ConfirmationReserve::new(start(500, 300), 4, false);
        assert_eq!(reserve.needed(), Duration::ZERO);
    }

    #[test]
    fn a_killed_mutant_reserves_only_the_mean_run_time() {
        let reserve = ConfirmationReserve::new(start(500, 300), 4, false);
        reserve.note(Retry::None, Duration::from_millis(100), 0, &files());
        assert_eq!(reserve.needed(), Duration::from_millis(200));
    }

    #[test]
    fn a_suspect_reserves_its_retry_and_the_companion_start() {
        let reserve = ConfirmationReserve::new(start(500, 300), 1, false);
        reserve.note(Retry::Confirmation, Duration::from_millis(100), 0, &files());
        reserve.note(
            Retry::DoubledTimeout,
            Duration::from_millis(100),
            0,
            &files(),
        );
        // 500 setup + 300 dry run of the one suspect file + (100 + 2 * 100)
        // retries = 1100, a quarter margin of 275, and 2 * 100 mean.
        assert_eq!(reserve.needed(), Duration::from_millis(1575));
    }

    #[test]
    fn the_companion_start_has_only_the_files_of_the_suspects() {
        let reserve = ConfirmationReserve::new(start(0, 300), 1, false);
        reserve.note(Retry::Confirmation, Duration::from_millis(100), 0, &[]);
        // No suspect file: 100 retry + 25 margin + 2 * 100 mean.
        assert_eq!(reserve.needed(), Duration::from_millis(325));
        reserve.note(
            Retry::None,
            Duration::from_millis(100),
            0,
            &["test/slow.test.ts".to_owned()],
        );
        // A killed mutant adds no file to the retry dry run.
        assert_eq!(reserve.needed(), Duration::from_millis(325));
        reserve.note(
            Retry::Confirmation,
            Duration::from_millis(100),
            0,
            &["test/slow.test.ts".to_owned()],
        );
        // 9,000 dry run + 200 retries + 2,300 margin + 2 * 100 mean.
        assert_eq!(reserve.needed(), Duration::from_millis(11_700));
    }

    #[test]
    fn a_suspect_file_counts_once_in_the_companion_start() {
        let reserve = ConfirmationReserve::new(start(0, 300), 1, false);
        for _ in 0..3 {
            reserve.note(Retry::Confirmation, Duration::from_millis(100), 0, &files());
        }
        // 300 dry run + 3 * 100 retries + 150 margin + 2 * 100 mean.
        assert_eq!(reserve.needed(), Duration::from_millis(950));
    }

    #[test]
    fn a_survivor_reserves_the_extra_time_of_its_whole_file_retry() {
        let reserve = ConfirmationReserve::new(start(0, 0), 1, false);
        reserve.note(
            Retry::Confirmation,
            Duration::from_millis(100),
            900,
            &files(),
        );
        // (100 + 900) whole-file retry + 250 margin + 2 * 100 mean.
        assert_eq!(reserve.needed(), Duration::from_millis(1450));
    }

    #[test]
    fn the_retries_share_no_more_workers_than_suspects() {
        let reserve = ConfirmationReserve::new(start(0, 0), 4, false);
        reserve.note(Retry::Confirmation, Duration::from_millis(400), 0, &files());
        // One suspect has one retry worker: 400 retry + 100 margin
        // + 2 * 400 mean.
        assert_eq!(reserve.needed(), Duration::from_millis(1300));
        reserve.note(Retry::Confirmation, Duration::from_millis(400), 0, &files());
        // Two suspects share two workers: 400 retries + 100 margin
        // + 2 * 400 mean.
        assert_eq!(reserve.needed(), Duration::from_millis(1300));
    }

    #[test]
    fn each_retry_worker_dry_runs_each_suspect_file() {
        let reserve = ConfirmationReserve::new(start(0, 300), 4, false);
        reserve.note(Retry::Confirmation, Duration::from_millis(100), 0, &files());
        reserve.note(Retry::Confirmation, Duration::from_millis(100), 0, &files());
        // 300 dry run on each worker + 200 retries / 2 workers + 100 margin
        // + 2 * 100 mean.
        assert_eq!(reserve.needed(), Duration::from_millis(700));
    }

    #[test]
    fn a_unique_kill_reserves_its_run_and_the_companion_start() {
        let reserve = ConfirmationReserve::new(start(500, 300), 1, false);
        reserve.note(Retry::Same, Duration::from_millis(100), 900, &files());
        // 500 setup + 300 dry run + 100 retry = 900, a quarter margin of 225,
        // and 2 * 100 mean. A unique kill reruns its own tests only.
        assert_eq!(reserve.needed(), Duration::from_millis(1325));
    }

    #[test]
    fn the_retry_dry_run_never_passes_one_dry_run_of_each_file() {
        let mut capped = start(0, 300);
        capped.dry_run_cap = 5_000;
        let reserve = ConfirmationReserve::new(capped, 1, false);
        let both = vec![FILE.to_owned(), "test/slow.test.ts".to_owned()];
        reserve.note(Retry::Confirmation, Duration::from_millis(100), 0, &both);
        // min(300 + 9,000, 5,000) dry run + 100 retry = 5,100, a quarter
        // margin of 1,275, and 2 * 100 mean.
        assert_eq!(reserve.needed(), Duration::from_millis(6575));
    }

    #[test]
    fn a_companion_that_takes_the_test_index_counts_only_the_file_setup() {
        let facts = DryRunFacts {
            passing: BTreeMap::from([
                ("a::t1".to_owned(), (FILE.to_owned(), 400.0)),
                ("a::t2".to_owned(), (FILE.to_owned(), 600.0)),
            ]),
            covering: FxHashMap::default(),
            static_ids: FxHashSet::default(),
            baseline: Vec::new(),
            file_setup: BTreeMap::from([(FILE.to_owned(), 50.0)]),
        };
        let second = Duration::from_secs(1);
        let dry_run = CompanionStart::new(Duration::ZERO, second, &facts, false);
        assert_eq!(dry_run.dry_run_ms(FILE), 1050);
        let adopted = CompanionStart::new(Duration::ZERO, second, &facts, true);
        assert_eq!(adopted.dry_run_ms(FILE), 50);
    }
}
