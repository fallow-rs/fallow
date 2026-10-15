//! The result cache of mutation runs.
//!
//! A run stores the result of each killed or survived mutant under a key.
//! A later run uses the stored result, and does not run the mutant, when the
//! key is equal. The key hashes these parts:
//!
//! - the key format and the Fallow version;
//! - the runner version and the runtime version;
//! - the mutant id and the content of the mutated source file;
//! - the covering test ids;
//! - the content of the covering test files, of the files that they reach
//!   through static imports, and of the runner setup files, the runner
//!   config modules at the root, and their reach;
//! - the lockfile;
//! - the runner config files at the root, and the configs that the root
//!   `tsconfig.json` extends.
//!
//! Dynamic imports that the module graph does not resolve, and files that a
//! test or a config reads, are not in the key. Each entry records this limit in
//! `not_keyed`. `--no-cache` turns the cache off.
//!
//! The cache is one file, `<cache dir>/mutation/results.json`. It keeps one
//! entry for each mutant id and drops the least recently used entries above
//! [`MAX_ENTRIES`].

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use fallow_engine::mutation::{MutationPlan, RUNNER_CONFIG_MODULES};
use fallow_output::MutantStatus;
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::protocol::Ready;
use super::scheduler::{CachedResult, ResultLookup, ScheduleOutcome, ScheduledMutant};

/// The file name of the cache below `<cache dir>/mutation`.
pub const RESULT_CACHE_FILE: &str = "results.json";
/// The version of the key and of the file format. A change to the key
/// parts or to the file shape needs a new version.
const FORMAT_VERSION: u32 = 3;
/// The most entries that the file keeps.
pub const MAX_ENTRIES: usize = 20_000;
/// The inputs of a test that the key does not hash.
pub const NOT_KEYED: [&str; 2] = ["dynamic-imports", "file-reads"];

/// The lockfile names. The first directory from the root upward that holds
/// one of them gives the lockfile part of the key.
const LOCKFILES: [&str; 6] = [
    "package-lock.json",
    "npm-shrinkwrap.json",
    "pnpm-lock.yaml",
    "yarn.lock",
    "bun.lock",
    "bun.lockb",
];

/// The runner config files at the root that are not modules. With
/// [`RUNNER_CONFIG_MODULES`], a file that is absent also is a key fact, so a
/// new config file changes the key. The files that a config module reaches
/// through static imports are in [`KeyFiles::support_files`].
const RUNNER_CONFIG_DATA: [&str; 4] = [
    "vitest.workspace.json",
    "bunfig.toml",
    "package.json",
    TSCONFIG,
];

/// The root tsconfig. The key also hashes each config of its `extends`
/// chain.
const TSCONFIG: &str = "tsconfig.json";

/// The most files of a tsconfig `extends` chain that the key reads.
const MAX_TSCONFIG_EXTENDS: usize = 32;

/// The hash value of a file that does not exist or cannot be read.
const ABSENT: &str = "-";

fn hex(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(text, "{byte:02x}");
    }
    text
}

fn file_hash(path: &Path) -> String {
    std::fs::read(path).map_or_else(|_| ABSENT.to_owned(), |bytes| hex(&Sha256::digest(bytes)))
}

/// Adds one length-prefixed text to `hasher`, so that two parts never run
/// together.
fn feed(hasher: &mut Sha256, text: &str) {
    hasher.update((text.len() as u64).to_le_bytes());
    hasher.update(text.as_bytes());
}

/// The lockfile digest: the lockfiles of the first directory from `root`
/// upward that holds one.
fn lockfile_digest(root: &Path) -> String {
    let mut hasher = Sha256::new();
    for dir in root.ancestors() {
        let found: Vec<&str> = LOCKFILES
            .iter()
            .copied()
            .filter(|name| dir.join(name).is_file())
            .collect();
        if found.is_empty() {
            continue;
        }
        for name in found {
            feed(&mut hasher, name);
            feed(&mut hasher, &file_hash(&dir.join(name)));
        }
        break;
    }
    hex(&hasher.finalize())
}

