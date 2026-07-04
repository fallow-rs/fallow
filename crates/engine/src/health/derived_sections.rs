use std::time::Instant;

use fallow_config::ResolvedConfig;
use fallow_output::{FileHealthScore, HotspotEntry, HotspotSummary, RefactoringTarget};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::baseline::{HealthBaselineData, filter_new_health_targets};
use crate::duplicates::{DuplicationReport, DuplicationStats};

use super::HealthExecutionOptions;
use super::filters::{
    collect_candidate_paths, filter_files_to_paths, filter_hotspots_by_diff,
    filter_refactoring_targets_by_diff,
};
use super::hotspots::{self, HotspotComputationInput, compute_hotspots};
use super::scoring;
use super::targets::{self, TargetAuxData, compute_refactoring_targets};

pub struct HealthDerivedSectionInput<'a> {
    pub(crate) config: &'a ResolvedConfig,
    pub(crate) files: &'a [fallow_types::discover::DiscoveredFile],
    pub(crate) modules: &'a [crate::source::ModuleInfo],
    pub(crate) file_paths:
        &'a rustc_hash::FxHashMap<crate::discover::FileId, &'a std::path::PathBuf>,
    pub(crate) ignore_set: &'a globset::GlobSet,
    pub(crate) changed_files: Option<&'a rustc_hash::FxHashSet<std::path::PathBuf>>,
    pub(crate) ws_roots: Option<&'a [std::path::PathBuf]>,
    pub(crate) file_scores: &'a [FileHealthScore],
    pub(crate) churn_fetch: Option<hotspots::ChurnFetchResult>,
    pub(crate) diff_index: Option<&'a fallow_output::DiffIndex>,
    pub(crate) score_output: Option<&'a scoring::FileScoreOutput>,
    pub(crate) loaded_baseline: Option<&'a HealthBaselineData>,
    pub(crate) pre_computed_duplication: Option<DuplicationReport>,
}

pub struct HealthDerivedSections {
    pub(crate) candidate_paths: rustc_hash::FxHashSet<std::path::PathBuf>,
    pub(crate) dupes_report: Option<crate::duplicates::DuplicationReport>,
    pub(crate) duplication_ms: f64,
    pub(crate) hotspots: Vec<HotspotEntry>,
    pub(crate) hotspot_summary: Option<HotspotSummary>,
    pub(crate) hotspots_ms: f64,
    pub(crate) targets: Vec<RefactoringTarget>,
    pub(crate) target_thresholds: Option<fallow_output::TargetThresholds>,
    pub(crate) targets_ms: f64,
}

pub fn prepare_health_derived_sections(
    opts: &HealthExecutionOptions<'_>,
    mut input: HealthDerivedSectionInput<'_>,
) -> HealthDerivedSections {
    let pre_computed_duplication = input.pre_computed_duplication.take();
    let (candidate_paths, dupes_report, duplication_ms) =
        prepare_health_section_dupes(opts, &input, pre_computed_duplication);
    let (hotspots, hotspot_summary, hotspots_ms) = prepare_health_section_hotspots(
        opts,
        HealthHotspotSectionInput {
            config: input.config,
            file_scores: input.file_scores,
            ignore_set: input.ignore_set,
            ws_roots: input.ws_roots,
            churn_fetch: input.churn_fetch,
            diff_index: input.diff_index,
        },
    );
    let (targets, target_thresholds, targets_ms) = prepare_health_section_targets(
        opts,
        &HealthTargetSectionInput {
            score_output: input.score_output,
            file_scores: input.file_scores,
            hotspots: &hotspots,
            loaded_baseline: input.loaded_baseline,
            config: input.config,
            diff_index: input.diff_index,
            dupes_report: dupes_report.as_ref(),
        },
    );

    HealthDerivedSections {
        candidate_paths,
        dupes_report,
        duplication_ms,
        hotspots,
        hotspot_summary,
        hotspots_ms,
        targets,
        target_thresholds,
        targets_ms,
    }
}

fn prepare_health_section_dupes(
    opts: &HealthExecutionOptions<'_>,
    input: &HealthDerivedSectionInput<'_>,
    pre_computed_duplication: Option<DuplicationReport>,
) -> (
    rustc_hash::FxHashSet<std::path::PathBuf>,
    Option<crate::duplicates::DuplicationReport>,
    f64,
) {
    prepare_health_duplication_data(HealthDuplicationDataInput {
        opts,
        config: input.config,
        files: input.files,
        modules: input.modules,
        file_paths: input.file_paths,
        changed_files: input.changed_files,
        ws_roots: input.ws_roots,
        ignore_set: input.ignore_set,
        pre_computed_duplication,
    })
}

