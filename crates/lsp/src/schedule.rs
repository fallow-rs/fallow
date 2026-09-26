//! When the server starts an analysis run, and when it cancels one.
//!
//! Workspace events (save, watched-file change, configuration change) do not
//! start a run at once. A run starts after `DEBOUNCE` without a new event, or
//! `MAX_WAIT` after the first event that no run has covered yet, whichever is
//! first. So a burst of events gives one run, and a steady stream of events
//! still gives a run at least each `MAX_WAIT`.
//!
//! An event that arrives during a run supersedes that run, and the server
//! cancels it, with one limit: after a cancelled run, the next run always
//! finishes. A run that finishes publishes, also when newer events arrived
//! during it, because the per-URI staleness check keeps old results off
//! edited buffers. Together these rules guarantee a publish under autosave,
//! where every run would otherwise see a newer save.
//!
//! The scheduler holds no clock. The caller passes `now`, so a test can drive
//! it with simulated time.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tokio::time::Instant;

/// Quiet time after the last event before a run starts.
pub const DEBOUNCE: Duration = Duration::from_millis(200);
/// Longest time from the first uncovered event to the start of a run.
pub const MAX_WAIT: Duration = Duration::from_secs(2);
/// Cancelled runs in a row before the next run must finish.
const MAX_CONSECUTIVE_CANCELS: u32 = 1;

/// How a run ended, for [`RunScheduler::finish_run`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunOutcome {
    /// The run finished and published its results.
    Published,
    /// The run stopped because a newer event superseded it.
    Cancelled,
    /// The run failed, or shutdown stopped it.
    Failed,
}

#[derive(Debug, Default)]
pub struct RunScheduler {
    last_event_at: Option<Instant>,
    /// The first event that no started run covers yet.
    first_pending_at: Option<Instant>,
    /// The cancellation token of the run in flight.
    running: Option<Arc<AtomicBool>>,
    consecutive_cancels: u32,
}

impl RunScheduler {
    /// Record a workspace event. Cancels the run in flight when the limit
    /// allows it, and returns whether it did.
    pub fn record_event(&mut self, now: Instant) -> bool {
        self.last_event_at = Some(now);
        self.first_pending_at.get_or_insert(now);
        let Some(token) = self.running.as_ref() else {
            return false;
        };
        if self.consecutive_cancels >= MAX_CONSECUTIVE_CANCELS {
            return false;
        }
        token.store(true, Ordering::SeqCst);
        true
    }

    /// The time at which the pending events should start a run, or `None`
    /// when no event is pending.
    pub fn run_deadline(&self) -> Option<Instant> {
        let first_pending = self.first_pending_at?;
        let quiet = self.last_event_at.unwrap_or(first_pending) + DEBOUNCE;
        Some(quiet.min(first_pending + MAX_WAIT))
    }

    /// A run starts now and covers all pending events. Returns its token.
    pub fn start_run(&mut self) -> Arc<AtomicBool> {
        self.first_pending_at = None;
        let token = Arc::new(AtomicBool::new(false));
        self.running = Some(Arc::clone(&token));
        token
    }

    /// Drop the pending events without a run, because a finished run
    /// already covers the current epoch.
    pub fn clear_pending(&mut self) {
        self.first_pending_at = None;
    }

    /// The run in flight ended.
    pub fn finish_run(&mut self, outcome: RunOutcome) {
        self.running = None;
        match outcome {
            RunOutcome::Cancelled => self.consecutive_cancels += 1,
            RunOutcome::Published => self.consecutive_cancels = 0,
            RunOutcome::Failed => {}
        }
    }

