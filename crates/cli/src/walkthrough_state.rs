//! Local, account-free viewed-state for `fallow review --walkthrough`.
//!
//! Per-file "I've looked at this" marks live in a small JSON ledger inside the
//! resolved cache dir (`.fallow/walkthrough-state.json`, already gitignored).
//! The ledger is purely local: no account, no network, human/git-readable.
//!
//! Staleness is keyed on the guide's `graph_snapshot_hash`. A mark is honored
//! ONLY when the stored hash matches the current guide hash, so a tree that
//! moved silently un-views every file for that render without deleting the marks
//! (a no-op carry-forward keeps them if the user reverts). Rendering is
//! read-only and idempotent; marks are written ONLY on an explicit
//! `--mark-viewed` mutation, never as a side effect of a render.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// Current ledger format version. Bumped only on a breaking shape change; a
/// reader that sees an unknown version treats the file as empty (safe default).
const VIEWED_STATE_VERSION: u32 = 1;

/// Stable schema discriminator, so a stray file under the cache dir is not
/// mistaken for a viewed-state ledger.
const VIEWED_STATE_SCHEMA: &str = "walkthrough-viewed-marks";

/// One viewed entry: when the file was marked viewed (RFC 3339-ish UTC).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewedEntry {
    /// UTC timestamp the mark was recorded.
    pub viewed_at: String,
}

/// The on-disk viewed-state ledger.
///
/// `entries` is keyed by the per-file VIEW key (the file path from the guide's
/// `direction.order`). `graph_snapshot_hash` pins the marks to the guide they
/// were recorded against; a mismatch on read means the tree moved and every
/// entry is treated as not-viewed for that render (see [`ViewedState::is_viewed`]).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViewedState {
    /// Ledger format version.
    pub version: u32,
    /// Schema discriminator.
    pub schema: String,
    /// The guide hash the marks were recorded against.
    #[serde(default)]
    pub graph_snapshot_hash: String,
    /// Per-file viewed marks, keyed by the file path.
    #[serde(default)]
    pub entries: BTreeMap<String, ViewedEntry>,
}

impl Default for ViewedState {
    fn default() -> Self {
        Self {
            version: VIEWED_STATE_VERSION,
            schema: VIEWED_STATE_SCHEMA.to_string(),
            graph_snapshot_hash: String::new(),
            entries: BTreeMap::new(),
        }
    }
}

impl ViewedState {
    /// Whether `file` is currently viewed: it must have a recorded mark AND the
    /// ledger's stored hash must match the guide's `current_hash`. A stale hash
    /// (the tree moved) makes every file read as not-viewed, the safe direction.
    #[must_use]
    pub fn is_viewed(&self, file: &str, current_hash: &str) -> bool {
        if self.graph_snapshot_hash.is_empty() || self.graph_snapshot_hash != current_hash {
            return false;
        }
        self.entries.contains_key(file)
    }

    /// Count of files in `order` that are currently viewed (hash-matched).
    #[must_use]
    pub fn viewed_count<'a>(
        &self,
        order: impl IntoIterator<Item = &'a str>,
        current_hash: &str,
    ) -> usize {
        order
            .into_iter()
            .filter(|file| self.is_viewed(file, current_hash))
            .count()
    }
}

/// Load the viewed-state ledger from `cache_dir`, returning an empty default
/// when the file is missing, unreadable, or not valid JSON.
///
/// A missing ledger is the common first-run case, and a garbled one must never
/// hard-error a render, so both collapse to the empty default rather than a
/// caller-visible error. A version the reader does not understand is also
/// treated as empty (forward-compat: a future writer's shape never crashes an
/// older render).
#[must_use]
pub fn load_viewed_state(cache_dir: &Path) -> ViewedState {
    let path = fallow_config::walkthrough_state_path(cache_dir);
    let Ok(contents) = std::fs::read_to_string(&path) else {
        return ViewedState::default();
    };
    let Ok(state) = serde_json::from_str::<ViewedState>(&contents) else {
        return ViewedState::default();
    };
    if state.version != VIEWED_STATE_VERSION || state.schema != VIEWED_STATE_SCHEMA {
        return ViewedState::default();
    }
    state
}

