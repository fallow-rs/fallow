//! The drain of the telemetry spool: upload the spooled events and keep
//! the rest.

use std::path::Path;

use super::{SPOOL_LOCK_NAME, SPOOL_MAX_EVENTS, SpoolLock, parse_spool_line, rewrite_spool};

/// POST spooled events oldest-first and drop the delivered ones in place.
///
/// Generic over the uploader so tests can inject a fake without touching the
/// network, and parameterized on the resolved install grouping token so tests
/// never read the real env/config dir (the live resolution happens at the
/// [`flush_spool_in_background`](super::flush_spool_in_background) spawn site). The flock is held for the whole drain so a concurrent drain or trim
/// cannot rewrite the spool underneath it. Events are delivered oldest-first and
/// the drain stops at the first POST failure (a likely-unreachable endpoint), so
/// the removed set is always a prefix; the file is rewritten to the undelivered
/// tail (capped, in case it grew while the endpoint was down). Crucially, if the
/// thread is abandoned at process exit mid-upload, no rewrite runs and the spool
/// is simply retried next run, bounded meanwhile by [`trim_spool_if_oversized`](super::trim_spool_if_oversized)
/// rather than by this function completing.
pub(super) fn drain_spool_file<P>(spool: &Path, install_id: Option<&str>, mut post: P)
where
    P: FnMut(&serde_json::Value, Option<&str>, Option<&str>) -> Result<(), String>,
{
    let lock_path = spool.with_file_name(SPOOL_LOCK_NAME);
    let Some(_lock) = SpoolLock::try_acquire(&lock_path) else {
        return;
    };
    let Ok(contents) = std::fs::read_to_string(spool) else {
        return;
    };
    let lines: Vec<&str> = contents
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    if lines.is_empty() {
        let _ = std::fs::remove_file(spool);
        return;
    }

    let mut removed = 0usize;
    for line in &lines {
        match parse_spool_line(line) {
            // A corrupt (non-JSON) line cannot be delivered, so it is dropped.
            Err(_) => removed += 1,
            Ok((payload, parent_run)) => {
                if post(&payload, parent_run.as_deref(), install_id).is_ok() {
                    removed += 1;
                } else {
                    // The endpoint is likely unreachable; stop and keep the rest.
                    break;
                }
            }
        }
    }

    if removed == 0 {
        return;
    }
    // Appends take no lock, so another process can add events during the
    // uploads. The file only grows while this lock is held, so every byte
    // past the first read is a new event that the rewrite must keep.
    let appended = std::fs::read_to_string(spool)
        .ok()
        .and_then(|now| now.get(contents.len()..).map(str::to_owned))
        .unwrap_or_default();
    let mut remaining: Vec<&str> = lines[removed..].to_vec();
    remaining.extend(
        appended
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty()),
    );
    let keep_from = remaining.len().saturating_sub(SPOOL_MAX_EVENTS);
    rewrite_spool(spool, &remaining[keep_from..]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_event_appended_during_the_uploads_is_kept() {
        let dir = tempfile::tempdir().expect("temp dir");
        let spool = dir.path().join(super::super::SPOOL_FILE_NAME);
        std::fs::write(&spool, "{\"old\":1}\n{\"old\":2}\n").expect("seed spool");
        let mut appended = false;
        drain_spool_file(&spool, None, |_value, _parent_run, _install| {
            if !appended {
                appended = true;
                let mut file = std::fs::OpenOptions::new()
                    .append(true)
                    .open(&spool)
                    .expect("open spool");
                std::io::Write::write_all(&mut file, b"{\"new\":1}\n").expect("append");
            }
            Ok(())
        });
        let left = std::fs::read_to_string(&spool).expect("the new event stays");
        assert_eq!(left, "{\"new\":1}\n");
    }
}
