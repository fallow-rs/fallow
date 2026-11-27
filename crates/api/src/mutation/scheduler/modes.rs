//! The mutant runs and their time in each worker mode of the companion.

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use super::super::protocol::RunWorkerMode;

/// The mutant runs of one worker mode and the sum of their run times.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ModeUse {
    pub runs: usize,
    pub duration_ms: u64,
}

/// The use of each worker mode, for the report.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WorkerModeCounts {
    pub isolated: ModeUse,
    pub shared: ModeUse,
    pub reused: ModeUse,
}

#[derive(Debug, Default)]
struct ModeTally {
    runs: AtomicUsize,
    duration_ms: AtomicU64,
}

impl ModeTally {
    fn read(&self) -> ModeUse {
        ModeUse {
            runs: self.runs.load(Ordering::Relaxed),
            duration_ms: self.duration_ms.load(Ordering::Relaxed),
        }
    }
}

/// Counts the worker mode of each mutant run over all workers of a run.
#[derive(Debug, Default)]
pub(super) struct WorkerModeTally {
    isolated: ModeTally,
    shared: ModeTally,
    reused: ModeTally,
}

impl WorkerModeTally {
    /// Counts one mutant run with its run time. A run with no mode adds
    /// nothing.
    pub(super) fn record(&self, mode: Option<RunWorkerMode>, duration_ms: Option<f64>) {
        let tally = match mode {
            Some(RunWorkerMode::Isolated) => &self.isolated,
            Some(RunWorkerMode::Shared) => &self.shared,
            Some(RunWorkerMode::Reused) => &self.reused,
            None => return,
        };
        tally.runs.fetch_add(1, Ordering::Relaxed);
        tally
            .duration_ms
            .fetch_add(whole_ms(duration_ms), Ordering::Relaxed);
    }

    /// The use of each mode, or `None` when no mutant run stated a mode.
    pub(super) fn counts(&self) -> Option<WorkerModeCounts> {
        let counts = WorkerModeCounts {
            isolated: self.isolated.read(),
            shared: self.shared.read(),
            reused: self.reused.read(),
        };
        let runs = counts.isolated.runs + counts.shared.runs + counts.reused.runs;
        (runs > 0).then_some(counts)
    }
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a finite, non-negative run time in ms fits in u64"
)]
fn whole_ms(duration_ms: Option<f64>) -> u64 {
    duration_ms
        .filter(|ms| ms.is_finite() && *ms > 0.0)
        .map_or(0, |ms| ms.round() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_the_runs_and_time_of_each_mode() {
        let tally = WorkerModeTally::default();
        assert_eq!(tally.counts(), None);
        tally.record(None, Some(5.0));
        assert_eq!(tally.counts(), None);
        tally.record(Some(RunWorkerMode::Reused), Some(10.4));
        tally.record(Some(RunWorkerMode::Reused), None);
        tally.record(Some(RunWorkerMode::Isolated), Some(150.6));
        assert_eq!(
            tally.counts(),
            Some(WorkerModeCounts {
                isolated: ModeUse {
                    runs: 1,
                    duration_ms: 151,
                },
                shared: ModeUse::default(),
                reused: ModeUse {
                    runs: 2,
                    duration_ms: 10,
                },
            })
        );
    }
}
