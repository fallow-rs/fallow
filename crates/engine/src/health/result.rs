//! Health result assembly helpers.

use std::time::Duration;

use fallow_config::ResolvedConfig;
use fallow_output::{HealthGrouping, HealthReport, HealthTimings};
use fallow_types::discover::DiscoveredFile;
use fallow_types::workspace::WorkspaceDiagnostic;

use crate::results::HealthAnalysisResult;

use super::HealthExecutionOptions;
use super::css_analytics::{HealthScanCtx, compute_css_analytics_report};
use super::pipeline::HealthScope;

pub(super) struct HealthOutputParts {
    pub(super) report: HealthReport,
    pub(super) grouping: Option<HealthGrouping>,
    pub(super) timings: Option<HealthTimings>,
    pub(super) coverage_gaps_has_findings: bool,
}

struct HealthReportSideEffectsInput<'a> {
    opts: &'a HealthExecutionOptions<'a>,
    report: &'a mut HealthReport,
    files: &'a [DiscoveredFile],
    config: &'a ResolvedConfig,
    ignore_set: &'a globset::GlobSet,
    changed_files: Option<&'a rustc_hash::FxHashSet<std::path::PathBuf>>,
    ws_roots: Option<&'a [std::path::PathBuf]>,
}

pub(super) struct HealthFinalizeInput<'a, R> {
    pub(super) opts: &'a HealthExecutionOptions<'a>,
    pub(super) config: ResolvedConfig,
    pub(super) files: &'a [DiscoveredFile],
    pub(super) scope: HealthScope<'a, R>,
    pub(super) output: HealthOutputParts,
    pub(super) elapsed: Duration,
    pub(super) should_fail_on_coverage_gaps: bool,
    pub(super) workspace_diagnostics: Vec<WorkspaceDiagnostic>,
}

struct HealthResultInput<R> {
    config: ResolvedConfig,
    report: HealthReport,
    grouping: Option<HealthGrouping>,
    group_resolver: Option<R>,
    elapsed: Duration,
    timings: Option<HealthTimings>,
    coverage_gaps_has_findings: bool,
    should_fail_on_coverage_gaps: bool,
    workspace_diagnostics: Vec<WorkspaceDiagnostic>,
}

pub(super) fn finalize_health_result<R>(
    input: HealthFinalizeInput<'_, R>,
) -> HealthAnalysisResult<R> {
    let HealthFinalizeInput {
        opts,
        config,
        files,
        scope,
        output,
        elapsed,
        should_fail_on_coverage_gaps,
        workspace_diagnostics,
    } = input;
    let HealthOutputParts {
        mut report,
        grouping,
        timings,
        coverage_gaps_has_findings,
    } = output;

    finalize_health_report_side_effects(&mut HealthReportSideEffectsInput {
        opts,
        report: &mut report,
        files,
        config: &config,
        ignore_set: &scope.ignore_set,
        changed_files: scope.changed_files.as_ref(),
        ws_roots: scope.ws_roots.as_deref(),
    });

    build_health_result(HealthResultInput {
        config,
        report,
        grouping,
        group_resolver: scope.group_resolver,
        elapsed,
        timings,
        coverage_gaps_has_findings,
        should_fail_on_coverage_gaps,
        workspace_diagnostics,
    })
}

fn finalize_health_report_side_effects(input: &mut HealthReportSideEffectsInput<'_>) {
    if input.opts.css {
        let computation = compute_css_analytics_report(
            input.files,
            HealthScanCtx {
                config: input.config,
                ignore_set: input.ignore_set,
                changed_files: input.changed_files,
                ws_roots: input.ws_roots,
            },
        );
        input.report.styling_health = computation.as_ref().map(|computation| {
            super::styling_score::compute_styling_health_with_inputs(
                &computation.report,
                &computation.scoring_inputs,
            )
        });
        input.report.css_analytics = computation.map(|computation| computation.report);
    }
}

fn build_health_result<R>(input: HealthResultInput<R>) -> HealthAnalysisResult<R> {
    let HealthResultInput {
        config,
        report,
        grouping,
        group_resolver,
        elapsed,
        timings,
        coverage_gaps_has_findings,
        should_fail_on_coverage_gaps,
        workspace_diagnostics,
    } = input;

    HealthAnalysisResult {
        report,
        grouping,
        group_resolver,
        config,
        workspace_diagnostics,
        elapsed,
        timings,
        coverage_gaps_has_findings,
        should_fail_on_coverage_gaps,
    }
}