struct HealthHotspotSectionInput<'a> {
    config: &'a ResolvedConfig,
    file_scores: &'a [FileHealthScore],
    ignore_set: &'a globset::GlobSet,
    ws_roots: Option<&'a [std::path::PathBuf]>,
    churn_fetch: Option<hotspots::ChurnFetchResult>,
    diff_index: Option<&'a fallow_output::DiffIndex>,
}

fn prepare_health_section_hotspots(
    opts: &HealthExecutionOptions<'_>,
    input: HealthHotspotSectionInput<'_>,
) -> (Vec<HotspotEntry>, Option<HotspotSummary>, f64) {
    compute_filtered_hotspots(FilteredHotspotInput {
        opts,
        config: input.config,
        file_scores_slice: input.file_scores,
        ignore_set: input.ignore_set,
        ws_roots: input.ws_roots,
        churn_fetch: input.churn_fetch,
        diff_index: input.diff_index,
    })
}

struct HealthTargetSectionInput<'a> {
    score_output: Option<&'a scoring::FileScoreOutput>,
    file_scores: &'a [FileHealthScore],
    hotspots: &'a [HotspotEntry],
    loaded_baseline: Option<&'a HealthBaselineData>,
    config: &'a ResolvedConfig,
    diff_index: Option<&'a fallow_output::DiffIndex>,
    dupes_report: Option<&'a crate::duplicates::DuplicationReport>,
}

fn prepare_health_section_targets(
    opts: &HealthExecutionOptions<'_>,
    input: &HealthTargetSectionInput<'_>,
) -> (
    Vec<RefactoringTarget>,
    Option<fallow_output::TargetThresholds>,
    f64,
) {
    compute_filtered_targets(FilteredTargetInput {
        opts,
        score_output: input.score_output,
        file_scores_slice: input.file_scores,
        hotspots: input.hotspots,
        loaded_baseline: input.loaded_baseline,
        config: input.config,
        diff_index: input.diff_index,
        dupes_report: input.dupes_report,
    })
}

struct FilteredHotspotInput<'a> {
    opts: &'a HealthExecutionOptions<'a>,
    config: &'a ResolvedConfig,
    file_scores_slice: &'a [FileHealthScore],
    ignore_set: &'a globset::GlobSet,
    ws_roots: Option<&'a [std::path::PathBuf]>,
    churn_fetch: Option<hotspots::ChurnFetchResult>,
    diff_index: Option<&'a fallow_output::DiffIndex>,
}

fn compute_filtered_hotspots(
    input: FilteredHotspotInput<'_>,
) -> (Vec<HotspotEntry>, Option<HotspotSummary>, f64) {
    let t = Instant::now();
    let (mut hotspots, hotspot_summary) = if let Some(churn_data) = input.churn_fetch {
        compute_hotspots(HotspotComputationInput {
            opts: input.opts,
            config: input.config,
            file_scores: input.file_scores_slice,
            ignore_set: input.ignore_set,
            ws_roots: input.ws_roots,
            churn_fetch: churn_data,
        })
    } else {
        (Vec::new(), None)
    };
    if let Some(diff_index) = input.diff_index {
        filter_hotspots_by_diff(&mut hotspots, diff_index, &input.config.root);
    }
    (
        hotspots,
        hotspot_summary,
        t.elapsed().as_secs_f64() * 1000.0,
    )
}

#[derive(Clone, Copy)]
struct FilteredTargetInput<'a> {
    opts: &'a HealthExecutionOptions<'a>,
    score_output: Option<&'a scoring::FileScoreOutput>,
    file_scores_slice: &'a [FileHealthScore],
    hotspots: &'a [HotspotEntry],
    loaded_baseline: Option<&'a HealthBaselineData>,
    config: &'a ResolvedConfig,
    diff_index: Option<&'a fallow_output::DiffIndex>,
    dupes_report: Option<&'a crate::duplicates::DuplicationReport>,
}

fn compute_filtered_targets(
    input: FilteredTargetInput<'_>,
) -> (
    Vec<RefactoringTarget>,
    Option<fallow_output::TargetThresholds>,
    f64,
) {
    let t = Instant::now();
    let (mut targets, target_thresholds) = compute_targets(&input);
    if let Some(diff_index) = input.diff_index {
        filter_refactoring_targets_by_diff(&mut targets, diff_index, &input.config.root);
    }
    (
        targets,
        target_thresholds,
        t.elapsed().as_secs_f64() * 1000.0,
    )
}

