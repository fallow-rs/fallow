//! Limit managed partial captures to positive execution evidence.

use fallow_output::RuntimeCoverageReport;

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
    println!(
        "  {} tracked, {} hit, {} unhit, {} untracked ({:.1}% covered)",
        report.summary.functions_tracked,
        report.summary.functions_hit,
        report.summary.functions_unhit,
        report.summary.functions_untracked,
        report.summary.coverage_percent,
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
