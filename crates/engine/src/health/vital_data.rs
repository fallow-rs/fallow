#![allow(
    clippy::print_stderr,
    reason = "human stderr notes for snapshots and trends are preserved from the health pipeline"
)]

use std::process::ExitCode;

use fallow_config::ResolvedConfig;
use fallow_output::{FileHealthScore, HealthScore, HotspotEntry, HotspotSummary};

use crate::error::emit_error;
use crate::vital_signs;

use super::actions::active_health_coverage_model;
use super::filters::filter_large_functions_by_diff;
use super::large_functions::{LargeFunctionInput, collect_large_functions};
use super::scoring;
use super::{
    HealthExecutionOptions, SubsetFilter, VitalSignsAndCountsInput, apply_duplication_metrics,
    compute_vital_signs_and_counts,
};

pub struct HealthVitalData {
    pub(crate) vital_signs: fallow_output::VitalSigns,
    pub(crate) health_score: Option<HealthScore>,
    pub(crate) health_trend: Option<fallow_output::HealthTrend>,
    pub(crate) large_functions: Vec<fallow_output::LargeFunctionEntry>,
}

pub struct HealthVitalDataInput<'a> {
    pub(crate) opts: &'a HealthExecutionOptions<'a>,
    pub(crate) modules: &'a [crate::source::ModuleInfo],
    pub(crate) file_paths:
        &'a rustc_hash::FxHashMap<crate::discover::FileId, &'a std::path::PathBuf>,
    pub(crate) score_output: Option<&'a scoring::FileScoreOutput>,
    pub(crate) file_scores_slice: &'a [FileHealthScore],
    pub(crate) hotspots: &'a [HotspotEntry],
    pub(crate) dupes_report: Option<&'a crate::duplicates::DuplicationReport>,
    pub(crate) candidate_paths: &'a rustc_hash::FxHashSet<std::path::PathBuf>,
    pub(crate) total_files: usize,
    pub(crate) config: &'a ResolvedConfig,
    pub(crate) ignore_set: &'a globset::GlobSet,
    pub(crate) changed_files: Option<&'a rustc_hash::FxHashSet<std::path::PathBuf>>,
    pub(crate) ws_roots: Option<&'a [std::path::PathBuf]>,
    pub(crate) diff_index: Option<&'a fallow_output::DiffIndex>,
    pub(crate) hotspot_summary: Option<&'a HotspotSummary>,
    pub(crate) has_istanbul_coverage: bool,
    pub(crate) needs_file_scores: bool,
}

/// Assign the prop-drilling chain count / max depth onto the vital signs. Prop
/// drilling is a whole-project graph signal (the chains live in AnalysisResults,
/// surfaced via FileScoreOutput); only populated when the opt-in `prop-drilling`
/// rule emitted chains, so the small capped penalty stays dormant by default.
fn apply_prop_drilling_metrics(
    vital_signs: &mut fallow_output::VitalSigns,
    score_output: &scoring::FileScoreOutput,
) {
    if score_output.prop_drilling_chains.is_empty() {
        return;
    }
    vital_signs.prop_drilling_chain_count =
        u32::try_from(score_output.prop_drilling_chains.len()).ok();
    vital_signs.prop_drilling_max_depth = score_output
        .prop_drilling_chains
        .iter()
        .map(|c| c.chain.depth)
        .max();
}

