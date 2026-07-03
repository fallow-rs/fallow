//! Per-group health computation for `--group-by`.
//!
//! Partitions the project's analyzed files by an ownership resolver and
//! produces a [`HealthGroup`] for each bucket. Each group computes its own
//! `VitalSigns` / `HealthScore` from the files in that group, mirroring
//! how `--workspace` already scopes a single subset (`SubsetFilter::Paths`
//! is the underlying primitive in both cases).

use std::path::{Path, PathBuf};

use rustc_hash::{FxHashMap, FxHashSet};

use super::scoring::FileScoreOutput;
use super::{
    HealthGroupResolver, SubsetFilter, VitalSignsAndCountsInput, apply_duplication_metrics,
    compute_vital_signs_and_counts,
};
use crate::vital_signs;
use crate::{discover::FileId, duplicates::DuplicationReport, source::ModuleInfo};
use fallow_output::{
    ComplexityViolation, FileHealthScore, HealthActionsMeta, HealthFinding, HealthGroup,
    HealthGrouping, HotspotEntry, HotspotFinding, LargeFunctionEntry, RefactoringTarget,
    RefactoringTargetFinding, VitalSigns, VitalSignsCounts, summarize_coverage_source_consistency,
};

/// Bucket of file paths sharing a resolver key.
struct GroupBucket {
    key: String,
    owners: Option<Vec<String>>,
    paths: FxHashSet<PathBuf>,
}

pub(super) struct HealthGroupingInput<'a> {
    pub modules: &'a [ModuleInfo],
    pub file_paths: &'a FxHashMap<FileId, &'a PathBuf>,
    pub score_output: Option<&'a FileScoreOutput>,
    pub file_scores: &'a [FileHealthScore],
    pub findings: &'a [ComplexityViolation],
    pub hotspots: &'a [HotspotEntry],
    pub large_functions: &'a [LargeFunctionEntry],
    pub targets: &'a [RefactoringTarget],
    pub score_requested: bool,
    pub dupes_report: Option<&'a DuplicationReport>,
    pub needs_file_scores: bool,
    pub needs_hotspots: bool,
    pub show_vital_signs: bool,
    pub action_ctx: &'a fallow_output::HealthActionContext,
}