/// The runner config digest of the files at `root`, and of the configs that
/// the root `tsconfig.json` extends.
fn runner_config_digest(root: &Path) -> String {
    let mut hasher = Sha256::new();
    for name in RUNNER_CONFIG_MODULES.iter().chain(&RUNNER_CONFIG_DATA) {
        feed(&mut hasher, name);
        feed(&mut hasher, &file_hash(&root.join(name)));
    }
    feed_tsconfig_extends(&mut hasher, &root.join(TSCONFIG));
    hex(&hasher.finalize())
}

/// Adds each config of the `extends` chain of `tsconfig` to `hasher`: the
/// `extends` text and the content of the file that it resolves to. The
/// chain stops at [`MAX_TSCONFIG_EXTENDS`] files, so a cycle ends.
fn feed_tsconfig_extends(hasher: &mut Sha256, tsconfig: &Path) {
    let mut configs = vec![tsconfig.to_path_buf()];
    let mut next = 0;
    while next < configs.len() {
        let config = configs[next].clone();
        next += 1;
        let Some(dir) = config.parent() else {
            continue;
        };
        for spec in extends_of(&config) {
            let resolved = resolve_extends(dir, &spec);
            feed(hasher, &spec);
            feed(
                hasher,
                &resolved
                    .as_deref()
                    .map_or_else(|| ABSENT.to_owned(), file_hash),
            );
            if let Some(resolved) = resolved
                && configs.len() < MAX_TSCONFIG_EXTENDS
                && !configs.contains(&resolved)
            {
                configs.push(resolved);
            }
        }
    }
}

/// The `extends` entries of a tsconfig file: one text or an array of texts.
/// A file that does not read or parse has none.
fn extends_of(config: &Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(config) else {
        return Vec::new();
    };
    let Ok(value) = fallow_config::jsonc::parse_to_value::<serde_json::Value>(&text) else {
        return Vec::new();
    };
    match value.get("extends") {
        Some(serde_json::Value::String(spec)) => vec![spec.clone()],
        Some(serde_json::Value::Array(specs)) => specs
            .iter()
            .filter_map(|spec| spec.as_str().map(str::to_owned))
            .collect(),
        _ => Vec::new(),
    }
}

/// The file of one `extends` entry, as TypeScript resolves it. A relative or
/// absolute path is the path, else the path with `.json`. A package path is
/// looked up in each `node_modules` directory from `dir` upward, as the path,
/// the path with `.json`, or `tsconfig.json` in the path. The `exports` map
/// of a package is not read.
fn resolve_extends(dir: &Path, spec: &str) -> Option<PathBuf> {
    let with_json = |path: PathBuf| -> Option<PathBuf> {
        if path.is_file() {
            return Some(path);
        }
        let mut name = path.into_os_string();
        name.push(".json");
        let path = PathBuf::from(name);
        path.is_file().then_some(path)
    };
    if spec.starts_with("./") || spec.starts_with("../") || Path::new(spec).is_absolute() {
        return with_json(dir.join(spec));
    }
    dir.ancestors().find_map(|ancestor| {
        let package = ancestor.join("node_modules").join(spec);
        with_json(package.clone()).or_else(|| {
            let nested = package.join(TSCONFIG);
            nested.is_file().then_some(nested)
        })
    })
}

/// One stored result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Entry {
    key: String,
    status: MutantStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    killed_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    duration_ms: Option<u64>,
    /// The inputs of a test that the key does not hash.
    not_keyed: Vec<String>,
    /// The generation of the last run that stored or used the entry.
    generation: u64,
}

/// The cache file.
#[derive(Debug, Default, Serialize, Deserialize)]
struct CacheFile {
    format_version: u32,
    generation: u64,
    entries: BTreeMap<String, Entry>,
}

/// True for a status that the cache stores. A timeout depends on the load
/// of the machine, and a flaky or runtime error result asks for a new run.
const fn is_cacheable(status: MutantStatus) -> bool {
    matches!(status, MutantStatus::Killed | MutantStatus::Survived)
}

