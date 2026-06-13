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
    let HealthReportAssembly {
        report_coverage_gaps,
        findings,
        threshold_overrides,
        files_analyzed,
        total_functions,
        total_above_threshold,
        max_cyclomatic,
        max_cognitive,
        max_crap,
        files_scored,
        average_maintainability,
        vital_signs,
        health_score,
        score_output,
        hotspots,
        hotspot_summary,
        targets,
        target_thresholds,
        health_trend,
        has_istanbul_coverage,
        runtime_coverage,
        large_functions,
        sev_critical,
        sev_high,
        sev_moderate,
    } = assembly;
    let coverage_gaps = build_report_coverage_gaps(report_coverage_gaps, score_output.as_ref());
    let (ist_matched, ist_total) = istanbul_counts_from_score_output(score_output.as_ref());
    let file_scores = build_report_file_scores(opts, score_output);
    let (report_hotspots, report_hotspot_summary) =
        report_hotspot_data(opts, hotspots, hotspot_summary);
    let summary = build_health_summary(
        opts,
        &HealthSummaryAssembly {
            findings: &findings,
            files_analyzed,
            total_functions,
            total_above_threshold,
            max_cyclomatic,
            max_cognitive,
            max_crap,
            files_scored,
            average_maintainability,
            report_coverage_gaps,
            has_istanbul_coverage,
            istanbul_matched: ist_matched,
            istanbul_total: ist_total,
            sev_critical,
            sev_high,
            sev_moderate,
        },
    );

    let mut report = HealthReport {
        summary,
        threshold_overrides: build_report_threshold_overrides(opts, threshold_overrides),
        vital_signs: if opts.score_only_output {
            None
        } else {
            Some(vital_signs)
        },
        health_score,
        findings: build_report_findings(opts, action_ctx, findings),
        file_scores,
        coverage_gaps: if opts.score_only_output {
            None
        } else {
            coverage_gaps
        },
        hotspots: build_report_hotspots(opts, report_hotspots),
        hotspot_summary: if opts.score_only_output {
            None
        } else {
            report_hotspot_summary
        },
        runtime_coverage,
        coverage_intelligence: None,
        large_functions: if opts.score_only_output {
            Vec::new()
        } else {
            large_functions
        },
        targets: build_report_targets(opts, targets),
        target_thresholds: if opts.score_only_output {
            None
        } else {
            target_thresholds
        },
        health_trend,
        actions_meta: build_health_actions_meta(action_ctx),
        css_analytics: None,
    };
    if !opts.score_only_output {
        report.coverage_intelligence = coverage_intelligence::build_coverage_intelligence(
            &report,
            opts.root,
            coverage_intelligence::CoverageIntelligenceContext {
                has_change_scope: opts.changed_since.is_some()
                    || opts.diff_index.is_some()
                    || opts.use_shared_diff_index,
            },
        );
    }
    report
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
