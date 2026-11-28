//! The reach record of each test file, and the rule that leaves a test file
//! out of the dry run.
//!
//! A run with reach selection probes each script file that a dry-run test
//! file reaches (see `fallow_mutation::instrument_with_probes`). After the dry
//! run, the record of each test file holds:
//!
//! - the environment part of its key: the result cache key without the
//!   reached files and the support files, the runner and runtime versions and
//!   capabilities, and the shared part of the dry-run key;
//! - the digest of the runner config modules and the files that they reach.
//!   The runner loads them in its main process, where no probe records, so
//!   the record holds their content;
//! - a snapshot of each reached file of the run: the key of its outline, or
//!   the hash of its content for a file without probes;
//! - the probe ids that its tests gave, and the probe ids of its dry-run
//!   share outside each test (a module load, a hook). The probe ids outside
//!   each test belong to each file of the share.
//!
//! A later run leaves test file T out of the dry run only when each of these
//! holds:
//!
//! 1. T has a record, the environment part is equal, and the runner config
//!    digest is equal. The setup files and the files that they reach count
//!    as files that T reaches.
//! 2. Each file that T reaches now is in the snapshot, and each probe id of
//!    the record names a file of the snapshot.
//! 3. For each such file F: F is equal to the snapshot; or F has probes and T
//!    did not load F; or F has probes and T runs the same code of F (see
//!    `same_run`: equal module code and import sources, each entered
//!    function unchanged under its key, and no changed binding that module
//!    code or an entered function names). A file without probes must be
//!    equal.
//! 4. No mutant of the run is in module code of a file that T loaded, in a
//!    function that T entered, or in a file without probes that T reaches. A
//!    mutant in a pure variable declarator counts only when module code or a
//!    function that T entered names the variable: T runs the declarator when
//!    it loads the module, but no code of T reads the value.
//!
//! Each other case keeps T. With an equal environment, equal module code of
//! each loaded file, equal entered functions and equal values of each name
//! that this code reads, T runs the code of the recorded run until it enters
//! a function that changed. The record shows that T entered no such
//! function, so T runs no changed code and, by rule 4, no mutant. The rule
//! assumes that a test gives the same run for the same inputs. It has the
//! limits of the result cache: dynamic imports
//! that the module graph does not resolve, and files that a test reads.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use fallow_engine::mutation::{MutationPlan, Outline, OutlineBody, SourceKind, probe_key};
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};
use xxhash_rust::xxh3::{Xxh3, xxh3_64};

use super::super::scheduler::{ReachSelection, ShareProbes};

/// The most snapshots that the store keeps. An entry whose snapshot is gone
/// has no record.
const MAX_SNAPSHOTS: usize = 16;

/// Why a test file stays in the dry run. The names are the keys of the
/// `kept` counts of the report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum KeepReason {
    /// The store has no complete record of the test file.
    NoRecord,
    /// The test file, its config or the runner changed.
    Environment,
    /// A runner config module or a file that it reaches changed.
    RunnerConfigChanged,
    /// The test file reaches a file that the record does not hold.
    NewReachFile,
    /// The record names a file that the snapshot does not hold.
    UnknownLoadedFile,
    /// A file without probes that the test file reaches changed.
    ChangedFileWithoutProbes,
    /// The code outside the function bodies of a loaded file changed.
    ChangedSkeleton,
    /// A function that the test file entered changed.
    ChangedEnteredFunction,
    /// An import or a declaration changed that module code or a function
    /// that the test file entered names.
    ChangedBinding,
    /// A mutant is outside each function body of a loaded file.
    MutantInLoadedCode,
    /// A mutant is in a function body that the test file entered.
    MutantInEnteredFunction,
    /// A mutant is in a file without probes that the test file reaches.
    MutantInFileWithoutProbes,
    /// The test file is the one kept loader of a mutant in a pure
    /// declarator that no other kept file is known to run.
    LoadsMutatedDeclarator,
}

