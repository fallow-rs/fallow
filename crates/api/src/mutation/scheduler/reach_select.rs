//! The selection of the dry-run test files by recorded function reach.
//!
//! In a run with reach selection, the instrumented files hold entry probes
//! (see `fallow_mutation::instrument_with_probes`). The runtime records a
//! probe id like a mutant hit, so the dry run of a share gives the probe ids
//! of each test and the probe ids outside each test. The scheduler takes the
//! probe ids out of the dry-run result before it reads the mutant hits, and
//! keeps them for the record of each test file ([`ShareProbes`]).
//!
//! Before the dry run, a [`ReachLookup`] can drop test files whose record
//! proves that they cannot run a mutant of the run (see the
//! `dry_run_cache::reach` module for the rule). In a check run, the dry run
//! keeps each file, and the run compares the covering tests of each mutant
//! with the dropped files afterwards.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};

use super::super::protocol::{DryRunResult, Ready, TestStatus};
use super::ScheduleOutcome;

/// True for a probe id. A mutant id has only hex digits.
pub fn is_probe(id: &str) -> bool {
    id.starts_with('r')
}

/// The probe ids of the dry run of one share.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ShareProbes {
    /// The test files of the share.
    pub files: Vec<String>,
    /// The probe ids outside each test, for example while a module loads.
    /// They belong to each file of the share.
    pub statics: BTreeSet<String>,
    /// The probe ids of the tests of each test file.
    pub entered: BTreeMap<String, BTreeSet<String>>,
    /// The test files with a failed test or an error. Their record is not
    /// complete.
    pub failed: BTreeSet<String>,
}

impl ShareProbes {
    /// Joins the probes of `shares` into one share. A recheck of failed
    /// files replaces their probes with `recheck`.
    pub(super) fn joined(shares: Vec<Self>, recheck: Option<Self>) -> Self {
        let mut joined = Self::default();
        for share in shares.into_iter().chain(recheck) {
            joined.files.extend(share.files);
            joined.statics.extend(share.statics);
            for (file, probes) in share.entered {
                joined.entered.insert(file, probes);
            }
            joined.failed.extend(share.failed);
        }
        joined.files.sort_unstable();
        joined.files.dedup();
        joined
    }
}

/// Takes the probe ids out of `dry`, the dry run of `files`, and leaves the
/// unloaded paths of the plan files only. A file in `probe_only` has no
/// mutant, so it is not an unloaded file of the plan.
pub(super) fn take_probes(
    files: &[String],
    dry: &mut DryRunResult,
    probe_only: &FxHashSet<String>,
) -> ShareProbes {
    let file_of: FxHashMap<&str, &str> = dry
        .tests
        .iter()
        .map(|test| (test.test_id.as_str(), test.file.as_str()))
        .collect();
    let mut entered: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (test, hits) in &mut dry.hits {
        let probes: Vec<String> = hits.iter().filter(|id| is_probe(id)).cloned().collect();
        hits.retain(|id| !is_probe(id));
        if let Some(file) = file_of.get(test.as_str()) {
            entered
                .entry((*file).to_owned())
                .or_default()
                .extend(probes);
        }
    }
    dry.hits.retain(|_, hits| !hits.is_empty());
    if let Some(file_hits) = &mut dry.file_hits {
        for (file, hits) in file_hits.iter_mut() {
            entered
                .entry(file.clone())
                .or_default()
                .extend(hits.iter().filter(|id| is_probe(id)).cloned());
            hits.retain(|id| !is_probe(id));
        }
    }
    let statics = dry
        .static_hits
        .iter()
        .filter(|id| is_probe(id))
        .cloned()
        .collect();
    dry.static_hits.retain(|id| !is_probe(id));
    dry.unloaded_paths.retain(|path| !probe_only.contains(path));
    let failed = dry
        .tests
        .iter()
        .filter(|test| test.status == TestStatus::Fail)
        .map(|test| test.file.clone())
        .chain(dry.unhandled_errors.iter().map(|error| {
            // An error outside each test file belongs to each file.
            error.file.clone().unwrap_or_default()
        }))
        .collect::<BTreeSet<String>>();
    let failed = if failed.contains("") {
        files.iter().cloned().collect()
    } else {
        failed
    };
    ShareProbes {
        files: files.to_vec(),
        statics,
        entered,
        failed,
    }
}

/// The selection of the dry-run test files of a run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReachSelection {
    /// Why the run cannot select by reach, when it cannot.
    pub disabled: Option<&'static str>,
    /// The test files that the record drops, sorted.
    pub dropped: Vec<String>,
    /// The count of kept test files for each reason.
    pub kept: BTreeMap<String, usize>,
}

/// The reach records of the test files of a run.
pub trait ReachLookup: std::fmt::Debug + Send + Sync {
    /// Selects the test files of `dry_files` that the dry run can leave out.
    /// `ready` is the answer of the first companion of the run.
    fn select(&self, ready: &Ready, dry_files: &[String]) -> ReachSelection;
}

/// The reach selection of a run.
#[derive(Debug, Clone)]
pub struct ReachConfig {
    pub lookup: Arc<dyn ReachLookup>,
    /// Dry-run each file, and give the selection only to compare.
    pub check: bool,
    /// The files that the run dir holds only for their probes.
    pub probe_only: FxHashSet<String>,
}

/// The reach facts of a run, for the record and the report.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReachOutcome {
    pub selection: ReachSelection,
    /// True when the dry run kept each file to check the selection.
    pub check: bool,
    /// The probe ids of each share of the new dry run.
    pub shares: Vec<ShareProbes>,
}

/// The dry-run files of the run: `dry_files` without the files that the
/// record drops. A check run keeps each file.
pub(super) fn narrow(
    config: Option<&ReachConfig>,
    ready: &Ready,
    dry_files: Vec<String>,
    outcome: &mut ScheduleOutcome,
) -> Vec<String> {
    let Some(config) = config else {
        return dry_files;
    };
    let selection = config.lookup.select(ready, &dry_files);
    let dropped: FxHashSet<&str> = selection.dropped.iter().map(String::as_str).collect();
    let kept = if config.check {
        dry_files
    } else {
        dry_files
            .into_iter()
            .filter(|file| !dropped.contains(file.as_str()))
            .collect()
    };
    outcome.reach = Some(ReachOutcome {
        selection,
        check: config.check,
        shares: Vec::new(),
    });
    kept
}
