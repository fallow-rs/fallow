use std::path::Path;

use colored::Colorize;

use super::{MAX_FLAT_ITEMS, format_path, health::format_window, thousands};
use crate::report::format_display_path;

pub(super) fn render_runtime_coverage(
    lines: &mut Vec<String>,
    report: &fallow_output::HealthReport,
    root: &Path,
) {
    let Some(ref production) = report.runtime_coverage else {
        return;
    };

    render_runtime_summary(lines, production);
    render_capture_quality_warning(lines, production);
    render_runtime_findings(lines, production, root);
    render_runtime_hot_paths(lines, production, root);
    render_runtime_warnings(lines, production);
    render_upgrade_prompt(lines, production);
    lines.push(String::new());
}

fn render_runtime_summary(
    lines: &mut Vec<String>,
    production: &fallow_output::RuntimeCoverageReport,
) {
    let verdict = match production.verdict {
        fallow_output::RuntimeCoverageReportVerdict::Clean => "clean",
        fallow_output::RuntimeCoverageReportVerdict::HotPathTouched => "hot path touched",
        fallow_output::RuntimeCoverageReportVerdict::ColdCodeDetected => "cold code detected",
        fallow_output::RuntimeCoverageReportVerdict::LicenseExpiredGrace => "license expired grace",
        fallow_output::RuntimeCoverageReportVerdict::Unknown => "unknown",
    };
    lines.push(format!(
        "{} {} {}",
        "\u{25cf}".cyan(),
        "Runtime coverage:".cyan().bold(),
        verdict
    ));
    lines.push(format!(
        "  {} tracked, {} hit, {} unhit, {} untracked ({:.1}% covered)",
        thousands(production.summary.functions_tracked),
        thousands(production.summary.functions_hit),
        thousands(production.summary.functions_unhit),
        thousands(production.summary.functions_untracked),
        production.summary.coverage_percent,
    ));
    if production.summary.trace_count > 0 || production.summary.period_days > 0 {
        lines.push(format!(
            "  based on {} traces over {} day{} ({} deployment{})",
            thousands(production.summary.trace_count as usize),
            production.summary.period_days,
            if production.summary.period_days == 1 {
                ""
            } else {
                "s"
            },
            production.summary.deployments_seen,
            if production.summary.deployments_seen == 1 {
                ""
            } else {
                "s"
            },
        ));
    }
    if matches!(
        production.watermark,
        Some(fallow_output::RuntimeCoverageWatermark::LicenseExpiredGrace)
    ) {
        lines.push(
            "  license expired grace active; refresh with `fallow license refresh`".to_owned(),
        );
    }
}

fn render_runtime_findings(
    lines: &mut Vec<String>,
    production: &fallow_output::RuntimeCoverageReport,
    root: &Path,
) {
    let shown_findings = production.findings.len().min(MAX_FLAT_ITEMS);
    for finding in &production.findings[..shown_findings] {
        let relative = format_path(&format_display_path(&finding.path, root));
        let invocations = finding.invocations.map_or_else(
            || "untracked".to_owned(),
            |hits| format!("{hits} invocations"),
        );
        lines.push(format!(
            "  {relative}:{} {} [{}, {}]",
            finding.line,
            finding.function,
            invocations,
            finding.verdict.human_label(),
        ));
    }
    if production.findings.len() > MAX_FLAT_ITEMS {
        lines.push(format!(
            "  ... and {} more production findings (--format json for full list)",
            production.findings.len() - MAX_FLAT_ITEMS
        ));
    }
}

fn render_runtime_hot_paths(
    lines: &mut Vec<String>,
    production: &fallow_output::RuntimeCoverageReport,
    root: &Path,
) {
    if !production.hot_paths.is_empty() {
        lines.push("  hot paths:".to_owned());
        for entry in production.hot_paths.iter().take(5) {
            let relative = format_path(&format_display_path(&entry.path, root));
            lines.push(format!(
                "    {relative}:{} {} ({} invocations, p{}{})",
                entry.line,
                entry.function,
                thousands(entry.invocations as usize),
                entry.percentile,
                optimization_cost_suffix(entry.optimization_target.as_ref()),
            ));
        }
    }
}

