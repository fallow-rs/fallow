//! The hidden `fallow telemetry claim-prompt` command.
//!
//! The MCP server can ask the person once, in a form, whether to share
//! usage data. Several servers can run on one machine at once, so the CLI
//! keeps one ask count for all of them in `telemetry-prompt.json`, next to
//! `telemetry.json`. This command answers whether a server may ask now, and
//! records the ask when it may. The count and the time stay on this
//! machine and are never uploaded.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::telemetry::{
    DEBUG_ENV, EffectiveMode, MODE_ENV, ModeSource, admin_disabled, config_path, effective_config,
    read_config_from,
};

/// The most forms that the MCP servers of one machine send in total.
const MAX_PROMPTS: u8 = 2;

/// The least time between two forms.
const MIN_PROMPT_GAP: Duration = Duration::from_hours(14 * 24);

/// A lock file older than this belongs to a process that died, so a new
/// claim may remove it.
const STALE_LOCK: Duration = Duration::from_mins(1);

const PROMPT_FILE_NAME: &str = "telemetry-prompt.json";
const PROMPT_LOCK_NAME: &str = "telemetry-prompt.lock";

/// The asks so far on this machine.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
struct PromptState {
    count: u8,
    /// Seconds since the Unix epoch of the last ask.
    last_at: u64,
}

/// Answer whether the MCP server may ask now, and record the ask when it
/// may. Prints `{"ask": <bool>}` and always exits 0.
pub(super) fn claim_prompt() -> ExitCode {
    let ask = config_path().is_some_and(|path| claim_at(&path, now_secs()));
    println!("{}", serde_json::json!({ "ask": ask }));
    ExitCode::SUCCESS
}

/// The whole decision for the telemetry config at `config`, at `now`.
fn claim_at(config: &Path, now: u64) -> bool {
    if !may_ask_here(config) {
        return false;
    }
    let Some(dir) = config.parent() else {
        return false;
    };
    // An unwritable config folder cannot keep the count, so every new
    // container or session would ask again.
    if std::fs::create_dir_all(dir).is_err() {
        return false;
    }
    let Some(_lock) = PromptLock::acquire(&dir.join(PROMPT_LOCK_NAME)) else {
        return false;
    };
    let path = dir.join(PROMPT_FILE_NAME);
    let state = read_state(&path);
    if !allows_ask(state, now) {
        return false;
    }
    let next = PromptState {
        count: state.count.saturating_add(1),
        last_at: now,
    };
    write_state(&path, next)
}

/// True when nothing on this machine has decided yet: no kill switch, no
/// CI, no environment override, and no recorded decision of the person.
fn may_ask_here(config: &Path) -> bool {
    if admin_disabled()
        || fallow_engine::ci_env::is_ci()
        || std::env::var_os(MODE_ENV).is_some()
        || std::env::var_os(DEBUG_ENV).is_some()
    {
        return false;
    }
    let effective = effective_config();
    if effective.mode != EffectiveMode::Off
        || !matches!(
            effective.source,
            ModeSource::UserConfig | ModeSource::Default
        )
    {
        return false;
    }
    read_config_from(config).map_or(true, |config| !config.explicit_decision && !config.enabled)
}

/// True when the count and the gap allow one more ask at `now`.
fn allows_ask(state: PromptState, now: u64) -> bool {
    state.count < MAX_PROMPTS
        && (state.count == 0 || now.saturating_sub(state.last_at) >= MIN_PROMPT_GAP.as_secs())
}

/// The recorded state. A missing file counts as no ask yet. A file that
/// cannot be read or parsed counts as the full limit, so damage to the file
/// never gives a machine new asks.
fn read_state(path: &Path) -> PromptState {
    let full = PromptState {
        count: MAX_PROMPTS,
        last_at: 0,
    };
    match std::fs::read_to_string(path) {
        Ok(raw) => serde_json::from_str(&raw).unwrap_or(full),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => PromptState::default(),
        Err(_) => full,
    }
}

