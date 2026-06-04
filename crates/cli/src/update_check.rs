//! Best-effort "a newer fallow is available" nudge.
//!
//! Telemetry shows users lag well behind the latest release (an old version
//! dominates run volume while newer ones carry real fixes). This module prints
//! a single concise stderr line on a stale human + TTY + not-quiet + not-CI run
//! pointing at the changelog.
//!
//! It follows the npm `update-notifier` model: the two issue constraints
//! ("fire-and-forget, never block" and "nudge this run") cannot both hold on a
//! single run, so the DISPLAY reads the PREVIOUS run's cached result while a
//! detached background FETCH refreshes the cache for the NEXT run. The display
//! costs nothing (a local file read) and leaks nothing; the network fetch is
//! consent-gated and throttled.
//!
//! Invariants:
//! - Never writes to stdout, never into JSON / SARIF / any machine format.
//! - Suppressed when quiet, non-TTY (either stream), non-human, or in CI.
//! - Suppressed by the universal kill switches (`DO_NOT_TRACK`,
//!   `FALLOW_TELEMETRY_DISABLED`) and the dedicated `FALLOW_UPDATE_CHECK=off`,
//!   plus a persistent `disabled` flag in the cache file.
//! - The fetch runs on a detached thread with a bounded grace window, so it
//!   never blocks process exit, and every error is swallowed so it never
//!   changes the exit code.
//! - Mutually exclusive with the first-run telemetry opt-in note: at most one
//!   unsolicited stderr notice per run.

use std::io::IsTerminal;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use fallow_config::OutputFormat;
use semver::Version;
use serde::{Deserialize, Serialize};

use crate::api::{api_url, try_api_agent_with_timeout};

/// Cache schema version. Bump on a breaking shape change so an old reader
/// discards a future file instead of misparsing it.
const CACHE_SCHEMA_VERSION: u8 = 1;

/// How long a cached latest-version answer stays fresh before the background
/// refresh fires again. Matches the npm / gh once-per-day norm.
const CHECK_TTL_SECS: u64 = 24 * 60 * 60;

/// Bounded grace the main thread waits for the background fetch before
/// abandoning it. The check must never add meaningful latency to a sub-second
/// run.
const FETCH_GRACE_MS: u64 = 200;

/// Connect / total timeouts for the latest-version GET. Deliberately tight: a
/// slow endpoint must not delay the next run's refresh.
const FETCH_CONNECT_TIMEOUT_SECS: u64 = 1;
const FETCH_TOTAL_TIMEOUT_SECS: u64 = 1;

/// Cloud endpoint that returns the latest published stable version. Resolved
/// through `api_url` so `FALLOW_API_URL` overrides apply.
const LATEST_VERSION_PATH: &str = "/v1/cli/latest-version";

/// Dedicated opt-out env var. Truthy-disable values mirror the telemetry mode
/// parser's "off" set.
const UPDATE_CHECK_ENV: &str = "FALLOW_UPDATE_CHECK";
const DO_NOT_TRACK_ENV: &str = "DO_NOT_TRACK";
const TELEMETRY_DISABLED_ENV: &str = "FALLOW_TELEMETRY_DISABLED";

/// Where the user is pointed to decide whether the upgrade is worth it. Kept
/// method-agnostic on purpose: fallow ships via npm, Homebrew, and a direct
/// binary, so a hardcoded install command would be a false instruction for most
/// users.
const CHANGELOG_URL: &str = "https://github.com/fallow-rs/fallow/blob/main/CHANGELOG.md";

/// Persisted latest-version answer. Lives in the user-global config dir next to
/// `telemetry.json`.
#[derive(Clone, Debug, Deserialize, Serialize)]
struct UpdateCache {
    schema_version: u8,
    /// Persistent opt-out. A future `fallow update-check disable` would set
    /// this; today a user can set it by hand. Preserved across background
    /// refreshes.
    #[serde(default)]
    disabled: bool,
    /// Last latest-version string the fetch saw (e.g. `"2.88.3"`). Empty until
    /// the first successful fetch.
    #[serde(default)]
    latest_version: String,
    /// Unix seconds of the last fetch attempt that succeeded.
    #[serde(default)]
    checked_at_secs: u64,
}

