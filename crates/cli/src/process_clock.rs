//! Process-level clock for `--performance`.
//!
//! The pipeline stages report their own time, but a command also parses
//! arguments, builds the thread pool, loads config, filters results and writes
//! the report. This clock measures those spans, so the report can show the
//! wall time and the time that no span covers.
//!
//! One CLI process runs one command, so the clock is process-global. A span
//! that runs more than once, for example config loading in combined mode,
//! adds up.

use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use fallow_types::pipeline_spans::ProcessTimings;

static START: OnceLock<Instant> = OnceLock::new();
static SPANS: Mutex<ProcessTimings> = Mutex::new(ProcessTimings {
    wall_ms: 0.0,
    startup_ms: 0.0,
    thread_pool_ms: 0.0,
    config_ms: 0.0,
    git_ms: 0.0,
    analysis_ms: 0.0,
    post_analysis_ms: 0.0,
    output_ms: 0.0,
});

/// A process span that the clock measures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessSpan {
    Startup,
    ThreadPool,
    Config,
    Git,
    Analysis,
    PostAnalysis,
    Output,
}

/// Mark the start of the process. Only the first call counts.
pub fn mark_process_start() {
    START.get_or_init(Instant::now);
}

fn elapsed_ms(since: Instant) -> f64 {
    since.elapsed().as_secs_f64() * 1000.0
}

/// Add `ms` to `span`.
pub fn record(span: ProcessSpan, ms: f64) {
    let Ok(mut spans) = SPANS.lock() else {
        return;
    };
    let slot = match span {
        ProcessSpan::Startup => &mut spans.startup_ms,
        ProcessSpan::ThreadPool => &mut spans.thread_pool_ms,
        ProcessSpan::Config => &mut spans.config_ms,
        ProcessSpan::Git => &mut spans.git_ms,
        ProcessSpan::Analysis => &mut spans.analysis_ms,
        ProcessSpan::PostAnalysis => &mut spans.post_analysis_ms,
        ProcessSpan::Output => &mut spans.output_ms,
    };
    *slot += ms;
}

/// Run `work` and add its wall time to `span`.
pub fn time<T>(span: ProcessSpan, work: impl FnOnce() -> T) -> T {
    let start = Instant::now();
    let output = work();
    record(span, elapsed_ms(start));
    output
}

/// Adds the time from its creation to its drop to one span, on every return
/// path.
pub struct SpanTimer {
    span: ProcessSpan,
    start: Instant,
}

impl Drop for SpanTimer {
    fn drop(&mut self) {
        record(self.span, elapsed_ms(self.start));
    }
}

/// Start a timer that adds to `span` when it drops.
#[must_use]
pub fn start(span: ProcessSpan) -> SpanTimer {
    SpanTimer {
        span,
        start: Instant::now(),
    }
}

/// Record the startup span: the time from process start until now.
pub fn record_startup() {
    if let Some(start) = START.get() {
        record(ProcessSpan::Startup, elapsed_ms(*start));
    }
}

/// The process spans so far, with `wall_ms` read now.
///
/// `None` when the process start was never marked, as in a library call that
/// does not go through the CLI entry point.
pub fn snapshot() -> Option<ProcessTimings> {
    let start = START.get()?;
    let mut timings = *SPANS.lock().ok()?;
    timings.wall_ms = elapsed_ms(*start);
    Some(timings)
}
