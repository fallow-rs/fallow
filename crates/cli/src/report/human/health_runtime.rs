use std::path::Path;

use colored::Colorize;

use super::{MAX_FLAT_ITEMS, format_path, health::format_window, relative_path, thousands};

pub(super) fn render_runtime_coverage(
    lines: &mut Vec<String>,
    report: &crate::health_types::HealthReport,
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
    production: &crate::health_types::RuntimeCoverageReport,
) {
    let verdict = match production.verdict {
        crate::health_types::RuntimeCoverageReportVerdict::Clean => "clean",
        crate::health_types::RuntimeCoverageReportVerdict::HotPathTouched => "hot path touched",
        crate::health_types::RuntimeCoverageReportVerdict::ColdCodeDetected => "cold code detected",
        crate::health_types::RuntimeCoverageReportVerdict::LicenseExpiredGrace => {
            "license expired grace"
        }
        crate::health_types::RuntimeCoverageReportVerdict::Unknown => "unknown",
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
        Some(crate::health_types::RuntimeCoverageWatermark::LicenseExpiredGrace)
    ) {
        lines.push(
            "  license expired grace active; refresh with `fallow license refresh`".to_owned(),
        );
    }
}

fn render_runtime_findings(
    lines: &mut Vec<String>,
    production: &crate::health_types::RuntimeCoverageReport,
    root: &Path,
) {
    let shown_findings = production.findings.len().min(MAX_FLAT_ITEMS);
    for finding in &production.findings[..shown_findings] {
        let relative = format_path(&relative_path(&finding.path, root).display().to_string());
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
    production: &crate::health_types::RuntimeCoverageReport,
    root: &Path,
) {
    if !production.hot_paths.is_empty() {
        lines.push("  hot paths:".to_owned());
        for entry in production.hot_paths.iter().take(5) {
            let relative = format_path(&relative_path(&entry.path, root).display().to_string());
            lines.push(format!(
                "    {relative}:{} {} ({} invocations, p{})",
                entry.line,
                entry.function,
                thousands(entry.invocations as usize),
                entry.percentile,
            ));
        }
    }
}

fn render_runtime_warnings(
    lines: &mut Vec<String>,
    production: &crate::health_types::RuntimeCoverageReport,
) {
    for warning in &production.warnings {
        lines.push(format!("  warning [{}]: {}", warning.code, warning.message));
    }
}

fn render_capture_quality_warning(
    lines: &mut Vec<String>,
    production: &crate::health_types::RuntimeCoverageReport,
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
    production: &crate::health_types::RuntimeCoverageReport,
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