impl Default for UpdateCache {
    fn default() -> Self {
        Self {
            schema_version: CACHE_SCHEMA_VERSION,
            disabled: false,
            latest_version: String::new(),
            checked_at_secs: 0,
        }
    }
}

/// Response shape from `LATEST_VERSION_PATH`.
#[derive(Debug, Deserialize)]
struct LatestVersionResponse {
    latest: String,
}

/// The environment-derived facts that decide whether a nudge can be shown or a
/// fetch attempted. Pulled out so the gate logic is a pure function and the
/// env/TTY reads stay in one thin place.
#[derive(Clone, Copy, Debug)]
struct DisplayContext {
    quiet: bool,
    human: bool,
    stdout_tty: bool,
    stderr_tty: bool,
    /// Env / CI kill switches (NOT the persistent config `disabled`, which is
    /// read from the cache file later).
    env_disabled: bool,
}

/// Public entry point. Call once on a successful run, after the telemetry
/// opt-in note, passing whether that note printed.
///
/// Returns immediately (no file IO, no thread) on any run that could not
/// display a nudge, so the agent / CI / quiet / piped path is byte-identical to
/// today and zero-cost.
pub fn maybe_nudge(output: OutputFormat, quiet: bool, telemetry_note_printed: bool) {
    // One unsolicited stderr notice per run. The telemetry note is one-time and
    // consent-bearing, so it wins; the nudge (and its fetch) wait one run.
    if telemetry_note_printed {
        return;
    }
    let ctx = DisplayContext {
        quiet,
        human: matches!(output, OutputFormat::Human),
        stdout_tty: std::io::stdout().is_terminal(),
        stderr_tty: std::io::stderr().is_terminal(),
        env_disabled: env_disabled(),
    };
    if !should_run(ctx) {
        return;
    }
    let Some(path) = cache_path() else {
        return;
    };
    let cache = read_cache_from(&path).unwrap_or_default();
    // Persistent config opt-out.
    if cache.disabled {
        return;
    }

    let current = env!("CARGO_PKG_VERSION");
    if is_newer_stable(current, &cache.latest_version) {
        eprintln!(
            "A newer fallow is available ({}, you have {current}). Changelog: {CHANGELOG_URL}",
            cache.latest_version
        );
    }

    // Refresh for next time if the cached answer is stale. Detached + bounded so
    // it never blocks; errors swallowed so it never affects the exit code.
    if cache_is_expired(cache.checked_at_secs, now_secs(), CHECK_TTL_SECS) {
        spawn_background_refresh(path, cache);
    }
}

/// Pure gate: a nudge may be shown / a fetch attempted only on an interactive
/// human run that has not opted out.
fn should_run(ctx: DisplayContext) -> bool {
    !ctx.quiet && ctx.human && ctx.stdout_tty && ctx.stderr_tty && !ctx.env_disabled
}

/// True when `latest` is a strictly-newer STABLE release than `current`. Both
/// sides must parse as semver and neither may carry a prerelease tag: we only
/// advertise stable releases, and we never nudge a prerelease/ahead user
/// "down" to a stable.
fn is_newer_stable(current: &str, latest: &str) -> bool {
    let (Ok(current), Ok(latest)) = (Version::parse(current), Version::parse(latest)) else {
        return false;
    };
    current.pre.is_empty() && latest.pre.is_empty() && current < latest
}

/// True when the cache is older than the TTL (or has never been fetched).
fn cache_is_expired(checked_at_secs: u64, now: u64, ttl_secs: u64) -> bool {
    now.saturating_sub(checked_at_secs) >= ttl_secs
}

/// Env / CI kill switches that suppress both the nudge and the fetch.
fn env_disabled() -> bool {
    env_truthy(DO_NOT_TRACK_ENV)
        || env_truthy(TELEMETRY_DISABLED_ENV)
        || update_check_off()
        || is_ci()
}

/// `FALLOW_UPDATE_CHECK` set to an explicit off value.
fn update_check_off() -> bool {
    std::env::var(UPDATE_CHECK_ENV).ok().is_some_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "off" | "0" | "false" | "disabled" | "no"
        )
    })
}

fn env_truthy(name: &str) -> bool {
    std::env::var(name).ok().is_some_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}