impl KeepReason {
    /// The report name.
    pub const fn name(self) -> &'static str {
        match self {
            Self::NoRecord => "no-record",
            Self::Environment => "environment",
            Self::RunnerConfigChanged => "runner-config-changed",
            Self::NewReachFile => "new-reach-file",
            Self::UnknownLoadedFile => "unknown-loaded-file",
            Self::ChangedFileWithoutProbes => "changed-file-without-probes",
            Self::ChangedSkeleton => "changed-skeleton",
            Self::ChangedEnteredFunction => "changed-entered-function",
            Self::ChangedBinding => "changed-binding",
            Self::MutantInLoadedCode => "mutant-in-loaded-code",
            Self::MutantInEnteredFunction => "mutant-in-entered-function",
            Self::MutantInFileWithoutProbes => "mutant-in-file-without-probes",
            Self::LoadsMutatedDeclarator => "loads-mutated-declarator",
        }
    }
}

/// The reached files of one run: path to fact key.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct Snapshot {
    files: BTreeMap<String, String>,
    generation: u64,
}

/// The record of one test file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ReachEntry {
    env: String,
    config: String,
    snapshot: String,
    statics: String,
    entered: Vec<String>,
    generation: u64,
}

/// The reach records of the store file.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct ReachStore {
    #[serde(default)]
    snapshots: BTreeMap<String, Snapshot>,
    /// The outline of each fact key of a file with probes.
    #[serde(default)]
    outlines: BTreeMap<String, Outline>,
    /// The probe ids outside each test of one share, by hash.
    #[serde(default)]
    statics: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    tests: BTreeMap<String, ReachEntry>,
}

/// The fact of one file in the current run.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Fact {
    /// `o<hex>` for a file with probes, `c<hex>` for a file without, `-` for
    /// a file that does not exist.
    key: String,
    outline: Option<Outline>,
}

/// The fact key of a file with probes: the hash of its whole outline, so a
/// change of any part (a binding, a key, a reference) changes the key.
fn outline_key(outline: &Outline) -> String {
    let text = serde_json::to_vec(outline).unwrap_or_default();
    format!("o{:016x}", xxh3_64(&text))
}

fn hash_ids<'a>(ids: impl Iterator<Item = &'a String>) -> String {
    let mut hasher = Xxh3::new();
    for id in ids {
        hasher.update(&(id.len() as u64).to_le_bytes());
        hasher.update(id.as_bytes());
    }
    format!("{:016x}", hasher.digest())
}

/// The facts of the current run that the rule reads.
#[derive(Debug)]
pub struct ReachFacts {
    root: PathBuf,
    /// The outline of each file that the plan probed.
    probed: FxHashMap<String, Outline>,
    /// The files that each dry-run test file reaches.
    reach: FxHashMap<String, Vec<String>>,
    /// The reached files of each dry-run test file, in one sorted list.
    reach_files: Vec<String>,
    /// The spans of the mutants of each file.
    mutants: FxHashMap<String, Vec<(u32, u32)>>,
    /// The runner config modules and the files that they reach.
    config_files: Vec<String>,
    facts: Mutex<FxHashMap<String, Fact>>,
}

