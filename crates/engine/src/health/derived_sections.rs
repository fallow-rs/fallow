use std::time::Instant;

use fallow_config::ResolvedConfig;
use fallow_output::{FileHealthScore, HotspotEntry, HotspotSummary, RefactoringTarget};

use crate::baseline::{HealthBaselineData, filter_new_health_targets};

use super::HealthExecutionOptions;
use super::filters::{
    collect_candidate_paths, filter_files_to_paths, filter_hotspots_by_diff,
    filter_refactoring_targets_by_diff,
};
use super::hotspots::{self, compute_hotspots};
use super::scoring;
use super::targets::{self, TargetAuxData, compute_refactoring_targets};

pub struct HealthDerivedSectionInput<'a> {
    pub(crate) config: &'a ResolvedConfig,
    pub(crate) files: &'a [fallow_types::discover::DiscoveredFile],
    pub(crate) ignore_set: &'a globset::GlobSet,
    pub(crate) changed_files: Option<&'a rustc_hash::FxHashSet<std::path::PathBuf>>,
    pub(crate) ws_roots: Option<&'a [std::path::PathBuf]>,
    pub(crate) file_scores: &'a [FileHealthScore],
    pub(crate) churn_fetch: Option<hotspots::ChurnFetchResult>,
    pub(crate) diff_index: Option<&'a fallow_output::DiffIndex>,
    pub(crate) score_output: Option<&'a scoring::FileScoreOutput>,
    pub(crate) loaded_baseline: Option<&'a HealthBaselineData>,
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
    input: HealthDerivedSectionInput<'_>,
) -> HealthDerivedSections {
    let (candidate_paths, dupes_report, duplication_ms) =
        prepare_health_section_dupes(opts, &input);
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
) -> (
    rustc_hash::FxHashSet<std::path::PathBuf>,
    Option<crate::duplicates::DuplicationReport>,
    f64,
) {
    prepare_health_duplication_data(
        opts,
        input.config,
        input.files,
        input.changed_files,
        input.ws_roots,
        input.ignore_set,
    )
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
        compute_hotspots(
            input.opts,
            input.config,
            input.file_scores_slice,
            input.ignore_set,
            input.ws_roots,
            churn_data,
        )
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

fn prepare_health_duplication_data(
    opts: &HealthExecutionOptions<'_>,
    config: &ResolvedConfig,
    files: &[fallow_types::discover::DiscoveredFile],
    changed_files: Option<&rustc_hash::FxHashSet<std::path::PathBuf>>,
    ws_roots: Option<&[std::path::PathBuf]>,
    ignore_set: &globset::GlobSet,
) -> (
    rustc_hash::FxHashSet<std::path::PathBuf>,
    Option<crate::duplicates::DuplicationReport>,
    f64,
) {
    let candidate_paths =
        collect_candidate_paths(files, config, changed_files, ws_roots, ignore_set);
    let (dupes_report, duplication_ms) =
        compute_health_duplication_report(opts, config, files, &candidate_paths);
    (candidate_paths, dupes_report, duplication_ms)
}

fn compute_health_duplication_report(
    opts: &HealthExecutionOptions<'_>,
    config: &ResolvedConfig,
    files: &[fallow_types::discover::DiscoveredFile],
    candidate_paths: &rustc_hash::FxHashSet<std::path::PathBuf>,
) -> (Option<crate::duplicates::DuplicationReport>, f64) {
    let t = Instant::now();
    let dupes_report = if opts.score || opts.targets {
        let scoped_files = filter_files_to_paths(files, candidate_paths);
        Some(if opts.no_cache {
            crate::duplicates::find_duplicates(&config.root, &scoped_files, &config.duplicates)
        } else {
            crate::duplicates::find_duplicates_cached(
                &config.root,
                &scoped_files,
                &config.duplicates,
                &config.cache_dir,
            )
        })
    } else {
        None
    };
    (dupes_report, t.elapsed().as_secs_f64() * 1000.0)
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