/// Record each path in `files` as viewed against `current_hash`, atomically.
///
/// Loads the existing ledger, upserts the marks, sets the stored hash to the
/// current guide hash, then writes via a temp file + rename so a crash mid-write
/// can never truncate the JSON. Returns the file count actually persisted.
/// Swallows IO errors (logged at most by the caller); the viewed-state is a
/// convenience, never load-bearing for the tour or the exit code.
pub fn mark_viewed(cache_dir: &Path, files: &[String], current_hash: &str) -> std::io::Result<()> {
    let mut state = load_viewed_state(cache_dir);
    // A hash change means the prior marks no longer apply; reset the ledger to
    // the current snapshot rather than mixing marks across two graph states.
    if state.graph_snapshot_hash != current_hash {
        state.entries.clear();
    }
    state.graph_snapshot_hash = current_hash.to_string();
    let now = crate::vital_signs::chrono_timestamp();
    for file in files {
        state.entries.insert(
            file.clone(),
            ViewedEntry {
                viewed_at: now.clone(),
            },
        );
    }
    write_atomic(cache_dir, &state)
}

/// Serialize `state` and write it to the ledger path via temp + rename.
fn write_atomic(cache_dir: &Path, state: &ViewedState) -> std::io::Result<()> {
    let path = fallow_config::walkthrough_state_path(cache_dir);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(state)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json.as_bytes())?;
    std::fs::rename(&tmp, &path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_cache() -> tempfile::TempDir {
        tempfile::tempdir().expect("tempdir")
    }

    #[test]
    fn missing_file_loads_empty_default() {
        let dir = temp_cache();
        let state = load_viewed_state(dir.path());
        assert!(state.entries.is_empty());
        assert_eq!(state.version, VIEWED_STATE_VERSION);
    }

    #[test]
    fn garbled_json_loads_empty_default() {
        let dir = temp_cache();
        let path = fallow_config::walkthrough_state_path(dir.path());
        std::fs::write(&path, b"{ not json").expect("write");
        let state = load_viewed_state(dir.path());
        assert!(state.entries.is_empty());
    }

    #[test]
    fn unknown_version_loads_empty_default() {
        let dir = temp_cache();
        let path = fallow_config::walkthrough_state_path(dir.path());
        std::fs::write(
            &path,
            br#"{"version":999,"schema":"walkthrough-viewed-marks","graph_snapshot_hash":"h","entries":{"a.ts":{"viewed_at":"t"}}}"#,
        )
        .expect("write");
        let state = load_viewed_state(dir.path());
        assert!(state.entries.is_empty());
    }

    #[test]
    fn mark_then_load_round_trips() {
        let dir = temp_cache();
        mark_viewed(dir.path(), &["src/a.ts".to_string()], "hash1").expect("mark");
        let state = load_viewed_state(dir.path());
        assert!(state.is_viewed("src/a.ts", "hash1"));
        assert!(!state.is_viewed("src/b.ts", "hash1"));
    }

    #[test]
    fn stale_hash_reads_as_not_viewed_but_keeps_entry() {
        let dir = temp_cache();
        mark_viewed(dir.path(), &["src/a.ts".to_string()], "hash1").expect("mark");
        let state = load_viewed_state(dir.path());
        // A different current hash: the mark is ignored for this render.
        assert!(!state.is_viewed("src/a.ts", "hash2"));
        // ...but the entry on disk is not deleted (carry-forward).
        assert!(state.entries.contains_key("src/a.ts"));
    }

    #[test]
    fn mark_against_new_hash_resets_prior_marks() {
        let dir = temp_cache();
        mark_viewed(dir.path(), &["src/a.ts".to_string()], "hash1").expect("mark a");
        mark_viewed(dir.path(), &["src/b.ts".to_string()], "hash2").expect("mark b");
        let state = load_viewed_state(dir.path());
        assert!(state.is_viewed("src/b.ts", "hash2"));
        // The hash1 mark was reset when the snapshot moved.
        assert!(!state.entries.contains_key("src/a.ts"));
    }

    #[test]
    fn viewed_count_only_counts_hash_matched() {
        let dir = temp_cache();
        mark_viewed(
            dir.path(),
            &["src/a.ts".to_string(), "src/b.ts".to_string()],
            "hash1",
        )
        .expect("mark");
        let state = load_viewed_state(dir.path());
        let order = ["src/a.ts", "src/b.ts", "src/c.ts"];
        assert_eq!(state.viewed_count(order.iter().copied(), "hash1"), 2);
        assert_eq!(state.viewed_count(order.iter().copied(), "stale"), 0);
    }

    #[test]
    fn empty_stored_hash_never_viewed() {
        let state = ViewedState::default();
        assert!(!state.is_viewed("a.ts", ""));
    }
}
