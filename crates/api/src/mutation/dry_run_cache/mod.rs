//! The dry-run cache of mutation runs.
//!
//! A run stores the dry-run facts of each test file under a key. A later run
//! uses the stored facts, and does not dry-run the file, when the key is
//! equal (see the scheduler module `dry_store` for the rule that joins
//! stored and new facts). The key hashes these parts:
//!
//! - the key format and each part of the result cache key that a test file
//!   gives: the Fallow version, the runner, the lockfile, the runner config
//!   and setup files, the selected Vitest projects, and the content of the
//!   test file and of each file that it reaches through static imports;
//! - the runner version, the runtime version and the runner capabilities;
//! - the instrumented code of each source file that the test file reaches;
//! - the selected operators, the project root (the test index entries of a
//!   runner hold absolute module paths), and the `init` options of the
//!   runner, without the test file selection.
//!
//! The result cache limits apply in the same way: dynamic imports that the
//! module graph does not resolve, and files that a test reads, are not in the
//! key. `--no-cache` turns the cache off.
//!
//! The cache is one file, `<cache dir>/mutation/dry-runs.json`. It keeps one
//! entry for each test file and drops the least recently used entries above
//! [`MAX_ENTRIES`]. A run with reach selection also keeps the reach record of
//! each test file in the same file (see the `reach` module).

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::protocol::{InitRequest, Ready};
use super::result_cache::ResultCache;
use super::run_dir::RunFile;
use super::scheduler::{
    DryRunEntry, DryRunLookup, ReachLookup, ReachSelection, ShareProbes, StoredDryRun,
};

mod reach;
pub use reach::ReachFacts;

/// The file name of the cache below `<cache dir>/mutation`.
pub const DRY_RUN_CACHE_FILE: &str = "dry-runs.json";
/// The version of the key and of the file format. A change to the key parts
/// or to the file shape needs a new version.
const FORMAT_VERSION: u32 = 2;
/// The most entries that the file keeps.
pub const MAX_ENTRIES: usize = 20_000;

fn hex(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(text, "{byte:02x}");
    }
    text
}

/// Adds one length-prefixed text to `hasher`, so that two parts never run
/// together.
fn feed(hasher: &mut Sha256, text: &str) {
    hasher.update((text.len() as u64).to_le_bytes());
    hasher.update(text.as_bytes());
}

/// One stored dry run of a test file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Entry {
    key: String,
    facts: StoredDryRun,
    /// The generation of the last run that stored or used the entry.
    generation: u64,
}

/// The cache file.
#[derive(Debug, Default, Serialize, Deserialize)]
struct CacheFile {
    format_version: u32,
    generation: u64,
    entries: BTreeMap<String, Entry>,
    #[serde(default)]
    reach: reach::ReachStore,
}

/// The dry-run cache of one run.
#[derive(Debug)]
pub struct DryRunCache {
    file: PathBuf,
    /// The result cache key of the run, for the parts of a test file.
    keys: Arc<ResultCache>,
    /// The hash of the instrumented code of each source file of the plan.
    instrumented: FxHashMap<String, String>,
    /// The key parts that each test file of the run shares.
    shared: String,
    state: Mutex<CacheFile>,
    max_entries: usize,
    /// The facts of the run for the reach selection, when the run selects by
    /// reach.
    reach: Option<ReachFacts>,
}

impl DryRunCache {
    /// Opens the cache of `mutation_cache` for a run with the `instrumented`
    /// files of its plan, the selected `operators` (`None` for the default
    /// set) and the `init` request of the runner. A missing file, a file of another format, or a
    /// file that does not parse gives an empty cache.
    #[must_use]
    pub fn open(
        mutation_cache: &Path,
        keys: Arc<ResultCache>,
        instrumented: &[RunFile<'_>],
        operators: Option<&[String]>,
        init: &InitRequest,
    ) -> Self {
        let file = mutation_cache.join(DRY_RUN_CACHE_FILE);
        let state = std::fs::read(&file)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<CacheFile>(&bytes).ok())
            .filter(|state| state.format_version == FORMAT_VERSION)
            .unwrap_or_default();
        let instrumented = instrumented
            .iter()
            .map(|file| {
                let hash = hex(&Sha256::digest(file.code.as_bytes()));
                (file.path.to_owned(), hash)
            })
            .collect();
        Self {
            file,
            keys,
            instrumented,
            shared: shared_part(operators, init),
            state: Mutex::new(state),
            max_entries: MAX_ENTRIES,
            reach: None,
        }
    }

    /// The cache with the reach facts of the run, so that it selects the
    /// dry-run files by reach and records the probes of the dry run.
    #[must_use]
    pub fn with_reach(self, facts: ReachFacts) -> Self {
        Self {
            reach: Some(facts),
            ..self
        }
    }

    /// The environment part of the reach record of `test_file`.
    fn reach_env(&self, ready: &Ready, test_file: &str) -> String {
        let mut hasher = Sha256::new();
        feed(&mut hasher, &self.shared);
        feed(&mut hasher, &ready.runner_version);
        feed(&mut hasher, &ready.runtime_version);
        feed(&mut hasher, &format!("{:?}", ready.capabilities));
        feed(&mut hasher, &self.keys.test_env_part(test_file));
        hex(&hasher.finalize())
    }