/// Write the state. `false` when the write fails: an ask that cannot be
/// counted must not happen.
fn write_state(path: &Path, state: PromptState) -> bool {
    let Ok(raw) = serde_json::to_string(&state) else {
        return false;
    };
    let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
    if std::fs::write(&tmp, raw).is_err() {
        return false;
    }
    if std::fs::rename(&tmp, path).is_err() {
        let _ = std::fs::remove_file(&tmp);
        return false;
    }
    true
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

/// A create-new lock file, so that two servers that claim at the same time
/// cannot both ask. The file holds a token that is unique to this claim, so
/// a claim removes only its own lock.
struct PromptLock {
    path: PathBuf,
    token: String,
}

impl PromptLock {
    fn acquire(path: &Path) -> Option<Self> {
        let token = format!(
            "{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |elapsed| elapsed.as_nanos())
        );
        if Self::create(path, &token) {
            return Some(Self {
                path: path.to_path_buf(),
                token,
            });
        }
        if !Self::take_over_stale(path, &token) {
            return None;
        }
        Self::create(path, &token).then(|| Self {
            path: path.to_path_buf(),
            token,
        })
    }

    fn create(path: &Path, token: &str) -> bool {
        let Ok(mut file) = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
        else {
            return false;
        };
        std::io::Write::write_all(&mut file, token.as_bytes()).is_ok()
    }

    /// Remove a lock that a dead process left. The rename is atomic, so of
    /// two claims that see the same stale lock, only one moves it. A claim
    /// that moved a lock with other content (a fresh lock of another claim)
    /// puts it back and gives up.
    fn take_over_stale(path: &Path, token: &str) -> bool {
        let Ok(seen) = std::fs::read_to_string(path) else {
            return false;
        };
        // A modification time in the future (after a clock change) also
        // counts as stale, so the lock cannot stop every later claim.
        let stale = std::fs::metadata(path)
            .and_then(|meta| meta.modified())
            .is_ok_and(|modified| modified.elapsed().map_or(true, |age| age >= STALE_LOCK));
        if !stale {
            return false;
        }
        let moved = path.with_extension(format!("stale.{token}"));
        if std::fs::rename(path, &moved).is_err() {
            return false;
        }
        if std::fs::read_to_string(&moved).ok().as_deref() != Some(seen.as_str()) {
            let _ = std::fs::rename(&moved, path);
            return false;
        }
        std::fs::remove_file(&moved).is_ok()
    }
}

impl Drop for PromptLock {
    fn drop(&mut self) {
        if std::fs::read_to_string(&self.path).ok().as_deref() == Some(self.token.as_str()) {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: u64 = 24 * 60 * 60;

    #[test]
    fn the_first_ask_is_allowed() {
        assert!(allows_ask(PromptState::default(), 0));
    }

    #[test]
    fn a_second_ask_waits_fourteen_days() {
        let first = PromptState {
            count: 1,
            last_at: 100 * DAY,
        };
        assert!(!allows_ask(first, 113 * DAY));
        assert!(allows_ask(first, 114 * DAY));
    }

    #[test]
    fn no_third_ask() {
        let second = PromptState {
            count: MAX_PROMPTS,
            last_at: 0,
        };
        assert!(!allows_ask(second, 1_000 * DAY));
    }

    #[test]
    fn a_claim_is_counted_and_the_next_claim_waits() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join(PROMPT_FILE_NAME);
        assert!(write_state(
            &path,
            PromptState {
                count: 0,
                last_at: 0
            }
        ));
        assert_eq!(read_state(&path).count, 0);
        let lock = dir.path().join(PROMPT_LOCK_NAME);
        let held = PromptLock::acquire(&lock).expect("first lock");
        assert!(PromptLock::acquire(&lock).is_none(), "the lock is held");
        drop(held);
        assert!(
            PromptLock::acquire(&lock).is_some(),
            "the lock is free again"
        );
    }

    #[test]
    fn a_missing_state_counts_as_no_ask() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join(PROMPT_FILE_NAME);
        assert_eq!(read_state(&path), PromptState::default());
    }

    #[test]
    fn a_damaged_state_counts_as_the_full_limit() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join(PROMPT_FILE_NAME);
        std::fs::write(&path, "not json").expect("write");
        assert!(!allows_ask(read_state(&path), u64::MAX));
    }

    #[test]
    fn a_stale_lock_is_taken_over_and_a_fresh_lock_is_kept() {
        let dir = tempfile::tempdir().expect("temp dir");
        let lock = dir.path().join(PROMPT_LOCK_NAME);
        std::fs::write(&lock, "dead-claim").expect("write lock");
        let old = SystemTime::now() - STALE_LOCK - Duration::from_secs(1);
        std::fs::File::options()
            .write(true)
            .open(&lock)
            .and_then(|file| file.set_modified(old))
            .expect("age the lock");
        let held = PromptLock::acquire(&lock).expect("a stale lock is taken over");
        assert!(PromptLock::acquire(&lock).is_none(), "a fresh lock is kept");
        drop(held);
        assert!(!lock.exists(), "the claim removes its own lock");
    }

    #[test]
    fn a_claim_never_removes_the_lock_of_another_claim() {
        let dir = tempfile::tempdir().expect("temp dir");
        let lock = dir.path().join(PROMPT_LOCK_NAME);
        let held = PromptLock::acquire(&lock).expect("first lock");
        std::fs::write(&lock, "another-claim").expect("replace the lock");
        drop(held);
        assert!(lock.exists(), "the lock of another claim stays");
    }
}
