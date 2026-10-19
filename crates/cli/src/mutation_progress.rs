//! Phase progress of a `fallow mutation` run on stderr.
//!
//! The lines are plain text, one line per update, so they stay readable in
//! captured non-TTY output. A phase start prints at once. A mutant count
//! prints at most once per interval and at the end of its phase. A ticker
//! thread prints a line when a phase prints nothing for one interval, so a
//! long baseline run does not look stalled.

use std::io::IsTerminal;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use fallow_api::MutationProgress;

/// The shortest time between two progress lines on a terminal.
const TERMINAL_INTERVAL: Duration = Duration::from_secs(5);
/// The shortest time between two progress lines in captured output.
const CAPTURED_INTERVAL: Duration = Duration::from_secs(15);
/// How often the ticker thread looks for a silent phase.
const TICK: Duration = Duration::from_secs(1);
/// The prefix of each progress line.
const PREFIX: &str = "fallow: mutation:";

/// The progress state and the rules that decide when a line prints.
#[derive(Debug)]
struct ProgressState {
    started: Instant,
    interval: Duration,
    last_line: Instant,
    phase: Option<MutationProgress>,
}

impl ProgressState {
    fn new(now: Instant, interval: Duration) -> Self {
        Self {
            started: now,
            interval,
            last_line: now,
            phase: None,
        }
    }

    /// The line for `event` at `now`, or `None` when the line waits.
    fn on_event(&mut self, event: MutationProgress, now: Instant) -> Option<String> {
        self.phase = Some(event);
        let prints = match event {
            MutationProgress::Mutants { done, total }
            | MutationProgress::Retries { done, total } => {
                total > 0
                    && (done == 0
                        || done == total
                        || now.duration_since(self.last_line) >= self.interval)
            }
            _ => true,
        };
        prints.then(|| self.line(event, now, false))
    }

    /// The line for a phase that printed nothing for one interval.
    fn on_tick(&mut self, now: Instant) -> Option<String> {
        let event = self.phase?;
        let idle = matches!(
            event,
            MutationProgress::Mutants { total: 0, .. } | MutationProgress::Retries { total: 0, .. }
        );
        if idle || now.duration_since(self.last_line) < self.interval {
            return None;
        }
        Some(self.line(event, now, true))
    }

    fn line(&mut self, event: MutationProgress, now: Instant, repeat: bool) -> String {
        self.last_line = now;
        let elapsed = format_elapsed(now.duration_since(self.started));
        let text = match event {
            MutationProgress::Setup { mutants } if repeat => {
                format!("the runner setup for {mutants} {} continues", noun(mutants))
            }
            MutationProgress::Setup { mutants } => {
                format!("runner setup for {mutants} {}", noun(mutants))
            }
            MutationProgress::Baseline { test_files } if repeat => format!(
                "the baseline run of {test_files} test {} continues",
                files(test_files)
            ),
            MutationProgress::Baseline { test_files } => format!(
                "baseline run of {test_files} test {} with no active mutant",
                files(test_files)
            ),
            MutationProgress::Mutants { done, total } => {
                format!("{done}/{total} {} done", noun(total))
            }
            MutationProgress::Retries { done, total } => {
                format!("{done}/{total} retries of survivors and timeouts done")
            }
            MutationProgress::Finalize if repeat => {
                "the worktree check and the report continue".to_owned()
            }
            _ => "the tests stopped; worktree check and report".to_owned(),
        };
        format!("{PREFIX} {text} ({elapsed})")
    }
}

fn noun(count: usize) -> &'static str {
    if count == 1 { "mutant" } else { "mutants" }
}

fn files(count: usize) -> &'static str {
    if count == 1 { "file" } else { "files" }
}

/// `45s`, or `2m05s` from one minute.
fn format_elapsed(elapsed: Duration) -> String {
    let secs = elapsed.as_secs();
    if secs < 60 {
        format!("{secs}s")
    } else {
        format!("{}m{:02}s", secs / 60, secs % 60)
    }
}

/// Prints the phase progress of one run on stderr until [`Self::finish`].
pub struct MutationProgressReporter {
    state: Arc<Mutex<ProgressState>>,
    stop: Option<Sender<()>>,
    ticker: Option<JoinHandle<()>>,
}

