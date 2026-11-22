use std::time::Instant;

use fallow_config::ResolvedConfig;
use fallow_output::{FileHealthScore, HotspotEntry, HotspotSummary, RefactoringTarget};

use crate::baseline::{HealthBaselineData, filter_new_health_targets};
use crate::duplicates::DuplicationReport;

use super::HealthExecutionOptions;
use super::change_coupling_section::{ChangeCouplingOutcome, compute_change_coupling};
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
    pub(crate) ignore_set: &'a globset::GlobSet,
    pub(crate) changed_files: Option<&'a rustc_hash::FxHashSet<std::path::PathBuf>>,
    pub(crate) ws_roots: Option<&'a crate::workspace_scope::WorkspaceScope>,
    pub(crate) file_scores: &'a [FileHealthScore],
    pub(crate) churn_fetch: Option<hotspots::ChurnFetchResult>,
    pub(crate) diff_index: Option<&'a fallow_output::DiffIndex>,
    pub(crate) score_output: Option<&'a scoring::FileScoreOutput>,
    pub(crate) loaded_baseline: Option<&'a HealthBaselineData>,
    pub(crate) pre_computed_duplication: Option<DuplicationReport>,
    /// Flag-resolved global CRAP ceiling (`HealthScope::max_crap`); target
    /// factors fall back to it on rows without their own effective ceiling.
    pub(crate) max_crap: f64,
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
    /// The `change_coupling` section after `--top`. `None` when the
    /// hotspots section did not run or the run read no git history.
    pub(crate) change_coupling: Option<fallow_output::ChangeCouplingReport>,
    /// Hotspots before `--top` for scoring, plus complete grouped targets.
    /// Each group applies `--top` to its own displayed lists.
    pub(crate) group_lists: GroupUntruncatedLists,
}

/// Lists before `--top`: hotspots serve project scoring and groups, while
/// targets serve groups. `None` means the project list is complete.
#[derive(Default)]
pub struct GroupUntruncatedLists {
    pub(crate) hotspots: Option<Vec<HotspotEntry>>,
    pub(crate) targets: Option<Vec<RefactoringTarget>>,
}

pub fn prepare_health_derived_sections(
    opts: &HealthExecutionOptions<'_>,
    mut input: HealthDerivedSectionInput<'_>,
) -> HealthDerivedSections {
    let pre_computed_duplication = input.pre_computed_duplication.take();
    let (candidate_paths, dupes_report, duplication_ms) =
        prepare_health_section_dupes(opts, &input, pre_computed_duplication);
    let coupling = prepare_change_coupling(opts, &input);
    let (hotspots, group_hotspots, hotspot_summary, hotspots_ms) = prepare_health_section_hotspots(
        opts,
        HealthHotspotSectionInput {
            config: input.config,
            file_scores: input.file_scores,
            ignore_set: input.ignore_set,
            ws_roots: input.ws_roots,
            churn_fetch: input.churn_fetch,
            diff_index: input.diff_index,
            dispersion: coupling
                .as_ref()
                .map(|outcome| &outcome.analysis.dispersion),
        },
    );
    let target_input = HealthTargetSectionInput {
        score_output: input.score_output,
        file_scores: input.file_scores,
        hotspots: &hotspots,
        loaded_baseline: input.loaded_baseline,
        config: input.config,
        diff_index: input.diff_index,
        dupes_report: dupes_report.as_ref(),
        max_crap: input.max_crap,
        co_change: coupling.as_ref().map(|outcome| &outcome.analysis),
    };
    let (targets, group_targets, target_thresholds, targets_ms) =
        prepare_health_section_targets(opts, &target_input, group_hotspots.as_deref());

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
        change_coupling: coupling.and_then(|outcome| report_change_coupling(opts, outcome)),
        group_lists: GroupUntruncatedLists {
            hotspots: group_hotspots,
            targets: group_targets,
        },
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
        changed_files: input.changed_files,
        ws_roots: input.ws_roots,
        ignore_set: input.ignore_set,
        pre_computed_duplication,
    })
}

/// Run change coupling over the churn commits when the run read git history
/// and built the static edge index.
fn prepare_change_coupling(
    opts: &HealthExecutionOptions<'_>,
    input: &HealthDerivedSectionInput<'_>,
) -> Option<ChangeCouplingOutcome> {
    let churn_fetch = input.churn_fetch.as_ref()?;
    let links = input.score_output?.static_links.as_ref()?;
    Some(compute_change_coupling(
        opts,
        input.config,
        churn_fetch,
        links,
        &crate::change_coupling::PairScope {
            root: &input.config.root,
            ignore_set: input.ignore_set,
            ws_roots: input.ws_roots,
            diff_index: input.diff_index,
        },
    ))
}