/// The files that the key reads, from the mutation plan.
#[derive(Debug, Clone, Default)]
pub struct KeyFiles {
    /// The files that the test files reach, sorted.
    pub reach_files: Vec<String>,
    /// The reach of each test file, as indices into `reach_files`.
    pub test_file_reach: BTreeMap<String, Vec<u32>>,
    /// The runner setup files and their reach.
    pub support_files: Vec<String>,
}

impl KeyFiles {
    /// The key files of `plan`.
    #[must_use]
    pub fn from_plan(plan: &MutationPlan) -> Self {
        Self {
            reach_files: plan.reach_files.clone(),
            test_file_reach: plan.test_file_reach.clone(),
            support_files: plan.support_files.clone(),
        }
    }
}

/// The result cache of one run.
#[derive(Debug)]
pub struct ResultCache {
    root: PathBuf,
    file: PathBuf,
    /// The key parts that each mutant of the run shares.
    shared: String,
    files: KeyFiles,
    /// The content hash of each file that a key read.
    hashes: Mutex<FxHashMap<String, String>>,
    /// The digest of each test file and its reach.
    test_digests: Mutex<FxHashMap<String, String>>,
    state: Mutex<CacheFile>,
    max_entries: usize,
}

/// The cache facts of a finished run.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CacheCounts {
    /// The mutants whose result came from the cache.
    pub hits: usize,
    /// The results that the run added or replaced.
    pub stored: usize,
}