/// Build [`HealthGrouping`] for the resolved `--group-by` mode.
///
/// `candidate_paths` is the set of files that already passed
/// workspace / changed-since / ignore filters, that is, the files that
/// contribute to the project-level report. Anything outside this set is
/// dropped before resolution so groups never include files the user has
/// excluded from the run.
pub(super) fn build_health_grouping(
    resolver: &dyn HealthGroupResolver,
    project_root: &Path,
    candidate_paths: &FxHashSet<PathBuf>,
    input: &HealthGroupingInput<'_>,
) -> HealthGrouping {
    let buckets = bucket_paths(resolver, project_root, candidate_paths);

    let groups: Vec<HealthGroup> = buckets
        .into_iter()
        .map(|bucket| build_group(bucket, project_root, input))
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
    resolver: &dyn HealthGroupResolver,
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

fn build_group(
    bucket: GroupBucket,
    project_root: &Path,
    input: &HealthGroupingInput<'_>,
) -> HealthGroup {
    let GroupBucket { key, owners, paths } = bucket;
    let subset = SubsetFilter::Paths(&paths);

    let group_findings = filter_group_items(input.findings, &paths, |finding| &finding.path);
    let group_file_scores = filter_group_items(input.file_scores, &paths, |score| &score.path);
    let group_hotspots = filter_group_items(input.hotspots, &paths, |hotspot| &hotspot.path);
    let group_large_functions =
        filter_group_items(input.large_functions, &paths, |function| &function.path);
    let total_files = paths.len();
    let (vital_signs, _) =
        compute_group_vital_signs(input, &paths, &subset, &group_file_scores, &group_hotspots);
    let health_score = input
        .score_requested
        .then(|| vital_signs::compute_health_score(&vital_signs, total_files));

    let functions_above_threshold = group_findings.len();
    let coverage_source_consistency = summarize_coverage_source_consistency(
        group_findings
            .iter()
            .filter_map(|finding| finding.coverage_source),
    );

    HealthGroup {
        key,
        owners,
        files_analyzed: total_files,
        functions_above_threshold,
        coverage_source_consistency,
        vital_signs: input.show_vital_signs.then_some(vital_signs),
        health_score,
        findings: wrap_group_findings(group_findings, input),
        file_scores: group_file_scores,
        hotspots: wrap_group_hotspots(group_hotspots, project_root),
        large_functions: group_large_functions,
        targets: wrap_group_targets(input.targets, &paths),
        actions_meta: group_actions_meta(input),
    }
}

fn filter_group_items<T: Clone>(
    items: &[T],
    paths: &FxHashSet<PathBuf>,
    path: impl Fn(&T) -> &PathBuf,
) -> Vec<T> {
    items
        .iter()
        .filter(|item| paths.contains(path(item)))
        .cloned()
        .collect()
}

fn compute_group_vital_signs(
    input: &HealthGroupingInput<'_>,
    paths: &FxHashSet<PathBuf>,
    subset: &SubsetFilter<'_>,
    group_file_scores: &[FileHealthScore],
    group_hotspots: &[HotspotEntry],
) -> (VitalSigns, VitalSignsCounts) {
    let vital_signs_input = VitalSignsAndCountsInput {
        score_output: input.score_output,
        modules: input.modules,
        file_paths: input.file_paths,
        needs_file_scores: input.needs_file_scores,
        file_scores_slice: group_file_scores,
        needs_hotspots: input.needs_hotspots,
        hotspots: group_hotspots,
        total_files: paths.len(),
        subset,
    };
    let (mut vital_signs, mut counts) = compute_vital_signs_and_counts(&vital_signs_input);
    apply_group_duplication_metrics(input, paths, &mut vital_signs, &mut counts);
    (vital_signs, counts)
}

fn apply_group_duplication_metrics(
    input: &HealthGroupingInput<'_>,
    paths: &FxHashSet<PathBuf>,
    vital_signs: &mut VitalSigns,
    counts: &mut VitalSignsCounts,
) {
    let Some(report) = input.dupes_report else {
        return;
    };
    let dupes_report = subset_duplication_report(report, input, paths);
    apply_duplication_metrics(vital_signs, counts, &dupes_report);
}

fn subset_duplication_report(
    report: &DuplicationReport,
    input: &HealthGroupingInput<'_>,
    paths: &FxHashSet<PathBuf>,
) -> DuplicationReport {
    let clone_groups = report
        .clone_groups
        .iter()
        .filter_map(|group| {
            let instances = group
                .instances
                .iter()
                .filter(|instance| paths.contains(&instance.file))
                .cloned()
                .collect::<Vec<_>>();
            (instances.len() > 1).then_some(fallow_types::duplicates::CloneGroup {
                instances,
                token_count: group.token_count,
                line_count: group.line_count,
            })
        })
        .collect::<Vec<_>>();
    DuplicationReport {
        stats: subset_duplication_stats(report, input, paths, &clone_groups),
        clone_groups,
        clone_families: Vec::new(),
        mirrored_directories: Vec::new(),
    }
}

fn subset_duplication_stats(
    report: &DuplicationReport,
    input: &HealthGroupingInput<'_>,
    paths: &FxHashSet<PathBuf>,
    clone_groups: &[fallow_types::duplicates::CloneGroup],
) -> fallow_types::duplicates::DuplicationStats {
    let mut files_with_clones: FxHashSet<&Path> = FxHashSet::default();
    let mut file_dup_lines: rustc_hash::FxHashMap<&Path, FxHashSet<usize>> =
        rustc_hash::FxHashMap::default();
    let mut duplicated_tokens = 0usize;
    let mut clone_instances = 0usize;

    for group in clone_groups {
        for instance in &group.instances {
            files_with_clones.insert(&instance.file);
            clone_instances += 1;
            let lines = file_dup_lines.entry(&instance.file).or_default();
            for line in instance.start_line..=instance.end_line {
                lines.insert(line);
            }
        }
        duplicated_tokens += group.token_count * group.instances.len().saturating_sub(1);
    }

    let duplicated_lines = file_dup_lines.values().map(FxHashSet::len).sum::<usize>();
    let total_lines = total_lines_for_paths(input, paths);

    fallow_types::duplicates::DuplicationStats {
        total_files: paths.len(),
        files_with_clones: files_with_clones.len(),
        total_lines,
        duplicated_lines,
        total_tokens: report.stats.total_tokens,
        duplicated_tokens: duplicated_tokens.min(report.stats.total_tokens),
        clone_groups: clone_groups.len(),
        clone_instances,
        duplication_percentage: if total_lines > 0 {
            (duplicated_lines as f64 / total_lines as f64) * 100.0
        } else {
            0.0
        },
        clone_groups_below_min_occurrences: report.stats.clone_groups_below_min_occurrences,
    }
}

fn total_lines_for_paths(input: &HealthGroupingInput<'_>, paths: &FxHashSet<PathBuf>) -> usize {
    input
        .modules
        .iter()
        .filter_map(|module| {
            let path = input.file_paths.get(&module.file_id)?;
            paths.contains(*path).then_some(module.line_offsets.len())
        })
        .sum()
}

fn wrap_group_findings(
    findings: Vec<ComplexityViolation>,
    input: &HealthGroupingInput<'_>,
) -> Vec<HealthFinding> {
    findings
        .into_iter()
        .map(|finding| HealthFinding::with_actions(finding, input.action_ctx))
        .collect()
}

fn wrap_group_hotspots(hotspots: Vec<HotspotEntry>, project_root: &Path) -> Vec<HotspotFinding> {
    hotspots
        .into_iter()
        .map(|hotspot| HotspotFinding::with_actions(hotspot, project_root))
        .collect()
}

fn wrap_group_targets(
    targets: &[RefactoringTarget],
    paths: &FxHashSet<PathBuf>,
) -> Vec<RefactoringTargetFinding> {
    filter_group_items(targets, paths, |target| &target.path)
        .into_iter()
        .map(RefactoringTargetFinding::with_actions)
        .collect()
}

fn group_actions_meta(input: &HealthGroupingInput<'_>) -> Option<HealthActionsMeta> {
    input
        .action_ctx
        .opts
        .omit_suppress_line
        .then(|| HealthActionsMeta {
            suppression_hints_omitted: true,
            reason: input
                .action_ctx
                .opts
                .omit_reason
                .unwrap_or("unspecified")
                .to_string(),
            scope: "health-findings".to_string(),
        })
}
