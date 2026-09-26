//! Run-scoped analysis clock.
//!
//! Churn recency weighting, ownership staleness, and the churn window all need
//! a "now". Reading the system clock separately at each of those sites makes
//! two runs over the same commit disagree: the recency decay is continuous, so
//! `weighted_commits` moves every run, and `stale_days` flips fixed thresholds
//! (owner-active, drift minimum file age) as the day rolls over. The clock here
//! resolves once per churn analysis from HEAD's committer timestamp, so one
//! commit always yields the same churn-derived numbers on any machine.
//!
//! `FALLOW_CLOCK_EPOCH` pins the reference epoch explicitly, for reproducible
//! builds and for comparing two checkouts against a fixed instant.

use std::path::Path;
use std::process::Stdio;

/// Environment override for the run's reference epoch, in unix seconds.
pub const CLOCK_EPOCH_ENV: &str = "FALLOW_CLOCK_EPOCH";

/// Seconds in one day.
const SECS_PER_DAY: u64 = 86_400;

/// Where a run's reference epoch came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnalysisClockSource {
    /// Pinned by [`CLOCK_EPOCH_ENV`].
    Environment,
    /// HEAD's committer timestamp: identical for every run over one commit.
    HeadCommit,
    /// The system wall clock, when HEAD has no readable committer timestamp.
    WallClock,
}

/// The single instant a run measures commit ages and staleness against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnalysisClock {
    epoch_secs: u64,
    source: AnalysisClockSource,
}

impl AnalysisClock {
    /// Resolve the clock for `root`: the environment override first, then
    /// HEAD's committer timestamp, then the system wall clock.
    #[must_use]
    pub fn for_repo(root: &Path) -> Self {
        if let Some(epoch_secs) = env_epoch_secs() {
            return Self {
                epoch_secs,
                source: AnalysisClockSource::Environment,
            };
        }
        if let Some(epoch_secs) = head_commit_epoch_secs(root) {
            return Self {
                epoch_secs,
                source: AnalysisClockSource::HeadCommit,
            };
        }
        Self {
            epoch_secs: wall_clock_secs(),
            source: AnalysisClockSource::WallClock,
        }
    }

    /// A clock pinned to an explicit epoch. Used by tests and by embedders that
    /// already know the instant they want the analysis measured against.
    #[must_use]
    pub const fn pinned(epoch_secs: u64) -> Self {
        Self {
            epoch_secs,
            source: AnalysisClockSource::Environment,
        }
    }

    /// The reference epoch, in unix seconds.
    #[must_use]
    pub const fn epoch_secs(self) -> u64 {
        self.epoch_secs
    }

    /// Where the reference epoch came from.
    #[must_use]
    pub const fn source(self) -> AnalysisClockSource {
        self.source
    }

    /// True when two runs over the same commit resolve the same epoch.
    #[must_use]
    pub const fn is_reproducible(self) -> bool {
        !matches!(self.source, AnalysisClockSource::WallClock)
    }

    /// The epoch `days` days before the clock.
    #[must_use]
    pub const fn minus_days(self, days: u64) -> u64 {
        self.epoch_secs
            .saturating_sub(days.saturating_mul(SECS_PER_DAY))
    }

    /// The epoch `months` calendar months before the clock, in UTC. A
    /// day-of-month that the earlier month does not have clamps to its last
    /// day, so "1 month before March 31" is February 28 rather than March 3.
    #[must_use]
    pub fn minus_months(self, months: u64) -> u64 {
        self.shift_months(i64::try_from(months).unwrap_or(i64::MAX))
    }

    /// The epoch `years` years before the clock, in UTC. February 29 clamps to
    /// February 28 in a non-leap year.
    #[must_use]
    pub fn minus_years(self, years: u64) -> u64 {
        self.shift_months(
            i64::try_from(years)
                .unwrap_or(i64::MAX / 12)
                .saturating_mul(12),
        )
    }

    fn shift_months(self, months: i64) -> u64 {
        let days = i64::try_from(self.epoch_secs / SECS_PER_DAY).unwrap_or(0);
        let time_of_day = self.epoch_secs % SECS_PER_DAY;
        let (year, month, day) = civil_from_days(days);

        let total = year * 12 + (month - 1) - months;
        let shifted_year = total.div_euclid(12);
        let shifted_month = total.rem_euclid(12) + 1;
        let shifted_day = day.min(days_in_month(shifted_year, shifted_month));

        let shifted_days = days_from_civil(shifted_year, shifted_month, shifted_day);
        u64::try_from(shifted_days)
            .unwrap_or(0)
            .saturating_mul(SECS_PER_DAY)
            .saturating_add(time_of_day)
    }
}

/// Parse an ISO `YYYY-MM-DD` date into the epoch of its UTC midnight.
///
/// Git's own date parser reads a bare date in the machine's local time zone, so
/// the same `--since 2025-06-01` covered a different span on two machines. UTC
/// makes the window mean one thing everywhere.
#[must_use]
pub fn utc_midnight_epoch(iso_date: &str) -> Option<u64> {
    let mut parts = iso_date.split('-');
    let year: i64 = parts.next()?.parse().ok()?;
    let month: i64 = parts.next()?.parse().ok()?;
    let day: i64 = parts.next()?.parse().ok()?;
    if parts.next().is_some() || !(1..=12).contains(&month) {
        return None;
    }
    if day < 1 || day > days_in_month(year, month) {
        return None;
    }
    u64::try_from(days_from_civil(year, month, day))
        .ok()
        .map(|days| days * SECS_PER_DAY)
}