struct HealthDuplicationDataInput<'a> {
    opts: &'a HealthExecutionOptions<'a>,
    config: &'a ResolvedConfig,
    files: &'a [fallow_types::discover::DiscoveredFile],
    modules: &'a [crate::source::ModuleInfo],
    file_paths: &'a rustc_hash::FxHashMap<crate::discover::FileId, &'a std::path::PathBuf>,
    changed_files: Option<&'a rustc_hash::FxHashSet<std::path::PathBuf>>,
    ws_roots: Option<&'a [std::path::PathBuf]>,
    ignore_set: &'a globset::GlobSet,
    pre_computed_duplication: Option<DuplicationReport>,
}

fn prepare_health_duplication_data(
    input: HealthDuplicationDataInput<'_>,
) -> (
    rustc_hash::FxHashSet<std::path::PathBuf>,
    Option<crate::duplicates::DuplicationReport>,
    f64,
) {
    let candidate_paths = collect_candidate_paths(
        input.files,
        input.config,
        input.changed_files,
        input.ws_roots,
        input.ignore_set,
    );
    let (dupes_report, duplication_ms) =
        compute_health_duplication_report(HealthDuplicationReportInput {
            opts: input.opts,
            config: input.config,
            files: input.files,
            modules: input.modules,
            file_paths: input.file_paths,
            candidate_paths: &candidate_paths,
            pre_computed_duplication: input.pre_computed_duplication,
        });
    (candidate_paths, dupes_report, duplication_ms)
}

struct HealthDuplicationReportInput<'a> {
    opts: &'a HealthExecutionOptions<'a>,
    config: &'a ResolvedConfig,
    files: &'a [fallow_types::discover::DiscoveredFile],
    modules: &'a [crate::source::ModuleInfo],
    file_paths: &'a rustc_hash::FxHashMap<crate::discover::FileId, &'a std::path::PathBuf>,
    candidate_paths: &'a rustc_hash::FxHashSet<std::path::PathBuf>,
    pre_computed_duplication: Option<DuplicationReport>,
}

fn compute_health_duplication_report(
    input: HealthDuplicationReportInput<'_>,
) -> (Option<crate::duplicates::DuplicationReport>, f64) {
    let t = Instant::now();
    let dupes_report = if input.opts.score || input.opts.targets {
        if let Some(report) = input.pre_computed_duplication {
            return (
                Some(subset_precomputed_duplication_report(
                    report,
                    input.modules,
                    input.file_paths,
                    input.candidate_paths,
                )),
                t.elapsed().as_secs_f64() * 1000.0,
            );
        }
        let scoped_files = filter_files_to_paths(input.files, input.candidate_paths);
        Some(if input.opts.no_cache {
            crate::duplicates::find_duplicates(
                &input.config.root,
                &scoped_files,
                &input.config.duplicates,
            )
        } else {
            crate::duplicates::find_duplicates_cached(
                &input.config.root,
                &scoped_files,
                &input.config.duplicates,
                &input.config.cache_dir,
            )
        })
    } else {
        None
    };
    (dupes_report, t.elapsed().as_secs_f64() * 1000.0)
}

fn subset_precomputed_duplication_report(
    report: DuplicationReport,
    modules: &[crate::source::ModuleInfo],
    file_paths: &rustc_hash::FxHashMap<crate::discover::FileId, &std::path::PathBuf>,
    candidate_paths: &FxHashSet<std::path::PathBuf>,
) -> DuplicationReport {
    let DuplicationReport {
        clone_groups,
        stats: original_stats,
        ..
    } = report;
    let clone_groups = clone_groups
        .into_iter()
        .filter_map(|group| {
            let instances = group
                .instances
                .into_iter()
                .filter(|instance| candidate_paths.contains(&instance.file))
                .collect::<Vec<_>>();
            (instances.len() > 1).then_some(fallow_types::duplicates::CloneGroup {
                instances,
                token_count: group.token_count,
                line_count: group.line_count,
            })
        })
        .collect::<Vec<_>>();
    let stats = subset_precomputed_duplication_stats(
        &original_stats,
        modules,
        file_paths,
        candidate_paths,
        &clone_groups,
    );
    DuplicationReport {
        clone_groups,
        clone_families: Vec::new(),
        mirrored_directories: Vec::new(),
        stats,
    }
}

