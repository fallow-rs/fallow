//! Timestamps and file names of the health snapshots in `.fallow/snapshots/`.

use std::path::{Path, PathBuf};

/// Number of seconds in one day.
const SECS_PER_DAY: u64 = 86_400;

/// ISO 8601 UTC timestamp without external chrono dependency.
pub fn chrono_timestamp() -> String {
    use std::time::SystemTime;
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();

    let days = secs / SECS_PER_DAY;
    let time_secs = secs % SECS_PER_DAY;
    let hours = time_secs / 3600;
    let minutes = (time_secs % 3600) / 60;
    let seconds = time_secs % 60;

    let (year, month, day) = days_to_ymd(days);

    format!("{year:04}-{month:02}-{day:02}T{hours:02}:{minutes:02}:{seconds:02}Z")
}

/// Convert days since Unix epoch to (year, month, day).
pub(super) const fn days_to_ymd(days: u64) -> (u64, u64, u64) {
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

/// The first snapshot path in `dir` that no file holds: `{stem}.json`, then
/// `{stem}-2.json`, `{stem}-3.json` and so on. The timestamp has whole
/// seconds, so two saves in one second would otherwise share one file and
/// the second save would replace the first.
pub(super) fn free_snapshot_path(dir: &Path, stem: &str) -> PathBuf {
    let first = dir.join(format!("{stem}.json"));
    if !first.exists() {
        return first;
    }
    (2..=u32::MAX)
        .map(|suffix| dir.join(format!("{stem}-{suffix}.json")))
        .find(|path| !path.exists())
        .unwrap_or(first)
}

/// The order of a snapshot among the saves of one second: 1 for
/// `{timestamp}.json`, N for the `{timestamp}-N.json` that
/// [`free_snapshot_path`] gives a later save.
pub(super) fn save_order(path: &Path) -> u32 {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .and_then(|stem| stem.rsplit_once('-'))
        .filter(|(timestamp, _)| timestamp.ends_with('Z'))
        .and_then(|(_, suffix)| suffix.parse().ok())
        .unwrap_or(1)
}
