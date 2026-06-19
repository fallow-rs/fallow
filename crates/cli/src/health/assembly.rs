use super::{HealthOptions, HealthReportAssembly, coverage_intelligence};
use crate::health_types::{ComplexityViolation, HealthReport, HealthSummary};

struct HealthSummaryAssembly<'a> {
    findings: &'a [ComplexityViolation],
    files_analyzed: usize,
    total_functions: usize,
    total_above_threshold: usize,
    max_cyclomatic: u16,
    max_cognitive: u16,
    max_crap: f64,
    files_scored: Option<usize>,
    average_maintainability: Option<f64>,
    report_coverage_gaps: bool,
    has_istanbul_coverage: bool,
    istanbul_matched: usize,
    istanbul_total: usize,
    sev_critical: usize,
    sev_high: usize,
    sev_moderate: usize,
}

/// Assemble the final `HealthReport` from all computed data.
pub(super) fn assemble_health_report(
    opts: &HealthOptions<'_>,
    action_ctx: &crate::health_types::HealthActionContext,
    assembly: HealthReportAssembly,
) -> HealthReport {
    // The summary reads the assembly by reference (scalars, findings, and the
    // score output) before the rest of the build consumes the owned fields.
    let summary = build_summary_from_assembly(opts, &assembly);
    build_health_report(opts, action_ctx, assembly, summary)
}

/// Compute the report summary from the assembly without consuming it.
fn build_summary_from_assembly(
    opts: &HealthOptions<'_>,
    assembly: &HealthReportAssembly,
) -> HealthSummary {
    let (ist_matched, ist_total) =
        istanbul_counts_from_score_output(assembly.score_output.as_ref());
    build_health_summary(
        opts,
        &HealthSummaryAssembly {
            findings: &assembly.findings,
            files_analyzed: assembly.files_analyzed,
            total_functions: assembly.total_functions,
            total_above_threshold: assembly.total_above_threshold,
            max_cyclomatic: assembly.max_cyclomatic,
            max_cognitive: assembly.max_cognitive,
            max_crap: assembly.max_crap,
            files_scored: assembly.files_scored,
            average_maintainability: assembly.average_maintainability,
            report_coverage_gaps: assembly.report_coverage_gaps,
            has_istanbul_coverage: assembly.has_istanbul_coverage,
            istanbul_matched: ist_matched,
            istanbul_total: ist_total,
            sev_critical: assembly.sev_critical,
            sev_high: assembly.sev_high,
            sev_moderate: assembly.sev_moderate,
        },
    )
}

/// Consume the assembly and the precomputed summary into the final report.
fn build_health_report(
    opts: &HealthOptions<'_>,
    action_ctx: &crate::health_types::HealthActionContext,
    assembly: HealthReportAssembly,
    summary: HealthSummary,
) -> HealthReport {
    let HealthReportAssembly {
        report_coverage_gaps,
        findings,
        threshold_overrides,
        vital_signs,
        health_score,
        score_output,
        hotspots,
        hotspot_summary,
        targets,
        target_thresholds,
        health_trend,
        runtime_coverage,
        framework_health,
        large_functions,
        ..
    } = assembly;
    let prelude = compute_report_prelude(
        opts,
        score_output,
        hotspots,
        hotspot_summary,
        report_coverage_gaps,
    );
    build_health_report_struct(
        opts,
        action_ctx,
        HealthReportStructParts {
            summary,
            threshold_overrides,
            vital_signs,
            health_score,
            findings,
            file_scores: prelude.file_scores,
            coverage_gaps: prelude.coverage_gaps,
            prop_drilling_chains: prelude.prop_drilling_chains,
            report_hotspots: prelude.report_hotspots,
            report_hotspot_summary: prelude.report_hotspot_summary,
            runtime_coverage,
            large_functions,
            targets,
            target_thresholds,
            health_trend,
            framework_health,
            render_fan_in_top: prelude.render_fan_in_top,
        },
    )
}

/// Score-output-derived report sections built before the struct assembly.
struct ReportPrelude {
    coverage_gaps: Option<crate::health_types::CoverageGaps>,
    prop_drilling_chains: Vec<fallow_types::output_dead_code::PropDrillingChainFinding>,
    render_fan_in_top: rustc_hash::FxHashMap<std::path::PathBuf, (String, u32)>,
    file_scores: Vec<crate::health_types::FileHealthScore>,
    report_hotspots: Vec<crate::health_types::HotspotEntry>,
    report_hotspot_summary: Option<crate::health_types::HotspotSummary>,
}