    /// Cancel the run in flight without limit, for shutdown.
    pub fn cancel_running(&self) {
        if let Some(token) = self.running.as_ref() {
            token.store(true, Ordering::SeqCst);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAVE_INTERVAL: Duration = Duration::from_secs(1);
    const ANALYSIS_TIME: Duration = Duration::from_secs(3);
    const CYCLES: u32 = 10;
    /// Simulation step. Every event and run time is a multiple of it.
    const TICK: Duration = Duration::from_millis(100);

    /// Simulate autosave: a save every `SAVE_INTERVAL`, and an analysis that
    /// takes `ANALYSIS_TIME` unless a save cancels it. Returns the number of
    /// runs that published within `CYCLES` analysis times.
    ///
    /// With `legacy`, the model is the old server: a run starts at once on a
    /// save, and a run discards its results when a newer save arrived during
    /// it.
    fn simulate_autosave(legacy: bool) -> u32 {
        let start = Instant::now();
        let end = start + ANALYSIS_TIME * CYCLES;
        let mut scheduler = RunScheduler::default();
        let mut run: Option<(Instant, Arc<AtomicBool>, bool)> = None;
        let mut next_save = start;
        let mut publishes = 0;
        let mut now = start;
        while now < end {
            if now >= next_save {
                next_save += SAVE_INTERVAL;
                if legacy {
                    if let Some((_, _, superseded)) = run.as_mut() {
                        *superseded = true;
                    } else {
                        run = Some((now, Arc::new(AtomicBool::new(false)), false));
                    }
                } else {
                    scheduler.record_event(now);
                }
            }
            if let Some((started, token, superseded)) = run.as_ref() {
                if token.load(Ordering::SeqCst) {
                    scheduler.finish_run(RunOutcome::Cancelled);
                    run = None;
                } else if now >= *started + ANALYSIS_TIME {
                    if legacy && *superseded {
                        // The old server discards the run and starts the
                        // queued one for the newer save.
                        run = Some((now, Arc::new(AtomicBool::new(false)), false));
                    } else {
                        publishes += 1;
                        scheduler.finish_run(RunOutcome::Published);
                        run = None;
                    }
                }
            }
            if !legacy
                && run.is_none()
                && scheduler
                    .run_deadline()
                    .is_some_and(|deadline| now >= deadline)
            {
                let token = scheduler.start_run();
                run = Some((now, token, false));
            }
            now += TICK;
        }
        publishes
    }

    #[test]
    fn autosave_faster_than_analysis_still_publishes() {
        assert_eq!(
            simulate_autosave(true),
            0,
            "the old policy discards every run under this autosave rate",
        );
        assert!(
            simulate_autosave(false) >= 1,
            "the publish floor must give at least one publish in {CYCLES} cycles",
        );
    }

    #[test]
    fn burst_of_events_waits_for_quiet_time() {
        let start = Instant::now();
        let mut scheduler = RunScheduler::default();
        for step in 0..5 {
            scheduler.record_event(start + Duration::from_millis(50) * step);
        }
        assert_eq!(
            scheduler.run_deadline(),
            Some(start + Duration::from_millis(200) + DEBOUNCE),
        );
    }

    #[test]
    fn steady_events_start_a_run_by_the_max_wait() {
        let start = Instant::now();
        let mut scheduler = RunScheduler::default();
        let mut now = start;
        while now < start + MAX_WAIT * 2 {
            scheduler.record_event(now);
            now += DEBOUNCE / 2;
        }
        assert_eq!(scheduler.run_deadline(), Some(start + MAX_WAIT));
    }

    #[test]
    fn started_run_covers_pending_events() {
        let mut scheduler = RunScheduler::default();
        scheduler.record_event(Instant::now());
        scheduler.start_run();
        assert_eq!(scheduler.run_deadline(), None);
    }

    #[test]
    fn event_cancels_the_run_in_flight_once_in_a_row() {
        let now = Instant::now();
        let mut scheduler = RunScheduler::default();
        let first = scheduler.start_run();
        assert!(scheduler.record_event(now));
        assert!(first.load(Ordering::SeqCst));
        scheduler.finish_run(RunOutcome::Cancelled);

        let second = scheduler.start_run();
        assert!(
            !scheduler.record_event(now),
            "after a cancelled run, the next run must finish",
        );
        assert!(!second.load(Ordering::SeqCst));
        scheduler.finish_run(RunOutcome::Published);

        let third = scheduler.start_run();
        assert!(scheduler.record_event(now), "a publish resets the limit");
        assert!(third.load(Ordering::SeqCst));
    }

    #[test]
    fn event_without_a_run_cancels_nothing() {
        let mut scheduler = RunScheduler::default();
        assert!(!scheduler.record_event(Instant::now()));
    }
}
