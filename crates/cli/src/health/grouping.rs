//! Per-group health computation for `--group-by`.
//!
//! Partitions the project's analyzed files by an [`OwnershipResolver`] and
//! produces a [`HealthGroup`] for each bucket. Each group computes its own
//! `VitalSigns` / `HealthScore` from the files in that group, mirroring
//! how `--workspace` already scopes a single subset (`SubsetFilter::Paths`
//! is the underlying primitive in both cases).

use std::path::{Path, PathBuf};

use rustc_hash::{FxHashMap, FxHashSet};

use super::scoring::FileScoreOutput;
use super::{SubsetFilter, apply_duplication_metrics, compute_vital_signs_and_counts};
use crate::health_types::{
    ComplexityViolation, FileHealthScore, HealthGroup, HealthGrouping, HotspotEntry,
    LargeFunctionEntry, RefactoringTarget,
};
use crate::report::OwnershipResolver;
use crate::vital_signs;

/// Bucket of file paths sharing a resolver key.
struct GroupBucket {
    key: String,
    owners: Option<Vec<String>>,
    paths: FxHashSet<PathBuf>,
}

/// Build [`HealthGrouping`] for the resolved `--group-by` mode.
///
/// `candidate_paths` is the set of files that already passed
/// workspace / changed-since / ignore filters, that is, the files that
/// contribute to the project-level report. Anything outside this set is
/// dropped before resolution so groups never include files the user has
/// excluded from the run.
#[expect(
    clippy::too_many_arguments,
    reason = "build_health_grouping aggregates the full health pipeline state into per-group sub-reports"
)]
pub(super) fn build_health_grouping(
    resolver: &OwnershipResolver,
    project_root: &Path,
    files: &[fallow_types::discover::DiscoveredFile],
    modules: &[fallow_core::extract::ModuleInfo],
    file_paths: &FxHashMap<fallow_core::discover::FileId, &PathBuf>,
    candidate_paths: &FxHashSet<PathBuf>,
    score_output: Option<&FileScoreOutput>,
    file_scores: &[FileHealthScore],
    findings: &[ComplexityViolation],
    hotspots: &[HotspotEntry],
    large_functions: &[LargeFunctionEntry],
    targets: &[RefactoringTarget],
    score_requested: bool,
    duplicates_config: Option<&fallow_config::DuplicatesConfig>,
    needs_file_scores: bool,
    needs_hotspots: bool,
    show_vital_signs: bool,
    action_ctx: &crate::health_types::HealthActionContext,
) -> HealthGrouping {
    let buckets = bucket_paths(resolver, project_root, candidate_paths);

    let groups: Vec<HealthGroup> = buckets
        .into_iter()
        .map(|bucket| {
            build_group(
                bucket,
                project_root,
                files,
                modules,
                file_paths,
                score_output,
                file_scores,
                findings,
                hotspots,
                large_functions,
                targets,
                score_requested,
                duplicates_config,
                needs_file_scores,
                needs_hotspots,
                show_vital_signs,
                action_ctx,
            )
        })
        .collect();

    HealthGrouping {
        mode: resolver.mode_label(),
        groups,
    }
}

/// Bucket every candidate path by the resolver key.
///
/// Output is sorted by descending file count with the unowned bucket pushed
/// last (matches the `dead-code` grouped output's ordering convention so that
/// human / JSON consumers see the same row ordering across analyses).
fn bucket_paths(
    resolver: &OwnershipResolver,
    project_root: &Path,
    candidate_paths: &FxHashSet<PathBuf>,
) -> Vec<GroupBucket> {
    let mut by_key: FxHashMap<String, GroupBucket> = FxHashMap::default();
    for path in candidate_paths {
        let rel = path.strip_prefix(project_root).unwrap_or(path);
        let (key, _rule) = resolver.resolve_with_rule(rel);
        let entry = by_key.entry(key.clone()).or_insert_with(|| GroupBucket {
            key: key.clone(),
            owners: resolver.section_owners_of(rel).map(<[_]>::to_vec),
            paths: FxHashSet::default(),
        });
        entry.paths.insert(path.clone());
    }
    let mut out: Vec<GroupBucket> = by_key.into_values().collect();
    out.sort_by(|a, b| {
        let unowned_a = is_unowned_label(&a.key);
        let unowned_b = is_unowned_label(&b.key);
        match (unowned_a, unowned_b) {
            (true, false) => std::cmp::Ordering::Greater,
            (false, true) => std::cmp::Ordering::Less,
            _ => b.paths.len().cmp(&a.paths.len()).then(a.key.cmp(&b.key)),
        }
    });
    out
}