/// Build the score-output-derived report sections, consuming `score_output`,
/// `hotspots`, and `hotspot_summary`.
fn compute_report_prelude(
    opts: &HealthOptions<'_>,
    score_output: Option<super::scoring::FileScoreOutput>,
    hotspots: Vec<crate::health_types::HotspotEntry>,
    hotspot_summary: Option<crate::health_types::HotspotSummary>,
    report_coverage_gaps: bool,
) -> ReportPrelude {
    let coverage_gaps = build_report_coverage_gaps(report_coverage_gaps, score_output.as_ref());
    let prop_drilling_chains = build_prop_drilling_chains(opts, score_output.as_ref());
    // Render fan-in is a descriptive blast-radius signal. Build the per-file
    // top-component lookup BEFORE moving `score_output` into the file-scores
    // builder, so the human hotspot/complexity drill-down can show `rendered in N
    // places` for the top component of a file. Empty on non-React runs. The
    // public surface stays the VitalSigns aggregate; this map is `#[serde(skip)]`.
    let render_fan_in_top = if opts.score_only_output {
        rustc_hash::FxHashMap::default()
    } else {
        build_render_fan_in_top(score_output.as_ref())
    };
    let file_scores = build_report_file_scores(opts, score_output);
    let (report_hotspots, report_hotspot_summary) =
        report_hotspot_data(opts, hotspots, hotspot_summary);
    ReportPrelude {
        coverage_gaps,
        prop_drilling_chains,
        render_fan_in_top,
        file_scores,
        report_hotspots,
        report_hotspot_summary,
    }
}

/// Prop-drilling chains ride on the whole-project score output. Surfaced in
/// the report (unless score-only output) so `health --hotspots` and the JSON
/// envelope carry the located records. Empty unless the opt-in rule is on.
fn build_prop_drilling_chains(
    opts: &HealthOptions<'_>,
    score_output: Option<&super::scoring::FileScoreOutput>,
) -> Vec<fallow_types::output_dead_code::PropDrillingChainFinding> {
    if opts.score_only_output {
        Vec::new()
    } else {
        score_output
            .map(|o| o.prop_drilling_chains.clone())
            .unwrap_or_default()
    }
}

/// Pieces consumed by the `HealthReport` struct literal builder.
struct HealthReportStructParts {
    summary: HealthSummary,
    threshold_overrides: Vec<crate::health_types::ThresholdOverrideState>,
    vital_signs: crate::health_types::VitalSigns,
    health_score: Option<crate::health_types::HealthScore>,
    findings: Vec<ComplexityViolation>,
    file_scores: Vec<crate::health_types::FileHealthScore>,
    coverage_gaps: Option<crate::health_types::CoverageGaps>,
    prop_drilling_chains: Vec<fallow_types::output_dead_code::PropDrillingChainFinding>,
    report_hotspots: Vec<crate::health_types::HotspotEntry>,
    report_hotspot_summary: Option<crate::health_types::HotspotSummary>,
    runtime_coverage: Option<crate::health_types::RuntimeCoverageReport>,
    large_functions: Vec<crate::health_types::LargeFunctionEntry>,
    targets: Vec<crate::health_types::RefactoringTarget>,
    target_thresholds: Option<crate::health_types::TargetThresholds>,
    health_trend: Option<crate::health_types::HealthTrend>,
    framework_health: Option<crate::health_types::FrameworkHealthDiagnostics>,
    render_fan_in_top: rustc_hash::FxHashMap<std::path::PathBuf, (String, u32)>,
}

/// Build the `HealthReport` struct, applying the score-only output gates and
/// (unless score-only) filling `coverage_intelligence` from the built report.
fn build_health_report_struct(
    opts: &HealthOptions<'_>,
    action_ctx: &crate::health_types::HealthActionContext,
    parts: HealthReportStructParts,
) -> HealthReport {
    let mut report = HealthReport {
        summary: parts.summary,
        threshold_overrides: build_report_threshold_overrides(opts, parts.threshold_overrides),
        vital_signs: if opts.score_only_output {
            None
        } else {
            Some(parts.vital_signs)
        },
        health_score: parts.health_score,
        findings: build_report_findings(opts, action_ctx, parts.findings),
        file_scores: parts.file_scores,
        coverage_gaps: if opts.score_only_output {
            None
        } else {
            parts.coverage_gaps
        },
        prop_drilling_chains: parts.prop_drilling_chains,
        hotspots: build_report_hotspots(opts, parts.report_hotspots),
        hotspot_summary: if opts.score_only_output {
            None
        } else {
            parts.report_hotspot_summary
        },
        runtime_coverage: parts.runtime_coverage,
        coverage_intelligence: None,
        large_functions: if opts.score_only_output {
            Vec::new()
        } else {
            parts.large_functions
        },
        targets: build_report_targets(opts, parts.targets),
        target_thresholds: if opts.score_only_output {
            None
        } else {
            parts.target_thresholds
        },
        health_trend: parts.health_trend,
        actions_meta: build_health_actions_meta(action_ctx),
        framework_health: parts.framework_health,
        css_analytics: None,
        render_fan_in_top: parts.render_fan_in_top,
    };
    fill_coverage_intelligence(&mut report, opts);
    report
}

