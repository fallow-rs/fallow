//! Health findings pipeline assembly.

use std::process::ExitCode;
use std::time::Instant;

use fallow_config::ResolvedConfig;
use fallow_output::{ComplexityViolation, FindingSeverity, RefactoringTarget};

use crate::baseline::HealthBaselineData;

use super::baseline_io::{HealthBaselineSaveInput, load_health_baseline, save_health_baseline};
use super::component_rollup::append_component_rollup_findings;
use super::filters::filter_complexity_findings_by_diff;
use super::findings::{
    CollectFindingsInput, CrapFindingMergeInput, collect_findings_with_resolver,
    merge_crap_findings,
};
use super::threshold_overrides::{
    GlobalHealthThresholds, ThresholdOverrideResolver, ThresholdOverrideStateTracker,
};
use super::{HealthOptions, scoring, sort_findings};

pub(super) struct HealthFindingsData {
    pub(super) findings: Vec<ComplexityViolation>,
    pub(super) threshold_overrides: Vec<fallow_output::ThresholdOverrideState>,
    pub(super) files_analyzed: usize,
    pub(super) total_functions: usize,
    pub(super) complexity_ms: f64,
    pub(super) total_above_threshold: usize,
    pub(super) sev_critical: usize,
    pub(super) sev_high: usize,
    pub(super) sev_moderate: usize,
    pub(super) loaded_baseline: Option<HealthBaselineData>,
}

struct CollectedHealthFindings {
    findings: Vec<ComplexityViolation>,
    files_analyzed: usize,
    total_functions: usize,
    complexity_ms: f64,
}

#[derive(Clone, Copy)]
pub(super) struct HealthFindingsInput<'a> {
    pub(super) opts: &'a HealthOptions<'a>,
    pub(super) config: &'a ResolvedConfig,
    pub(super) modules: &'a [crate::source::ModuleInfo],
    pub(super) file_paths:
        &'a rustc_hash::FxHashMap<crate::discover::FileId, &'a std::path::PathBuf>,
    pub(super) ignore_set: &'a globset::GlobSet,
    pub(super) changed_files: Option<&'a rustc_hash::FxHashSet<std::path::PathBuf>>,
    pub(super) ws_roots: Option<&'a [std::path::PathBuf]>,
    pub(super) diff_index: Option<&'a fallow_output::DiffIndex>,
    pub(super) max_cyclomatic: u16,
    pub(super) max_cognitive: u16,
    pub(super) max_crap: f64,
    pub(super) enforce_crap: bool,
    pub(super) score_output: Option<&'a scoring::FileScoreOutput>,
}

pub(super) fn prepare_health_findings(
    input: HealthFindingsInput<'_>,
) -> Result<HealthFindingsData, ExitCode> {
    let t = Instant::now();
    let global_thresholds = GlobalHealthThresholds {
        cyclomatic: input.max_cyclomatic,
        cognitive: input.max_cognitive,
        crap: input.max_crap,
    };
    let threshold_resolver =
        ThresholdOverrideResolver::new(&input.config.health.threshold_overrides, global_thresholds);
    let mut threshold_state_tracker = ThresholdOverrideStateTracker::default();
    let mut collected =
        collect_health_findings(input, &threshold_resolver, &mut threshold_state_tracker, t);

    let mut crap_ctx = HealthCrapMergeContext {
        modules: input.modules,
        file_paths: input.file_paths,
        ignore_set: input.ignore_set,
        changed_files: input.changed_files,
        ws_roots: input.ws_roots,
        max_cyclomatic: input.max_cyclomatic,
        max_cognitive: input.max_cognitive,
        enforce_crap: input.enforce_crap,
        score_output: input.score_output,
        config_root: &input.config.root,
        threshold_resolver: &threshold_resolver,
        threshold_state_tracker: &mut threshold_state_tracker,
    };
    apply_optional_crap_findings(input.opts, &mut collected.findings, &mut crap_ctx);
    let (total_above_threshold, sev_critical, sev_high, sev_moderate, loaded_baseline) =
        finalize_health_findings(
            input.opts,
            input.config,
            &mut collected.findings,
            input.diff_index,
        )?;
    threshold_state_tracker.record_no_match_entries(
        &threshold_resolver,
        should_emit_no_match_threshold_overrides(
            input.opts,
            input.changed_files,
            input.ws_roots,
            input.diff_index,
        ),
    );

    Ok(HealthFindingsData {
        findings: collected.findings,
        threshold_overrides: threshold_state_tracker.into_states(),
        files_analyzed: collected.files_analyzed,
        total_functions: collected.total_functions,
        complexity_ms: collected.complexity_ms,
        total_above_threshold,
        sev_critical,
        sev_high,
        sev_moderate,
        loaded_baseline,
    })
}

fn collect_health_findings(
    input: HealthFindingsInput<'_>,
    threshold_resolver: &ThresholdOverrideResolver,
    threshold_state_tracker: &mut ThresholdOverrideStateTracker,
    started_at: Instant,
) -> CollectedHealthFindings {
    let mut collect_input = CollectFindingsInput {
        modules: input.modules,
        file_paths: input.file_paths,
        config_root: &input.config.root,
        ignore_set: input.ignore_set,
        changed_files: input.changed_files,
        ws_roots: input.ws_roots,
        threshold_resolver,
        threshold_state_tracker,
        complexity_breakdown: input.opts.complexity_breakdown,
    };
    let (findings, files_analyzed, total_functions) =
        collect_findings_with_resolver(&mut collect_input);

    CollectedHealthFindings {
        findings,
        files_analyzed,
        total_functions,
        complexity_ms: started_at.elapsed().as_secs_f64() * 1000.0,
    }
}