fn is_unowned_label(key: &str) -> bool {
    key == crate::codeowners::UNOWNED_LABEL
}

#[expect(
    clippy::too_many_arguments,
    reason = "per-group computation reads the full health pipeline state"
)]
fn build_group(
    bucket: GroupBucket,
    project_root: &Path,
    files: &[fallow_types::discover::DiscoveredFile],
    modules: &[fallow_core::extract::ModuleInfo],
    file_paths: &FxHashMap<fallow_core::discover::FileId, &PathBuf>,
    score_output: Option<&FileScoreOutput>,
    file_scores: &[FileHealthScore],
    findings: &[ComplexityViolation],
    hotspots: &[HotspotEntry],
    large_functions: &[LargeFunctionEntry],
    targets: &[RefactoringTarget],
    score_requested: bool,
    duplicates_config: Option<&fallow_config::DuplicatesConfig>,
    needs_file_scores: bool,
    needs_hotspots: bool,
    show_vital_signs: bool,
    action_ctx: &crate::health_types::HealthActionContext,
) -> HealthGroup {
    let GroupBucket { key, owners, paths } = bucket;
    let subset = SubsetFilter::Paths(&paths);

    let group_findings: Vec<ComplexityViolation> = findings
        .iter()
        .filter(|f| paths.contains(&f.path))
        .cloned()
        .collect();
    let group_file_scores: Vec<FileHealthScore> = file_scores
        .iter()
        .filter(|s| paths.contains(&s.path))
        .cloned()
        .collect();
    let group_hotspots: Vec<HotspotEntry> = hotspots
        .iter()
        .filter(|h| paths.contains(&h.path))
        .cloned()
        .collect();
    let group_large_functions: Vec<LargeFunctionEntry> = large_functions
        .iter()
        .filter(|l| paths.contains(&l.path))
        .cloned()
        .collect();
    let group_targets: Vec<RefactoringTarget> = targets
        .iter()
        .filter(|t| paths.contains(&t.path))
        .cloned()
        .collect();

    let total_files = paths.len();
    let (mut vital_signs, mut counts) = compute_vital_signs_and_counts(
        score_output,
        modules,
        file_paths,
        needs_file_scores,
        &group_file_scores,
        needs_hotspots,
        &group_hotspots,
        total_files,
        &subset,
    );
    if let Some(config) = duplicates_config {
        let group_files: Vec<fallow_types::discover::DiscoveredFile> = files
            .iter()
            .filter(|file| paths.contains(&file.path))
            .cloned()
            .collect();
        let dupes_report =
            fallow_core::duplicates::find_duplicates(project_root, &group_files, config);
        apply_duplication_metrics(&mut vital_signs, &mut counts, &dupes_report);
    }
    let health_score =
        score_requested.then(|| vital_signs::compute_health_score(&vital_signs, total_files));

    let functions_above_threshold = group_findings.len();
    let wrapped_findings: Vec<crate::health_types::HealthFinding> = group_findings
        .into_iter()
        .map(|v| crate::health_types::HealthFinding::with_actions(v, action_ctx))
        .collect();

    HealthGroup {
        key,
        owners,
        files_analyzed: total_files,
        functions_above_threshold,
        vital_signs: show_vital_signs.then_some(vital_signs),
        health_score,
        findings: wrapped_findings,
        file_scores: group_file_scores,
        hotspots: group_hotspots,
        large_functions: group_large_functions,
        targets: group_targets,
        actions_meta: if action_ctx.opts.omit_suppress_line {
            Some(crate::health_types::HealthActionsMeta {
                suppression_hints_omitted: true,
                reason: action_ctx
                    .opts
                    .omit_reason
                    .unwrap_or("unspecified")
                    .to_string(),
                scope: "health-findings".to_string(),
            })
        } else {
            None
        },
    }
}