    /// Records the probes of the new dry run of `shares`, and gives the
    /// number of test files with a record. A runner whose workers keep the
    /// modules of one test file for the next one gives no record, because
    /// a test file can then use code that an earlier file ran.
    pub fn record_reach(&self, ready: &Ready, shares: &[ShareProbes]) -> usize {
        let Some(facts) = &self.reach else {
            return 0;
        };
        let Ok(mut state) = self.state.lock() else {
            return 0;
        };
        let generation = state.generation + 1;
        let complete = ready.capabilities.fresh_modules_per_run;
        let config = self.keys.files_digest(facts.config_files());
        state.reach.record(
            facts,
            shares,
            complete,
            (|test_file: &str| self.reach_env(ready, test_file), &config),
            generation,
        )
    }

    /// Adds `entries` and gives the number of entries that it added or
    /// replaced.
    pub fn record(&self, entries: &[DryRunEntry]) -> usize {
        let Ok(mut state) = self.state.lock() else {
            return 0;
        };
        let generation = state.generation + 1;
        for entry in entries {
            state.entries.insert(
                entry.test_file.clone(),
                Entry {
                    key: entry.key.clone(),
                    facts: entry.facts.clone(),
                    generation,
                },
            );
        }
        entries.len()
    }

    /// Writes the cache file. The write goes to a temporary file first, so a
    /// reader never sees half a file.
    ///
    /// # Errors
    ///
    /// Returns the IO error as text.
    pub fn save(&self) -> Result<(), String> {
        let Ok(mut state) = self.state.lock() else {
            return Err("the dry-run cache lock is poisoned".to_owned());
        };
        state.format_version = FORMAT_VERSION;
        state.generation += 1;
        if state.entries.len() > self.max_entries {
            let mut by_age: Vec<(u64, String)> = state
                .entries
                .iter()
                .map(|(file, entry)| (entry.generation, file.clone()))
                .collect();
            by_age.sort();
            let surplus = state.entries.len() - self.max_entries;
            for (_, file) in by_age.into_iter().take(surplus) {
                state.entries.remove(&file);
            }
        }
        state.reach.prune(self.max_entries);
        let bytes = serde_json::to_vec(&*state)
            .map_err(|error| format!("cannot serialize the dry-run cache: {error}"))?;
        if let Some(parent) = self.file.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("cannot write {}: {error}", parent.display()))?;
        }
        let temporary = self
            .file
            .with_extension(format!("json.{}.tmp", std::process::id()));
        std::fs::write(&temporary, bytes)
            .and_then(|()| std::fs::rename(&temporary, &self.file))
            .map_err(|error| {
                let _ = std::fs::remove_file(&temporary);
                format!("cannot write {}: {error}", self.file.display())
            })
    }
}

/// The key parts of the run that do not depend on the test file: the format,
/// the operators, and the `init` request without the parts that change with
/// the scope or with the run directory.
fn shared_part(operators: Option<&[String]>, init: &InitRequest) -> String {
    let mut hasher = Sha256::new();
    feed(&mut hasher, &FORMAT_VERSION.to_string());
    match operators {
        None => feed(&mut hasher, "default operators"),
        Some(operators) => {
            let mut sorted: Vec<&str> = operators.iter().map(String::as_str).collect();
            sorted.sort_unstable();
            sorted.dedup();
            hasher.update((sorted.len() as u64).to_le_bytes());
            for operator in sorted {
                feed(&mut hasher, operator);
            }
        }
    }
    // The test index entries of a stored dry run hold absolute module paths.
    feed(&mut hasher, &init.root);
    feed(&mut hasher, &init.runner);
    feed(&mut hasher, init.config_path.as_deref().unwrap_or("-"));
    let mut options = init.options.clone();
    options.test_files.clear();
    options.extra_test_files.clear();
    feed(
        &mut hasher,
        &serde_json::to_string(&options).unwrap_or_default(),
    );
    hex(&hasher.finalize())
}

impl ReachLookup for DryRunCache {
    fn select(&self, ready: &Ready, dry_files: &[String]) -> ReachSelection {
        let (Some(facts), Ok(state)) = (&self.reach, self.state.lock()) else {
            return ReachSelection::default();
        };
        if !ready.capabilities.fresh_modules_per_run {
            return ReachSelection {
                disabled: Some("runner-shares-modules"),
                ..ReachSelection::default()
            };
        }
        let config = self.keys.files_digest(facts.config_files());
        state.reach.select(
            facts,
            dry_files,
            (|test_file: &str| self.reach_env(ready, test_file), &config),
        )
    }
}

impl DryRunLookup for DryRunCache {
    fn key(&self, ready: &Ready, test_file: &str) -> String {
        let mut hasher = Sha256::new();
        feed(&mut hasher, &self.shared);
        feed(&mut hasher, &ready.runner_version);
        feed(&mut hasher, &ready.runtime_version);
        feed(&mut hasher, &format!("{:?}", ready.capabilities));
        feed(&mut hasher, &self.keys.test_file_part(test_file));
        let instrumented: Vec<(&str, &str)> = self
            .keys
            .reach_of(test_file)
            .filter_map(|path| {
                let hash = self.instrumented.get(path)?;
                Some((path, hash.as_str()))
            })
            .collect();
        hasher.update((instrumented.len() as u64).to_le_bytes());
        for (path, hash) in instrumented {
            feed(&mut hasher, path);
            feed(&mut hasher, hash);
        }
        hex(&hasher.finalize())
    }

    fn get(&self, test_file: &str, key: &str) -> Option<StoredDryRun> {
        let mut state = self.state.lock().ok()?;
        let generation = state.generation + 1;
        let hit = state
            .entries
            .get_mut(test_file)
            .filter(|entry| entry.key == key)
            .map(|entry| {
                entry.generation = generation;
                entry.facts.clone()
            });
        drop(state);
        hit
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "tests unwrap to keep assertions short")]
mod tests;
