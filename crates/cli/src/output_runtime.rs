//! CLI-owned runtime state for root output serialization.

use std::sync::Mutex;

static TELEMETRY_ANALYSIS_RUN_ID: Mutex<Option<String>> = Mutex::new(None);

/// The baseline this run loaded, recorded for the `recheck-baseline` next step.
static LOADED_BASELINE: Mutex<Option<LoadedBaselineRecheck>> = Mutex::new(None);

/// What the `recheck-baseline` next step needs about the loaded baseline.
///
/// Recorded at load time rather than threaded through the render layer. Every
/// JSON render site already receives the staleness object, but none of them
/// receives the path the caller wrote, and the path is the only part a
/// consumer cannot reconstruct from the envelope. One slot per process is
/// honest because a standalone command loads at most one baseline; `command`
/// is what keeps a `dupes` render from offering a `dead-code` path when a
/// single process ran both.
#[derive(Clone, Debug)]
pub struct LoadedBaselineRecheck {
    /// The command that loaded it, as `fallow <command>` spells it.
    pub command: &'static str,
    /// The `--baseline` path exactly as the caller wrote it.
    pub path: String,
    /// Entries the baseline carried.
    pub baseline_entries: usize,
    /// The channels that narrowed the run this baseline was compared against.
    pub scope_reasons: fallow_output::BaselineScopeReasons,
}

pub fn set_loaded_baseline(loaded: LoadedBaselineRecheck) {
    if let Ok(mut current) = LOADED_BASELINE.lock() {
        *current = Some(loaded);
    }
}

/// The loaded baseline, when `command` is the command that loaded it.
#[must_use]
pub fn loaded_baseline_for(command: &str) -> Option<LoadedBaselineRecheck> {
    LOADED_BASELINE
        .lock()
        .ok()
        .and_then(|loaded| loaded.clone())
        .filter(|loaded| loaded.command == command)
}

#[allow(
    dead_code,
    reason = "used by the CLI binary and output contract tests; the library target only reads runtime output state"
)]
pub fn set_telemetry_analysis_run_id(run_id: Option<String>) {
    if let Ok(mut current) = TELEMETRY_ANALYSIS_RUN_ID.lock() {
        *current = run_id;
    }
}

#[must_use]
pub fn telemetry_analysis_run_id() -> Option<String> {
    TELEMETRY_ANALYSIS_RUN_ID
        .lock()
        .ok()
        .and_then(|id| id.clone())
}
