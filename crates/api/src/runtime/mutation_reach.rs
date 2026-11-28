//! The reach selection of a `fallow mutation` run: the probed run files, the
//! scheduler config, and the report part (see `dry_run_cache::reach`).

use std::path::Path;
use std::sync::Arc;

use fallow_engine::mutation::MutationPlan;
use fallow_output::{
    MutationReachSelectionMode, MutationReachSelectionOutput, MutationReachViolation,
};
use rustc_hash::FxHashSet;

use crate::mutation::ReachSelectionMode;
use crate::mutation::dry_run_cache::{DryRunCache, ReachFacts};
use crate::mutation::run_dir::RunFile;
use crate::mutation::scheduler::{ReachConfig, ReachLookup, ScheduleIssue, ScheduleOutcome};

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

/// The reach selection that the caller asked for, and why the run does not
/// select by reach when the run turns it off.
#[derive(Debug, Clone, Copy)]
pub(super) struct ReachRequest {
    pub mode: ReachSelectionMode,
    pub disabled: Option<&'static str>,
}

impl ReachRequest {
    pub(super) fn new(mode: ReachSelectionMode, targets: bool) -> Self {
        Self {
            mode,
            disabled: (mode != ReachSelectionMode::Off && targets).then_some("kill-matrix-targets"),
        }
    }
}

/// Records the probes of the dry run, and gives the report part. In a check
/// run, each mutant that a dropped test file covers is a violation, and the
/// run gets a `reach-selection-violation` issue.
pub(super) fn finish(
    request: ReachRequest,
    dry_cache: Option<&DryRunCache>,
    outcome: &mut ScheduleOutcome,
) -> Option<MutationReachSelectionOutput> {
    let mode = match request.mode {
        ReachSelectionMode::Off => return None,
        ReachSelectionMode::On => MutationReachSelectionMode::On,
        ReachSelectionMode::Check => MutationReachSelectionMode::Check,
    };
    let Some(reach) = outcome.reach.as_ref() else {
        return request.disabled.map(|reason| MutationReachSelectionOutput {
            mode,
            disabled: Some(reason.to_owned()),
            dropped: 0,
            dropped_files: Vec::new(),
            kept: std::collections::BTreeMap::new(),
            recorded: 0,
            violations: Vec::new(),
        });
    };
    let recorded = match (dry_cache, &outcome.ready) {
        (Some(cache), Some(ready)) => cache.record_reach(ready, &reach.shares),
        _ => 0,
    };
    let violations = if reach.check {
        violations(reach.selection.dropped.as_slice(), outcome)
    } else {
        Vec::new()
    };
    let output = MutationReachSelectionOutput {
        mode,
        disabled: reach.selection.disabled.map(str::to_owned),
        dropped: reach.selection.dropped.len(),
        dropped_files: reach.selection.dropped.clone(),
        kept: reach.selection.kept.clone(),
        recorded,
        violations,
    };
    if let Some(first) = output.violations.first() {
        outcome.issues.push(ScheduleIssue {
            code: "reach-selection-violation",
            message: format!(
                "the reach record drops {} although a test of it covers mutant {}, and {} more such pairs. Report it at https://github.com/fallow-rs/fallow/issues with the `cache.reach_selection` object of the JSON report",
                first.test_file,
                first.mutant_id,
                output.violations.len() - 1
            ),
            mutant_id: Some(first.mutant_id.clone()),
            stderr_tail: None,
        });
    }
    Some(output)
}

/// Each mutant that a test of a `dropped` file covers, sorted.
fn violations(dropped: &[String], outcome: &ScheduleOutcome) -> Vec<MutationReachViolation> {
    let dropped: FxHashSet<&str> = dropped.iter().map(String::as_str).collect();
    let mut violations: Vec<MutationReachViolation> = outcome
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
        .collect();
    violations.sort_by(|left, right| {
        (&left.test_file, &left.mutant_id).cmp(&(&right.test_file, &right.mutant_id))
    });
    violations.dedup();
    violations
}