struct HealthCrapMergeContext<'a> {
    modules: &'a [crate::source::ModuleInfo],
    file_paths: &'a rustc_hash::FxHashMap<crate::discover::FileId, &'a std::path::PathBuf>,
    ignore_set: &'a globset::GlobSet,
    changed_files: Option<&'a rustc_hash::FxHashSet<std::path::PathBuf>>,
    ws_roots: Option<&'a [std::path::PathBuf]>,
    max_cyclomatic: u16,
    max_cognitive: u16,
    enforce_crap: bool,
    score_output: Option<&'a scoring::FileScoreOutput>,
    config_root: &'a std::path::Path,
    threshold_resolver: &'a ThresholdOverrideResolver,
    threshold_state_tracker: &'a mut ThresholdOverrideStateTracker,
}

fn apply_optional_crap_findings(
    opts: &HealthOptions<'_>,
    findings: &mut Vec<ComplexityViolation>,
    ctx: &mut HealthCrapMergeContext<'_>,
) {
    if ctx.enforce_crap
        && let Some(score_out) = ctx.score_output
    {
        let mut input = CrapFindingMergeInput {
            modules: ctx.modules,
            file_paths: ctx.file_paths,
            config_root: ctx.config_root,
            ignore_set: ctx.ignore_set,
            changed_files: ctx.changed_files,
            ws_roots: ctx.ws_roots,
            per_function_crap: &score_out.per_function_crap,
            template_inherit_provenance: &score_out.template_inherit_provenance,
            complexity_breakdown: opts.complexity_breakdown,
            threshold_resolver: ctx.threshold_resolver,
            threshold_state_tracker: ctx.threshold_state_tracker,
        };
        merge_crap_findings(findings, &mut input);
    }
    append_component_rollup_findings(
        findings,
        ctx.score_output
            .map(|output| &output.template_inherit_provenance),
        ctx.max_cyclomatic,
        ctx.max_cognitive,
    );
}

fn should_emit_no_match_threshold_overrides(
    opts: &HealthOptions<'_>,
    changed_files: Option<&rustc_hash::FxHashSet<std::path::PathBuf>>,
    ws_roots: Option<&[std::path::PathBuf]>,
    diff_index: Option<&fallow_output::DiffIndex>,
) -> bool {
    opts.changed_since.is_none()
        && opts.diff_index.is_none()
        && !opts.use_shared_diff_index
        && opts.workspace.is_none()
        && opts.changed_workspaces.is_none()
        && changed_files.is_none()
        && ws_roots.is_none()
        && diff_index.is_none()
}

type HealthFindingFinalizeResult = (usize, usize, usize, usize, Option<HealthBaselineData>);

fn finalize_health_findings(
    opts: &HealthOptions<'_>,
    config: &ResolvedConfig,
    findings: &mut Vec<ComplexityViolation>,
    diff_index: Option<&fallow_output::DiffIndex>,
) -> Result<HealthFindingFinalizeResult, ExitCode> {
    if let Some(diff_index) = diff_index {
        filter_complexity_findings_by_diff(findings, diff_index, &config.root);
    }
    sort_findings(findings, opts.sort);
    let total_above_threshold = findings.len();
    let (sev_critical, sev_high, sev_moderate) = count_finding_severities(findings);
    let loaded_baseline = apply_health_baseline_and_top(opts, config, findings)?;
    Ok((
        total_above_threshold,
        sev_critical,
        sev_high,
        sev_moderate,
        loaded_baseline,
    ))
}

fn count_finding_severities(findings: &[ComplexityViolation]) -> (usize, usize, usize) {
    let (mut critical, mut high, mut moderate) = (0usize, 0usize, 0usize);
    for finding in findings {
        match finding.severity {
            FindingSeverity::Critical => critical += 1,
            FindingSeverity::High => high += 1,
            FindingSeverity::Moderate => moderate += 1,
        }
    }
    (critical, high, moderate)
}

fn apply_health_baseline_and_top(
    opts: &HealthOptions<'_>,
    config: &ResolvedConfig,
    findings: &mut Vec<ComplexityViolation>,
) -> Result<Option<HealthBaselineData>, ExitCode> {
    let loaded_baseline = if let Some(load_path) = opts.baseline {
        Some(load_health_baseline(
            load_path,
            findings,
            &config.root,
            opts.quiet,
            opts.output,
        )?)
    } else {
        None
    };
    if let Some(top) = opts.top {
        findings.truncate(top);
    }
    Ok(loaded_baseline)
}

pub(super) fn save_health_baseline_if_requested(
    opts: &HealthOptions<'_>,
    config: &ResolvedConfig,
    findings: &[ComplexityViolation],
    runtime_coverage: Option<&fallow_output::RuntimeCoverageReport>,
    targets: &[RefactoringTarget],
) -> Result<(), ExitCode> {
    if let Some(save_path) = opts.save_baseline {
        save_health_baseline(&HealthBaselineSaveInput {
            save_path,
            findings,
            runtime_coverage_findings: runtime_coverage
                .map_or(&[], |report| report.findings.as_slice()),
            targets,
            config_root: &config.root,
            quiet: opts.quiet,
            output: opts.output,
        })?;
    }
    Ok(())
}