impl MutationProgressReporter {
    /// Starts the reporter and its ticker thread.
    pub fn start() -> Self {
        let interval = if std::io::stderr().is_terminal() {
            TERMINAL_INTERVAL
        } else {
            CAPTURED_INTERVAL
        };
        let state = Arc::new(Mutex::new(ProgressState::new(Instant::now(), interval)));
        let (stop, stopped) = mpsc::channel::<()>();
        let ticker_state = Arc::clone(&state);
        let ticker = std::thread::Builder::new()
            .name("fallow-mutation-progress".to_owned())
            .spawn(move || {
                while stopped.recv_timeout(TICK) == Err(RecvTimeoutError::Timeout) {
                    let line = ticker_state
                        .lock()
                        .ok()
                        .and_then(|mut state| state.on_tick(Instant::now()));
                    if let Some(line) = line {
                        eprintln!("{line}");
                    }
                }
            })
            .ok();
        Self {
            state,
            stop: Some(stop),
            ticker,
        }
    }

    /// Prints the line of `event` when the interval rules allow it.
    pub fn report(&self, event: MutationProgress) {
        // The lock stays held while the line prints, so two workers never
        // print the same count twice or out of order.
        if let Ok(mut state) = self.state.lock()
            && let Some(line) = state.on_event(event, Instant::now())
        {
            eprintln!("{line}");
        }
    }

    /// Stops the ticker thread. The report that follows prints no progress.
    pub fn finish(mut self) {
        self.stop_ticker();
    }

    fn stop_ticker(&mut self) {
        drop(self.stop.take());
        if let Some(ticker) = self.ticker.take() {
            let _ = ticker.join();
        }
    }
}

impl Drop for MutationProgressReporter {
    fn drop(&mut self) {
        self.stop_ticker();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const INTERVAL: Duration = Duration::from_secs(10);

    fn at(state: &ProgressState, secs: u64) -> Instant {
        state.started + Duration::from_secs(secs)
    }

    #[test]
    fn each_phase_start_prints_at_once_with_the_run_time() {
        let start = Instant::now();
        let mut state = ProgressState::new(start, INTERVAL);
        let lines: Vec<String> = [
            (MutationProgress::Setup { mutants: 20 }, 0),
            (MutationProgress::Baseline { test_files: 1 }, 2),
            (MutationProgress::Mutants { done: 0, total: 20 }, 3),
            (MutationProgress::Retries { done: 0, total: 3 }, 65),
            (MutationProgress::Finalize, 70),
        ]
        .into_iter()
        .filter_map(|(event, secs)| state.on_event(event, at(&state, secs)))
        .collect();
        assert_eq!(
            lines,
            [
                "fallow: mutation: runner setup for 20 mutants (0s)",
                "fallow: mutation: baseline run of 1 test file with no active mutant (2s)",
                "fallow: mutation: 0/20 mutants done (3s)",
                "fallow: mutation: 0/3 retries of survivors and timeouts done (1m05s)",
                "fallow: mutation: the tests stopped; worktree check and report (1m10s)",
            ]
        );
    }

    #[test]
    fn a_mutant_count_prints_once_per_interval_and_at_the_end() {
        let mut state = ProgressState::new(Instant::now(), INTERVAL);
        let total = 4;
        let mut printed = Vec::new();
        for (done, secs) in [(0, 0), (1, 3), (2, 11), (3, 12), (4, 13)] {
            let now = at(&state, secs);
            if state
                .on_event(MutationProgress::Mutants { done, total }, now)
                .is_some()
            {
                printed.push(done);
            }
        }
        assert_eq!(printed, [0, 2, 4]);
    }

    #[test]
    fn a_silent_phase_prints_a_line_after_one_interval() {
        let mut state = ProgressState::new(Instant::now(), INTERVAL);
        assert_eq!(state.on_tick(at(&state, 30)), None, "no phase started yet");
        state.on_event(MutationProgress::Baseline { test_files: 3 }, at(&state, 1));
        assert_eq!(state.on_tick(at(&state, 5)), None);
        assert_eq!(
            state.on_tick(at(&state, 11)).as_deref(),
            Some("fallow: mutation: the baseline run of 3 test files continues (11s)")
        );
        assert_eq!(
            state.on_tick(at(&state, 12)),
            None,
            "the tick reset the interval"
        );
        state.on_event(
            MutationProgress::Mutants { done: 1, total: 9 },
            at(&state, 13),
        );
        assert_eq!(
            state.on_tick(at(&state, 21)).as_deref(),
            Some("fallow: mutation: 1/9 mutants done (21s)"),
            "a held count prints on the tick"
        );
    }

    #[test]
    fn a_phase_with_no_mutant_job_prints_no_count() {
        let mut state = ProgressState::new(Instant::now(), INTERVAL);
        assert_eq!(
            state.on_event(
                MutationProgress::Mutants { done: 0, total: 0 },
                at(&state, 0)
            ),
            None
        );
        assert_eq!(state.on_tick(at(&state, 30)), None);
    }
}
