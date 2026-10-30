//! Limit managed partial captures to positive execution evidence.

use fallow_output::{
    RuntimeCoverageCaptureQuality, RuntimeCoverageDataSource, RuntimeCoverageReport,
    RuntimeCoverageSummary,
};

use super::cloud_client::{
    CloudNeverCalledSource, CloudRuntimeContext, CloudRuntimeWarning, CloudTrackingState,
};

const POSITIVE_ONLY_MESSAGE: &str = "Partial capture provides positive execution evidence only. Application coverage percentages and cold or deletion verdicts are unavailable.";

pub(super) fn apply_capture_policy(context: &mut CloudRuntimeContext) {
    if context.capture_policy.is_none() {
        return;
    }
    context.actionable = Some(false);
    context.actionability_reason = Some(POSITIVE_ONLY_MESSAGE.to_owned());
    context.verdict = Some("insufficient_evidence".to_owned());
    for function in &mut context.functions {
        if function.tracking_state == CloudTrackingState::Called
            && function.hit_count.unwrap_or(0) > 0
        {
            continue;
        }
        function.tracking_state = CloudTrackingState::Untracked;
        function.period_tracking_state = Some(CloudTrackingState::Untracked);
        function.hit_count = None;
        function.never_called_source = CloudNeverCalledSource::Unknown;
    }
    context.warnings.push(CloudRuntimeWarning::Object {
        code: Some("positive_only_capture".to_owned()),
        message: Some(POSITIVE_ONLY_MESSAGE.to_owned()),
    });
}

pub(super) fn print_runtime_summary(report: &RuntimeCoverageReport) {
    if report.has_positive_only_capture() {
        println!("Runtime coverage: partial capture");
        println!(
            "  {} functions with positive execution evidence, {} untracked",
            report.summary.functions_hit, report.summary.functions_untracked
        );
        return;
    }
    println!("Runtime coverage: {}", report.verdict);
    let count_or_unknown = |count: Option<usize>| {
        count.map_or_else(|| "unknown".to_owned(), |value| value.to_string())
    };
    println!(
        "  {} tracked, {} hit, {} unhit, {} untracked ({}% covered)",
        count_or_unknown(report.summary.functions_tracked),
        report.summary.functions_hit,
        count_or_unknown(report.summary.functions_unhit),
        report.summary.functions_untracked,
        report
            .summary
            .coverage_percent
            .map_or_else(|| "unknown".to_owned(), |percent| format!("{percent:.1}")),
    );
}

pub(super) fn report_verdict(
    context: &CloudRuntimeContext,
    findings: &[fallow_output::RuntimeCoverageFinding],
) -> fallow_output::RuntimeCoverageReportVerdict {
    if context.capture_policy.is_some() {
        return fallow_output::RuntimeCoverageReportVerdict::Unknown;
    }
    if findings.is_empty() {
        fallow_output::RuntimeCoverageReportVerdict::Clean
    } else {
        fallow_output::RuntimeCoverageReportVerdict::ColdCodeDetected
    }
}

/// A partial capture reports positive execution evidence only. The server turns
/// never-called functions into untracked ones, so the tracked total, the unhit
/// total and the coverage percent are not measured and serialize as null.
pub(super) fn cloud_report_summary(
    snapshot: &CloudRuntimeContext,
    capture_quality: Option<RuntimeCoverageCaptureQuality>,
) -> RuntimeCoverageSummary {
    let measured = snapshot.capture_policy.is_none();
    RuntimeCoverageSummary {
        data_source: RuntimeCoverageDataSource::Cloud,
        last_received_at: snapshot.summary.last_received_at.clone(),
        functions_tracked: measured.then_some(snapshot.summary.functions_tracked),
        functions_hit: snapshot.summary.functions_hit,
        functions_unhit: measured.then_some(snapshot.summary.functions_unhit),
        functions_untracked: snapshot.summary.functions_untracked,
        coverage_percent: measured.then_some(snapshot.summary.coverage_percent),
        trace_count: snapshot.summary.trace_count,
        period_days: snapshot.window.period_days,
        deployments_seen: snapshot.summary.deployments_seen,
        capture_quality,
    }
}
