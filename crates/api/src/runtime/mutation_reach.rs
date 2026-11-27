//! The reach selection of a `fallow mutation` run: the probed run files, the
//! scheduler config, and the report part (see `dry_run_cache::reach`).

use std::path::Path;
use std::sync::Arc;

use fallow_engine::mutation::MutationPlan;
use fallow_output::{MutationReachSelectionOutput, MutationReachViolation};
use rustc_hash::FxHashSet;

use crate::mutation::ReachSelectionMode;
use crate::mutation::dry_run_cache::{DryRunCache, ReachFacts};
use crate::mutation::run_dir::RunFile;
use crate::mutation::scheduler::{ReachConfig, ReachLookup, ScheduleOutcome};

/// The instrumented files of `plan` as the run directory takes them. A plan
/// with probes gives the probed code of each file with a mutant, and a
/// probed copy of each other script file that a test file reaches.
pub(super) fn run_files(plan: &MutationPlan) -> Vec<RunFile<'_>> {
    let probed = plan.probes.as_ref().map(|probes| &probes.files);
    let mut files: Vec<RunFile<'_>> = plan
        .files
        .iter()
        .map(|file| RunFile {
            path: &file.path,
            code: probed
                .and_then(|probed| probed.get(&file.path))
                .map_or(&file.instrumented_code, |probed| &probed.code),
        })
        .collect();
    if let Some(probed) = probed {
        let planned: FxHashSet<&str> = plan.files.iter().map(|file| file.path.as_str()).collect();
        files.extend(
            probed
                .iter()
                .filter(|(path, _)| !planned.contains(path.as_str()))
                .map(|(path, file)| RunFile {
                    path,
                    code: &file.code,
                }),
        );
        files.sort_by(|left, right| left.path.cmp(right.path));
    }
    files
}

/// True when the run selects by reach. The selection needs the dry-run
/// cache, and a kill matrix target needs a dry run of each target file.
pub(super) fn enabled(mode: ReachSelectionMode, no_cache: bool, targets: bool) -> bool {
    mode != ReachSelectionMode::Off && !no_cache && !targets
}

/// The dry-run cache with the reach facts of `plan`, when the run selects by
/// reach.
pub(super) fn with_facts(cache: DryRunCache, root: &Path, plan: &MutationPlan) -> DryRunCache {
    if plan.probes.is_some() {
        cache.with_reach(ReachFacts::new(root, plan))
    } else {
        cache
    }
}

/// The scheduler config of the reach selection.
pub(super) fn schedule_config(
    mode: ReachSelectionMode,
    plan: &MutationPlan,
    dry_cache: Option<&Arc<DryRunCache>>,
) -> Option<ReachConfig> {
    let probes = plan.probes.as_ref()?;
    let lookup = Arc::clone(dry_cache?) as Arc<dyn ReachLookup>;
    let planned: FxHashSet<&str> = plan.files.iter().map(|file| file.path.as_str()).collect();
    Some(ReachConfig {
        lookup,
        check: mode == ReachSelectionMode::Check,
        probe_only: probes
            .files
            .keys()
            .filter(|path| !planned.contains(path.as_str()))
            .cloned()
            .collect(),
    })
}

/// Records the probes of the dry run, and gives the report part. In a check
/// run, each mutant that a dropped test file covers is a violation.
pub(super) fn finish(
    dry_cache: Option<&DryRunCache>,
    outcome: &ScheduleOutcome,
) -> Option<MutationReachSelectionOutput> {
    let reach = outcome.reach.as_ref()?;
    let recorded = match (dry_cache, &outcome.ready) {
        (Some(cache), Some(ready)) => cache.record_reach(ready, &reach.shares),
        _ => 0,
    };
    let dropped: FxHashSet<&str> = reach.selection.dropped.iter().map(String::as_str).collect();
    let mut violations: Vec<MutationReachViolation> = if reach.check {
        outcome
            .results
            .iter()
            .flat_map(|(mutant_id, result)| {
                result.covering_tests.iter().filter_map(|test| {
                    let file = outcome.test_files.get(test)?;
                    dropped
                        .contains(file.as_str())
                        .then(|| MutationReachViolation {
                            test_file: file.clone(),
                            mutant_id: mutant_id.clone(),
                        })
                })
            })
            .collect()
    } else {
        Vec::new()
    };
    violations.sort_by(|left, right| {
        (&left.test_file, &left.mutant_id).cmp(&(&right.test_file, &right.mutant_id))
    });
    violations.dedup();
    Some(MutationReachSelectionOutput {
        check: reach.check,
        dropped: reach.selection.dropped.len(),
        kept: reach.selection.kept.clone(),
        recorded,
        violations,
    })
}
