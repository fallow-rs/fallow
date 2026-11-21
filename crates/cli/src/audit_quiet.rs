//! The actionable view of an audit for `--quiet` in the human, compact and
//! markdown formats.
//!
//! The view keeps introduced and worsened findings, diff-risk findings, the
//! configuration and analysis diagnostics, and the threshold-override records
//! that need a configuration change (`insufficient` and `no_match`). It drops
//! inherited findings, aggregate metrics and the routine override records
//! (`stale` and `active`). The verdict was decided before the filter runs, so
//! the exit code does not change.

use std::process::ExitCode;

use fallow_config::OutputFormat;
use fallow_output::{AuditFindingAttribution, HealthReport, ThresholdOverrideStatus};

use super::super::{AuditOptions, AuditResult};

/// Print the report of a plain audit run and return its exit code. Under
/// `--quiet`, the report is the actionable view of `result`. The verdict in
/// `result` does not change, so the exit code does not change.
pub fn print_audit_report(
    opts: &AuditOptions<'_>,
    result: &mut AuditResult,
    json_style: crate::json_style::JsonStyle,
) -> ExitCode {
    if opts.quiet && quiet_filters_format(opts.output) {
        retain_actionable(result);
    }
    super::print_audit_result_with_style(result, opts.quiet, opts.explain, json_style)
}

/// Whether `--quiet` filters the report of this format. Machine formats
/// (JSON, SARIF, CodeClimate, review and GitHub formats) keep the complete
/// report, because their consumers read the inherited context.
const fn quiet_filters_format(output: OutputFormat) -> bool {
    matches!(
        output,
        OutputFormat::Human | OutputFormat::Compact | OutputFormat::Markdown
    )
}

/// Whether the complexity section has rows to print: complexity findings, or
/// under `--quiet` the override records that need a configuration change.
/// Those records stay when every complexity finding is inherited.
pub(super) fn complexity_section_has_rows(result: &AuditResult, quiet: bool) -> bool {
    result.summary.complexity_findings > 0
        || (quiet
            && result.health.as_ref().is_some_and(|health| {
                !health.report.threshold_overrides.is_empty()
                    || !health.report.inline_threshold_overrides.is_empty()
                    || !health.report.inline_threshold_override_problems.is_empty()
            }))
}

/// Print one line when the quiet human or markdown report has no row. The
/// compact format prints one line per record, so it stays empty.
pub(super) fn print_quiet_empty_state(output: OutputFormat) {
    if matches!(output, OutputFormat::Human | OutputFormat::Markdown) {
        crate::report::sink::outln!("{QUIET_EMPTY_STATE}");
    }
}

/// The quiet human and markdown report when no row is actionable.
const QUIET_EMPTY_STATE: &str = "No introduced or worsened findings.";

/// Reduce `result` to its actionable view. Without a base snapshot (for
/// example under `--gate all`) every finding is part of the gate, so only the
/// aggregate metrics and the routine override records go.
fn retain_actionable(result: &mut AuditResult) {
    let attributed = result.base_snapshot.is_some();
    if let Some(health) = result.health.as_mut() {
        retain_actionable_health(&mut health.report, attributed);
        result.summary.complexity_findings = health.report.findings.len();
    }
    if !attributed {
        return;
    }
    let Some(comparison) = result.comparison.as_ref() else {
        return;
    };
    if let Some(check) = result.check.as_mut() {
        comparison
            .dead_code
            .retain_introduced_results(&mut check.results);
        result.summary.dead_code_issues = comparison.dead_code.visible_introduced_count();
    }
    if let Some(dupes) = result.dupes.as_mut() {
        let mut introduced = comparison.dupes.introduced();
        dupes
            .report
            .clone_groups
            .retain(|_| introduced.next().unwrap_or(true));
        result.summary.duplication_clone_groups = dupes.report.clone_groups.len();
    }
}

fn retain_actionable_health(report: &mut HealthReport, attributed: bool) {
    if attributed {
        report
            .findings
            .retain(|finding| finding.attribution != Some(AuditFindingAttribution::Inherited));
        report
            .styling_findings
            .retain(|finding| finding.internal.introduced != Some(false));
    }
    report.threshold_overrides.retain(|entry| {
        !matches!(
            entry.status,
            ThresholdOverrideStatus::Stale | ThresholdOverrideStatus::Active
        )
    });
    report.inline_threshold_overrides.retain(|entry| {
        !matches!(
            entry.status,
            ThresholdOverrideStatus::Stale | ThresholdOverrideStatus::Active
        )
    });
    report.vital_signs = None;
    report.health_score = None;
    report.file_scores.clear();
    report.hotspots.clear();
    report.hotspot_summary = None;
    report.change_coupling = None;
    report.large_functions.clear();
    report.targets.clear();
    report.target_thresholds = None;
    report.health_trend = None;
    report.css_analytics = None;
    report.styling_health = None;
}