/// The UTC calendar date of a unix epoch, as `YYYY-MM-DD`.
#[must_use]
pub fn utc_date(epoch_secs: u64) -> String {
    let days = i64::try_from(epoch_secs / SECS_PER_DAY).unwrap_or(0);
    let (year, month, day) = civil_from_days(days);
    format!("{year:04}-{month:02}-{day:02}")
}

/// A unix epoch as an RFC 3339 UTC timestamp, `YYYY-MM-DDTHH:MM:SSZ`.
#[must_use]
pub fn utc_timestamp(epoch_secs: u64) -> String {
    let time_of_day = epoch_secs % SECS_PER_DAY;
    format!(
        "{}T{:02}:{:02}:{:02}Z",
        utc_date(epoch_secs),
        time_of_day / 3600,
        (time_of_day % 3600) / 60,
        time_of_day % 60
    )
}

fn env_epoch_secs() -> Option<u64> {
    let raw = std::env::var(CLOCK_EPOCH_ENV).ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    match trimmed.parse::<u64>() {
        Ok(epoch_secs) => Some(epoch_secs),
        Err(e) => {
            tracing::warn!("ignoring {CLOCK_EPOCH_ENV}={raw}: {e}");
            None
        }
    }
}

fn head_commit_epoch_secs(root: &Path) -> Option<u64> {
    let output = crate::git_env::git_command()
        .args(["log", "-1", "--format=%ct", "HEAD"])
        .current_dir(root)
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout).trim().parse().ok()
}

fn wall_clock_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Days since 1970-01-01 for a proleptic-Gregorian date.
///
/// Howard Hinnant's `days_from_civil`, so month and year windows land on real
/// calendar boundaries instead of on a 30-day approximation.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let day_of_year = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// Inverse of [`days_from_civil`], returning `(year, month, day)`.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let shifted = days + 719_468;
    let era = if shifted >= 0 {
        shifted
    } else {
        shifted - 146_096
    } / 146_097;
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = if month_prime < 10 {
        month_prime + 3
    } else {
        month_prime - 9
    };
    (if month <= 2 { year + 1 } else { year }, month, day)
}

fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        2 if is_leap_year(year) => 29,
        2 => 28,
        // 4, 6, 9 and 11 have 30 days; callers only ever pass 1..=12, so the
        // wildcard covers them and nothing else.
        _ => 30,
    }
}

const fn is_leap_year(year: i64) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

#[cfg(test)]
mod tests {
    use super::{
        AnalysisClock, AnalysisClockSource, civil_from_days, days_from_civil, utc_date,
        utc_midnight_epoch, utc_timestamp,
    };

    /// 2026-09-07T12:00:00Z.
    const NOON: u64 = 1_788_782_400;

    #[test]
    fn utc_date_and_timestamp_format_the_epoch() {
        assert_eq!(utc_date(0), "1970-01-01");
        assert_eq!(utc_timestamp(1_758_758_400), "2025-09-25T00:00:00Z");
        assert_eq!(utc_timestamp(1_709_210_096), "2024-02-29T12:34:56Z");
        assert_eq!(utc_date(1_709_210_096), "2024-02-29");
    }

    #[test]
    fn civil_conversions_round_trip() {
        for days in [-719_468, -1, 0, 1, 19_000, 20_338, 100_000] {
            let (year, month, day) = civil_from_days(days);
            assert_eq!(days_from_civil(year, month, day), days);
        }
    }

    #[test]
    fn utc_midnight_epoch_parses_and_rejects() {
        assert_eq!(utc_midnight_epoch("1970-01-01"), Some(0));
        assert_eq!(utc_midnight_epoch("2025-06-01"), Some(1_748_736_000));
        assert_eq!(utc_midnight_epoch("2024-02-29"), Some(1_709_164_800));
        assert_eq!(utc_midnight_epoch("2025-02-29"), None);
        assert_eq!(utc_midnight_epoch("2025-13-01"), None);
        assert_eq!(utc_midnight_epoch("2025-06"), None);
        assert_eq!(utc_midnight_epoch("2025-06-01-01"), None);
    }

    #[test]
    fn relative_windows_land_on_calendar_boundaries() {
        let clock = AnalysisClock::pinned(NOON);
        assert_eq!(clock.minus_days(0), NOON);
        assert_eq!(clock.minus_days(7), NOON - 7 * 86_400);
        // 2026-09-07 minus six months is 2026-03-07, not 180 days.
        assert_eq!(clock.minus_months(6), NOON - 184 * 86_400);
        // 2026-09-07 minus one year is 2025-09-07.
        assert_eq!(clock.minus_years(1), NOON - 365 * 86_400);
    }

    #[test]
    fn month_shift_clamps_a_missing_day_of_month() {
        // 2026-03-31T00:00:00Z minus one month clamps to 2026-02-28.
        let clock = AnalysisClock::pinned(1_774_915_200);
        assert_eq!(clock.minus_months(1), 1_772_236_800);
    }

    #[test]
    fn pinned_clock_is_reproducible() {
        let clock = AnalysisClock::pinned(NOON);
        assert_eq!(clock.epoch_secs(), NOON);
        assert_eq!(clock.source(), AnalysisClockSource::Environment);
        assert!(clock.is_reproducible());
    }
}