fn subset_precomputed_duplication_stats(
    original: &DuplicationStats,
    modules: &[crate::source::ModuleInfo],
    file_paths: &rustc_hash::FxHashMap<crate::discover::FileId, &std::path::PathBuf>,
    candidate_paths: &FxHashSet<std::path::PathBuf>,
    clone_groups: &[fallow_types::duplicates::CloneGroup],
) -> DuplicationStats {
    let mut files_with_clones: FxHashSet<&std::path::Path> = FxHashSet::default();
    let mut file_dup_lines: FxHashMap<&std::path::Path, FxHashSet<usize>> = FxHashMap::default();
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

    let total_files = candidate_paths.len();
    let total_lines = total_lines_for_candidate_paths(modules, file_paths, candidate_paths);
    let duplicated_lines = file_dup_lines.values().map(FxHashSet::len).sum::<usize>();

    DuplicationStats {
        total_files,
        files_with_clones: files_with_clones.len(),
        total_lines,
        duplicated_lines,
        total_tokens: original.total_tokens,
        duplicated_tokens: duplicated_tokens.min(original.total_tokens),
        clone_groups: clone_groups.len(),
        clone_instances,
        duplication_percentage: if total_lines > 0 {
            (duplicated_lines as f64 / total_lines as f64) * 100.0
        } else {
            0.0
        },
        clone_groups_below_min_occurrences: original.clone_groups_below_min_occurrences,
    }
}

fn total_lines_for_candidate_paths(
    modules: &[crate::source::ModuleInfo],
    file_paths: &rustc_hash::FxHashMap<crate::discover::FileId, &std::path::PathBuf>,
    candidate_paths: &FxHashSet<std::path::PathBuf>,
) -> usize {
    modules
        .iter()
        .filter_map(|module| {
            let path = file_paths.get(&module.file_id)?;
            candidate_paths
                .contains(*path)
                .then_some(module.line_offsets.len())
        })
        .sum()
}

/// Compute refactoring targets when requested, applying baseline and top filters.
fn compute_targets(
    input: &FilteredTargetInput<'_>,
) -> (
    Vec<RefactoringTarget>,
    Option<fallow_output::TargetThresholds>,
) {
    if !input.opts.targets {
        return (Vec::new(), None);
    }
    let Some(output) = input.score_output else {
        return (Vec::new(), None);
    };
    let clone_siblings = input
        .dupes_report
        .map_or_else(rustc_hash::FxHashMap::default, |report| {
            targets::build_clone_sibling_evidence(report)
        });
    let target_aux = TargetAuxData::from_output(output, &clone_siblings);
    let (mut tgts, thresholds) =
        compute_refactoring_targets(input.file_scores_slice, &target_aux, input.hotspots);
    if let Some(baseline) = input.loaded_baseline {
        tgts = filter_new_health_targets(tgts, baseline, &input.config.root);
    }
    if let Some(ref effort) = input.opts.effort {
        tgts.retain(|t| t.effort == *effort);
    }
    if let Some(top) = input.opts.top {
        tgts.truncate(top);
    }
    (tgts, Some(thresholds))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use fallow_types::duplicates::{CloneGroup, CloneInstance};

    use super::*;

    #[test]
    fn subset_precomputed_duplication_stats_match_detector_token_model() {
        let path_a = PathBuf::from("/repo/src/a.ts");
        let path_b = PathBuf::from("/repo/src/b.ts");
        let clone_groups = vec![CloneGroup {
            instances: vec![clone_instance(&path_a), clone_instance(&path_b)],
            token_count: 25,
            line_count: 3,
        }];
        let mut candidate_paths = FxHashSet::default();
        candidate_paths.insert(path_a);
        candidate_paths.insert(path_b);

        let stats = subset_precomputed_duplication_stats(
            &DuplicationStats {
                total_tokens: 100,
                ..DuplicationStats::default()
            },
            &[],
            &FxHashMap::default(),
            &candidate_paths,
            &clone_groups,
        );

        assert_eq!(stats.duplicated_tokens, 25);
        assert_eq!(stats.clone_instances, 2);
    }

    fn clone_instance(file: &std::path::Path) -> CloneInstance {
        CloneInstance {
            file: file.to_path_buf(),
            start_line: 1,
            end_line: 3,
            start_col: 0,
            end_col: 1,
            fragment: "const value = 1;".to_string(),
        }
    }
}