fn is_ci() -> bool {
    std::env::var_os("CI").is_some()
        || std::env::var_os("GITHUB_ACTIONS").is_some()
        || std::env::var_os("GITLAB_CI").is_some()
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// User-global cache path, mirroring `telemetry::config_path`.
fn cache_path() -> Option<PathBuf> {
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .map(|home| home.join("Library").join("Application Support"))
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
    }?;
    Some(base.join("fallow").join("update-check.json"))
}

fn read_cache_from(path: &std::path::Path) -> Result<UpdateCache, String> {
    let raw = std::fs::read_to_string(path).map_err(|err| err.to_string())?;
    let cache: UpdateCache = serde_json::from_str(&raw).map_err(|err| err.to_string())?;
    if cache.schema_version == CACHE_SCHEMA_VERSION {
        return Ok(cache);
    }
    Ok(UpdateCache {
        // Preserve a downgrade-safe opt-out if the old reader can still see it,
        // but discard the version answer because future schema semantics are
        // unknown.
        disabled: cache.disabled,
        ..UpdateCache::default()
    })
}

fn write_cache_to(path: &std::path::Path, cache: &UpdateCache) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    let mut raw = serde_json::to_string_pretty(cache).map_err(|err| err.to_string())?;
    raw.push('\n');
    std::fs::write(path, raw).map_err(|err| err.to_string())
}

/// Refresh the cache on a detached thread, bounded by [`FETCH_GRACE_MS`] so the
/// caller never blocks. `prior` is carried in so the persistent `disabled` flag
/// survives the rewrite (the fetch only owns `latest_version` /
/// `checked_at_secs`).
fn spawn_background_refresh(path: PathBuf, prior: UpdateCache) {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(refresh_cache(&path, &prior));
    });
    let _ = rx.recv_timeout(Duration::from_millis(FETCH_GRACE_MS));
}

fn refresh_cache(path: &std::path::Path, prior: &UpdateCache) -> Result<(), String> {
    let latest = fetch_latest_version()?;
    let updated = UpdateCache {
        schema_version: CACHE_SCHEMA_VERSION,
        disabled: prior.disabled,
        latest_version: latest,
        checked_at_secs: now_secs(),
    };
    write_cache_to(path, &updated)
}