impl ReachFacts {
    /// The facts of `plan`, a plan with probes, under `root`.
    #[must_use]
    pub fn new(root: &Path, plan: &MutationPlan) -> Self {
        let probed = plan
            .probes
            .as_ref()
            .map(|probes| {
                probes
                    .files
                    .iter()
                    .map(|(path, file)| (path.clone(), file.outline.clone()))
                    .collect()
            })
            .unwrap_or_default();
        // Each test worker loads the setup files and their reach.
        let config: rustc_hash::FxHashSet<&str> = plan
            .config_support_files
            .iter()
            .map(String::as_str)
            .collect();
        let setup: Vec<String> = plan
            .support_files
            .iter()
            .filter(|path| !config.contains(path.as_str()))
            .cloned()
            .collect();
        let reach = plan
            .test_files
            .iter()
            .map(|test| {
                let mut files: Vec<String> = plan
                    .test_file_reach
                    .get(test)
                    .into_iter()
                    .flatten()
                    .filter_map(|&index| plan.reach_files.get(index as usize).cloned())
                    .chain(setup.iter().cloned())
                    .collect();
                files.sort_unstable();
                files.dedup();
                (test.clone(), files)
            })
            .collect();
        let mut reach_files: Vec<String> = plan.reach_files.iter().chain(&setup).cloned().collect();
        reach_files.sort_unstable();
        reach_files.dedup();
        let mut mutants: FxHashMap<String, Vec<(u32, u32)>> = FxHashMap::default();
        for mutant in &plan.mutants {
            mutants
                .entry(mutant.file.clone())
                .or_default()
                .push(mutant.mutant.span);
        }
        Self {
            root: root.to_path_buf(),
            probed,
            reach,
            reach_files,
            mutants,
            config_files: plan.config_support_files.clone(),
            facts: Mutex::new(FxHashMap::default()),
        }
    }

    /// The runner config modules and the files that they reach.
    pub(super) fn config_files(&self) -> &[String] {
        &self.config_files
    }

    /// Facts for a test: the outline of each probed file, the reach of each
    /// test file, and the mutant spans of each file.
    #[cfg(test)]
    pub(crate) fn from_parts(
        root: &Path,
        probed: FxHashMap<String, Outline>,
        reach: FxHashMap<String, Vec<String>>,
        mutants: FxHashMap<String, Vec<(u32, u32)>>,
    ) -> Self {
        let mut reach_files: Vec<String> = reach.values().flatten().cloned().collect();
        reach_files.sort_unstable();
        reach_files.dedup();
        Self {
            root: root.to_path_buf(),
            probed,
            reach,
            reach_files,
            mutants,
            config_files: Vec::new(),
            facts: Mutex::new(FxHashMap::default()),
        }
    }

    fn fact(&self, path: &str) -> Fact {
        if let Some(fact) = self
            .facts
            .lock()
            .ok()
            .and_then(|facts| facts.get(path).cloned())
        {
            return fact;
        }
        let fact = self.read_fact(path);
        if let Ok(mut facts) = self.facts.lock() {
            facts.insert(path.to_owned(), fact.clone());
        }
        fact
    }

    fn read_fact(&self, path: &str) -> Fact {
        if let Some(outline) = self.probed.get(path) {
            return Fact {
                key: outline_key(outline),
                outline: Some(outline.clone()),
            };
        }
        let Ok(bytes) = std::fs::read(self.root.join(path)) else {
            return Fact {
                key: "-".to_owned(),
                outline: None,
            };
        };
        Fact {
            key: format!("c{:016x}", xxh3_64(&bytes)),
            outline: None,
        }
    }

    /// The outline of `path` for a file that the run did not probe but that
    /// a record names. Only a script file has one.
    fn outline_on_disk(&self, path: &str) -> Option<Outline> {
        let kind = SourceKind::from_path(path)?;
        let source = std::fs::read_to_string(self.root.join(path)).ok()?;
        fallow_engine::mutation::outline(&source, kind).ok()
    }
}

