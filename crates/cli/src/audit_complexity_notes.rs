//! Complexity notes of the audit view: the findings that are worse than the
//! base, and the findings in test files that do not gate the verdict.

use fallow_config::{AuditGate, AuditWorsened, OutputFormat};

use crate::report::sink::outln;

use super::AuditResult;

/// One row for each complexity finding whose measured values grew against
/// the matched base finding, with the base and head values. Prints nothing
/// without a base snapshot or when no value grew. Compact and markdown get
/// their own row shape.
pub(super) fn print_audit_worsened_section(
    result: &AuditResult,
    health: &crate::health::HealthResult,
    show_headers: bool,
) {
    let lines = audit_worsened_lines(result, health);
    if lines.is_empty() {
        return;
    }
    match result.output {
        OutputFormat::Markdown => {
            outln!();
            outln!("### Worse than base\n");
            outln!("| File | Function | Attribution | Grown metrics |");
            outln!("|:-----|:---------|:------------|:--------------|");
        }
        OutputFormat::Compact => {}
        _ => super::print_audit_section_header(
            show_headers,
            "── Worse than base ────────────────────────────────",
        ),
    }
    for line in lines {
        outln!("{line}");
    }
}

/// One complexity finding with grown metrics, for the worsened section.
struct WorsenedRow<'a> {
    path: String,
    line: u32,
    name: &'a str,
    worsened: bool,
    metrics: &'a [fallow_output::WorsenedMetric],
}

/// The rendered rows of [`print_audit_worsened_section`] in the output format
/// of the run.
fn audit_worsened_lines(result: &AuditResult, health: &crate::health::HealthResult) -> Vec<String> {
    let root = &health.config.root;
    health
        .report
        .findings
        .iter()
        .filter(|finding| !finding.worsened_metrics.is_empty())
        .map(|finding| {
            let row = WorsenedRow {
                path: finding
                    .path
                    .strip_prefix(root)
                    .unwrap_or(&finding.path)
                    .display()
                    .to_string()
                    .replace('\\', "/"),
                line: finding.line,
                name: &finding.name,
                worsened: finding.attribution
                    == Some(fallow_output::AuditFindingAttribution::Worsened),
                metrics: &finding.worsened_metrics,
            };
            render_worsened_row(&row, result.output, result.attribution.worsened)
        })
        .collect()
}

/// Compact token of a grown metric, the same spelling as its JSON value.
const fn worsened_metric_token(metric: fallow_output::WorsenedMetricKind) -> &'static str {
    match metric {
        fallow_output::WorsenedMetricKind::Cyclomatic => "cyclomatic",
        fallow_output::WorsenedMetricKind::Cognitive => "cognitive",
        fallow_output::WorsenedMetricKind::Crap => "crap",
        fallow_output::WorsenedMetricKind::LineCount => "line_count",
    }
}

fn render_worsened_row(row: &WorsenedRow<'_>, output: OutputFormat, mode: AuditWorsened) -> String {
    let attribution = if row.worsened {
        "worsened"
    } else {
        "introduced"
    };
    match output {
        OutputFormat::Compact => {
            let metrics = row
                .metrics
                .iter()
                .map(|metric| {
                    format!(
                        "{}={}->{}",
                        worsened_metric_token(metric.metric),
                        fallow_output::format_metric_value(metric.base),
                        fallow_output::format_metric_value(metric.head),
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            format!(
                "worse-than-base:{}:{}:{}:attribution={attribution},{metrics}",
                row.path, row.line, row.name
            )
        }
        OutputFormat::Markdown => format!(
            "| {} | {} | {attribution} | {} |",
            fallow_output::markdown_table_code_span(&format!("{}:{}", row.path, row.line)),
            fallow_output::markdown_table_code_span(row.name),
            fallow_output::markdown_table_text(&fallow_output::describe_worsened_metrics(
                row.metrics
            ))
        ),
        _ => {
            let note = if row.worsened {
                worsened_class_label(mode)
            } else {
                "introduced: a new limit or a higher tier"
            };
            format!(
                "  {}:{} {}  {}  ({note})",
                row.path,
                row.line,
                row.name,
                fallow_output::describe_worsened_metrics(row.metrics)
            )
        }
    }
}

/// How a worsened finding takes part in the verdict, for human output.
pub(super) const fn worsened_class_label(mode: AuditWorsened) -> &'static str {
    match mode {
        AuditWorsened::Warn => "worsened: warn only, --worsened error gates it",
        AuditWorsened::Error => "worsened: gated by --worsened error",
        AuditWorsened::Off => "worsened: off",
    }
}

/// One row for each complexity finding in a test file that would otherwise
/// take part in the gate: every such finding under `--gate all`, and the
/// introduced and worsened ones under `--gate new-only`. The JSON field
/// `gate_exemption` marks every finding in a test file.
pub(super) fn print_audit_test_file_section(
    result: &AuditResult,
    health: &crate::health::HealthResult,
    show_headers: bool,
) {
    let root = &health.config.root;
    let new_only = matches!(result.attribution.gate, AuditGate::NewOnly);
    let lines: Vec<String> = health
        .report
        .findings
        .iter()
        .filter(|finding| {
            finding.gate_exemption.is_some()
                && (!new_only
                    || finding.introduced == Some(true)
                    || finding.attribution
                        == Some(fallow_output::AuditFindingAttribution::Worsened))
        })
        .filter_map(|finding| {
            let exemption = finding.gate_exemption?;
            let path = finding
                .path
                .strip_prefix(root)
                .unwrap_or(&finding.path)
                .display()
                .to_string()
                .replace('\\', "/");
            Some(render_test_file_row(
                &path,
                finding.line,
                &finding.name,
                exemption,
                result.output,
            ))
        })
        .collect();
    if lines.is_empty() {
        return;
    }
    match result.output {
        OutputFormat::Markdown => {
            outln!();
            outln!("### Test files, not gated\n");
            outln!("| File | Function | Gate |");
            outln!("|:-----|:---------|:-----|");
        }
        OutputFormat::Compact => {}
        _ => super::print_audit_section_header(
            show_headers,
            "── Test files, not gated ──────────────────────────",
        ),
    }
    for line in lines {
        outln!("{line}");
    }
}

fn render_test_file_row(
    path: &str,
    line: u32,
    name: &str,
    exemption: fallow_output::AuditGateExemption,
    output: OutputFormat,
) -> String {
    match output {
        OutputFormat::Compact => {
            format!("gate-exemption:{path}:{line}:{name}:{}", exemption.token())
        }
        OutputFormat::Markdown => format!(
            "| {} | {} | warn only: test file |",
            fallow_output::markdown_table_code_span(&format!("{path}:{line}")),
            fallow_output::markdown_table_code_span(name),
        ),
        _ => format!("  {path}:{line} {name}  (test file: warn only, audit does not gate it)"),
    }
}
