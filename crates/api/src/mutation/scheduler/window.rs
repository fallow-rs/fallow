//! The time windows of a mutant run: the timeout of the run and the loop
//! budget that the companion checks inside a test (section 8).

/// The fixed part of `timeout_ms` (section 8, the Stryker default).
const TIMEOUT_BASE_MS: u64 = 5000;
/// The factor on the covering-test duration (section 8, the Stryker default).
const TIMEOUT_FACTOR: f64 = 1.5;
/// The fixed part of `loop_budget_ms`. The companion stops a test of a mutant
/// run that still evaluates instrumented code after the budget, so an
/// endless loop through instrumented code ends in under a second instead of
/// at the watchdog.
pub(super) const LOOP_BUDGET_BASE_MS: u64 = 500;
/// The largest loop budget whose loop timeout needs no retry. The companion
/// measures the budget in the CPU time of the test thread, so other load on
/// the host does not use it. Up to this value, the budget is at least four
/// times the covering-test time of the dry run.
const SETTLED_LOOP_BUDGET_MS: u64 = 800;

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

/// A run duration in whole ms. A value that is not finite or is negative
/// gives `None`.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "durations are finite and non-negative milliseconds"
)]
pub(super) fn round_ms(value: Option<f64>) -> Option<u64> {
    value
        .filter(|value| value.is_finite() && *value >= 0.0)
        .map(|value| value.round() as u64)
}

/// True when a loop timeout of a job with `loop_budget_ms` is the final
/// status, so the job gets no retry.
pub(super) const fn settles_loop_timeout(loop_budget_ms: u64) -> bool {
    loop_budget_ms <= SETTLED_LOOP_BUDGET_MS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_loop_budget_keeps_the_scaled_test_time_with_its_own_base() {
        assert_eq!(timeout_ms(102.0), 5153);
        assert_eq!(loop_budget_ms(timeout_ms(102.0)), 653);
        assert_eq!(loop_budget_ms(timeout_ms(0.0)), LOOP_BUDGET_BASE_MS);
    }

    #[test]
    fn a_timeout_below_the_timeout_base_gives_the_loop_base() {
        assert_eq!(loop_budget_ms(0), LOOP_BUDGET_BASE_MS);
        assert_eq!(loop_budget_ms(TIMEOUT_BASE_MS - 1), LOOP_BUDGET_BASE_MS);
    }

    #[test]
    fn a_loop_timeout_settles_up_to_covering_tests_of_200_ms() {
        assert!(settles_loop_timeout(loop_budget_ms(timeout_ms(200.0))));
        assert!(!settles_loop_timeout(loop_budget_ms(timeout_ms(201.0))));
    }

    #[test]
    fn a_duration_that_is_not_finite_scales_to_zero() {
        assert_eq!(scaled_ms(f64::NAN), 0);
        assert_eq!(scaled_ms(f64::INFINITY), 0);
        assert_eq!(scaled_ms(-3.0), 0);
    }
}