/// Checks that a test file that loaded a file with the `recorded` outline,
/// and that entered the `entered` functions of it, runs the same code of the
/// file in its `current` outline:
///
/// - the module code and the import sources are equal;
/// - each entered function has the same text under the same key;
/// - each binding that changed (an import or a pure declaration) is named by
///   no module code and by no entered function, and a new binding does not
///   take a name that the recorded text already used (a global).
fn same_run(
    recorded: &Outline,
    current: &Outline,
    entered: &[&OutlineBody],
) -> Result<(), KeepReason> {
    if recorded.skeleton != current.skeleton {
        return Err(KeepReason::ChangedSkeleton);
    }
    for body in entered {
        let same = current
            .function(&body.key)
            .is_some_and(|ordinal| current.bodies[ordinal].hash == body.hash);
        if !same {
            return Err(KeepReason::ChangedEnteredFunction);
        }
    }
    let names: BTreeSet<&String> = recorded
        .bindings
        .keys()
        .chain(current.bindings.keys())
        .collect();
    let named = |list: &[String], name: &String| list.binary_search(name).is_ok();
    for name in names {
        if recorded.bindings.get(name) == current.bindings.get(name) {
            continue;
        }
        let used = named(&recorded.skeleton_refs, name)
            || named(&current.skeleton_refs, name)
            || entered.iter().any(|body| named(&body.refs, name))
            || (!recorded.bindings.contains_key(name) && named(&recorded.refs, name));
        if used {
            return Err(KeepReason::ChangedBinding);
        }
    }
    Ok(())
}

/// True when a test file that loaded a file with `outline` and entered the
/// `entered` functions of it can observe a mutant at `span` outside each
/// function body. A mutant in a pure variable declarator runs when the module
/// loads, and only code that reads the name sees its value: module code that
/// names it, or an entered function that names it. Each other mutant outside
/// a function body runs as module code.
fn observes_declarator(outline: &Outline, entered: &[&OutlineBody], span: (u32, u32)) -> bool {
    let Some(name) = outline.declarator_of(span) else {
        return true;
    };
    let named = |list: &[String]| {
        list.binary_search_by(|item| item.as_str().cmp(name))
            .is_ok()
    };
    named(&outline.skeleton_refs) || entered.iter().any(|body| named(&body.refs))
}

/// The probe ids of one file in a record: the module probe and the entered
/// ordinals.
#[derive(Debug, Default)]
struct FileProbes {
    loaded: bool,
    entered: BTreeSet<usize>,
}

fn group_probes<'a>(
    ids: impl Iterator<Item = &'a String>,
) -> Option<FxHashMap<&'a str, FileProbes>> {
    let mut groups: FxHashMap<&str, FileProbes> = FxHashMap::default();
    for id in ids {
        let (key, part) = id.strip_prefix('r')?.split_once('.')?;
        let group = groups.entry(key).or_default();
        group.loaded = true;
        if part != "m" {
            group.entered.insert(part.parse().ok()?);
        }
    }
    Some(groups)
}

impl ReachStore {
    /// Decides for `test_file` with the environment part `env` and the
    /// runner config digest `config`.
    #[cfg(test)]
    pub(super) fn decide(
        &self,
        facts: &ReachFacts,
        test_file: &str,
        (env, config): (&str, &str),
    ) -> Result<(), KeepReason> {
        self.decide_with(facts, test_file, (env, config), &mut Vec::new())
    }

    /// Decides for `test_file` with the environment part `env` and the
    /// runner config digest `config`, and adds to `unobserved` each mutant in
    /// a pure declarator of a file that the test file loads but whose value
    /// it never reads.
    fn decide_with(
        &self,
        facts: &ReachFacts,
        test_file: &str,
        (env, config): (&str, &str),
        unobserved: &mut Vec<(String, (u32, u32))>,
    ) -> Result<(), KeepReason> {
        let entry = self.tests.get(test_file).ok_or(KeepReason::NoRecord)?;
        let snapshot = self
            .snapshots
            .get(&entry.snapshot)
            .ok_or(KeepReason::NoRecord)?;
        let statics = self
            .statics
            .get(&entry.statics)
            .ok_or(KeepReason::NoRecord)?;
        if entry.env != env {
            return Err(KeepReason::Environment);
        }
        if entry.config != config {
            return Err(KeepReason::RunnerConfigChanged);
        }
        let groups = group_probes(entry.entered.iter().chain(statics))
            .ok_or(KeepReason::UnknownLoadedFile)?;
        let by_key: FxHashMap<String, &str> = snapshot
            .files
            .keys()
            .map(|path| (probe_key(path), path.as_str()))
            .collect();
        let mut files: BTreeSet<&str> = BTreeSet::new();
        for key in groups.keys() {
            files.insert(by_key.get(*key).ok_or(KeepReason::UnknownLoadedFile)?);
        }
        let reach = facts
            .reach
            .get(test_file)
            .map(Vec::as_slice)
            .unwrap_or_default();
        for path in reach {
            if !snapshot.files.contains_key(path) {
                return Err(KeepReason::NewReachFile);
            }
            files.insert(path);
        }
        for path in files {
            let probes = groups.get(probe_key(path).as_str());
            self.check_file(facts, snapshot, path, probes, unobserved)?;
        }
        Ok(())
    }

