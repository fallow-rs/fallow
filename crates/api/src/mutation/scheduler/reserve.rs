//! The deadline of a scheduler phase. The first pass of a budgeted run keeps
//! time free for the confirmation of its survivors and timeouts. Without that
//! time, the first pass can use the full budget, no suspect gets its retry,
//! and the result cache stores none of them, so a repeat run with the same
//! budget does the same work again.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use fallow_output::MutantStatus;

use super::{Attempt, ScheduleConfig, ScheduleTimings};

/// The deadline of a phase. Only the first pass has a reserve.
pub(super) struct PhaseDeadline {
    at: Instant,
    reserve: Option<ConfirmationReserve>,
}

impl PhaseDeadline {
    /// The deadline of the first pass, with a reserve for the retries.
    pub(super) fn first_pass(config: &ScheduleConfig, timings: &ScheduleTimings) -> Option<Self> {
        let startup = timings.setup + timings.dry_run;
        config.deadline.map(|at| Self {
            at,
            reserve: Some(ConfirmationReserve::new(startup, config.concurrency)),
        })
    }

    /// The deadline of the retry phase. The retries use the real deadline.
    pub(super) fn retries(at: Instant) -> Self {
        Self { at, reserve: None }
    }

    /// Whether a new job must stay pending.
    pub(super) fn passed(&self) -> bool {
        let needed = self
            .reserve
            .as_ref()
            .map_or(Duration::ZERO, ConfirmationReserve::needed);
        Instant::now()
            .checked_add(needed)
            .is_none_or(|end| end >= self.at)
    }

    /// Runs one job and adds its wall time and status to the reserve, when
    /// the phase has one. `retry_extra_ms` is the extra time of the retry of
    /// a survivor: its retry can run more tests than the first run.
    pub(super) fn timed<E>(
        deadline: Option<&Self>,
        retry_extra_ms: u64,
        run: impl FnOnce() -> Result<Attempt, E>,
    ) -> Result<Attempt, E> {
        let started = Instant::now();
        let attempt = run()?;
        if let Some(reserve) = deadline.and_then(|deadline| deadline.reserve.as_ref()) {
            reserve.note(attempt.status, started.elapsed(), retry_extra_ms);
        }
        Ok(attempt)
    }
}

/// Whether a plain deadline has passed.
pub(super) fn expired(deadline: Option<Instant>) -> bool {
    deadline.is_some_and(|at| Instant::now() >= at)
}

struct ConfirmationReserve {
    /// The start of the retry companions: the setup and the first dry run.
    startup_ms: u64,
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
    fn new(startup: Duration, workers: usize) -> Self {
        Self {
            startup_ms: duration_ms(startup),
            workers: u64::try_from(workers.max(1)).unwrap_or(u64::MAX),
            suspect_ms: AtomicU64::new(0),
            suspects: AtomicU64::new(0),
            run_ms: AtomicU64::new(0),
            runs: AtomicU64::new(0),
        }
    }

    /// Adds a finished first-pass job. A survivor reserves its run time plus
    /// `retry_extra_ms` for its retry. A timeout reserves twice its run time,
    /// because its retry gets a doubled timeout.
    fn note(&self, status: MutantStatus, wall: Duration, retry_extra_ms: u64) {
        let wall_ms = duration_ms(wall);
        self.run_ms.fetch_add(wall_ms, Ordering::SeqCst);
        self.runs.fetch_add(1, Ordering::SeqCst);
        let retry_ms = match status {
            MutantStatus::Survived => wall_ms.saturating_add(retry_extra_ms),
            MutantStatus::Timeout => wall_ms.saturating_mul(2),
            _ => return,
        };
        self.suspect_ms.fetch_add(retry_ms, Ordering::SeqCst);
        self.suspects.fetch_add(1, Ordering::SeqCst);
    }

    /// The time that the next first-pass job needs before the deadline: the
    /// retries of the known suspects, the start of their companions, and the
    /// mean run time twice, for the job and its possible retry.
    fn needed(&self) -> Duration {
        let runs = self.runs.load(Ordering::SeqCst);
        let mean_ms = self
            .run_ms
            .load(Ordering::SeqCst)
            .checked_div(runs)
            .unwrap_or(0);
        let suspect_ms = self.suspect_ms.load(Ordering::SeqCst);
        let startup_ms = if suspect_ms > 0 { self.startup_ms } else { 0 };
        let workers = self
            .workers
            .min(self.suspects.load(Ordering::SeqCst))
            .max(1);
        Duration::from_millis(
            startup_ms
                .saturating_add(suspect_ms.div_ceil(workers))
                .saturating_add(mean_ms.saturating_mul(2)),
        )
    }
}

fn duration_ms(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_reserve_is_empty_before_the_first_job() {
        let reserve = ConfirmationReserve::new(Duration::from_millis(500), 4);
        assert_eq!(reserve.needed(), Duration::ZERO);
    }

    #[test]
    fn a_killed_mutant_reserves_only_the_mean_run_time() {
        let reserve = ConfirmationReserve::new(Duration::from_millis(500), 4);
        reserve.note(MutantStatus::Killed, Duration::from_millis(100), 0);
        assert_eq!(reserve.needed(), Duration::from_millis(200));
    }

    #[test]
    fn a_suspect_reserves_its_retry_and_the_companion_start() {
        let reserve = ConfirmationReserve::new(Duration::from_millis(500), 1);
        reserve.note(MutantStatus::Survived, Duration::from_millis(100), 0);
        reserve.note(MutantStatus::Timeout, Duration::from_millis(100), 0);
        // 500 start + (100 + 2 * 100) retries + 2 * 100 mean.
        assert_eq!(reserve.needed(), Duration::from_millis(1000));
    }

    #[test]
    fn a_survivor_reserves_the_extra_time_of_its_whole_file_retry() {
        let reserve = ConfirmationReserve::new(Duration::ZERO, 1);
        reserve.note(MutantStatus::Survived, Duration::from_millis(100), 900);
        // (100 + 900) whole-file retry + 2 * 100 mean.
        assert_eq!(reserve.needed(), Duration::from_millis(1200));
    }

    #[test]
    fn the_retries_share_no_more_workers_than_suspects() {
        let reserve = ConfirmationReserve::new(Duration::ZERO, 4);
        reserve.note(MutantStatus::Survived, Duration::from_millis(400), 0);
        // One suspect has one retry worker: 400 retry + 2 * 400 mean.
        assert_eq!(reserve.needed(), Duration::from_millis(1200));
        reserve.note(MutantStatus::Survived, Duration::from_millis(400), 0);
        // Two suspects share two workers: 400 retries + 2 * 400 mean.
        assert_eq!(reserve.needed(), Duration::from_millis(1200));
    }
}