/// Populate `coverage_intelligence` from the built report unless score-only.
fn fill_coverage_intelligence(report: &mut HealthReport, opts: &HealthOptions<'_>) {
    if opts.score_only_output {
        return;
    }
    report.coverage_intelligence = coverage_intelligence::build_coverage_intelligence(
        report,
        opts.root,
        coverage_intelligence::CoverageIntelligenceContext {
            has_change_scope: opts.changed_since.is_some()
                || opts.diff_index.is_some()
                || opts.use_shared_diff_index,
        },
    );
}

fn build_report_coverage_gaps(
    report_coverage_gaps: bool,
    score_output: Option<&super::scoring::FileScoreOutput>,
) -> Option<crate::health_types::CoverageGaps> {
    report_coverage_gaps.then(|| score_output.map(|o| o.coverage.report.clone()))?
}

fn istanbul_counts_from_score_output(
    score_output: Option<&super::scoring::FileScoreOutput>,
) -> (usize, usize) {
    score_output.map_or((0, 0), |o| (o.istanbul_matched, o.istanbul_total))
}

/// Build the per-file top-render-fan-in lookup for the human drill-down: maps a
/// component file's absolute path to its highest-render-SITE component
/// `(component name, render sites)`. A file with several components keeps only
/// its most-rendered one (the file's blast-radius headline). Empty on non-React
/// runs (the core metric is `None`). Descriptive only; never serialized.
fn build_render_fan_in_top(
    score_output: Option<&super::scoring::FileScoreOutput>,
) -> rustc_hash::FxHashMap<std::path::PathBuf, (String, u32)> {
    let mut top: rustc_hash::FxHashMap<std::path::PathBuf, (String, u32)> =
        rustc_hash::FxHashMap::default();
    let Some(metric) = score_output.and_then(|o| o.render_fan_in.as_ref()) else {
        return top;
    };
    for component in &metric.per_component {
        let entry = top
            .entry(component.file.clone())
            .or_insert_with(|| (component.component.clone(), component.render_sites));
        if component.render_sites > entry.1 {
            *entry = (component.component.clone(), component.render_sites);
        }
    }
    top
}

fn report_hotspot_data(
    opts: &HealthOptions<'_>,
    hotspots: Vec<crate::health_types::HotspotEntry>,
    hotspot_summary: Option<crate::health_types::HotspotSummary>,
) -> (
    Vec<crate::health_types::HotspotEntry>,
    Option<crate::health_types::HotspotSummary>,
) {
    if opts.hotspots {
        (hotspots, hotspot_summary)
    } else {
        (Vec::new(), None)
    }
}

fn build_health_summary(
    opts: &HealthOptions<'_>,
    input: &HealthSummaryAssembly<'_>,
) -> HealthSummary {
    let (istanbul_matched, istanbul_total) = summary_istanbul_counts(
        opts,
        input.has_istanbul_coverage,
        input.istanbul_matched,
        input.istanbul_total,
    );
    HealthSummary {
        files_analyzed: input.files_analyzed,
        functions_analyzed: input.total_functions,
        functions_above_threshold: input.total_above_threshold,
        max_cyclomatic_threshold: input.max_cyclomatic,
        max_cognitive_threshold: input.max_cognitive,
        max_crap_threshold: input.max_crap,
        files_scored: summary_file_score_count(opts, input.files_scored),
        average_maintainability: summary_average_maintainability(
            opts,
            input.average_maintainability,
        ),
        coverage_model: summary_coverage_model(
            opts,
            input.report_coverage_gaps,
            input.has_istanbul_coverage,
        ),
        coverage_source_consistency: summary_coverage_source_consistency(opts, input.findings),
        istanbul_matched,
        istanbul_total,
        severity_critical_count: input.sev_critical,
        severity_high_count: input.sev_high,
        severity_moderate_count: input.sev_moderate,
    }
}