/// The section as the report carries it: only with the hotspots section,
/// never in score-only output, and cut to `--top`.
fn report_change_coupling(
    opts: &HealthExecutionOptions<'_>,
    outcome: ChangeCouplingOutcome,
) -> Option<fallow_output::ChangeCouplingReport> {
    if !opts.hotspots || opts.score_only_output {
        return None;
    }
    let mut report = outcome.report;
    if let Some(top) = opts.top {
        report.pairs.truncate(top);
    }
    Some(report)
}

struct HealthHotspotSectionInput<'a> {
    config: &'a ResolvedConfig,
    file_scores: &'a [FileHealthScore],
    ignore_set: &'a globset::GlobSet,
    ws_roots: Option<&'a crate::workspace_scope::WorkspaceScope>,
    churn_fetch: Option<hotspots::ChurnFetchResult>,
    diff_index: Option<&'a fallow_output::DiffIndex>,
    dispersion: Option<
        &'a rustc_hash::FxHashMap<std::path::PathBuf, crate::change_coupling::FileDispersion>,
    >,
}

fn prepare_health_section_hotspots(
    opts: &HealthExecutionOptions<'_>,
    input: HealthHotspotSectionInput<'_>,
) -> (
    Vec<HotspotEntry>,
    Option<Vec<HotspotEntry>>,
    Option<HotspotSummary>,
    f64,
) {
    compute_filtered_hotspots(FilteredHotspotInput {
        opts,
        config: input.config,
        file_scores_slice: input.file_scores,
        ignore_set: input.ignore_set,
        ws_roots: input.ws_roots,
        churn_fetch: input.churn_fetch,
        diff_index: input.diff_index,
        dispersion: input.dispersion,
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
    max_crap: f64,
    co_change: Option<&'a crate::change_coupling::ChangeHistoryAnalysis>,
}

/// Compute the project targets, plus the complete group list of a grouped run
/// whose `--top` removes targets or hotspots.
///
/// The project targets use the truncated hotspot list, as an ungrouped run
/// does. The group list uses the complete hotspot list, so a group target
/// does not depend on a global `--top` cut.
fn prepare_health_section_targets(
    opts: &HealthExecutionOptions<'_>,
    input: &HealthTargetSectionInput<'_>,
    group_hotspots: Option<&[HotspotEntry]>,
) -> (
    Vec<RefactoringTarget>,
    Option<Vec<RefactoringTarget>>,
    Option<fallow_output::TargetThresholds>,
    f64,
) {
    let filtered = |hotspots: &[HotspotEntry], top: Option<usize>| {
        compute_filtered_targets(
            FilteredTargetInput {
                opts,
                score_output: input.score_output,
                file_scores_slice: input.file_scores,
                hotspots,
                loaded_baseline: input.loaded_baseline,
                config: input.config,
                diff_index: input.diff_index,
                dupes_report: input.dupes_report,
                max_crap: input.max_crap,
                co_change: input.co_change,
            },
            top,
        )
    };
    let (targets, target_thresholds, targets_ms) = filtered(input.hotspots, opts.top);
    let group_targets = (opts.group_by.is_some() && opts.top.is_some() && opts.targets)
        .then(|| filtered(group_hotspots.unwrap_or(input.hotspots), None).0);
    (targets, group_targets, target_thresholds, targets_ms)
}

struct FilteredHotspotInput<'a> {
    opts: &'a HealthExecutionOptions<'a>,
    config: &'a ResolvedConfig,
    file_scores_slice: &'a [FileHealthScore],
    ignore_set: &'a globset::GlobSet,
    ws_roots: Option<&'a crate::workspace_scope::WorkspaceScope>,
    churn_fetch: Option<hotspots::ChurnFetchResult>,
    diff_index: Option<&'a fallow_output::DiffIndex>,
    dispersion: Option<
        &'a rustc_hash::FxHashMap<std::path::PathBuf, crate::change_coupling::FileDispersion>,
    >,
}

fn compute_filtered_hotspots(
    input: FilteredHotspotInput<'_>,
) -> (
    Vec<HotspotEntry>,
    Option<Vec<HotspotEntry>>,
    Option<HotspotSummary>,
    f64,
) {
    let t = Instant::now();
    let (mut hotspots, hotspot_summary) = if let Some(churn_data) = input.churn_fetch {
        compute_hotspots(HotspotComputationInput {
            opts: input.opts,
            config: input.config,
            file_scores: input.file_scores_slice,
            ignore_set: input.ignore_set,
            ws_roots: input.ws_roots,
            churn_fetch: churn_data,
            dispersion: input.dispersion,
        })
    } else {
        (Vec::new(), None)
    };
    let mut group_hotspots = input
        .opts
        .top
        .filter(|&top| top < hotspots.len())
        .map(|top| {
            let displayed = hotspots[..top].to_vec();
            std::mem::replace(&mut hotspots, displayed)
        });
    if let Some(diff_index) = input.diff_index {
        filter_hotspots_by_diff(&mut hotspots, diff_index, &input.config.root);
        if let Some(group_hotspots) = group_hotspots.as_mut() {
            filter_hotspots_by_diff(group_hotspots, diff_index, &input.config.root);
        }
    }
    (
        hotspots,
        group_hotspots,
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
    max_crap: f64,
    co_change: Option<&'a crate::change_coupling::ChangeHistoryAnalysis>,
}

fn compute_filtered_targets(
    input: FilteredTargetInput<'_>,
    top: Option<usize>,
) -> (
    Vec<RefactoringTarget>,
    Option<fallow_output::TargetThresholds>,
    f64,
) {
    let t = Instant::now();
    let (mut targets, target_thresholds) = compute_targets(&input);
    if let Some(top) = top {
        targets.truncate(top);
    }
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
    changed_files: Option<&'a rustc_hash::FxHashSet<std::path::PathBuf>>,
    ws_roots: Option<&'a crate::workspace_scope::WorkspaceScope>,
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
            candidate_paths: &candidate_paths,
            pre_computed_duplication: input.pre_computed_duplication,
        });
    (candidate_paths, dupes_report, duplication_ms)
}