    fn check_file(
        &self,
        facts: &ReachFacts,
        snapshot: &Snapshot,
        path: &str,
        probes: Option<&FileProbes>,
        unobserved: &mut Vec<(String, (u32, u32))>,
    ) -> Result<(), KeepReason> {
        let Some(old) = snapshot.files.get(path) else {
            return Err(KeepReason::NewReachFile);
        };
        let current = facts.fact(path);
        let mutants = facts
            .mutants
            .get(path)
            .map(Vec::as_slice)
            .unwrap_or_default();
        if !old.starts_with('o') {
            if *old != current.key {
                return Err(KeepReason::ChangedFileWithoutProbes);
            }
            if !mutants.is_empty() {
                return Err(KeepReason::MutantInFileWithoutProbes);
            }
            return Ok(());
        }
        let Some(probes) = probes.filter(|probes| probes.loaded) else {
            // The test file did not load the file. A change of the file
            // reaches it only through code that it runs and that changed.
            return Ok(());
        };
        let recorded = self.outlines.get(old).ok_or(KeepReason::NoRecord)?;
        let entered: Vec<&OutlineBody> = probes
            .entered
            .iter()
            .map(|&ordinal| recorded.bodies.get(ordinal))
            .collect::<Option<_>>()
            .ok_or(KeepReason::UnknownLoadedFile)?;
        let outline = match current.outline {
            Some(outline) => outline,
            None if current.key == *old => recorded.clone(),
            None => facts
                .outline_on_disk(path)
                .ok_or(KeepReason::ChangedFileWithoutProbes)?,
        };
        if current.key != *old {
            same_run(recorded, &outline, &entered)?;
        }
        let entered_keys: BTreeSet<&str> = entered.iter().map(|body| body.key.as_str()).collect();
        for &span in mutants {
            match outline.body_of(span) {
                None if observes_declarator(&outline, &entered, span) => {
                    return Err(KeepReason::MutantInLoadedCode);
                }
                None => unobserved.push((path.to_owned(), span)),
                Some(ordinal) if entered_keys.contains(outline.bodies[ordinal].key.as_str()) => {
                    return Err(KeepReason::MutantInEnteredFunction);
                }
                Some(_) => {}
            }
        }
        Ok(())
    }

    /// Selects the files of `dry_files` that the dry run can leave out.
    pub(super) fn select(
        &self,
        facts: &ReachFacts,
        dry_files: &[String],
        (env, config): (impl Fn(&str) -> String, &str),
    ) -> ReachSelection {
        let mut selection = ReachSelection::default();
        // Each mutant in a pure declarator that a dropped file loads, with
        // the first such file.
        let mut loaders: BTreeMap<(String, (u32, u32)), String> = BTreeMap::new();
        for test_file in dry_files {
            let mut unobserved = Vec::new();
            match self.decide_with(facts, test_file, (&env(test_file), config), &mut unobserved) {
                Ok(()) => {
                    selection.dropped.push(test_file.clone());
                    for mutant in unobserved {
                        loaders.entry(mutant).or_insert_with(|| test_file.clone());
                    }
                }
                Err(reason) => {
                    *selection.kept.entry(reason.name().to_owned()).or_default() += 1;
                }
            }
        }
        // A dropped file still runs such a mutant when it loads the module,
        // so the full run counts it as covered. One loader stays for each,
        // so the mutant keeps a covering test and its verdict.
        let keep: BTreeSet<String> = loaders.into_values().collect();
        if !keep.is_empty() {
            selection.dropped.retain(|file| !keep.contains(file));
            *selection
                .kept
                .entry(KeepReason::LoadsMutatedDeclarator.name().to_owned())
                .or_default() += keep.len();
        }
        selection.dropped.sort_unstable();
        selection
    }

