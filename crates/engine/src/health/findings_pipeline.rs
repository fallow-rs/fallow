//! Health findings pipeline assembly.

use std::time::Instant;

use fallow_config::ResolvedConfig;
use fallow_output::{ComplexityViolation, FindingSeverity, RefactoringTarget};

use crate::baseline::HealthBaselineData;

use super::baseline_io::{HealthBaselineSaveInput, load_health_baseline, save_health_baseline};
use super::component_rollup::{
    ComponentTemplateUnitScope, append_component_rollup_findings, collect_component_template_units,
};
use super::filters::filter_complexity_findings_by_diff;
use super::findings::{
    CollectFindingsInput, CrapFindingMergeInput, collect_findings_with_resolver,
    merge_crap_findings, record_template_crap_override_rows,
};
use super::threshold_overrides::{
    GlobalHealthThresholds, ThresholdOverrideResolver, ThresholdOverrideStateTracker,
};
use super::{HealthError, HealthOptions, scoring, sort_findings};

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
    pub(super) baseline_staleness: Option<fallow_output::HealthBaselineStaleness>,
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
) -> Result<HealthFindingsData, HealthError> {
    let t = Instant::now();
    let global_thresholds = GlobalHealthThresholds {
        cyclomatic: input.max_cyclomatic,
        cognitive: input.max_cognitive,
        crap: input.max_crap,
        unit_size: input.config.health.max_unit_size,
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
        enforce_crap: input.enforce_crap,
        score_output: input.score_output,
        config_root: &input.config.root,
        threshold_resolver: &threshold_resolver,
        threshold_state_tracker: &mut threshold_state_tracker,
    };
    apply_optional_crap_findings(input.opts, &mut collected.findings, &mut crap_ctx);
    let HealthFindingFinalizeResult {
        total_above_threshold,
        sev_critical,
        sev_high,
        sev_moderate,
        loaded_baseline,
        baseline_staleness,
    } = finalize_health_findings(
        input.opts,
        input.config,
        &mut collected.findings,
        input.diff_index,
        is_change_scoped(&input),
        &mut threshold_state_tracker,
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
        baseline_staleness,
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
        record_template_crap_override_rows(&mut input);
    }
    let template_units = collect_component_template_units(
        ctx.modules,
        ctx.file_paths,
        ctx.score_output
            .map(|output| &output.template_inherit_provenance),
        &ComponentTemplateUnitScope {
            config_root: ctx.config_root,
            ignore_set: ctx.ignore_set,
            changed_files: ctx.changed_files,
            ws_roots: ctx.ws_roots,
        },
    );
    append_component_rollup_findings(
        findings,
        &template_units,
        ctx.threshold_resolver,
        ctx.config_root,
        ctx.threshold_state_tracker,
    );
}

/// Complete each matched override row's `outstanding` list with every dimension
/// on which the unit still has a live finding.
///
/// The list is not filtered by what the entry configures. An override that
/// raises `maxCrap` too little, or that raises `maxCyclomatic` while the unit
/// breaches on cognitive, still leaves a live finding, and suppressing the
/// disclosure in those cases is what made issue #2163 look like a
/// threshold-resolution bug.
///
/// The finding-backed dimensions cannot be derived inside the tracker.
/// `record_complexity` runs during collection, before the CRAP merge, so at
/// record time it is unknown whether a finding survives at all, let alone on
/// which dimension. The unit-size term is the mirror image and is seeded by the
/// tracker instead, because it never produces a finding to read back here.
pub(super) fn annotate_outstanding_dimensions(
    tracker: &mut ThresholdOverrideStateTracker,
    findings: &[ComplexityViolation],
) {
    // Both sides carry the discovery path and the same `(line, col)` the
    // finding is built from, so no relativization is needed. Name and position
    // are BOTH part of the key, and neither alone is an identity: one file can
    // hold several units sharing a name, and the synthetic `<component>` rollup
    // is deliberately anchored at the worst class method's exact `(path, line,
    // col)` while that method keeps its own finding, so keying on either half
    // alone let one unit overwrite the other's `exceeded` (issue #2163).
    let mut exceeded_by_unit: rustc_hash::FxHashMap<
        (&std::path::Path, u32, u32, &str),
        fallow_output::ExceededThreshold,
    > = rustc_hash::FxHashMap::default();
    for finding in findings {
        exceeded_by_unit.insert(
            (
                finding.path.as_path(),
                finding.line,
                finding.col,
                finding.name.as_str(),
            ),
            finding.exceeded,
        );
    }

    for state in tracker.states_mut() {
        let (Some(path), Some(line), Some(col), Some(function)) = (
            state.path.as_deref(),
            state.line,
            state.col,
            state.function.as_deref(),
        ) else {
            continue;
        };
        let Some(exceeded) = exceeded_by_unit.get(&(path, line, col, function)).copied() else {
            continue;
        };
        // `record_complexity` may already have seeded `Complexity` for a
        // surviving unit-size breach, which produces no finding of its own, so
        // the dimension is added only when it is not there yet.
        if (exceeded.includes_cyclomatic() || exceeded.includes_cognitive())
            && !state
                .outstanding
                .contains(&fallow_output::ThresholdOverrideDimension::Complexity)
        {
            state
                .outstanding
                .push(fallow_output::ThresholdOverrideDimension::Complexity);
        }
        if exceeded.includes_crap()
            && !state
                .outstanding
                .contains(&fallow_output::ThresholdOverrideDimension::Crap)
        {
            state
                .outstanding
                .push(fallow_output::ThresholdOverrideDimension::Crap);
        }
    }
}

