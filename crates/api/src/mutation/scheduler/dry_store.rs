//! The dry-run facts of each test file from an earlier run.
//!
//! A run asks the [`DryRunLookup`] for the stored facts of each dry-run test
//! file. The key of a file holds each input of its dry run, see
//! `dry_run_cache`. Only the files without valid facts dry-run. The scheduler
//! joins the stored facts with the facts of the new dry run, so the rest of
//! the run sees one dry run of each file.
//!
//! The runner gives two facts only for a whole dry run, not for each file:
//! the mutants that ran when a module loaded (static hits), and the
//! instrumented files that loaded. A stored file keeps the facts of its
//! dry-run share that belong to the source files that it reaches through
//! static imports. Another file of the share that reaches the same source
//! file can be the file that gave the fact. So the facts of a file are valid
//! only when each such file (its `bound` files) is valid in the same run.
//! Without this rule, a test file that imports only types from a module
//! could keep a static hit that a changed test file gave.

use serde::{Deserialize, Serialize};

use super::dry_phase::{DryPhase, ShareFacts, dry_phase};
use super::*;

/// One test of a stored dry run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoredTest {
    pub test_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<f64>,
    pub status: TestStatus,
    /// The entry of the test in the test index of the companion that ran
    /// it. A companion of a later run takes it with an `adopt` request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index: Option<serde_json::Value>,
}

/// The dry-run facts of one test file. The store keeps only a file with no
/// failed test and no error.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct StoredDryRun {
    /// The tests of the file, in runner order.
    pub tests: Vec<StoredTest>,
    /// The mutants that each test hits.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub hits: BTreeMap<String, Vec<String>>,
    /// The mutants that the file hits, from a runner without the hits of
    /// each test.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_hits: Option<Vec<String>>,
    /// The static hits of the dry-run share of the file, cut to the mutants
    /// of the source files that the file reaches.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub static_hits: Vec<String>,
    /// The instrumented files that the dry-run share loaded, cut to the
    /// files that the file reaches.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub loaded_paths: Vec<String>,
    /// The setup time of the file outside its tests.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub setup_ms: Option<f64>,
    /// The part of the measured wall time of its dry-run share that belongs
    /// to the file, by its setup and test time. The dry-run timeout of a
    /// later companion scales with it, as with the time of a new dry run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dry_run_ms: Option<f64>,
    /// The ids of the tests of the file that share a name with another test.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub duplicate_test_ids: Vec<String>,
    /// The other test files of the dry-run share that reach a source file of
    /// `static_hits` or `loaded_paths`. The facts are valid only when each
    /// of these files has valid facts in the same run.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bound: Vec<String>,
}

impl StoredDryRun {
    /// The dry-run time of the file: its part of the wall time of its share,
    /// or its setup and test time when the entry has no such part.
    fn duration(&self) -> Duration {
        let ms = self.dry_run_ms.unwrap_or_else(|| {
            self.setup_ms.unwrap_or(0.0)
                + self
                    .tests
                    .iter()
                    .filter_map(|test| test.duration_ms)
                    .sum::<f64>()
        });
        Duration::from_secs_f64(ms.max(0.0) / 1000.0)
    }

    /// True when each mutant id of the facts is in the plan. Another id
    /// comes from an instrumented file that the key does not hold, for
    /// example a module that a test loads through a dynamic import.
    fn names_known_ids(&self, plan: &SchedulePlan) -> bool {
        self.hits
            .values()
            .flatten()
            .chain(self.file_hits.iter().flatten())
            .chain(&self.static_hits)
            .all(|id| plan.known_ids.contains(id))
    }
}

/// The new dry-run facts of one test file, for the store.
#[derive(Debug, Clone, PartialEq)]
pub struct DryRunEntry {
    pub test_file: String,
    pub key: String,
    pub facts: StoredDryRun,
}

/// The stored dry-run facts of each test file.
pub trait DryRunLookup: std::fmt::Debug + Send + Sync {
    /// The key of the dry-run facts of `test_file` in this run. `ready` is
    /// the answer of the first companion of the run.
    fn key(&self, ready: &Ready, test_file: &str) -> String;

    /// The stored facts of `test_file` when they have the key `key`.
    fn get(&self, test_file: &str, key: &str) -> Option<StoredDryRun>;
}