    /// Records the probes of `shares`. A test file of a failed share part, or
    /// of a run whose workers keep modules between test files, loses its
    /// record. `complete` is false for such a run.
    pub(super) fn record(
        &mut self,
        facts: &ReachFacts,
        shares: &[ShareProbes],
        complete: bool,
        (env, config): (impl Fn(&str) -> String, &str),
        generation: u64,
    ) -> usize {
        let files: BTreeMap<String, String> = facts
            .reach_files
            .iter()
            .map(|path| {
                let fact = facts.fact(path);
                if let Some(outline) = &fact.outline {
                    self.outlines.insert(fact.key.clone(), outline.clone());
                }
                (path.clone(), fact.key)
            })
            .collect();
        let snapshot = hash_ids(files.iter().flat_map(|(path, key)| [path, key]));
        let mut recorded = 0;
        for share in shares {
            let statics = hash_ids(share.statics.iter());
            for test_file in &share.files {
                if !complete || share.failed.contains(test_file) {
                    self.tests.remove(test_file);
                    continue;
                }
                let entered = share
                    .entered
                    .get(test_file)
                    .map(|ids| ids.iter().cloned().collect())
                    .unwrap_or_default();
                self.tests.insert(
                    test_file.clone(),
                    ReachEntry {
                        env: env(test_file),
                        config: config.to_owned(),
                        snapshot: snapshot.clone(),
                        statics: statics.clone(),
                        entered,
                        generation,
                    },
                );
                recorded += 1;
            }
            self.statics
                .entry(statics)
                .or_insert_with(|| share.statics.iter().cloned().collect());
        }
        if recorded > 0 {
            self.snapshots
                .insert(snapshot, Snapshot { files, generation });
        }
        recorded
    }

    /// Drops the records of the oldest entries above `max_entries`, the
    /// oldest snapshots above [`MAX_SNAPSHOTS`], and each part that no
    /// entry names.
    pub(super) fn prune(&mut self, max_entries: usize) {
        if self.tests.len() > max_entries {
            let mut by_age: Vec<(u64, String)> = self
                .tests
                .iter()
                .map(|(file, entry)| (entry.generation, file.clone()))
                .collect();
            by_age.sort();
            let surplus = self.tests.len() - max_entries;
            for (_, file) in by_age.into_iter().take(surplus) {
                self.tests.remove(&file);
            }
        }
        if self.snapshots.len() > MAX_SNAPSHOTS {
            let mut by_age: Vec<(u64, String)> = self
                .snapshots
                .iter()
                .map(|(id, snapshot)| (snapshot.generation, id.clone()))
                .collect();
            by_age.sort();
            let surplus = self.snapshots.len() - MAX_SNAPSHOTS;
            for (_, id) in by_age.into_iter().take(surplus) {
                self.snapshots.remove(&id);
            }
        }
        let snapshots = &self.snapshots;
        self.tests
            .retain(|_, entry| snapshots.contains_key(&entry.snapshot));
        let named_statics: BTreeSet<&String> =
            self.tests.values().map(|entry| &entry.statics).collect();
        self.statics.retain(|id, _| named_statics.contains(id));
        let named_outlines: BTreeSet<&String> = self
            .snapshots
            .values()
            .flat_map(|snapshot| snapshot.files.values())
            .collect();
        self.outlines.retain(|key, _| named_outlines.contains(key));
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "tests unwrap to keep assertions short")]
mod tests;