struct HealthDuplicationReportInput<'a> {
    opts: &'a HealthExecutionOptions<'a>,
    config: &'a ResolvedConfig,
    files: &'a [fallow_types::discover::DiscoveredFile],
    candidate_paths: &'a rustc_hash::FxHashSet<std::path::PathBuf>,
    pre_computed_duplication: Option<DuplicationReport>,
}

fn compute_health_duplication_report(
    input: HealthDuplicationReportInput<'_>,
) -> (Option<crate::duplicates::DuplicationReport>, f64) {
    let t = Instant::now();
    // `duplicates.enabled: false` leaves the score without a duplication
    // measure and the targets without clone evidence. A run that names the
    // duplication analysis does not change this, so the score of a project
    // does not depend on the analyses that one run selects.
    let measures_duplication = input.config.duplicates.runs_by_default();
    let dupes_report = if measures_duplication && (input.opts.score || input.opts.targets) {
        // A report over every file is the exact result: it covers the same
        // files with the same config as the detection below. A report over
        // more files than the health scope cannot be cut down to the scope,
        // because the detector drops groups below `minOccurrences` and
        // counts its totals over the files that it reads, so the scoped
        // detection runs instead.
        if let Some(report) = input.pre_computed_duplication
            && input.candidate_paths.len() == input.files.len()
        {
            return (Some(report), t.elapsed().as_secs_f64() * 1000.0);
        }
        let scoped_files = filter_files_to_paths(input.files, input.candidate_paths);
        let cache_dir = (!input.opts.no_cache).then_some(input.config.cache_dir.as_path());
        Some(
            crate::duplicates::find_duplicates_with_defaults(
                &input.config.root,
                &scoped_files,
                &input.config.duplicates,
                cache_dir,
            )
            .report,
        )
    } else {
        None
    };
    (dupes_report, t.elapsed().as_secs_f64() * 1000.0)
}

/// Compute refactoring targets when requested, applying the baseline and effort
/// filters. The caller applies `--top`.
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
    let target_aux = TargetAuxData::from_output(output, &clone_siblings, input.co_change);
    let (mut tgts, thresholds) = compute_refactoring_targets(
        input.file_scores_slice,
        &target_aux,
        input.hotspots,
        input.max_crap,
    );
    if let Some(baseline) = input.loaded_baseline {
        tgts = filter_new_health_targets(tgts, baseline, &input.config.root);
    }
    if let Some(ref effort) = input.opts.effort {
        tgts.retain(|t| t.effort == *effort);
    }
    (tgts, Some(thresholds))
}