impl ResultCache {
    /// Opens the cache of `mutation_cache` for a project at `root` and the
    /// `runner` of the run. A missing file, a file of another format, or a
    /// file that does not parse gives an empty cache.
    #[must_use]
    pub fn open(mutation_cache: &Path, root: &Path, runner: &str, files: KeyFiles) -> Self {
        let file = mutation_cache.join(RESULT_CACHE_FILE);
        let state = std::fs::read(&file)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<CacheFile>(&bytes).ok())
            .filter(|state| state.format_version == FORMAT_VERSION)
            .map(|mut state| {
                state.entries.retain(|_, entry| is_cacheable(entry.status));
                state
            })
            .unwrap_or_default();
        let mut hasher = Sha256::new();
        feed(&mut hasher, &FORMAT_VERSION.to_string());
        feed(&mut hasher, env!("CARGO_PKG_VERSION"));
        feed(&mut hasher, runner);
        feed(&mut hasher, &lockfile_digest(root));
        feed(&mut hasher, &runner_config_digest(root));
        hasher.update((files.support_files.len() as u64).to_le_bytes());
        for support in &files.support_files {
            feed(&mut hasher, support);
            feed(&mut hasher, &file_hash(&root.join(support)));
        }
        Self {
            root: root.to_path_buf(),
            file,
            shared: hex(&hasher.finalize()),
            files,
            hashes: Mutex::new(FxHashMap::default()),
            test_digests: Mutex::new(FxHashMap::default()),
            state: Mutex::new(state),
            max_entries: MAX_ENTRIES,
        }
    }

    /// The digest of `test_file` and of each file that it reaches, computed
    /// one time for each run.
    fn test_digest(&self, test_file: &str) -> String {
        if let Ok(digests) = self.test_digests.lock()
            && let Some(digest) = digests.get(test_file)
        {
            return digest.clone();
        }
        let mut hasher = Sha256::new();
        feed(&mut hasher, test_file);
        feed(&mut hasher, &self.hash_of(test_file));
        let reach = self.files.test_file_reach.get(test_file);
        hasher.update((reach.map_or(0, Vec::len) as u64).to_le_bytes());
        for path in reach
            .into_iter()
            .flatten()
            .filter_map(|&index| self.files.reach_files.get(index as usize))
        {
            feed(&mut hasher, path);
            feed(&mut hasher, &self.hash_of(path));
        }
        let digest = hex(&hasher.finalize());
        if let Ok(mut digests) = self.test_digests.lock() {
            digests.insert(test_file.to_owned(), digest.clone());
        }
        digest
    }

    /// The content hash of the root-relative `path`, read one time for each
    /// run.
    fn hash_of(&self, path: &str) -> String {
        if let Ok(hashes) = self.hashes.lock()
            && let Some(hash) = hashes.get(path)
        {
            return hash.clone();
        }
        let hash = file_hash(&self.root.join(path));
        if let Ok(mut hashes) = self.hashes.lock() {
            hashes.insert(path.to_owned(), hash.clone());
        }
        hash
    }

    /// Adds the stored results of `outcome` and returns the counts. Only a
    /// killed mutant, or a survivor that a retry confirmed, is stored.
    pub fn record(&self, outcome: &ScheduleOutcome) -> CacheCounts {
        let mut counts = CacheCounts {
            hits: outcome
                .results
                .values()
                .filter(|result| result.cached)
                .count(),
            stored: 0,
        };
        let Ok(mut state) = self.state.lock() else {
            return counts;
        };
        let generation = state.generation + 1;
        for (id, key) in &outcome.cache_keys {
            let Some(result) = outcome.results.get(id) else {
                continue;
            };
            if result.cached || !is_cacheable(result.status) || outcome.unretried.contains(id) {
                continue;
            }
            state.entries.insert(
                id.clone(),
                Entry {
                    key: key.clone(),
                    status: result.status,
                    killed_by: result.killed_by.clone(),
                    duration_ms: result.duration_ms,
                    not_keyed: NOT_KEYED.iter().map(|&item| item.to_owned()).collect(),
                    generation,
                },
            );
            counts.stored += 1;
        }
        counts
    }

    /// Writes the cache file. The write goes to a temporary file first, so a
    /// reader never sees half a file.
    ///
    /// # Errors
    ///
    /// Returns the IO error as text.
    pub fn save(&self) -> Result<(), String> {
        let Ok(mut state) = self.state.lock() else {
            return Err("the result cache lock is poisoned".to_owned());
        };
        state.format_version = FORMAT_VERSION;
        state.generation += 1;
        if state.entries.len() > self.max_entries {
            let mut by_age: Vec<(u64, String)> = state
                .entries
                .iter()
                .map(|(id, entry)| (entry.generation, id.clone()))
                .collect();
            by_age.sort();
            let surplus = state.entries.len() - self.max_entries;
            for (_, id) in by_age.into_iter().take(surplus) {
                state.entries.remove(&id);
            }
        }
        let bytes = serde_json::to_vec(&*state)
            .map_err(|error| format!("cannot serialize the result cache: {error}"))?;
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

impl ResultLookup for ResultCache {
    fn key(
        &self,
        ready: &Ready,
        mutant: &ScheduledMutant,
        tests: &[String],
        test_files: &[String],
    ) -> String {
        let mut files: Vec<&str> = test_files.iter().map(String::as_str).collect();
        files.sort_unstable();
        files.dedup();
        let mut tests: Vec<&str> = tests.iter().map(String::as_str).collect();
        tests.sort_unstable();
        let mut hasher = Sha256::new();
        feed(&mut hasher, &self.shared);
        feed(&mut hasher, &ready.runner_version);
        feed(&mut hasher, &ready.runtime_version);
        feed(&mut hasher, &mutant.id);
        feed(&mut hasher, &mutant.file);
        feed(&mut hasher, &self.hash_of(&mutant.file));
        hasher.update((tests.len() as u64).to_le_bytes());
        for test in tests {
            feed(&mut hasher, test);
        }
        hasher.update((files.len() as u64).to_le_bytes());
        for file in files {
            feed(&mut hasher, &self.test_digest(file));
        }
        hex(&hasher.finalize())
    }

    fn get(&self, mutant_id: &str, key: &str) -> Option<CachedResult> {
        let mut state = self.state.lock().ok()?;
        let generation = state.generation + 1;
        let hit = state
            .entries
            .get_mut(mutant_id)
            .filter(|entry| entry.key == key)
            .map(|entry| {
                entry.generation = generation;
                CachedResult {
                    status: entry.status,
                    killed_by: entry.killed_by.clone(),
                    duration_ms: entry.duration_ms,
                }
            });
        drop(state);
        hit
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "tests unwrap to keep assertions short")]
mod tests;