/// Assign the descriptive render fan-in blast-radius metric (p95 / high-pct / max
/// distinct parents plus a located top-N list) onto the vital signs. Aggregates
/// are precomputed in core and ride on FileScoreOutput; non-React runs leave the
/// fields `None` (skip_serializing_if), so the JSON contract is unchanged.
fn apply_render_fan_in_metrics(
    vital_signs: &mut fallow_output::VitalSigns,
    score_output: &scoring::FileScoreOutput,
    config: &ResolvedConfig,
) {
    let Some(metric) = score_output.render_fan_in.as_ref() else {
        return;
    };
    vital_signs.p95_render_fan_in = metric.p95_distinct_parents;
    vital_signs.render_fan_in_high_pct = metric.high_pct;
    // The public headline (`max_render_fan_in`) is the max DISTINCT-PARENTS:
    // honest blast radius = the most distinct render LOCATIONS any one
    // component is rendered from. `render_sites` (incl. repeats) is secondary.
    vital_signs.max_render_fan_in = metric.max_distinct_parents;

    // Located top-N list so a consumer sees WHICH component carries the
    // headline fan-in, not just the number. The core carrier is sorted by
    // (path, component) for run-stability and INCLUDES rendered-nowhere `0`
    // entries (for the percentile distribution), so re-sort by
    // distinct_parents (the honest headline axis) descending, tie-break on
    // render_sites descending, and drop the `0`-fan-in entries here. Final
    // tie-break on (path, component) so the cap is deterministic. Cap at a
    // small N.
    const MAX_TOP_RENDER_FAN_IN: usize = 20;
    let mut top: Vec<&fallow_types::results::RenderFanInComponent> = metric
        .per_component
        .iter()
        .filter(|c| c.distinct_parents > 0)
        .collect();
    top.sort_by(|a, b| {
        b.distinct_parents
            .cmp(&a.distinct_parents)
            .then_with(|| b.render_sites.cmp(&a.render_sites))
            .then_with(|| a.file.cmp(&b.file))
            .then_with(|| a.component.cmp(&b.component))
    });
    vital_signs.top_render_fan_in = top
        .into_iter()
        .take(MAX_TOP_RENDER_FAN_IN)
        .map(|c| fallow_output::RenderFanInTopComponent {
            component: c.component.clone(),
            path: c
                .file
                .strip_prefix(&config.root)
                .unwrap_or(&c.file)
                .to_path_buf(),
            render_sites: c.render_sites,
            distinct_parents: c.distinct_parents,
        })
        .collect();
}

/// Compute the scoped vital signs / counts for the candidate subset, then assign
/// the prop-drilling and render fan-in metrics onto the vital signs.
fn compute_scoped_vital_signs(
    input: &HealthVitalDataInput<'_>,
    total_files_scoped: usize,
    project_subset: &SubsetFilter<'_>,
) -> (fallow_output::VitalSigns, fallow_output::VitalSignsCounts) {
    let vital_signs_input = VitalSignsAndCountsInput {
        score_output: input.score_output,
        modules: input.modules,
        file_paths: input.file_paths,
        needs_file_scores: input.needs_file_scores,
        file_scores_slice: input.file_scores_slice,
        needs_hotspots: input.opts.hotspots || input.opts.targets,
        hotspots: input.hotspots,
        total_files: total_files_scoped,
        subset: project_subset,
    };
    let (mut vital_signs, counts) = compute_vital_signs_and_counts(&vital_signs_input);

    if let Some(score_output) = input.score_output {
        apply_prop_drilling_metrics(&mut vital_signs, score_output);
        apply_render_fan_in_metrics(&mut vital_signs, score_output, input.config);
    }
    (vital_signs, counts)
}

/// Persist the health snapshot when `--save-snapshot` was requested.
fn maybe_save_health_snapshot(
    input: &HealthVitalDataInput<'_>,
    vital_signs: &fallow_output::VitalSigns,
    counts: &fallow_output::VitalSignsCounts,
    health_score: Option<&HealthScore>,
) -> Result<(), ExitCode> {
    if let Some(ref snapshot_path) = input.opts.save_snapshot {
        save_snapshot(SnapshotInput {
            opts: input.opts,
            snapshot_path,
            vital_signs,
            counts,
            hotspot_summary: input.hotspot_summary,
            health_score,
            coverage_model: Some(active_health_coverage_model(input.has_istanbul_coverage)),
        })?;
    }
    Ok(())
}

pub fn prepare_health_vital_data(
    input: &HealthVitalDataInput<'_>,
) -> Result<HealthVitalData, ExitCode> {
    let project_subset = if input.candidate_paths.len() == input.total_files {
        SubsetFilter::Full
    } else {
        SubsetFilter::Paths(input.candidate_paths)
    };
    let total_files_scoped = input.candidate_paths.len();
    let (mut vital_signs, mut counts) =
        compute_scoped_vital_signs(input, total_files_scoped, &project_subset);

    let health_score = compute_health_score_metrics(
        input.opts,
        input.dupes_report,
        &mut vital_signs,
        &mut counts,
        total_files_scoped,
    );
    let large_functions = collect_filtered_large_functions(FilteredLargeFunctionInput {
        vital_signs: &vital_signs,
        modules: input.modules,
        file_paths: input.file_paths,
        config: input.config,
        ignore_set: input.ignore_set,
        changed_files: input.changed_files,
        ws_roots: input.ws_roots,
        diff_index: input.diff_index,
    });
    maybe_save_health_snapshot(input, &vital_signs, &counts, health_score.as_ref())?;
    let health_trend =
        compute_health_trend(input.opts, &vital_signs, &counts, health_score.as_ref());

    Ok(HealthVitalData {
        vital_signs,
        health_score,
        health_trend,
        large_functions,
    })
}