/// The stored facts of `dry_files` and the files that must dry-run, in the
/// order of `dry_files`.
struct Reuse {
    stored: Vec<(String, StoredDryRun)>,
    run: Vec<String>,
    keys: FxHashMap<String, String>,
}

fn reuse(
    plan: &SchedulePlan,
    config: &ScheduleConfig,
    ready: &Ready,
    dry_files: &[String],
) -> Reuse {
    let Some(store) = config.dry_runs.as_deref() else {
        return Reuse {
            stored: Vec::new(),
            run: dry_files.to_vec(),
            keys: FxHashMap::default(),
        };
    };
    let keys: FxHashMap<String, String> = dry_files
        .iter()
        .map(|file| (file.clone(), store.key(ready, file)))
        .collect();
    let mut valid: FxHashMap<&str, StoredDryRun> = dry_files
        .iter()
        .filter_map(|file| {
            let facts = store.get(file, &keys[file])?;
            facts
                .names_known_ids(plan)
                .then_some((file.as_str(), facts))
        })
        .collect();
    loop {
        let unbound: Vec<&str> = valid
            .iter()
            .filter(|(_, facts)| {
                facts
                    .bound
                    .iter()
                    .any(|file| !valid.contains_key(file.as_str()))
            })
            .map(|(file, _)| *file)
            .collect();
        if unbound.is_empty() {
            break;
        }
        for file in unbound {
            valid.remove(file);
        }
    }
    let mut stored = Vec::new();
    let mut run = Vec::new();
    for file in dry_files {
        match valid.remove(file.as_str()) {
            Some(facts) => stored.push((file.clone(), facts)),
            None => run.push(file.clone()),
        }
    }
    Reuse { stored, run, keys }
}

/// The source file of each mutant of the plan.
fn mutant_files(plan: &SchedulePlan) -> FxHashMap<&str, &str> {
    plan.mutants
        .iter()
        .chain(&plan.unreached)
        .chain(&plan.pending)
        .chain(&plan.sampled_out)
        .map(|mutant| (mutant.id.as_str(), mutant.file.as_str()))
        .collect()
}

/// The share facts of one file, and the files that are bound to it.
#[derive(Default)]
struct Claims {
    static_hits: Vec<String>,
    loaded_paths: Vec<String>,
    bound: BTreeSet<String>,
}

/// The facts of one share that belong to each file, and its bound files. A
/// fact belongs to each file of the share that reaches its source file
/// through static imports, also through a module that `vi.mock` replaces: a
/// mock factory can load the original module. A
/// fact with a source file that no file of the share reaches (a dynamic
/// import or a setup file) belongs to each file of the share.
fn share_claims(
    plan: &SchedulePlan,
    universe: &BTreeMap<String, String>,
    share: &ShareFacts,
) -> FxHashMap<String, Claims> {
    let sources = mutant_files(plan);
    let reach: FxHashMap<&str, FxHashSet<&str>> = plan
        .reaching_test_files
        .iter()
        .map(|(source, files)| (source.as_str(), files.iter().map(String::as_str).collect()))
        .collect();
    let holders = |source: &str| -> Vec<&String> {
        let found: Vec<&String> = share
            .files
            .iter()
            .filter(|file| {
                reach
                    .get(source)
                    .is_some_and(|files| files.contains(file.as_str()))
            })
            .collect();
        if found.is_empty() {
            share.files.iter().collect()
        } else {
            found
        }
    };
    let unloaded: FxHashSet<&str> = share.unloaded_paths.iter().map(String::as_str).collect();
    let mut claims: FxHashMap<String, Claims> = share
        .files
        .iter()
        .map(|file| (file.clone(), Claims::default()))
        .collect();
    let mut claim = |source: &str, add: &dyn Fn(&mut Claims)| {
        let owners = holders(source);
        for owner in &owners {
            if let Some(entry) = claims.get_mut(owner.as_str()) {
                add(entry);
                let others = owners.iter().filter(|other| *other != owner);
                entry.bound.extend(others.map(|other| (*other).clone()));
            }
        }
    };
    for id in &share.static_hits {
        if let Some(source) = sources.get(id.as_str()) {
            claim(source, &|entry| entry.static_hits.push(id.clone()));
        }
    }
    for path in universe
        .keys()
        .filter(|path| !unloaded.contains(path.as_str()))
    {
        claim(path, &|entry| entry.loaded_paths.push(path.clone()));
    }
    claims
}

