//! The time windows of a mutant run: the timeout of the run and the loop
//! budget that the companion checks inside a test (section 8).

/// The fixed part of `timeout_ms` (section 8, the Stryker default).
const TIMEOUT_BASE_MS: u64 = 5000;
/// The factor on the covering-test duration (section 8, the Stryker default).
const TIMEOUT_FACTOR: f64 = 1.5;
/// The fixed part of `loop_budget_ms`. The companion stops a test that still
/// runs the active mutant after the budget, so an endless loop through the
/// mutant ends in under a second instead of at the watchdog.
pub(super) const LOOP_BUDGET_BASE_MS: u64 = 500;

/// `1.5 * duration`, rounded up. A duration that is not finite gives 0.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the duration sum is finite and non-negative"
)]
pub(super) fn scaled_ms(duration_ms: f64) -> u64 {
    let scaled = (duration_ms.max(0.0) * TIMEOUT_FACTOR).ceil();
    if scaled.is_finite() { scaled as u64 } else { 0 }
}

/// `timeout_ms = 1.5 * covering-test duration + 5000`, rounded up.
pub(super) fn timeout_ms(total_duration_ms: f64) -> u64 {
    scaled_ms(total_duration_ms) + TIMEOUT_BASE_MS
}

/// The loop budget of a job with `timeout_ms`: the same scaled test time,
/// with [`LOOP_BUDGET_BASE_MS`] in place of [`TIMEOUT_BASE_MS`].
pub(super) fn loop_budget_ms(timeout_ms: u64) -> u64 {
    timeout_ms.saturating_sub(TIMEOUT_BASE_MS) + LOOP_BUDGET_BASE_MS
}