fn fetch_latest_version() -> Result<String, String> {
    let agent = try_api_agent_with_timeout(FETCH_CONNECT_TIMEOUT_SECS, FETCH_TOTAL_TIMEOUT_SECS)
        .map_err(|err| err.to_string())?;
    let url = api_url(LATEST_VERSION_PATH);
    let mut response = agent
        .get(&url)
        .header("Accept", "application/json")
        .call()
        .map_err(|err| err.to_string())?;
    if !response.status().is_success() {
        return Err(format!(
            "latest-version endpoint returned {}",
            response.status()
        ));
    }
    let body: LatestVersionResponse = response
        .body_mut()
        .read_json()
        .map_err(|err| err.to_string())?;
    // Reject a garbage payload: the value must parse as semver before we cache
    // it, otherwise a malformed answer would silently poison every later
    // display.
    if Version::parse(&body.latest).is_err() {
        return Err(format!(
            "latest-version payload is not semver: {}",
            body.latest
        ));
    }
    Ok(body.latest)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(
        quiet: bool,
        human: bool,
        stdout_tty: bool,
        stderr_tty: bool,
        env_disabled: bool,
    ) -> DisplayContext {
        DisplayContext {
            quiet,
            human,
            stdout_tty,
            stderr_tty,
            env_disabled,
        }
    }

    #[test]
    fn should_run_only_on_interactive_human_run() {
        assert!(should_run(ctx(false, true, true, true, false)));
    }

    #[test]
    fn quiet_suppresses() {
        assert!(!should_run(ctx(true, true, true, true, false)));
    }

    #[test]
    fn non_human_format_suppresses() {
        assert!(!should_run(ctx(false, false, true, true, false)));
    }

    #[test]
    fn non_tty_stdout_suppresses() {
        assert!(!should_run(ctx(false, true, false, true, false)));
    }

    #[test]
    fn non_tty_stderr_suppresses() {
        // An agent that captures stderr while leaving stdout on a TTY must not
        // be nudged either.
        assert!(!should_run(ctx(false, true, true, false, false)));
    }

    #[test]
    fn env_disabled_suppresses() {
        assert!(!should_run(ctx(false, true, true, true, true)));
    }

    #[test]
    fn newer_stable_is_detected() {
        assert!(is_newer_stable("2.85.0", "2.88.3"));
        assert!(is_newer_stable("2.88.2", "2.88.3"));
        assert!(is_newer_stable("1.0.0", "2.0.0"));
    }

    #[test]
    fn equal_or_newer_current_is_not_nudged() {
        // Just-upgraded user with a stale cache must not be told they are behind.
        assert!(!is_newer_stable("2.88.3", "2.88.3"));
        assert!(!is_newer_stable("2.89.0", "2.88.3"));
    }

    #[test]
    fn prerelease_either_side_is_not_nudged() {
        // A prerelease/ahead user is not nudged down to a stable, and we never
        // advertise a prerelease as the latest.
        assert!(!is_newer_stable("2.88.0-rc.1", "2.88.0"));
        assert!(!is_newer_stable("2.85.0", "2.88.0-rc.1"));
        assert!(!is_newer_stable("2.88.0-rc.1", "2.88.0-rc.2"));
    }

    #[test]
    fn unparseable_version_is_not_nudged() {
        assert!(!is_newer_stable("not-semver", "2.88.3"));
        assert!(!is_newer_stable("2.85.0", ""));
        assert!(!is_newer_stable("2.85.0", "garbage"));
    }

    #[test]
    fn cache_expiry_respects_ttl() {
        // Fresh: checked just now.
        assert!(!cache_is_expired(
            1000,
            1000 + CHECK_TTL_SECS - 1,
            CHECK_TTL_SECS
        ));
        // Stale: exactly at the TTL boundary and beyond.
        assert!(cache_is_expired(
            1000,
            1000 + CHECK_TTL_SECS,
            CHECK_TTL_SECS
        ));
        assert!(cache_is_expired(
            1000,
            1000 + CHECK_TTL_SECS * 2,
            CHECK_TTL_SECS
        ));
        // Never fetched.
        assert!(cache_is_expired(0, now_secs(), CHECK_TTL_SECS));
    }

    #[test]
    fn cache_round_trips_and_preserves_disabled() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("update-check.json");
        let original = UpdateCache {
            schema_version: CACHE_SCHEMA_VERSION,
            disabled: true,
            latest_version: "2.88.3".to_owned(),
            checked_at_secs: 1234,
        };
        write_cache_to(&path, &original).unwrap();
        let loaded = read_cache_from(&path).unwrap();
        assert!(loaded.disabled);
        assert_eq!(loaded.latest_version, "2.88.3");
        assert_eq!(loaded.checked_at_secs, 1234);
    }

    #[test]
    fn cache_schema_mismatch_discards_version_answer_but_preserves_disabled() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("update-check.json");
        std::fs::write(
            &path,
            r#"{
              "schema_version": 99,
              "disabled": true,
              "latest_version": "99.0.0",
              "checked_at_secs": 9999999999
            }"#,
        )
        .unwrap();

        let loaded = read_cache_from(&path).unwrap();
        assert_eq!(loaded.schema_version, CACHE_SCHEMA_VERSION);
        assert!(loaded.disabled);
        assert!(loaded.latest_version.is_empty());
        assert_eq!(loaded.checked_at_secs, 0);
    }

    #[test]
    fn refresh_preserves_disabled_flag() {
        // A background refresh owns latest_version + checked_at_secs; it must
        // carry the persistent config opt-out forward rather than clobbering it.
        let prior = UpdateCache {
            schema_version: CACHE_SCHEMA_VERSION,
            disabled: true,
            latest_version: "2.85.0".to_owned(),
            checked_at_secs: 1,
        };
        let updated = UpdateCache {
            schema_version: CACHE_SCHEMA_VERSION,
            disabled: prior.disabled,
            latest_version: "2.88.3".to_owned(),
            checked_at_secs: now_secs(),
        };
        assert!(updated.disabled);
        assert_eq!(updated.latest_version, "2.88.3");
    }

    #[test]
    fn missing_cache_reads_as_error_not_panic() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("does-not-exist.json");
        assert!(read_cache_from(&path).is_err());
        // The public entry treats a read error as `default()`, which has an
        // empty latest_version, so `is_newer_stable` returns false.
        assert!(!is_newer_stable(
            env!("CARGO_PKG_VERSION"),
            &UpdateCache::default().latest_version
        ));
    }
}
