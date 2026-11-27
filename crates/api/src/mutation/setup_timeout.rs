//! The hints that name `--setup-timeout` after a setup timeout.

use std::time::Duration;

use super::protocol::LinkFault;

/// The smallest suggested value. A short value that failed, for example
/// `3s`, gets a value that a slow host can meet.
const MIN_SUGGESTION: Duration = Duration::from_mins(5);

const SECS_PER_MINUTE: u64 = 60;
const MINUTES_PER_HOUR: u64 = 60;

/// The largest number of minutes that `--setup-timeout` accepts: the value
/// in seconds must fit in a `u64`.
const MAX_MINUTES: u64 = u64::MAX / SECS_PER_MINUTE;

/// Gives a `--setup-timeout` value that is longer than `failed`: two times
/// `failed`, minimum 5 minutes, rounded up to full minutes. The text uses
/// hours when the minutes divide into full hours, for example `30m` or `2h`.
pub fn longer_setup_timeout(failed: Duration) -> String {
    let doubled = failed.saturating_mul(2).max(MIN_SUGGESTION);
    let minutes = doubled.as_secs().div_ceil(SECS_PER_MINUTE).min(MAX_MINUTES);
    if minutes.is_multiple_of(MINUTES_PER_HOUR) {
        format!("{}h", minutes / MINUTES_PER_HOUR)
    } else {
        format!("{minutes}m")
    }
}

/// The help of a self-check that timed out. The control run waits the
/// longer of the setup timeout `setup` and the job timeout `job_ms`, so
/// only a value above both gives the run more time.
pub fn self_check_help(setup: Duration, job_ms: u64) -> String {
    let waited = setup.max(Duration::from_millis(job_ms));
    format!(
        "on a slow or busy host, pass --setup-timeout with a longer duration, for example {}, or run on a less loaded host",
        longer_setup_timeout(waited)
    )
}

/// The hint for a setup watchdog fault of a restarted companion. The run
/// continues, so the hint goes into the mutant error.
pub fn setup_timeout_hint(fault: &LinkFault) -> &'static str {
    if matches!(fault, LinkFault::Timeout) {
        "; on a slow or busy host, pass --setup-timeout with a longer duration"
    } else {
        ""
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_suggestion_is_longer_than_the_value_that_failed() {
        for (failed, expected) in [
            (Duration::from_millis(500), "5m"),
            (Duration::from_secs(3), "5m"),
            (Duration::from_mins(2), "5m"),
            (Duration::from_secs(150), "5m"),
            (Duration::from_secs(151), "6m"),
            (Duration::from_mins(5), "10m"),
            (Duration::from_mins(15), "30m"),
            (Duration::from_mins(30), "1h"),
            (Duration::from_mins(45), "90m"),
            (Duration::from_hours(1), "2h"),
        ] {
            assert_eq!(longer_setup_timeout(failed), expected, "{failed:?}");
        }
    }

    #[test]
    fn a_huge_value_gives_a_value_that_fits_in_seconds() {
        let text = longer_setup_timeout(Duration::MAX);
        let (number, secs_per_unit) = match text.strip_suffix('h') {
            Some(hours) => (hours, SECS_PER_MINUTE * MINUTES_PER_HOUR),
            None => (text.trim_end_matches('m'), SECS_PER_MINUTE),
        };
        let number: u64 = number.parse().expect("a number");
        assert!(number.checked_mul(secs_per_unit).is_some(), "{text}");
    }

    #[test]
    fn the_self_check_help_uses_the_longer_of_the_two_timeouts() {
        let setup = Duration::from_mins(1);
        assert!(self_check_help(setup, 5_000).contains("for example 5m"));
        assert!(self_check_help(setup, 605_000).contains("for example 21m"));
    }
}
