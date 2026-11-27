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

use fallow_output::MutantStatus;
use rustc_hash::{FxHashMap, FxHashSet};

use super::clock::Clock;
use super::{Attempt, DryRunFacts, Job, ScheduleConfig, ScheduleTimings};

/// The margin on the estimated retry phase, as a divisor: a quarter. The
/// estimate comes from the first runs and the dry run, but a retry runs in a
/// new companion. In a measured run with 252 suspects, the real retry phase
/// took about 5% more time than the estimate, and without a margin the last
/// 14 suspects got no retry.
const RETRY_MARGIN_DIVISOR: u64 = 4;

/// The deadline of a phase. Only the first pass has a reserve.
pub(super) struct PhaseDeadline {
    at: Instant,
    clock: Arc<dyn Clock>,
    reserve: Option<ConfirmationReserve>,
}

impl PhaseDeadline {
    /// The deadline of the first pass, with a reserve for the retries.
    pub(super) fn first_pass(
        config: &ScheduleConfig,
        timings: &ScheduleTimings,
        facts: &DryRunFacts,
    ) -> Option<Self> {
        config.deadline.map(|at| Self {
            at,
            clock: Arc::clone(&config.clock),
            reserve: Some(ConfirmationReserve::new(
                CompanionStart::new(timings.setup, facts),
                config.concurrency,
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
        reserve.note(attempt.status, wall, job.retry_extra_ms, &job.test_files);
        Ok(attempt)
    }
}

/// Whether the time budget of `config` has ended.
pub(super) fn expired(config: &ScheduleConfig) -> bool {
    config.deadline.is_some_and(|at| config.clock.now() >= at)
}

/// The start time of a retry companion: the setup of the run and the dry run
/// of the test files of the suspects. Each retry worker starts its own
/// companion at the same time and dry-runs each of these files, so the dry
/// run does not get shorter with more workers.
pub(super) struct CompanionStart {
    setup_ms: u64,
    /// The dry-run time of each passing test file: the durations of its
    /// passing tests plus its setup time.
    file_ms: FxHashMap<String, u64>,
}

impl CompanionStart {
    pub(super) fn new(setup: Duration, facts: &DryRunFacts) -> Self {
        let mut file_ms: FxHashMap<String, f64> = FxHashMap::default();
        for (file, duration) in facts.passing.values() {
            *file_ms.entry(file.clone()).or_default() += duration;
        }
        for (file, setup) in &facts.file_setup {
            if let Some(total) = file_ms.get_mut(file) {
                *total += setup;
            }
        }
        Self {
            setup_ms: duration_ms(setup),
            file_ms: file_ms
                .into_iter()
                .map(|(file, total)| (file, ceil_ms(total)))
                .collect(),
        }
    }

    /// The dry-run time that `file` adds to the start of a retry companion.
    fn dry_run_ms(&self, file: &str) -> u64 {
        self.file_ms.get(file).copied().unwrap_or(0)
    }
}

struct ConfirmationReserve {
    start: CompanionStart,
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
    fn new(start: CompanionStart, workers: usize) -> Self {
        Self {
            start,
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
    /// `retry_extra_ms` for its retry. A timeout reserves twice its run time,
    /// because its retry gets a doubled timeout. The `test_files` of a
    /// suspect add their dry run to the start of the retry companions.
    fn note(
        &self,
        status: MutantStatus,
        wall: Duration,
        retry_extra_ms: u64,
        test_files: &[String],
    ) {
        let wall_ms = duration_ms(wall);
        self.run_ms.fetch_add(wall_ms, Ordering::SeqCst);
        self.runs.fetch_add(1, Ordering::SeqCst);
        let retry_ms = match status {
            MutantStatus::Survived => wall_ms.saturating_add(retry_extra_ms),
            MutantStatus::Timeout => wall_ms.saturating_mul(2),
            _ => return,
        };
        if let Ok(mut files) = self.suspect_files.lock() {
            for file in test_files {
                if files.insert(file.clone()) {
                    self.suspect_dry_run_ms
                        .fetch_add(self.start.dry_run_ms(file), Ordering::SeqCst);
                }
            }
        }
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
        let suspect_ms = self.suspect_ms.load(Ordering::SeqCst);
        let startup_ms = if suspect_ms > 0 {
            self.start
                .setup_ms
                .saturating_add(self.suspect_dry_run_ms.load(Ordering::SeqCst))
        } else {
            0
        };
        let workers = self
            .workers
            .min(self.suspects.load(Ordering::SeqCst))
            .max(1);
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
    use super::*;

    const FILE: &str = "test/a.test.ts";

    /// A companion start of `setup_ms` with a dry run of `dry_ms` for
    /// [`FILE`] and of 9,000 ms for a slow file that no suspect needs.
    fn start(setup_ms: u64, dry_ms: u64) -> CompanionStart {
        CompanionStart {
            setup_ms,
            file_ms: FxHashMap::from_iter([
                (FILE.to_owned(), dry_ms),
                ("test/slow.test.ts".to_owned(), 9_000),
            ]),
        }
    }

    fn files() -> Vec<String> {
        vec![FILE.to_owned()]
    }

    #[test]
    fn the_reserve_is_empty_before_the_first_job() {
        let reserve = ConfirmationReserve::new(start(500, 300), 4);
        assert_eq!(reserve.needed(), Duration::ZERO);
    }

    #[test]
    fn a_killed_mutant_reserves_only_the_mean_run_time() {
        let reserve = ConfirmationReserve::new(start(500, 300), 4);
        reserve.note(
            MutantStatus::Killed,
            Duration::from_millis(100),
            0,
            &files(),
        );
        assert_eq!(reserve.needed(), Duration::from_millis(200));
    }

    #[test]
    fn a_suspect_reserves_its_retry_and_the_companion_start() {
        let reserve = ConfirmationReserve::new(start(500, 300), 1);
        reserve.note(
            MutantStatus::Survived,
            Duration::from_millis(100),
            0,
            &files(),
        );
        reserve.note(
            MutantStatus::Timeout,
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
        let reserve = ConfirmationReserve::new(start(0, 300), 1);
        reserve.note(MutantStatus::Survived, Duration::from_millis(100), 0, &[]);
        // No suspect file: 100 retry + 25 margin + 2 * 100 mean.
        assert_eq!(reserve.needed(), Duration::from_millis(325));
        reserve.note(
            MutantStatus::Killed,
            Duration::from_millis(100),
            0,
            &["test/slow.test.ts".to_owned()],
        );
        // A killed mutant adds no file to the retry dry run.
        assert_eq!(reserve.needed(), Duration::from_millis(325));
        reserve.note(
            MutantStatus::Survived,
            Duration::from_millis(100),
            0,
            &["test/slow.test.ts".to_owned()],
        );
        // 9,000 dry run + 200 retries + 2,300 margin + 2 * 100 mean.
        assert_eq!(reserve.needed(), Duration::from_millis(11_700));
    }

    #[test]
    fn a_suspect_file_counts_once_in_the_companion_start() {
        let reserve = ConfirmationReserve::new(start(0, 300), 1);
        for _ in 0..3 {
            reserve.note(
                MutantStatus::Survived,
                Duration::from_millis(100),
                0,
                &files(),
            );
        }
        // 300 dry run + 3 * 100 retries + 150 margin + 2 * 100 mean.
        assert_eq!(reserve.needed(), Duration::from_millis(950));
    }

    #[test]
    fn a_survivor_reserves_the_extra_time_of_its_whole_file_retry() {
        let reserve = ConfirmationReserve::new(start(0, 0), 1);
        reserve.note(
            MutantStatus::Survived,
            Duration::from_millis(100),
            900,
            &files(),
        );
        // (100 + 900) whole-file retry + 250 margin + 2 * 100 mean.
        assert_eq!(reserve.needed(), Duration::from_millis(1450));
    }

    #[test]
    fn the_retries_share_no_more_workers_than_suspects() {
        let reserve = ConfirmationReserve::new(start(0, 0), 4);
        reserve.note(
            MutantStatus::Survived,
            Duration::from_millis(400),
            0,
            &files(),
        );
        // One suspect has one retry worker: 400 retry + 100 margin
        // + 2 * 400 mean.
        assert_eq!(reserve.needed(), Duration::from_millis(1300));
        reserve.note(
            MutantStatus::Survived,
            Duration::from_millis(400),
            0,
            &files(),
        );
        // Two suspects share two workers: 400 retries + 100 margin
        // + 2 * 400 mean.
        assert_eq!(reserve.needed(), Duration::from_millis(1300));
    }

    #[test]
    fn each_retry_worker_dry_runs_each_suspect_file() {
        let reserve = ConfirmationReserve::new(start(0, 300), 4);
        reserve.note(
            MutantStatus::Survived,
            Duration::from_millis(100),
            0,
            &files(),
        );
        reserve.note(
            MutantStatus::Survived,
            Duration::from_millis(100),
            0,
            &files(),
        );
        // 300 dry run on each worker + 200 retries / 2 workers + 100 margin
        // + 2 * 100 mean.
        assert_eq!(reserve.needed(), Duration::from_millis(700));
    }
}
