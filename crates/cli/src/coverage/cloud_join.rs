//! The join of Fallow Cloud runtime functions with the local AST in
//! `coverage analyze --cloud`: which cloud functions match, and the gate that
//! caps `safe_to_delete` when the local graph misses too many of them.

use fallow_output::{
    RuntimeCoverageConfidence, RuntimeCoverageFinding, RuntimeCoverageHotPath,
    RuntimeCoverageMessage, RuntimeCoverageVerdict,
};

use super::analyze::{
    StaticFunctionInfo, StaticIndex, cloud_finding, cloud_finding_decision, cloud_observation_days,
    collect_called_cloud_function, match_cloud_function,
};
use super::cloud_client::{CloudRuntimeContext, CloudRuntimeFunction, CloudTrackingState};
use super::cloud_rows;

/// Share of cloud runtime functions that must have a local AST counterpart
/// before the join can issue `safe_to_delete`. Below it, the local graph
/// misses too much of what runs (another branch, a partial checkout, a path
/// mismatch), so its "statically unused" answer is not trusted on its own.
const CLOUD_JOIN_MIN_MATCH_PERCENT: usize = 80;

/// How many cloud runtime functions the local AST matched.
#[derive(Debug, Clone, Copy)]
pub(super) struct CloudJoin {
    pub(super) total: usize,
    pub(super) matched: usize,
}

impl CloudJoin {
    /// True below `CLOUD_JOIN_MIN_MATCH_PERCENT`. Integer math, so exactly
    /// the limit is complete.
    fn incomplete(self) -> bool {
        self.matched * 100 < self.total * CLOUD_JOIN_MIN_MATCH_PERCENT
    }

    /// The warning when the join capped at least one finding.
    pub(super) fn warning(self, capped_findings: usize) -> Option<RuntimeCoverageMessage> {
        (self.incomplete() && capped_findings > 0).then(|| RuntimeCoverageMessage {
            code: "cloud_local_join_incomplete".to_owned(),
            message: format!(
                "Only {} of {} cloud function(s) matched the local AST, below the {CLOUD_JOIN_MIN_MATCH_PERCENT}% that a safe_to_delete verdict needs. {capped_findings} finding(s) are review_required instead. Run the analysis on the deployed commit, or check the path prefix of the runtime paths.",
                self.matched, self.total,
            ),
        })
    }
}

/// Caps `safe_to_delete` at `review_required` with medium confidence when the
/// local AST matched too few cloud functions (`CloudJoin::incomplete`). The
/// "statically unused" input of that verdict comes from the local graph, so
/// the verdict cannot be more certain than that graph is complete.
pub(super) fn cap_for_incomplete_join(
    decision: (
        RuntimeCoverageVerdict,
        RuntimeCoverageConfidence,
        Option<u64>,
    ),
    join_incomplete: bool,
) -> (
    RuntimeCoverageVerdict,
    RuntimeCoverageConfidence,
    Option<u64>,
) {
    match decision {
        (RuntimeCoverageVerdict::SafeToDelete, _, invocations) if join_incomplete => (
            RuntimeCoverageVerdict::ReviewRequired,
            RuntimeCoverageConfidence::Medium,
            invocations,
        ),
        other => other,
    }
}

pub(super) struct CloudMergeEntries {
    pub(super) findings: Vec<RuntimeCoverageFinding>,
    pub(super) hot_paths: Vec<RuntimeCoverageHotPath>,
    pub(super) synthesized_blast_radius: Vec<fallow_output::RuntimeCoverageBlastRadiusEntry>,
    pub(super) synthesized_importance:
        Vec<(fallow_output::RuntimeCoverageImportanceEntry, Option<u32>)>,
    pub(super) unmatched_cloud_functions: Vec<UnmatchedCloudFunction>,
    /// Local AST structure of each matched function that ran, for the
    /// cloud blast-radius and importance rows.
    pub(super) local_structure: cloud_rows::LocalStructureIndex,
    pub(super) join: CloudJoin,
    /// `safe_to_delete` findings that the incomplete join capped.
    pub(super) capped_findings: usize,
}

/// One cloud runtime function with no local counterpart, kept so the drop is
/// inspectable instead of only counted.
#[derive(Debug, Clone)]
pub struct UnmatchedCloudFunction {
    pub path: String,
    pub name: String,
    pub line: Option<u32>,
    pub invocations: u64,
}

pub(super) fn collect_cloud_merge_entries(
    snapshot: &CloudRuntimeContext,
    static_index: &StaticIndex,
    min_invocations_hot: u64,
) -> CloudMergeEntries {
    let matches: Vec<(&CloudRuntimeFunction, Option<StaticFunctionInfo>)> = snapshot
        .functions
        .iter()
        .map(|function| (function, match_cloud_function(function, static_index)))
        .collect();
    let join = CloudJoin {
        total: matches.len(),
        matched: matches.iter().filter(|(_, local)| local.is_some()).count(),
    };
    let mut entries = CloudMergeEntries {
        findings: Vec::new(),
        hot_paths: Vec::new(),
        synthesized_blast_radius: Vec::new(),
        synthesized_importance: Vec::new(),
        unmatched_cloud_functions: Vec::new(),
        local_structure: cloud_rows::LocalStructureIndex::default(),
        join,
        capped_findings: 0,
    };
    for (function, local) in matches {
        let Some(local) = local else {
            let line = function.start_line.or(function.line_number);
            tracing::debug!(
                path = %function.file_path,
                function = %function.function_name,
                line = ?line,
                invocations = function.hit_count.unwrap_or(0),
                "cloud runtime function has no local counterpart; omitted from findings"
            );
            entries
                .unmatched_cloud_functions
                .push(UnmatchedCloudFunction {
                    path: function.file_path.clone(),
                    name: function.function_name.clone(),
                    line,
                    invocations: function.hit_count.unwrap_or(0),
                });
            continue;
        };
        // The cloud builds a blast-radius and importance row for each
        // function with hits, whatever its tracking state.
        if let Some(invocations) = function.hit_count.filter(|hits| *hits > 0) {
            entries.local_structure.insert(
                function.stable_id.as_deref(),
                &function.file_path,
                &function.function_name,
                function.start_line.or(function.line_number).unwrap_or(0),
                cloud_rows::LocalStructure {
                    caller_count: local.caller_count,
                    cyclomatic: local.cyclomatic,
                    owner_count: local.owner_count,
                    invocations,
                },
            );
        }
        if matches!(function.tracking_state, CloudTrackingState::Called) {
            collect_called_cloud_function(&mut entries, function, &local, min_invocations_hot);
        } else {
            if join.incomplete()
                && cloud_finding_decision(function, &local).0
                    == RuntimeCoverageVerdict::SafeToDelete
            {
                entries.capped_findings += 1;
            }
            entries.findings.push(cloud_finding(
                function,
                &local,
                cloud_observation_days(snapshot, function),
                join.incomplete(),
            ));
        }
    }
    entries
}

#[cfg(test)]
#[path = "cloud_join_tests.rs"]
mod tests;