fn optimization_cost_suffix(
    target: Option<&fallow_output::RuntimeCoverageOptimizationTarget>,
) -> String {
    let Some(target) = target else {
        return String::new();
    };
    let score = thousands(usize::try_from(target.cost_score).unwrap_or(usize::MAX));
    match target.inner_iterations_per_call {
        Some(ratio) => format!(", cost {score} at {ratio:.2} iterations/call"),
        None => format!(", cost {score} at cognitive {}", target.cognitive),
    }
}

fn render_runtime_warnings(
    lines: &mut Vec<String>,
    production: &fallow_output::RuntimeCoverageReport,
) {
    for warning in &production.warnings {
        lines.push(format!("  warning [{}]: {}", warning.code, warning.message));
    }
}

fn render_capture_quality_warning(
    lines: &mut Vec<String>,
    production: &fallow_output::RuntimeCoverageReport,
) {
    let Some(ref quality) = production.summary.capture_quality else {
        return;
    };
    if !quality.lazy_parse_warning {
        return;
    }
    let instances = quality.instances_observed;
    let instance_label = if instances == 1 {
        "instance"
    } else {
        "instances"
    };
    let window = format_window(quality.window_seconds);
    lines.push(format!(
        "  {}",
        format!(
            "note: short capture ({window} from {instances} {instance_label}); {:.1}% of functions untracked, lazy-parsed scripts may not appear.",
            quality.untracked_ratio_percent,
        )
        .yellow()
    ));
    lines.push(
        "  extend the capture or switch to continuous monitoring for a trustworthy reading."
            .to_owned(),
    );
}

fn render_upgrade_prompt(
    lines: &mut Vec<String>,
    production: &fallow_output::RuntimeCoverageReport,
) {
    let Some(ref quality) = production.summary.capture_quality else {
        return;
    };
    if !quality.lazy_parse_warning {
        return;
    }
    let window = format_window(quality.window_seconds);
    let instances = quality.instances_observed;
    let instance_label = if instances == 1 {
        "instance"
    } else {
        "instances"
    };
    lines.push(format!(
        "  captured {window} from {instances} {instance_label}."
    ));
    lines.push(
        "  continuous monitoring over 30 days evaluates more paths and surfaces additional candidates the local capture missed."
            .to_owned(),
    );
    lines.push(
        "  start a trial: `fallow license activate --trial --email you@company.com`".to_owned(),
    );
}

#[cfg(test)]
mod tests {
    use fallow_output::{RuntimeCoverageCostBasis, RuntimeCoverageOptimizationTarget};

    use super::optimization_cost_suffix;

    fn target(
        cost_score: u64,
        cost_basis: RuntimeCoverageCostBasis,
        inner_iterations_per_call: Option<f64>,
    ) -> RuntimeCoverageOptimizationTarget {
        RuntimeCoverageOptimizationTarget {
            cost_score,
            cost_basis,
            cognitive: 7,
            cyclomatic: 5,
            line_count: 15,
            inner_iterations_per_call,
        }
    }

    #[test]
    fn hot_path_line_shows_measured_cost() {
        let target = target(3_600, RuntimeCoverageCostBasis::InnerIterations, Some(3.0));
        assert_eq!(
            optimization_cost_suffix(Some(&target)),
            ", cost 3,600 at 3.00 iterations/call"
        );
    }

    #[test]
    fn hot_path_line_shows_static_cost_without_block_counts() {
        let target = target(1_050, RuntimeCoverageCostBasis::Cognitive, None);
        assert_eq!(
            optimization_cost_suffix(Some(&target)),
            ", cost 1,050 at cognitive 7"
        );
    }

    #[test]
    fn hot_path_line_is_unchanged_without_a_target() {
        assert_eq!(optimization_cost_suffix(None), "");
    }
}