/// `no_match` rows are the only feedback a user gets that an override glob
/// matched nothing, so a full-repo run must emit them. The `diff_index`
/// parameter is the RESOLVED index, so a run that actually loaded a shared diff
/// is already excluded by the `diff_index.is_none()` clause; gating on the
/// `use_shared_diff_index` request instead made the rows unreachable from every
/// CLI entry point (issue #2163).
fn should_emit_no_match_threshold_overrides(
    opts: &HealthOptions<'_>,
    changed_files: Option<&rustc_hash::FxHashSet<std::path::PathBuf>>,
    ws_roots: Option<&[std::path::PathBuf]>,
    diff_index: Option<&fallow_output::DiffIndex>,
) -> bool {
    opts.changed_since.is_none()
        && opts.diff_index.is_none()
        && opts.workspace.is_none()
        && opts.changed_workspaces.is_none()
        && changed_files.is_none()
        && ws_roots.is_none()
        && diff_index.is_none()
}

/// True when this run analyzes a subset of the project, so a loaded full-repo
/// baseline would look stale for reasons that have nothing to do with rot.
fn is_change_scoped(input: &HealthFindingsInput<'_>) -> bool {
    input.diff_index.is_some() || input.changed_files.is_some() || input.ws_roots.is_some()
}

struct HealthFindingFinalizeResult {
    total_above_threshold: usize,
    sev_critical: usize,
    sev_high: usize,
    sev_moderate: usize,
    loaded_baseline: Option<HealthBaselineData>,
    baseline_staleness: Option<fallow_output::HealthBaselineStaleness>,
}

fn finalize_health_findings(
    opts: &HealthOptions<'_>,
    config: &ResolvedConfig,
    findings: &mut Vec<ComplexityViolation>,
    diff_index: Option<&fallow_output::DiffIndex>,
    change_scoped: bool,
    threshold_state_tracker: &mut ThresholdOverrideStateTracker,
) -> Result<HealthFindingFinalizeResult, HealthError> {
    // Runs before every downstream narrowing. The override rows were recorded
    // over the whole collection pass, while `--diff-file`/`--diff-stdin`,
    // `--baseline` and `--top` all narrow the findings list for DISPLAY, so
    // annotating after any of them leaves an `insufficient` row with an empty
    // `outstanding`: the one contradiction a CI gate cannot detect.
    annotate_outstanding_dimensions(threshold_state_tracker, findings);
    if let Some(diff_index) = diff_index {
        filter_complexity_findings_by_diff(findings, diff_index, &config.root);
    }
    sort_findings(findings, opts.sort);
    let total_above_threshold = findings.len();
    let (sev_critical, sev_high, sev_moderate) = count_finding_severities(findings);
    let (loaded_baseline, baseline_staleness) =
        apply_health_baseline_and_top(opts, config, findings, change_scoped)?;
    Ok(HealthFindingFinalizeResult {
        total_above_threshold,
        sev_critical,
        sev_high,
        sev_moderate,
        loaded_baseline,
        baseline_staleness,
    })
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

type LoadedBaselineParts = (
    Option<HealthBaselineData>,
    Option<fallow_output::HealthBaselineStaleness>,
);

fn apply_health_baseline_and_top(
    opts: &HealthOptions<'_>,
    config: &ResolvedConfig,
    findings: &mut Vec<ComplexityViolation>,
    change_scoped: bool,
) -> Result<LoadedBaselineParts, HealthError> {
    let (loaded_baseline, baseline_staleness) = if let Some(load_path) = opts.baseline {
        let loaded = load_health_baseline(
            load_path,
            findings,
            &config.root,
            opts.quiet,
            opts.baseline_mode,
            change_scoped,
        )?;
        (Some(loaded.data), Some(loaded.staleness))
    } else {
        (None, None)
    };
    if let Some(top) = opts.top {
        findings.truncate(top);
    }
    Ok((loaded_baseline, baseline_staleness))
}

pub(super) fn save_health_baseline_if_requested(
    opts: &HealthOptions<'_>,
    config: &ResolvedConfig,
    findings: &[ComplexityViolation],
    runtime_coverage: Option<&fallow_output::RuntimeCoverageReport>,
    targets: &[RefactoringTarget],
) -> Result<(), HealthError> {
    if let Some(save_path) = opts.save_baseline {
        save_health_baseline(&HealthBaselineSaveInput {
            save_path,
            findings,
            runtime_coverage_findings: runtime_coverage
                .map_or(&[], |report| report.findings.as_slice()),
            targets,
            config_root: &config.root,
            quiet: opts.quiet,
            mode: opts.baseline_mode,
            mode_explicit: opts.baseline_mode_explicit,
        })?;
    }
    Ok(())
}