fn compute_health_score_metrics(
    opts: &HealthExecutionOptions<'_>,
    dupes_report: Option<&crate::duplicates::DuplicationReport>,
    vital_signs: &mut fallow_output::VitalSigns,
    counts: &mut fallow_output::VitalSignsCounts,
    total_files_scoped: usize,
) -> Option<HealthScore> {
    if opts.score
        && let Some(report) = dupes_report
    {
        apply_duplication_metrics(vital_signs, counts, report);
    }
    opts.score
        .then(|| vital_signs::compute_health_score(vital_signs, total_files_scoped))
}

#[derive(Clone, Copy)]
struct FilteredLargeFunctionInput<'a> {
    vital_signs: &'a fallow_output::VitalSigns,
    modules: &'a [crate::source::ModuleInfo],
    file_paths: &'a rustc_hash::FxHashMap<crate::discover::FileId, &'a std::path::PathBuf>,
    config: &'a ResolvedConfig,
    ignore_set: &'a globset::GlobSet,
    changed_files: Option<&'a rustc_hash::FxHashSet<std::path::PathBuf>>,
    ws_roots: Option<&'a [std::path::PathBuf]>,
    diff_index: Option<&'a fallow_output::DiffIndex>,
}

fn collect_filtered_large_functions(
    input: FilteredLargeFunctionInput<'_>,
) -> Vec<fallow_output::LargeFunctionEntry> {
    let large_input = LargeFunctionInput {
        vital_signs: input.vital_signs,
        modules: input.modules,
        file_paths: input.file_paths,
        config_root: &input.config.root,
        ignore_set: input.ignore_set,
        changed_files: input.changed_files,
        ws_roots: input.ws_roots,
    };
    let mut large_functions = collect_large_functions(&large_input);
    if let Some(diff_index) = input.diff_index {
        filter_large_functions_by_diff(&mut large_functions, diff_index, &input.config.root);
    }
    large_functions
}

/// Save a vital signs snapshot to disk if requested.
struct SnapshotInput<'a> {
    opts: &'a HealthExecutionOptions<'a>,
    snapshot_path: &'a std::path::Path,
    vital_signs: &'a fallow_output::VitalSigns,
    counts: &'a fallow_output::VitalSignsCounts,
    hotspot_summary: Option<&'a fallow_output::HotspotSummary>,
    health_score: Option<&'a fallow_output::HealthScore>,
    coverage_model: Option<fallow_output::CoverageModel>,
}

fn save_snapshot(input: SnapshotInput<'_>) -> Result<(), ExitCode> {
    let shallow = input.hotspot_summary.is_some_and(|s| s.shallow_clone);
    let snapshot = vital_signs::build_snapshot(
        input.vital_signs.clone(),
        input.counts.clone(),
        input.opts.root,
        shallow,
        input.health_score,
        input.coverage_model,
    );
    let explicit = if input.snapshot_path.as_os_str().is_empty() {
        None
    } else {
        Some(input.snapshot_path)
    };
    match vital_signs::save_snapshot(&snapshot, input.opts.root, explicit) {
        Ok(saved_path) => {
            if !input.opts.quiet {
                eprintln!("Saved vital signs snapshot to {}", saved_path.display());
            }
            Ok(())
        }
        Err(e) => Err(emit_error(&e, 2, input.opts.output)),
    }
}

/// Compute health trend from historical snapshots if requested.
fn compute_health_trend(
    opts: &HealthExecutionOptions<'_>,
    vital_signs: &fallow_output::VitalSigns,
    counts: &fallow_output::VitalSignsCounts,
    health_score: Option<&fallow_output::HealthScore>,
) -> Option<fallow_output::HealthTrend> {
    if !opts.trend {
        return None;
    }
    if opts.changed_since.is_some() && !opts.quiet {
        eprintln!(
            "warning: --trend comparison may be inaccurate with --changed-since; \
             snapshots are typically from full-project runs"
        );
    }
    let snapshots = vital_signs::load_snapshots(opts.root);
    if snapshots.is_empty() && !opts.quiet {
        eprintln!(
            "No snapshots found. Run `fallow health --save-snapshot` to save a \
             baseline, then use --trend on subsequent runs to track progress."
        );
    }
    vital_signs::compute_trend(
        vital_signs,
        counts,
        health_score.map(|s| s.score),
        &snapshots,
    )
}