/// The new facts of each file of `shares` that dry-ran with no failure.
fn new_entries(
    plan: &SchedulePlan,
    universe: &BTreeMap<String, String>,
    phase: &DryPhase,
    keys: &FxHashMap<String, String>,
) -> Vec<DryRunEntry> {
    let dry = &phase.result;
    if keys.is_empty()
        || dry
            .unhandled_errors
            .iter()
            .any(|error| error.file.is_none())
    {
        return Vec::new();
    }
    let failed: FxHashSet<&str> = dry
        .tests
        .iter()
        .filter(|test| test.status == TestStatus::Fail)
        .map(|test| test.file.as_str())
        .chain(
            dry.unhandled_errors
                .iter()
                .filter_map(|error| error.file.as_deref()),
        )
        .collect();
    let duplicates: FxHashSet<&str> = dry.duplicate_test_ids.iter().map(String::as_str).collect();
    let mut entries = Vec::new();
    for share in &phase.shares {
        let start = entries.len();
        for (file, claims) in share_claims(plan, universe, share) {
            let Some(key) = keys.get(&file).filter(|_| !failed.contains(file.as_str())) else {
                continue;
            };
            let tests: Vec<&DryTest> = dry.tests.iter().filter(|test| test.file == file).collect();
            let mut facts = StoredDryRun {
                tests: tests
                    .iter()
                    .map(|test| StoredTest {
                        test_id: test.test_id.clone(),
                        duration_ms: test.duration_ms,
                        status: test.status,
                        index: test.index.clone(),
                    })
                    .collect(),
                hits: tests
                    .iter()
                    .filter_map(|test| {
                        let ids = dry.hits.get(&test.test_id)?;
                        Some((test.test_id.clone(), ids.clone()))
                    })
                    .collect(),
                file_hits: dry
                    .file_hits
                    .as_ref()
                    .and_then(|hits| hits.get(&file).cloned()),
                static_hits: claims.static_hits,
                loaded_paths: claims.loaded_paths,
                setup_ms: dry.file_setup_ms.get(&file).copied(),
                dry_run_ms: None,
                duplicate_test_ids: tests
                    .iter()
                    .filter(|test| duplicates.contains(test.test_id.as_str()))
                    .map(|test| test.test_id.clone())
                    .collect(),
                bound: claims.bound.into_iter().collect(),
            };
            facts.static_hits.sort_unstable();
            facts.loaded_paths.sort_unstable();
            entries.push(DryRunEntry {
                test_file: file.clone(),
                key: key.clone(),
                facts,
            });
        }
        apportion(&mut entries[start..], share, dry);
    }
    entries.sort_by(|left, right| left.test_file.cmp(&right.test_file));
    entries
}

/// Gives each entry of `share` its part of the wall time of the share, by
/// the setup and test time of each file of the share in `dry`. A share with
/// no such time gives each file an equal part.
fn apportion(entries: &mut [DryRunEntry], share: &ShareFacts, dry: &DryRunResult) {
    let own = |file: &str| -> f64 {
        dry.file_setup_ms.get(file).copied().unwrap_or(0.0)
            + dry
                .tests
                .iter()
                .filter(|test| test.file == file)
                .filter_map(|test| test.duration_ms)
                .sum::<f64>()
    };
    let total_ms = share.took.as_secs_f64() * 1000.0;
    let share_ms: f64 = share.files.iter().map(|file| own(file)).sum();
    let files = share.files.len().max(1) as f64;
    for entry in entries {
        let part = if share_ms > 0.0 {
            own(&entry.test_file) / share_ms
        } else {
            1.0 / files
        };
        entry.facts.dry_run_ms = Some(total_ms * part);
    }
}

