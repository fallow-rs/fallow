//! The final status line of a human health run.

use std::time::Duration;

use colored::Colorize;

/// The final status line of a health run. A run with no finding and no
/// failing gate shows the success mark. Otherwise the mark follows the gate
/// result, see [`super::findings_status_line`].
pub(super) fn print_health_final_status(
    report: &fallow_output::HealthReport,
    elapsed: Duration,
    has_findings: bool,
    run_fails: bool,
) {
    let s = &report.summary;
    let mut parts = vec![above_threshold_part(s)];
    parts.push(format!("{} analyzed", s.functions_analyzed));
    let styling = report.styling_findings.len();
    if styling > 0 {
        parts.push(format!(
            "{styling} styling finding{}",
            if styling == 1 { "" } else { "s" }
        ));
    }
    if let Some(avg) = s.average_maintainability {
        let label = if avg >= 85.0 {
            "good"
        } else if avg >= 65.0 {
            "moderate"
        } else {
            "low"
        };
        parts.push(format!("maintainability {avg:.1} ({label})"));
    }
    if let Some(unhit) = report
        .runtime_coverage
        .as_ref()
        .and_then(|production| production.summary.functions_unhit)
    {
        parts.push(format!("{unhit} unhit in production"));
    }
    let text = format!(
        "{} ({:.2}s)",
        parts.join(" \u{00b7} "),
        elapsed.as_secs_f64()
    );
    if has_findings || run_fails {
        eprintln!("{}", super::findings_status_line(&text, run_fails));
    } else {
        eprintln!("{}", format!("\u{2713} {text}").green().bold());
    }
    if s.average_maintainability.is_some_and(|mi| mi < 85.0) {
        eprintln!(
            "{}",
            "  Maintainability scale: good \u{2265}85, moderate \u{2265}65, low <65 (0\u{2013}100)"
                .dimmed()
        );
    }
}

/// `<n> above threshold` for the final status line. With a loaded health
/// baseline, `n` counts only the functions that the baseline does not accept,
/// and the part names the accepted functions: `0 above threshold (3 in the
/// baseline)`.
fn above_threshold_part(summary: &fallow_output::HealthSummary) -> String {
    let (remaining, accepted) = functions_above_after_baseline(summary);
    if accepted == 0 {
        format!("{remaining} above threshold")
    } else {
        format!("{remaining} above threshold ({accepted} in the baseline)")
    }
}

/// The functions above a threshold that a loaded baseline does not accept,
/// and the functions that it accepts. Without a baseline, every function is
/// in the first count.
pub(super) fn functions_above_after_baseline(
    summary: &fallow_output::HealthSummary,
) -> (usize, usize) {
    let total = summary.functions_above_threshold;
    summary
        .baseline_staleness
        .as_ref()
        .and_then(|staleness| staleness.remaining_findings)
        .map_or((total, 0), |remaining| {
            (remaining, total.saturating_sub(remaining))
        })
}
