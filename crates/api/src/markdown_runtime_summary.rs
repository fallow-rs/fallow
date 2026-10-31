use std::fmt::Write as _;

use super::format_window;

/// Write the runtime-coverage summary header and capture-quality lines.
pub(super) fn write_runtime_coverage_summary(
    out: &mut String,
    production: &fallow_output::RuntimeCoverageReport,
) {
    if production.has_positive_only_capture() {
        let _ = writeln!(
            out,
            "## Runtime Coverage\n\nPartial capture provides positive execution evidence only.\n\n- Observed called functions: {}\n- Untracked: {}\n",
            production.summary.functions_hit, production.summary.functions_untracked
        );
        return;
    }
    let _ = writeln!(
        out,
        "## Runtime Coverage\n\n- Verdict: {}\n- Functions tracked: {}\n- Hit: {}\n- Unhit: {}\n- Untracked: {}\n- Coverage: {:.1}%\n- Traces observed: {}\n- Period: {} day(s), {} deployment(s)\n",
        production.verdict,
        production.summary.functions_tracked.unwrap_or_default(),
        production.summary.functions_hit,
        production.summary.functions_unhit.unwrap_or_default(),
        production.summary.functions_untracked,
        production.summary.coverage_percent.unwrap_or_default(),
        production.summary.trace_count,
        production.summary.period_days,
        production.summary.deployments_seen,
    );
    if let Some(watermark) = production.watermark {
        let _ = writeln!(out, "- Watermark: {watermark}\n");
    }
    if let Some(ref quality) = production.summary.capture_quality
        && quality.lazy_parse_warning
    {
        let window = format_window(quality.window_seconds);
        let _ = writeln!(
            out,
            "- Capture quality: short window ({} from {} instance(s), {:.1}% of functions untracked); lazy-parsed scripts may not appear.\n",
            window, quality.instances_observed, quality.untracked_ratio_percent,
        );
    }
}
