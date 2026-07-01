//! CLI-owned runtime state for root output serialization.

use std::sync::Mutex;

static TELEMETRY_ANALYSIS_RUN_ID: Mutex<Option<String>> = Mutex::new(None);

#[must_use]
pub fn current_root_envelope_mode() -> fallow_output::RootEnvelopeMode {
    fallow_output::RootEnvelopeMode::Tagged
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