fn summary_file_score_count(
    opts: &HealthOptions<'_>,
    files_scored: Option<usize>,
) -> Option<usize> {
    if opts.score_only_output || !opts.file_scores {
        None
    } else {
        files_scored
    }
}

fn summary_average_maintainability(
    opts: &HealthOptions<'_>,
    average_maintainability: Option<f64>,
) -> Option<f64> {
    if opts.score_only_output || !opts.file_scores {
        None
    } else {
        average_maintainability
    }
}

fn summary_coverage_source_consistency(
    opts: &HealthOptions<'_>,
    findings: &[ComplexityViolation],
) -> Option<crate::health_types::CoverageSourceConsistency> {
    if opts.score_only_output || !opts.complexity {
        return None;
    }

    crate::health_types::summarize_coverage_source_consistency(
        findings
            .iter()
            .filter_map(|finding| finding.coverage_source),
    )
}

fn summary_coverage_model(
    opts: &HealthOptions<'_>,
    report_coverage_gaps: bool,
    has_istanbul_coverage: bool,
) -> Option<crate::health_types::CoverageModel> {
    if opts.score_only_output
        || !(opts.file_scores || report_coverage_gaps || opts.hotspots || opts.targets)
    {
        return None;
    }

    Some(if has_istanbul_coverage {
        crate::health_types::CoverageModel::Istanbul
    } else {
        crate::health_types::CoverageModel::StaticEstimated
    })
}

fn summary_istanbul_counts(
    opts: &HealthOptions<'_>,
    has_istanbul_coverage: bool,
    matched: usize,
    total: usize,
) -> (Option<usize>, Option<usize>) {
    if opts.score_only_output || !has_istanbul_coverage {
        (None, None)
    } else {
        (Some(matched), Some(total))
    }
}

fn build_report_threshold_overrides(
    opts: &HealthOptions<'_>,
    threshold_overrides: Vec<crate::health_types::ThresholdOverrideState>,
) -> Vec<crate::health_types::ThresholdOverrideState> {
    if opts.score_only_output {
        Vec::new()
    } else {
        threshold_overrides
    }
}

fn build_report_file_scores(
    opts: &HealthOptions<'_>,
    score_output: Option<super::scoring::FileScoreOutput>,
) -> Vec<crate::health_types::FileHealthScore> {
    if opts.score_only_output || !opts.file_scores {
        return Vec::new();
    }

    let mut scores = score_output.map(|o| o.scores).unwrap_or_default();
    if let Some(top) = opts.top {
        scores.truncate(top);
    }
    scores
}

fn build_report_findings(
    opts: &HealthOptions<'_>,
    action_ctx: &crate::health_types::HealthActionContext,
    findings: Vec<crate::health_types::ComplexityViolation>,
) -> Vec<crate::health_types::HealthFinding> {
    if !opts.complexity {
        return Vec::new();
    }

    findings
        .into_iter()
        .map(|v| crate::health_types::HealthFinding::with_actions(v, action_ctx))
        .collect()
}

fn build_report_hotspots(
    opts: &HealthOptions<'_>,
    hotspots: Vec<crate::health_types::HotspotEntry>,
) -> Vec<crate::health_types::HotspotFinding> {
    hotspots
        .into_iter()
        .map(|h| crate::health_types::HotspotFinding::with_actions(h, opts.root))
        .collect()
}

fn build_report_targets(
    opts: &HealthOptions<'_>,
    targets: Vec<crate::health_types::RefactoringTarget>,
) -> Vec<crate::health_types::RefactoringTargetFinding> {
    if opts.score_only_output {
        return Vec::new();
    }

    targets
        .into_iter()
        .map(crate::health_types::RefactoringTargetFinding::with_actions)
        .collect()
}

fn build_health_actions_meta(
    action_ctx: &crate::health_types::HealthActionContext,
) -> Option<crate::health_types::HealthActionsMeta> {
    if !action_ctx.opts.omit_suppress_line {
        return None;
    }

    Some(crate::health_types::HealthActionsMeta {
        suppression_hints_omitted: true,
        reason: action_ctx
            .opts
            .omit_reason
            .unwrap_or("unspecified")
            .to_string(),
        scope: "health-findings".to_string(),
    })
}