/// Joins the new dry run of some files (`None` when no file dry-ran) with
/// the stored facts of the other files. The tests keep the order of
/// `dry_files`. A path is unloaded when no new share and no stored file
/// loaded it.
fn join(
    fresh: Option<DryRunResult>,
    stored: &[(String, StoredDryRun)],
    dry_files: &[String],
    universe: &BTreeMap<String, String>,
) -> DryRunResult {
    let fresh_unloaded = fresh.as_ref().map(|dry| dry.unloaded_paths.clone());
    let mut joined = fresh.unwrap_or(DryRunResult {
        tests: Vec::new(),
        hits: BTreeMap::new(),
        static_hits: Vec::new(),
        file_hits: None,
        unhandled_errors: Vec::new(),
        unloaded_paths: Vec::new(),
        file_setup_ms: BTreeMap::new(),
        duplicate_test_ids: Vec::new(),
    });
    let mut loaded: FxHashSet<&str> = FxHashSet::default();
    for (file, facts) in stored {
        joined.tests.extend(facts.tests.iter().map(|test| DryTest {
            test_id: test.test_id.clone(),
            file: file.clone(),
            duration_ms: test.duration_ms,
            status: test.status,
            failure_kind: None,
            message: None,
            index: test.index.clone(),
        }));
        joined.hits.extend(facts.hits.clone());
        if let Some(ids) = &facts.file_hits {
            joined
                .file_hits
                .get_or_insert_with(BTreeMap::new)
                .insert(file.clone(), ids.clone());
        }
        joined.static_hits.extend(facts.static_hits.iter().cloned());
        if let Some(setup) = facts.setup_ms {
            joined.file_setup_ms.insert(file.clone(), setup);
        }
        joined
            .duplicate_test_ids
            .extend(facts.duplicate_test_ids.iter().cloned());
        loaded.extend(facts.loaded_paths.iter().map(String::as_str));
    }
    let order: FxHashMap<&str, usize> = dry_files
        .iter()
        .enumerate()
        .map(|(index, file)| (file.as_str(), index))
        .collect();
    joined
        .tests
        .sort_by_key(|test| order.get(test.file.as_str()).copied().unwrap_or(usize::MAX));
    joined.static_hits.sort_unstable();
    joined.static_hits.dedup();
    joined.duplicate_test_ids.sort_unstable();
    joined.duplicate_test_ids.dedup();
    joined.unloaded_paths = fresh_unloaded
        .unwrap_or_else(|| universe.keys().cloned().collect())
        .into_iter()
        .filter(|path| !loaded.contains(path.as_str()))
        .collect();
    joined
}

/// The dry run of a run: the stored facts of each file with valid facts,
/// and a new dry run of the other files on `primary` and more companions.
/// With no file to dry-run, `primary` shuts down and no dry run starts. The
/// counts and the new entries of the store, and the dry-run time, go to
/// `outcome`.
pub(super) fn joined_dry_phase(
    spawner: &dyn CompanionSpawner,
    primary: Box<dyn CompanionLink>,
    (plan, ready, dry_files): (&SchedulePlan, &Ready, &[String]),
    config: &ScheduleConfig,
    (progress, outcome): (MutationProgressSink<'_>, &mut ScheduleOutcome),
) -> Result<DryPhase, ScheduleError> {
    let started = config.clock.now();
    let Reuse { stored, run, keys } = reuse(plan, config, ready, dry_files);
    // The run dir of a run with reach selection also holds files that have
    // only entry probes. They are no plan files, so the loaded paths leave
    // them out.
    let plan_paths: BTreeMap<String, String>;
    let universe = match &config.reach {
        Some(reach) if !reach.probe_only.is_empty() => {
            plan_paths = config
                .init
                .path_map
                .iter()
                .filter(|(path, _)| !reach.probe_only.contains(*path))
                .map(|(path, target)| (path.clone(), target.clone()))
                .collect();
            &plan_paths
        }
        _ => &config.init.path_map,
    };
    let stored_time: Duration = stored.iter().map(|(_, facts)| facts.duration()).sum();
    outcome.dry_run_reused = stored.len();
    if run.is_empty() {
        let mut primary = primary;
        primary.shutdown();
        outcome.timings.dry_run = config.clock.now().saturating_duration_since(started);
        return Ok(DryPhase {
            result: join(None, &stored, dry_files, universe),
            links: Vec::new(),
            serial: stored_time,
            shares: Vec::new(),
        });
    }
    progress(MutationProgress::Baseline {
        test_files: run.len(),
    });
    let phase = dry_phase(spawner, primary, &run, config, !stored.is_empty())?;
    outcome.timings.dry_run = config.clock.now().saturating_duration_since(started);
    outcome.dry_run_missed = if keys.is_empty() { 0 } else { run.len() };
    outcome.dry_run_entries = new_entries(plan, universe, &phase, &keys);
    if let Some(reach) = &mut outcome.reach {
        reach.shares = phase
            .shares
            .iter()
            .map(|share| share.probes.clone())
            .collect();
    }
    Ok(DryPhase {
        result: join(Some(phase.result), &stored, dry_files, universe),
        serial: phase.serial + stored_time,
        links: phase.links,
        shares: phase.shares,
    })
}
