use crate::report::sink::outln;
use std::fmt::Write as _;
use std::path::Path;
use std::time::Duration;

use colored::Colorize;

use super::health_hotspots::render_hotspots;
use super::health_runtime::render_runtime_coverage;
use super::health_targets::render_refactoring_targets;
use super::{
    MAX_FLAT_ITEMS, format_path, plural, print_explain_tip_if_tty, relative_path,
    split_dir_filename, thousands,
};
use crate::health::scoring::{FileScoreConcern, file_score_concern_axis};

/// Docs base URL for health explanations.
const DOCS_HEALTH: &str = "https://docs.fallow.tools/explanations/health";

pub(in crate::report) struct PrintHealthHumanInput<'a> {
    pub(in crate::report) report: &'a crate::health_types::HealthReport,
    pub(in crate::report) root: &'a Path,
    pub(in crate::report) elapsed: Duration,
    pub(in crate::report) quiet: bool,
    pub(in crate::report) show_explain_tip: bool,
    pub(in crate::report) explain: bool,
    pub(in crate::report) skip_score_and_trend: bool,
}

pub(in crate::report) fn print_health_human(input: &PrintHealthHumanInput<'_>) {
    let report = input.report;
    let root = input.root;
    let elapsed = input.elapsed;
    let quiet = input.quiet;
    let show_explain_tip = input.show_explain_tip;
    let explain = input.explain;
    let skip_score_and_trend = input.skip_score_and_trend;
    if !quiet {
        eprintln!();
    }

    let has_score = report.health_score.is_some();
    if report.findings.is_empty()
        && report.file_scores.is_empty()
        && report.coverage_gaps.is_none()
        && report.hotspots.is_empty()
        && report.targets.is_empty()
        && report.runtime_coverage.is_none()
        && report.coverage_intelligence.is_none()
        && !has_score
    {
        if !quiet {
            eprintln!(
                "{}",
                format!(
                    "\u{2713} No functions exceed complexity thresholds ({:.2}s)",
                    elapsed.as_secs_f64()
                )
                .green()
                .bold()
            );
            eprintln!(
                "{}",
                format!(
                    "  {} functions analyzed (max cyclomatic: {}, max cognitive: {}, max CRAP: {:.1})",
                    report.summary.functions_analyzed,
                    report.summary.max_cyclomatic_threshold,
                    report.summary.max_cognitive_threshold,
                    report.summary.max_crap_threshold,
                )
                .dimmed()
            );
        }
        return;
    }

    let has_findings = !report.findings.is_empty()
        || report.coverage_gaps.as_ref().is_some_and(|gaps| {
            gaps.summary.untested_files > 0 || gaps.summary.untested_exports > 0
        })
        || report
            .runtime_coverage
            .as_ref()
            .is_some_and(|coverage| !coverage.findings.is_empty());
    print_explain_tip_if_tty(show_explain_tip && has_findings, quiet);

    let lines = build_health_human_lines_with_explain(report, root, explain, skip_score_and_trend);
    for line in lines {
        outln!("{line}");
    }

    if !quiet {
        let s = &report.summary;
        let mut parts = Vec::new();
        parts.push(format!("{} above threshold", s.functions_above_threshold));
        parts.push(format!("{} analyzed", s.functions_analyzed));
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
        if let Some(ref production) = report.runtime_coverage {
            parts.push(format!(
                "{} unhit in production",
                production.summary.functions_unhit
            ));
        }
        eprintln!(
            "{}",
            format!(
                "\u{2717} {} ({:.2}s)",
                parts.join(" \u{00b7} "),
                elapsed.as_secs_f64()
            )
            .red()
            .bold()
        );
        if s.average_maintainability.is_some_and(|mi| mi < 85.0) {
            eprintln!(
                "{}",
                "  Maintainability scale: good \u{2265}85, moderate \u{2265}65, low <65 (0\u{2013}100)"
                    .dimmed()
            );
        }
    }
}

/// Build human-readable output lines for health (complexity) findings.
///
#[cfg(test)]
fn build_health_human_lines(
    report: &crate::health_types::HealthReport,
    root: &Path,
) -> Vec<String> {
    build_health_human_lines_with_explain(report, root, false, false)
}

fn build_health_human_lines_with_explain(
    report: &crate::health_types::HealthReport,
    root: &Path,
    explain: bool,
    skip_score_and_trend: bool,
) -> Vec<String> {
    let mut lines = Vec::new();
    if !skip_score_and_trend {
        render_health_score(&mut lines, report);
        render_health_trend(&mut lines, report);
    }
    render_runtime_coverage(&mut lines, report, root);
    render_coverage_intelligence(&mut lines, report, root);
    render_vital_signs(&mut lines, report);
    render_risk_profiles(&mut lines, report);
    render_large_functions(&mut lines, report, root);
    render_findings(&mut lines, report, root);
    render_coverage_gaps(&mut lines, report, root);
    render_file_scores(&mut lines, report, root);
    render_hotspots(&mut lines, report, root);
    render_refactoring_targets(&mut lines, report, root);
    if explain {
        inject_explain_blocks(lines)
    } else {
        lines
    }
}

fn render_coverage_intelligence(
    lines: &mut Vec<String>,
    report: &crate::health_types::HealthReport,
    root: &Path,
) {
    let Some(ref intelligence) = report.coverage_intelligence else {
        return;
    };

    lines.push(String::new());
    lines.push("Coverage intelligence".bold().to_string());
    lines.push(
        format!("  Verdict: {}", intelligence.verdict)
            .bold()
            .to_string(),
    );
    if intelligence.findings.is_empty() {
        if intelligence.summary.skipped_ambiguous_matches > 0 {
            let match_word = if intelligence.summary.skipped_ambiguous_matches == 1 {
                "match"
            } else {
                "matches"
            };
            lines.push(format!(
                "  No actionable findings; skipped {} ambiguous evidence {match_word}.",
                intelligence.summary.skipped_ambiguous_matches
            ));
        }
        return;
    }
    for finding in intelligence.findings.iter().take(MAX_FLAT_ITEMS) {
        let relative = relative_path(&finding.path, root);
        let identity = finding
            .identity
            .as_deref()
            .map_or(String::new(), |name| format!(" {name}"));
        let signals = finding
            .signals
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ");
        let action = finding
            .actions
            .first()
            .map_or("Review this finding", |action| action.description.as_str());
        lines.push(format!(
            "  {}:{}{} {} [{}]",
            format_path(&relative.display().to_string()),
            finding.line,
            identity,
            finding.verdict,
            signals,
        ));
        lines.push(format!("    {action}"));
    }
}

fn inject_explain_blocks(lines: Vec<String>) -> Vec<String> {
    let mut out = Vec::with_capacity(lines.len());
    for line in lines {
        let explain = health_explain_for_header(&line);
        out.push(line);
        if let Some(text) = explain {
            out.push(format!("  {}", format!("Description: {text}").dimmed()));
        }
    }
    out
}

fn health_explain_for_header(line: &str) -> Option<String> {
    if line.contains("Runtime coverage:") {
        return rule_full("fallow/runtime-coverage");
    }
    if line.contains("Health score:") {
        return Some(
            "The 0-100 project health grade combines dead code, complexity, maintainability, duplication, dependency, hotspot, and coverage signals when available."
                .to_string(),
        );
    }
    if line.contains("Metrics:") {
        return Some(
            "Vital signs summarize the analyzed project before truncation: dead-code percentages, maintainability index, hotspot count, circular dependencies, unused dependencies, and duplication where available."
                .to_string(),
        );
    }
    if line.contains("Large functions (") {
        return rule_full("fallow/high-cyclomatic-complexity");
    }
    if line.contains("High complexity functions (") {
        return rule_full("fallow/high-complexity");
    }
    if line.contains("Coverage gaps (") {
        return Some(
            "Coverage gaps identify runtime-reachable files or exports with no static path from discovered test entry points."
                .to_string(),
        );
    }
    if line.contains("Hotspots (") {
        return Some(
            "Hotspots combine recent churn with complexity so frequently changed risky files surface before quieter debt."
                .to_string(),
        );
    }
    if line.contains("Refactoring targets (") {
        return rule_full("fallow/refactoring-target");
    }
    None
}

fn rule_full(id: &str) -> Option<String> {
    crate::explain::rule_by_id(id).map(|rule| rule.full.to_string())
}

/// Format `seconds` as a human-readable window label like "12 min" or "6 h".
///
/// Used by both the terminal and markdown renderers so a multi-day window
/// consistently reads as "N d" in both surfaces instead of diverging to
/// "N h" in one of them.
pub(in crate::report) fn format_window(seconds: u64) -> String {
    if seconds < 60 {
        return format!("{seconds} s");
    }
    let minutes = seconds / 60;
    if minutes < 120 {
        return format!("{minutes} min");
    }
    let hours = minutes / 60;
    if hours < 48 {
        format!("{hours} h")
    } else {
        format!("{} d", hours / 24)
    }
}

pub fn render_health_score(lines: &mut Vec<String>, report: &crate::health_types::HealthReport) {
    let Some(ref hs) = report.health_score else {
        return;
    };

    let score_str = format!("{:.0}", hs.score);
    let grade_str = hs.grade;
    let score_colored = if hs.score >= 85.0 {
        format!("{score_str} {grade_str}")
            .green()
            .bold()
            .to_string()
    } else if hs.score >= 70.0 {
        format!("{score_str} {grade_str}")
            .yellow()
            .bold()
            .to_string()
    } else if hs.score >= 55.0 {
        format!("{score_str} {grade_str}").yellow().to_string()
    } else {
        format!("{score_str} {grade_str}").red().bold().to_string()
    };
    lines.push(format!(
        "{} {} {}",
        "\u{25cf}".cyan(),
        "Health score:".cyan().bold(),
        score_colored,
    ));

    let p = &hs.penalties;
    let mut penalties: Vec<(&str, f64)> = Vec::new();
    if let Some(df) = p.dead_files {
        penalties.push(("dead files", df));
    }
    if let Some(de) = p.dead_exports {
        penalties.push(("dead exports", de));
    }
    penalties.push(("complexity", p.complexity));
    penalties.push(("p90", p.p90_complexity));
    if let Some(mi) = p.maintainability {
        penalties.push(("maintainability", mi));
    }
    if let Some(hp) = p.hotspots {
        penalties.push(("hotspots", hp));
    }
    if let Some(ud) = p.unused_deps {
        penalties.push(("unused deps", ud));
    }
    if let Some(cd) = p.circular_deps {
        penalties.push(("circular deps", cd));
    }
    if let Some(us) = p.unit_size {
        penalties.push(("unit size", us));
    }
    if let Some(cp) = p.coupling {
        penalties.push(("coupling", cp));
    }
    if let Some(dp) = p.duplication {
        penalties.push(("duplication", dp));
    }
    penalties.retain(|&(_, v)| v > 0.0);
    penalties.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    if !penalties.is_empty() {
        let parts: Vec<String> = penalties
            .iter()
            .enumerate()
            .map(|(i, &(label, val))| {
                let text = format!("{label} -{val:.1}");
                if i == 0 {
                    text.yellow().to_string()
                } else {
                    text.dimmed().to_string()
                }
            })
            .collect();
        lines.push(format!(
            "  {} {}",
            "Deductions:".dimmed(),
            parts.join(&format!(" {} ", "\u{00b7}".dimmed()))
        ));
    }
    let mut na_parts = Vec::new();
    if p.dead_files.is_none() {
        na_parts.push("dead code");
    }
    if p.maintainability.is_none() {
        na_parts.push("maintainability");
    }
    if p.hotspots.is_none() {
        na_parts.push("hotspots");
    }
    if !na_parts.is_empty() {
        lines.push(format!(
            "  {}",
            format!(
                "N/A: {} (enable the corresponding analysis flags)",
                na_parts.join(", ")
            )
            .dimmed()
        ));
    }
    if p.duplication.is_some_and(|dp| dp >= 5.0) {
        lines.push(format!(
            "  {}",
            "Tip: add \"dist\" or \"__generated__\" to health.ignore in your config to exclude from duplication analysis"
                .dimmed()
        ));
    }
    lines.push(String::new());
}

/// Format a float for trend display: show as integer if it is one, otherwise 1dp.
fn fmt_trend_val(v: f64, unit: &str) -> String {
    if unit == "%" {
        format!("{v:.1}%")
    } else if (v - v.round()).abs() < 0.05 {
        format!("{v:.0}")
    } else {
        format!("{v:.1}")
    }
}

/// Format a delta for trend display: show with sign prefix.
fn fmt_trend_delta(v: f64, unit: &str) -> String {
    if unit == "%" {
        format!("{v:+.1}%")
    } else if (v - v.round()).abs() < 0.05 {
        format!("{v:+.0}")
    } else {
        format!("{v:+.1}")
    }
}

pub fn render_health_trend(lines: &mut Vec<String>, report: &crate::health_types::HealthReport) {
    let Some(ref trend) = report.health_trend else {
        return;
    };

    use crate::health_types::TrendDirection;

    let date = trend
        .compared_to
        .timestamp
        .get(..10)
        .unwrap_or(&trend.compared_to.timestamp);
    let sha_str = trend
        .compared_to
        .git_sha
        .as_deref()
        .map_or(String::new(), |sha| format!(" \u{00b7} {sha}"));
    let direction_label = format!(
        "{} {}",
        trend.overall_direction.arrow(),
        trend.overall_direction.label()
    );
    let direction_colored = match trend.overall_direction {
        TrendDirection::Improving => direction_label.green().bold().to_string(),
        TrendDirection::Declining => direction_label.red().bold().to_string(),
        TrendDirection::Stable => direction_label.dimmed().to_string(),
    };
    lines.push(format!(
        "{} {} {} {}",
        "\u{25cf}".cyan(),
        "Trend:".cyan().bold(),
        direction_colored,
        format!("(vs {date}{sha_str})").dimmed(),
    ));

    if let (Some(prev_model), Some(cur_model)) = (
        &trend.compared_to.coverage_model,
        &report.summary.coverage_model,
    ) && prev_model != cur_model
    {
        let prev_str = serde_json::to_string(prev_model).unwrap_or_default();
        let cur_str = serde_json::to_string(cur_model).unwrap_or_default();
        lines.push(format!(
            "  {}",
            format!(
                "note: CRAP model changed ({} \u{2192} {}); score delta may reflect model change, not code change",
                prev_str.trim_matches('"'),
                cur_str.trim_matches('"'),
            )
            .yellow()
        ));
    }

    if let Some(prev_version) = trend.compared_to.snapshot_schema_version
        && prev_version < crate::health_types::SNAPSHOT_SCHEMA_VERSION
    {
        lines.push(format!(
            "  {}",
            format!(
                "note: snapshot schema updated to v{} (added total LOC vital sign); score comparison still valid",
                crate::health_types::SNAPSHOT_SCHEMA_VERSION
            )
                .yellow()
        ));
    }

    let all_stable = trend
        .metrics
        .iter()
        .all(|m| m.direction == TrendDirection::Stable);
    if all_stable {
        lines.push(format!(
            "  {}",
            format!("All {} metrics unchanged", trend.metrics.len()).dimmed()
        ));
        lines.push(String::new());
        return;
    }

    for m in &trend.metrics {
        let label = format!("{:<18}", m.label);
        let prev_str = fmt_trend_val(m.previous, m.unit);
        let cur_str = fmt_trend_val(m.current, m.unit);
        let delta_str = fmt_trend_delta(m.delta, m.unit);

        let direction_str = match m.direction {
            TrendDirection::Improving => format!("{} {}", m.direction.arrow(), m.direction.label())
                .green()
                .to_string(),
            TrendDirection::Declining => format!("{} {}", m.direction.arrow(), m.direction.label())
                .red()
                .to_string(),
            TrendDirection::Stable => format!("{} {}", m.direction.arrow(), m.direction.label())
                .dimmed()
                .to_string(),
        };

        let values = format!("{prev_str:>8}  {cur_str:<8}");
        lines.push(format!(
            "  {label} {values}  {delta_str:<10} {direction_str}"
        ));
    }

    lines.push(String::new());
}

fn render_vital_signs(lines: &mut Vec<String>, report: &crate::health_types::HealthReport) {
    if report.health_trend.is_some() {
        return;
    }
    let Some(ref vs) = report.vital_signs else {
        return;
    };

    let mut parts = Vec::new();
    if vs.total_loc > 0 {
        parts.push(format!("{} LOC", thousands(vs.total_loc as usize)));
    }
    if let Some(dfp) = vs.dead_file_pct {
        parts.push(format!("dead files {dfp:.1}%"));
    }
    if let Some(dep) = vs.dead_export_pct {
        parts.push(format!("dead exports {dep:.1}%"));
    }
    parts.push(format!("avg cyclomatic {:.1}", vs.avg_cyclomatic));
    parts.push(format!("p90 cyclomatic {}", vs.p90_cyclomatic));
    if let Some(mi) = vs.maintainability_avg {
        let label = if mi >= 85.0 {
            "good"
        } else if mi >= 65.0 {
            "moderate"
        } else {
            "low"
        };
        parts.push(format!("maintainability {mi:.1} ({label})"));
    }
    if let Some(hc) = vs.hotspot_count {
        let since_suffix = report
            .hotspot_summary
            .as_ref()
            .map(|s| format!(" (since {})", s.since))
            .unwrap_or_default();
        parts.push(format!(
            "{hc} churn hotspot{}{since_suffix}",
            plural(hc as usize)
        ));
    }
    if let Some(cd) = vs.circular_dep_count
        && cd > 0
    {
        parts.push(format!(
            "{cd} circular {}",
            if cd == 1 { "dep" } else { "deps" }
        ));
    }
    if let Some(ud) = vs.unused_dep_count
        && ud > 0
    {
        parts.push(format!(
            "{ud} unused {}",
            if ud == 1 { "dep" } else { "deps" }
        ));
    }
    if let Some(dp) = vs.duplication_pct {
        parts.push(format!("duplication {dp:.1}%"));
    }
    lines.push(format!(
        "{} {} {}",
        "\u{25a0}".dimmed(),
        "Metrics:".dimmed(),
        parts.join(" \u{00b7} ").dimmed()
    ));
    lines.push(String::new());
}

fn render_risk_profiles(lines: &mut Vec<String>, report: &crate::health_types::HealthReport) {
    let Some(ref vs) = report.vital_signs else {
        return;
    };

    let format_profile = |profile: &crate::health_types::RiskProfile| -> String {
        format!(
            "{:.0}% low \u{00b7} {:.0}% medium \u{00b7} {:.0}% high \u{00b7} {:.0}% very high",
            profile.low_risk, profile.medium_risk, profile.high_risk, profile.very_high_risk
        )
    };

    let before = lines.len();

    if let Some(ref profile) = vs.unit_size_profile
        && profile.very_high_risk >= 3.0
    {
        lines.push(format!(
            "  {} {}  {}",
            "Function size:".dimmed(),
            format_profile(profile).dimmed(),
            "(1-15 / 16-30 / 31-60 / >60 LOC)".dimmed()
        ));
    }

    if let Some(ref profile) = vs.unit_interfacing_profile
        && (profile.very_high_risk > 0.0 || profile.high_risk > 1.0)
    {
        lines.push(format!(
            "  {}    {}  {}",
            "Parameters:".dimmed(),
            format_profile(profile).dimmed(),
            "(0-2 / 3-4 / 5-6 / >=7 params)".dimmed()
        ));
    }

    if lines.len() > before {
        lines.push(String::new());
    }
}

fn render_large_functions(
    lines: &mut Vec<String>,
    report: &crate::health_types::HealthReport,
    root: &Path,
) {
    if report.large_functions.is_empty() {
        return;
    }

    let total = report.large_functions.len();
    let shown = total.min(MAX_FLAT_ITEMS);
    lines.push(format!(
        "{} {}",
        "\u{25cf}".red(),
        if shown < total {
            format!("Large functions ({shown} shown, {total} total)")
        } else {
            format!("Large functions ({total})")
        }
        .red()
        .bold()
    ));

    let mut last_file = String::new();
    for entry in report.large_functions.iter().take(MAX_FLAT_ITEMS) {
        let file_str = relative_path(&entry.path, root).display().to_string();
        if file_str != last_file {
            lines.push(format!("  {}", format_path(&file_str)));
            last_file = file_str;
        }
        lines.push(format!(
            "    {} {}  {} lines",
            format!(":{}", entry.line).dimmed(),
            entry.name.bold(),
            format!("{:>3}", entry.line_count).red().bold(),
        ));
    }
    lines.push(format!(
        "  {}",
        format!("Functions exceeding 60 lines of code (very high risk): {DOCS_HEALTH}#unit-size")
            .dimmed()
    ));
    if shown < total {
        lines.push(format!(
            "  {}",
            format!("use --top {total} to see all").dimmed()
        ));
    }
    lines.push(String::new());
}

/// Append per-finding-kind suppression hints to the findings section footer.
///
/// External `.html` templates take a file-level HTML comment; inline
/// `@Component` templates take a line-level TS comment placed directly above
/// the decorator. `<component>` rollups suppress through the worst class
/// method (the rollup anchors at that method's line). Generic function
/// findings get the catch-all hint above a `>=3` noise threshold. Extracted
/// from `render_findings` to keep that function under the SIG unit-size
/// threshold.
fn append_suppression_hints(lines: &mut Vec<String>, report: &crate::health_types::HealthReport) {
    let has_html_template = report.findings.iter().any(|finding| {
        finding.name == "<template>"
            && finding
                .path
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("html"))
    });
    let has_inline_template = report.findings.iter().any(|finding| {
        finding.name == "<template>"
            && finding
                .path
                .extension()
                .and_then(|ext| ext.to_str())
                .is_none_or(|ext| !ext.eq_ignore_ascii_case("html"))
    });
    let has_component_rollup = report
        .findings
        .iter()
        .any(|finding| finding.name == "<component>");
    let has_function_finding = report
        .findings
        .iter()
        .any(|finding| finding.name != "<template>" && finding.name != "<component>");
    if has_html_template {
        lines.push(format!(
            "  {}",
            "To suppress HTML templates: <!-- fallow-ignore-file complexity -->".dimmed()
        ));
    }
    if has_inline_template {
        lines.push(format!(
            "  {}",
            "To suppress inline templates: // fallow-ignore-next-line complexity (above @Component)"
                .dimmed()
        ));
    }
    if has_component_rollup {
        lines.push(format!(
            "  {}",
            "To suppress a <component> rollup: suppress the worst class method (// fallow-ignore-next-line complexity above it hides both)"
                .dimmed()
        ));
    }
    if has_function_finding && report.findings.len() >= 3 {
        lines.push(format!(
            "  {}",
            "To suppress: // fallow-ignore-next-line complexity".dimmed()
        ));
    }
}

/// Render the breakdown line for a synthetic `<component>` rollup finding.
///
/// Returns `Some(line)` when the finding carries a `component_rollup` payload
/// (the rollup's cyc/cog totals are `worst_class_function + template`, so this
/// line names the pre-summation numbers + the worst-class-function identifier
/// so readers can see why the component ranks high without re-deriving the
/// link from the JSON payload), `None` otherwise. Extracted from
/// `render_findings` to keep that function under the SIG unit-size threshold.
///
/// Renders `template_path` workspace-relative (issue #547) so Angular
/// projects with many `*.component.html` files unambiguously identify the
/// template fallow scored.
fn render_component_rollup_breakdown(
    finding: &crate::health_types::ComplexityViolation,
    root: &Path,
) -> Option<String> {
    let rollup = finding.component_rollup.as_ref()?;
    let template_display = crate::report::format_display_path(&rollup.template_path, root);
    Some(format!(
        "         {}",
        format!(
            "rolled up: {}cyc {}cog on `{}.{}` + {}cyc {}cog on {}",
            rollup.class_cyclomatic,
            rollup.class_cognitive,
            rollup.component,
            rollup.class_worst_function,
            rollup.template_cyclomatic,
            rollup.template_cognitive,
            template_display,
        )
        .dimmed(),
    ))
}

fn render_findings(
    lines: &mut Vec<String>,
    report: &crate::health_types::HealthReport,
    root: &Path,
) {
    if report.findings.is_empty() {
        return;
    }

    lines.push(format!(
        "{} {}",
        "\u{25cf}".red(),
        if report.findings.len() < report.summary.functions_above_threshold {
            format!(
                "High complexity functions ({} shown, {} total)",
                report.findings.len(),
                report.summary.functions_above_threshold
            )
        } else {
            format!(
                "High complexity functions ({})",
                report.summary.functions_above_threshold
            )
        }
        .red()
        .bold()
    ));
    if let Some(note) = crap_coverage_note(report) {
        lines.push(format!("  {}", note.dimmed()));
    }

    let mut last_file = String::new();
    for finding in &report.findings {
        let file_str = crate::report::format_display_path(&finding.path, root);
        if file_str != last_file {
            lines.push(format!("  {}", format_path(&file_str)));
            last_file = file_str;
        }

        let cyc_val = format!("{:>3}", finding.cyclomatic);
        let cog_val = format!("{:>3}", finding.cognitive);

        let cyc_colored = if finding.cyclomatic > report.summary.max_cyclomatic_threshold {
            cyc_val.red().bold().to_string()
        } else {
            cyc_val.dimmed().to_string()
        };
        let cog_colored = if finding.cognitive > report.summary.max_cognitive_threshold {
            cog_val.red().bold().to_string()
        } else {
            cog_val.dimmed().to_string()
        };

        let severity_tag = match finding.severity {
            crate::health_types::FindingSeverity::Critical => {
                format!(" {}", "CRITICAL".red().bold())
            }
            crate::health_types::FindingSeverity::High => {
                format!(" {}", "HIGH".yellow().bold())
            }
            crate::health_types::FindingSeverity::Moderate => String::new(),
        };
        let generated_tag = if is_likely_generated(&finding.name, finding.cyclomatic) {
            format!(" {}", "(generated)".dimmed())
        } else {
            String::new()
        };
        lines.push(format!(
            "    {} {}{}{}",
            format!(":{}", finding.line).dimmed(),
            finding.name.bold(),
            severity_tag,
            generated_tag,
        ));
        lines.push(format!(
            "         {} cyclomatic  {} cognitive  {} lines",
            cyc_colored,
            cog_colored,
            format!("{:>3}", finding.line_count).dimmed(),
        ));
        if let Some(line) = render_component_rollup_breakdown(finding, root) {
            lines.push(line);
        }
        if let Some(crap) = finding.crap {
            let crap_colored = format!("{crap:>5.1}").red().bold().to_string();
            let coverage_suffix = if let Some(pct) = finding.coverage_pct {
                format!("  ({pct:.0}% tested)")
            } else if matches!(
                finding.coverage_source,
                Some(crate::health_types::CoverageSource::EstimatedComponentInherited)
            ) && let Some(ref owner) = finding.inherited_from
            {
                let owner_display = crate::report::format_display_path(owner, root);
                format!("  (inherited from {owner_display})")
            } else {
                String::new()
            };
            lines.push(format!(
                "         {crap_colored} CRAP{}",
                coverage_suffix.dimmed(),
            ));
        }
    }
    lines.push(format!(
        "  {}",
        format!(
            "Functions exceeding cyclomatic, cognitive, or CRAP thresholds ({DOCS_HEALTH}#complexity-metrics)"
        )
        .dimmed()
    ));
    append_suppression_hints(lines, report);
    if report.findings.len() < report.summary.functions_above_threshold {
        let total = report.summary.functions_above_threshold;
        lines.push(format!(
            "  {}",
            format!("use --top {total} to see all").dimmed()
        ));
    }
    lines.push(String::new());
}

fn crap_coverage_note(report: &crate::health_types::HealthReport) -> Option<String> {
    if !report.findings.iter().any(|finding| finding.crap.is_some()) {
        return None;
    }

    let istanbul_counts = (
        report.summary.istanbul_matched,
        report.summary.istanbul_total,
    );
    let has_istanbul_counts = matches!(istanbul_counts, (Some(_), Some(total)) if total > 0);

    if matches!(
        report.summary.coverage_model,
        Some(crate::health_types::CoverageModel::Istanbul)
    ) || has_istanbul_counts
    {
        let match_info = match (
            report.summary.istanbul_matched,
            report.summary.istanbul_total,
        ) {
            (Some(matched), Some(total)) if total > 0 && matched < total => {
                return Some(format!(
                    "CRAP scores use Istanbul coverage where matched ({matched}/{total} functions); unmatched functions are estimated from export references."
                ));
            }
            (Some(matched), Some(total)) if total > 0 => {
                format!(" ({matched}/{total} functions matched)")
            }
            _ => String::new(),
        };
        return Some(format!(
            "CRAP scores use Istanbul coverage data{match_info}."
        ));
    }

    Some(
        "CRAP scores are estimated from export references; run `fallow health --coverage <coverage-final.json>` for exact scores."
            .to_string(),
    )
}

/// Detect likely generated code based on function name patterns.
fn is_likely_generated(name: &str, cyclomatic: u16) -> bool {
    if name.starts_with("validate")
        && name.len() > 8
        && name[8..].chars().all(|c| c.is_ascii_digit())
    {
        return true;
    }
    if cyclomatic > 200 && (name == "module.exports" || name == "default" || name == "<anonymous>")
    {
        return true;
    }
    false
}

fn render_file_scores(
    lines: &mut Vec<String>,
    report: &crate::health_types::HealthReport,
    root: &Path,
) {
    if report.file_scores.is_empty() {
        return;
    }

    lines.push(format!(
        "{} {} {}",
        "\u{25cf}".cyan(),
        format!("File health scores ({} files)", report.file_scores.len())
            .cyan()
            .bold(),
        "\u{b7} sorted by triage concern".dimmed(),
    ));
    lines.push(String::new());

    let shown_scores = report.file_scores.len().min(MAX_FLAT_ITEMS);
    for score in &report.file_scores[..shown_scores] {
        let file_str = relative_path(&score.path, root).display().to_string();
        let mi = score.maintainability_index;

        let mi_str = format!("{mi:>5.1}");
        let mi_colored = if mi >= 80.0 {
            mi_str.green().to_string()
        } else if mi >= 50.0 {
            mi_str.yellow().to_string()
        } else {
            mi_str.red().bold().to_string()
        };

        let (dir, filename) = split_dir_filename(&file_str);

        let concern = file_score_concern_axis(score);
        let label = concern.label();
        let concern_colored = match concern {
            FileScoreConcern::Risk => {
                if score.crap_max >= 30.0 {
                    label.red().bold().to_string()
                } else if score.crap_max >= 15.0 {
                    label.yellow().to_string()
                } else {
                    label.dimmed().to_string()
                }
            }
            FileScoreConcern::Structural => {
                if mi < 50.0 {
                    label.red().bold().to_string()
                } else if mi < 80.0 {
                    label.yellow().to_string()
                } else {
                    label.dimmed().to_string()
                }
            }
        };

        const CONCERN_TAG_COLUMN: usize = 48;
        let pad = CONCERN_TAG_COLUMN
            .saturating_sub(file_str.chars().count())
            .max(2);
        lines.push(format!(
            "  {}    {}{}{}{}",
            mi_colored,
            dir.dimmed(),
            filename,
            " ".repeat(pad),
            concern_colored,
        ));

        let risk_suffix = if score.crap_max > 0.0 {
            let risk_str = if score.crap_max > 999.0 {
                ">999".to_string()
            } else {
                format!("{:.1}", score.crap_max)
            };
            let risk_colored = if score.crap_max >= 30.0 {
                risk_str.red().bold().to_string()
            } else if score.crap_max >= 15.0 {
                risk_str.yellow().to_string()
            } else {
                risk_str.dimmed().to_string()
            };
            format!("  {risk_colored} risk")
        } else {
            String::new()
        };
        lines.push(format!(
            "         {} LOC  {} fan-in  {} fan-out  {} dead  {} density{}",
            format!("{:>6}", score.lines).dimmed(),
            format!("{:>3}", score.fan_in).dimmed(),
            format!("{:>3}", score.fan_out).dimmed(),
            format!("{:>3.0}%", score.dead_code_ratio * 100.0).dimmed(),
            format!("{:.2}", score.complexity_density).dimmed(),
            risk_suffix,
        ));

        lines.push(String::new());
    }
    if report.file_scores.len() > MAX_FLAT_ITEMS {
        lines.push(format!(
            "  {}",
            format!(
                "... and {} more files (--format json for full list)",
                report.file_scores.len() - MAX_FLAT_ITEMS
            )
            .dimmed()
        ));
        lines.push(String::new());
    }
    let crap_note = if matches!(
        report.summary.coverage_model,
        Some(crate::health_types::CoverageModel::Istanbul)
    ) {
        let match_info = match (
            report.summary.istanbul_matched,
            report.summary.istanbul_total,
        ) {
            (Some(m), Some(t)) if t > 0 => format!(" ({m}/{t} functions matched)"),
            _ => String::new(),
        };
        format!("CRAP from Istanbul coverage data{match_info}.")
    } else {
        "CRAP estimated from export references (85% direct, 40% indirect, 0% untested). Run `fallow health --coverage <coverage-final.json>` for exact scores.".to_string()
    };
    lines.push(format!(
        "  {}",
        format!("Sorted by triage concern: the larger of low-MI concern and CRAP risk. The risk / structure tag marks which one placed each file. MI reflects complexity, coupling, and dead code; risk reflects untested complexity (CRAP) and can diverge from MI. Risk: low <15, moderate 15-30, high >=30. {crap_note} {DOCS_HEALTH}#file-health-scores").dimmed()
    ));
    lines.push(String::new());
}

fn render_coverage_gaps(
    lines: &mut Vec<String>,
    report: &crate::health_types::HealthReport,
    root: &Path,
) {
    let Some(ref gaps) = report.coverage_gaps else {
        return;
    };

    lines.push(format!(
        "{} {}",
        "\u{25cf}".yellow(),
        format!(
            "Coverage gaps ({} untested {}, {} untested {}, {:.1}% file coverage)",
            gaps.summary.untested_files,
            if gaps.summary.untested_files == 1 {
                "file"
            } else {
                "files"
            },
            gaps.summary.untested_exports,
            if gaps.summary.untested_exports == 1 {
                "export"
            } else {
                "exports"
            },
            gaps.summary.file_coverage_pct,
        )
        .yellow()
        .bold()
    ));
    lines.push(String::new());

    if !gaps.files.is_empty() {
        let shown_files = gaps.files.len().min(MAX_FLAT_ITEMS);
        lines.push(format!("  {}", "Files".dimmed()));
        for item in &gaps.files[..shown_files] {
            let file_str = relative_path(&item.file.path, root).display().to_string();
            let (dir, filename) = split_dir_filename(&file_str);
            lines.push(format!("  {}{}", dir.dimmed(), filename));
        }
        if gaps.files.len() > MAX_FLAT_ITEMS {
            lines.push(format!(
                "  {}",
                format!(
                    "... and {} more files (--format json for full list)",
                    gaps.files.len() - MAX_FLAT_ITEMS
                )
                .dimmed()
            ));
        }
        lines.push(String::new());
    }

    if !gaps.exports.is_empty() {
        lines.push(format!("  {}", "Exports".dimmed()));

        let mut by_file: Vec<(
            &std::path::Path,
            Vec<&crate::health_types::UntestedExportFinding>,
        )> = Vec::new();
        for item in &gaps.exports {
            if let Some(entry) = by_file
                .last_mut()
                .filter(|(p, _)| *p == item.export.path.as_path())
            {
                entry.1.push(item);
            } else {
                by_file.push((item.export.path.as_path(), vec![item]));
            }
        }

        let mut shown = 0;
        for (file_path, exports) in &by_file {
            if shown >= MAX_FLAT_ITEMS {
                break;
            }
            let file_str = relative_path(file_path, root).display().to_string();
            if exports.len() > 10 {
                lines.push(format!(
                    "  {} ({} untested re-exports)",
                    file_str.dimmed(),
                    exports.len(),
                ));
                shown += 1;
            } else {
                for item in exports {
                    if shown >= MAX_FLAT_ITEMS {
                        break;
                    }
                    lines.push(format!(
                        "  {}:{} `{}`",
                        file_str.dimmed(),
                        item.export.line,
                        item.export.export_name,
                    ));
                    shown += 1;
                }
            }
        }
        let total_exports = gaps.exports.len();
        if total_exports > shown {
            lines.push(format!(
                "  {}",
                format!(
                    "... and {} more exports (--format json for full list)",
                    total_exports - shown
                )
                .dimmed()
            ));
        }
        lines.push(String::new());
    }

    lines.push(format!(
        "  {}",
        format!(
            "Static test dependency gaps (not line-level coverage): {DOCS_HEALTH}#coverage-gaps"
        )
        .dimmed()
    ));
    lines.push(String::new());
}

/// Print a concise health summary showing only aggregate statistics.
pub(in crate::report) fn print_health_summary(
    report: &crate::health_types::HealthReport,
    elapsed: Duration,
    quiet: bool,
    heading: bool,
) {
    let s = &report.summary;

    if heading {
        outln!("{}", "Health Summary".bold());
        outln!();
    }
    outln!("  {:>6}  Functions analyzed", s.functions_analyzed);
    outln!("  {:>6}  Above threshold", s.functions_above_threshold);
    if let Some(mi) = s.average_maintainability {
        let label = if mi >= 85.0 {
            "good"
        } else if mi >= 65.0 {
            "moderate"
        } else {
            "low"
        };
        outln!("  {mi:>5.1}   Average maintainability ({label})");
    }
    if let Some(ref score) = report.health_score {
        outln!("  {:>5.0} {}  Health score", score.score, score.grade);
    }
    if let Some(ref gaps) = report.coverage_gaps {
        outln!(
            "  {:>6}  Untested {} ({:.1}% file coverage)",
            gaps.summary.untested_files,
            if gaps.summary.untested_files == 1 {
                "file"
            } else {
                "files"
            },
            gaps.summary.file_coverage_pct,
        );
        outln!(
            "  {:>6}  Untested {}",
            gaps.summary.untested_exports,
            if gaps.summary.untested_exports == 1 {
                "export"
            } else {
                "exports"
            },
        );
    }
    if let Some(ref production) = report.runtime_coverage {
        outln!(
            "  {:>6}  Unhit in production",
            production.summary.functions_unhit,
        );
        outln!(
            "  {:>6}  Untracked by V8 (lazy-parsed / worker / dynamic)",
            production.summary.functions_untracked,
        );
    }

    if !quiet {
        eprintln!(
            "{}",
            format!(
                "\u{2713} {} functions analyzed ({:.2}s)",
                s.functions_analyzed,
                elapsed.as_secs_f64()
            )
            .green()
            .bold()
        );
    }
}

/// Render a per-group summary block beneath the project-level human report.
///
/// Layout: a header row (`key  score  grade  files  hot  p90`) followed by
/// one row per group. The `score`/`grade` columns are omitted entirely when
/// no group carries a health score (no `--score` requested). The `p90`
/// column is omitted entirely when no group carries vital signs
/// (`--score-only` was active).
///
/// When scores are present, groups are sorted ascending by score (worst
/// first) so the rows match the user's "where do I refactor first?"
/// question. Otherwise the resolver's own ordering (descending by file
/// count, unowned last) is preserved.
///
/// Grade is colored to match the project-level grade: A/B green, C yellow,
/// D/F red.
///
/// Goes to stdout (the rows are content, not progress) so the block survives
/// `fallow health --group-by package > out.txt`. The leading blank line,
/// the `(root)` legend, and the JSON-parity hint go to stderr because they
/// are display affordances, not data.
pub(in crate::report) fn print_health_grouping(
    grouping: &crate::health_types::HealthGrouping,
    _root: &Path,
    quiet: bool,
) {
    if grouping.groups.is_empty() {
        return;
    }
    if !quiet {
        eprintln!();
    }
    outln!(
        "{} {}",
        "\u{25cf}".cyan(),
        format!("Per-{} health", grouping.mode).cyan().bold()
    );
    let key_width = grouping
        .groups
        .iter()
        .map(|g| g.key.len())
        .max()
        .unwrap_or(0)
        .max(8);
    let any_score = grouping.groups.iter().any(|g| g.health_score.is_some());
    let any_vitals = grouping.groups.iter().any(|g| g.vital_signs.is_some());

    let mut ordered: Vec<&crate::health_types::HealthGroup> = grouping.groups.iter().collect();
    if any_score {
        ordered.sort_by(|a, b| {
            let a_score = a.health_score.as_ref().map_or(f64::INFINITY, |hs| hs.score);
            let b_score = b.health_score.as_ref().map_or(f64::INFINITY, |hs| hs.score);
            a_score
                .partial_cmp(&b_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    }

    let mut header = format!("  {:<width$}", "", width = key_width);
    if any_score {
        let _ = write!(header, "  {:>9}  grade", "score");
    }
    let _ = write!(header, "  {:>5}", "files");
    let _ = write!(header, "  {:>3}", "hot");
    if any_vitals {
        let _ = write!(header, "  {:>3}", "p90");
    }
    outln!("{}", header.dimmed());

    let mut has_root_bucket = false;
    for group in ordered {
        if group.key == "(root)" {
            has_root_bucket = true;
        }
        let mut row = format!("  {:<width$}", group.key, width = key_width);
        if any_score {
            if let Some(ref hs) = group.health_score {
                let grade_colored = colorize_grade(hs.grade);
                let _ = write!(row, "  {:>9.1}  {}", hs.score, grade_colored);
            } else {
                row.push_str("                  ");
            }
        }
        let _ = write!(row, "  {:>5}", group.files_analyzed);
        let _ = write!(row, "  {:>3}", group.hotspots.len());
        if any_vitals {
            if let Some(ref vs) = group.vital_signs {
                let _ = write!(row, "  {:>3}", vs.p90_cyclomatic);
            } else {
                row.push_str("     ");
            }
        }
        outln!("{row}");
    }
    if !quiet {
        if has_root_bucket {
            eprintln!(
                "  {}",
                "(root) = files outside any workspace package".dimmed()
            );
        }
        eprintln!(
            "  {}",
            "per-group summary only; --format json includes per-group findings, file scores, and hotspots"
                .dimmed()
        );
    }
}

/// Color a grade letter to match the project-level grade rendering.
fn colorize_grade(grade: &str) -> String {
    match grade {
        "A" | "B" => grade.green().to_string(),
        "C" => grade.yellow().to_string(),
        _ => grade.red().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::super::plain;
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn health_empty_findings_produces_no_header() {
        let root = PathBuf::from("/project");
        let report = crate::health_types::HealthReport {
            summary: crate::health_types::HealthSummary {
                files_analyzed: 10,
                functions_analyzed: 50,
                ..Default::default()
            },
            ..Default::default()
        };
        let lines = build_health_human_lines(&report, &root);
        let text = plain(&lines);
        assert!(!text.contains("High complexity functions"));
    }

    #[test]
    fn health_findings_show_function_details() {
        let root = PathBuf::from("/project");
        let report = crate::health_types::HealthReport {
            findings: vec![
                crate::health_types::ComplexityViolation {
                    path: root.join("src/parser.ts"),
                    name: "parseExpression".to_string(),
                    line: 42,
                    col: 0,
                    cyclomatic: 25,
                    cognitive: 30,
                    line_count: 80,
                    param_count: 0,
                    exceeded: crate::health_types::ExceededThreshold::Both,
                    severity: crate::health_types::FindingSeverity::High,
                    crap: None,
                    coverage_pct: None,
                    coverage_tier: None,
                    coverage_source: None,
                    inherited_from: None,
                    component_rollup: None,
                    contributions: Vec::new(),
                }
                .into(),
            ],
            summary: crate::health_types::HealthSummary {
                files_analyzed: 10,
                functions_analyzed: 50,
                functions_above_threshold: 1,
                ..Default::default()
            },
            ..Default::default()
        };
        let lines = build_health_human_lines(&report, &root);
        let text = plain(&lines);
        assert!(text.contains("High complexity functions (1)"));
        assert!(text.contains("src/parser.ts"));
        assert!(text.contains(":42"));
        assert!(text.contains("parseExpression"));
        assert!(text.contains("25 cyclomatic"));
        assert!(text.contains("30 cognitive"));
        assert!(text.contains("80 lines"));
    }

    #[test]
    fn health_shown_vs_total_when_truncated() {
        let root = PathBuf::from("/project");
        let report = crate::health_types::HealthReport {
            findings: vec![
                crate::health_types::ComplexityViolation {
                    path: root.join("src/a.ts"),
                    name: "fn1".to_string(),
                    line: 1,
                    col: 0,
                    cyclomatic: 25,
                    cognitive: 20,
                    line_count: 50,
                    param_count: 0,
                    exceeded: crate::health_types::ExceededThreshold::Both,
                    severity: crate::health_types::FindingSeverity::High,
                    crap: None,
                    coverage_pct: None,
                    coverage_tier: None,
                    coverage_source: None,
                    inherited_from: None,
                    component_rollup: None,
                    contributions: Vec::new(),
                }
                .into(),
            ],
            summary: crate::health_types::HealthSummary {
                files_analyzed: 100,
                functions_analyzed: 500,
                functions_above_threshold: 10,
                ..Default::default()
            },
            ..Default::default()
        };
        let lines = build_health_human_lines(&report, &root);
        let text = plain(&lines);
        assert!(text.contains("1 shown, 10 total"));
    }

    #[test]
    fn health_findings_explain_estimated_crap_scores() {
        let root = PathBuf::from("/project");
        let report = crate::health_types::HealthReport {
            findings: vec![
                crate::health_types::ComplexityViolation {
                    path: root.join("src/risky.ts"),
                    name: "risky".to_string(),
                    line: 7,
                    col: 0,
                    cyclomatic: 25,
                    cognitive: 20,
                    line_count: 80,
                    param_count: 0,
                    exceeded: crate::health_types::ExceededThreshold::Crap,
                    severity: crate::health_types::FindingSeverity::High,
                    crap: Some(650.0),
                    coverage_pct: None,
                    coverage_tier: Some(crate::health_types::CoverageTier::None),
                    coverage_source: Some(crate::health_types::CoverageSource::Estimated),
                    inherited_from: None,
                    component_rollup: None,
                    contributions: Vec::new(),
                }
                .into(),
            ],
            summary: crate::health_types::HealthSummary {
                files_analyzed: 1,
                functions_analyzed: 1,
                functions_above_threshold: 1,
                coverage_model: Some(crate::health_types::CoverageModel::StaticEstimated),
                coverage_source_consistency: None,
                ..Default::default()
            },
            ..Default::default()
        };
        let text = plain(&build_health_human_lines(&report, &root));
        assert!(text.contains("CRAP scores are estimated from export references"));
        assert!(text.contains("fallow health --coverage <coverage-final.json>"));
    }

    #[test]
    fn health_findings_explain_mixed_istanbul_crap_scores() {
        let root = PathBuf::from("/project");
        let report = crate::health_types::HealthReport {
            findings: vec![
                crate::health_types::ComplexityViolation {
                    path: root.join("src/risky.ts"),
                    name: "risky".to_string(),
                    line: 7,
                    col: 0,
                    cyclomatic: 25,
                    cognitive: 20,
                    line_count: 80,
                    param_count: 0,
                    exceeded: crate::health_types::ExceededThreshold::Crap,
                    severity: crate::health_types::FindingSeverity::High,
                    crap: Some(45.0),
                    coverage_pct: Some(40.0),
                    coverage_tier: Some(crate::health_types::CoverageTier::Partial),
                    coverage_source: Some(crate::health_types::CoverageSource::Istanbul),
                    inherited_from: None,
                    component_rollup: None,
                    contributions: Vec::new(),
                }
                .into(),
            ],
            summary: crate::health_types::HealthSummary {
                files_analyzed: 1,
                functions_analyzed: 2,
                functions_above_threshold: 1,
                coverage_model: Some(crate::health_types::CoverageModel::Istanbul),
                coverage_source_consistency: None,
                istanbul_matched: Some(1),
                istanbul_total: Some(2),
                ..Default::default()
            },
            ..Default::default()
        };
        let text = plain(&build_health_human_lines(&report, &root));
        assert!(
            text.contains(
                "CRAP scores use Istanbul coverage where matched (1/2 functions); unmatched functions are estimated"
            ),
            "mixed Istanbul note missing from output: {text}"
        );
    }

    #[test]
    fn health_findings_explain_istanbul_counts_without_summary_model() {
        let root = PathBuf::from("/project");
        let report = crate::health_types::HealthReport {
            findings: vec![
                crate::health_types::ComplexityViolation {
                    path: root.join("src/risky.ts"),
                    name: "risky".to_string(),
                    line: 7,
                    col: 0,
                    cyclomatic: 25,
                    cognitive: 20,
                    line_count: 80,
                    param_count: 0,
                    exceeded: crate::health_types::ExceededThreshold::Crap,
                    severity: crate::health_types::FindingSeverity::High,
                    crap: Some(45.0),
                    coverage_pct: None,
                    coverage_tier: Some(crate::health_types::CoverageTier::None),
                    coverage_source: Some(crate::health_types::CoverageSource::Estimated),
                    inherited_from: None,
                    component_rollup: None,
                    contributions: Vec::new(),
                }
                .into(),
            ],
            summary: crate::health_types::HealthSummary {
                files_analyzed: 1,
                functions_analyzed: 2,
                functions_above_threshold: 1,
                coverage_model: None,
                coverage_source_consistency: None,
                istanbul_matched: Some(1),
                istanbul_total: Some(2),
                ..Default::default()
            },
            ..Default::default()
        };
        let text = plain(&build_health_human_lines(&report, &root));
        assert!(
            text.contains(
                "CRAP scores use Istanbul coverage where matched (1/2 functions); unmatched functions are estimated"
            ),
            "Istanbul counts should drive the note even when coverage_model is omitted: {text}"
        );
    }

    #[test]
    fn health_findings_grouped_by_file() {
        let root = PathBuf::from("/project");
        let report = crate::health_types::HealthReport {
            findings: vec![
                crate::health_types::ComplexityViolation {
                    path: root.join("src/parser.ts"),
                    name: "fn1".to_string(),
                    line: 10,
                    col: 0,
                    cyclomatic: 25,
                    cognitive: 20,
                    line_count: 40,
                    param_count: 0,
                    exceeded: crate::health_types::ExceededThreshold::Both,
                    severity: crate::health_types::FindingSeverity::High,
                    crap: None,
                    coverage_pct: None,
                    coverage_tier: None,
                    coverage_source: None,
                    inherited_from: None,
                    component_rollup: None,
                    contributions: Vec::new(),
                }
                .into(),
                crate::health_types::ComplexityViolation {
                    path: root.join("src/parser.ts"),
                    name: "fn2".to_string(),
                    line: 60,
                    col: 0,
                    cyclomatic: 22,
                    cognitive: 18,
                    line_count: 30,
                    param_count: 0,
                    exceeded: crate::health_types::ExceededThreshold::Both,
                    severity: crate::health_types::FindingSeverity::High,
                    crap: None,
                    coverage_pct: None,
                    coverage_tier: None,
                    coverage_source: None,
                    inherited_from: None,
                    component_rollup: None,
                    contributions: Vec::new(),
                }
                .into(),
            ],
            summary: crate::health_types::HealthSummary {
                files_analyzed: 10,
                functions_analyzed: 50,
                functions_above_threshold: 2,
                ..Default::default()
            },
            ..Default::default()
        };
        let lines = build_health_human_lines(&report, &root);
        let text = plain(&lines);
        let count = text.matches("src/parser.ts").count();
        assert_eq!(count, 1, "File header should appear once for grouped items");
    }

    fn empty_report() -> crate::health_types::HealthReport {
        crate::health_types::HealthReport {
            summary: crate::health_types::HealthSummary {
                files_analyzed: 10,
                functions_analyzed: 50,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn health_runtime_coverage_renders_section() {
        let root = PathBuf::from("/project");
        let mut report = empty_report();
        report.runtime_coverage = Some(crate::health_types::RuntimeCoverageReport {
            schema_version: crate::health_types::RuntimeCoverageSchemaVersion::V1,
            verdict: crate::health_types::RuntimeCoverageReportVerdict::ColdCodeDetected,
            signals: Vec::new(),
            summary: crate::health_types::RuntimeCoverageSummary {
                data_source: crate::health_types::RuntimeCoverageDataSource::Local,
                last_received_at: None,
                functions_tracked: 4,
                functions_hit: 2,
                functions_unhit: 1,
                functions_untracked: 1,
                coverage_percent: 50.0,
                trace_count: 2_847_291,
                period_days: 30,
                deployments_seen: 14,
                capture_quality: None,
            },
            findings: vec![crate::health_types::RuntimeCoverageFinding {
                id: "fallow:prod:deadbeef".to_owned(),
                stable_id: None,
                path: root.join("src/cold.ts"),
                function: "coldPath".to_owned(),
                line: 14,
                verdict: crate::health_types::RuntimeCoverageVerdict::ReviewRequired,
                invocations: Some(0),
                confidence: crate::health_types::RuntimeCoverageConfidence::Medium,
                evidence: crate::health_types::RuntimeCoverageEvidence {
                    static_status: "used".to_owned(),
                    test_coverage: "not_covered".to_owned(),
                    v8_tracking: "tracked".to_owned(),
                    untracked_reason: None,
                    observation_days: 30,
                    deployments_observed: 14,
                },
                actions: vec![],
                source_hash: None,
            }],
            hot_paths: vec![crate::health_types::RuntimeCoverageHotPath {
                id: "fallow:hot:cafebabe".to_owned(),
                stable_id: None,
                path: root.join("src/hot.ts"),
                function: "hotPath".to_owned(),
                line: 3,
                end_line: 9,
                invocations: 250,
                percentile: 99,
                actions: vec![],
            }],
            blast_radius: vec![],
            importance: vec![],
            watermark: Some(crate::health_types::RuntimeCoverageWatermark::LicenseExpiredGrace),
            warnings: vec![],
        });

        let text = plain(&build_health_human_lines(&report, &root));
        assert!(text.contains("Runtime coverage: cold code detected"));
        assert!(text.contains("src/cold.ts:14 coldPath [0 invocations, review required]"));
        assert!(text.contains("license expired grace active"));
        assert!(text.contains("hot paths:"));
        assert!(text.contains("src/hot.ts:3 hotPath (250 invocations, p99)"));
        assert!(!text.contains("short capture:"));
        assert!(!text.contains("start a trial"));
    }

    #[test]
    fn health_coverage_intelligence_renders_findings_and_ambiguity_summary() {
        use crate::health_types::{
            CoverageIntelligenceAction, CoverageIntelligenceConfidence,
            CoverageIntelligenceEvidence, CoverageIntelligenceFinding,
            CoverageIntelligenceMatchConfidence, CoverageIntelligenceRecommendation,
            CoverageIntelligenceReport, CoverageIntelligenceSchemaVersion,
            CoverageIntelligenceSignal, CoverageIntelligenceSummary, CoverageIntelligenceVerdict,
        };

        let root = PathBuf::from("/project");
        let mut report = empty_report();
        report.coverage_intelligence = Some(CoverageIntelligenceReport {
            schema_version: CoverageIntelligenceSchemaVersion::V1,
            verdict: CoverageIntelligenceVerdict::HighConfidenceDelete,
            summary: CoverageIntelligenceSummary {
                findings: 1,
                high_confidence_deletes: 1,
                ..Default::default()
            },
            findings: vec![CoverageIntelligenceFinding {
                id: "fallow:coverage-intel:abc123".to_owned(),
                path: root.join("src/dead.ts"),
                identity: Some("deadPath".to_owned()),
                line: 9,
                verdict: CoverageIntelligenceVerdict::HighConfidenceDelete,
                signals: vec![CoverageIntelligenceSignal::RuntimeCold],
                recommendation: CoverageIntelligenceRecommendation::DeleteAfterConfirmingOwner,
                confidence: CoverageIntelligenceConfidence::High,
                related_ids: vec![],
                evidence: CoverageIntelligenceEvidence {
                    match_confidence: CoverageIntelligenceMatchConfidence::Direct,
                    ..Default::default()
                },
                actions: vec![CoverageIntelligenceAction {
                    kind: "delete-after-confirming-owner".to_owned(),
                    description: "Confirm ownership before deleting".to_owned(),
                    auto_fixable: false,
                }],
            }],
        });

        let text = plain(&build_health_human_lines(&report, &root));
        assert!(text.contains("Coverage intelligence"));
        assert!(text.contains("src/dead.ts:9 deadPath high-confidence-delete"));
        assert!(text.contains("Confirm ownership before deleting"));

        report.coverage_intelligence = Some(CoverageIntelligenceReport {
            schema_version: CoverageIntelligenceSchemaVersion::V1,
            verdict: CoverageIntelligenceVerdict::Clean,
            summary: CoverageIntelligenceSummary {
                skipped_ambiguous_matches: 2,
                ..Default::default()
            },
            findings: vec![],
        });
        let text = plain(&build_health_human_lines(&report, &root));
        assert!(text.contains("skipped 2 ambiguous evidence matches"));
    }

    fn runtime_coverage_report_with_quality(
        quality: Option<crate::health_types::RuntimeCoverageCaptureQuality>,
    ) -> crate::health_types::RuntimeCoverageReport {
        crate::health_types::RuntimeCoverageReport {
            schema_version: crate::health_types::RuntimeCoverageSchemaVersion::V1,
            verdict: crate::health_types::RuntimeCoverageReportVerdict::Clean,
            signals: Vec::new(),
            summary: crate::health_types::RuntimeCoverageSummary {
                data_source: crate::health_types::RuntimeCoverageDataSource::Local,
                last_received_at: None,
                functions_tracked: 10,
                functions_hit: 7,
                functions_unhit: 0,
                functions_untracked: 3,
                coverage_percent: 70.0,
                trace_count: 1_000,
                period_days: 1,
                deployments_seen: 1,
                capture_quality: quality,
            },
            findings: vec![],
            hot_paths: vec![],
            blast_radius: vec![],
            importance: vec![],
            watermark: None,
            warnings: vec![],
        }
    }

    #[test]
    fn health_runtime_coverage_short_capture_shows_warning_and_prompt() {
        let root = PathBuf::from("/project");
        let mut report = empty_report();
        report.runtime_coverage = Some(runtime_coverage_report_with_quality(Some(
            crate::health_types::RuntimeCoverageCaptureQuality {
                window_seconds: 720, // 12 min
                instances_observed: 1,
                lazy_parse_warning: true,
                untracked_ratio_percent: 42.5,
            },
        )));
        let text = plain(&build_health_human_lines(&report, &root));
        assert!(
            text.contains(
                "note: short capture (12 min from 1 instance); 42.5% of functions untracked, lazy-parsed scripts may not appear."
            ),
            "warning banner missing or malformed in:\n{text}"
        );
        assert!(
            text.contains("extend the capture or switch to continuous monitoring"),
            "warning follow-up line missing in:\n{text}"
        );
        assert!(
            text.contains("captured 12 min from 1 instance."),
            "upgrade prompt header missing in:\n{text}"
        );
        assert!(
            text.contains("continuous monitoring over 30 days evaluates more paths"),
            "upgrade prompt body missing in:\n{text}"
        );
        assert!(
            text.contains("fallow license activate --trial --email you@company.com"),
            "trial CTA command missing in:\n{text}"
        );
    }

    #[test]
    fn health_runtime_coverage_long_capture_shows_neither_warning_nor_prompt() {
        let root = PathBuf::from("/project");
        let mut report = empty_report();
        report.runtime_coverage = Some(runtime_coverage_report_with_quality(Some(
            crate::health_types::RuntimeCoverageCaptureQuality {
                window_seconds: 7 * 24 * 3600, // 7 days
                instances_observed: 4,
                lazy_parse_warning: false,
                untracked_ratio_percent: 3.1,
            },
        )));
        let text = plain(&build_health_human_lines(&report, &root));
        assert!(
            !text.contains("short capture"),
            "long capture should not emit short-capture warning:\n{text}"
        );
        assert!(
            !text.contains("start a trial"),
            "long capture should not emit trial CTA:\n{text}"
        );
    }

    #[test]
    fn format_window_labels() {
        assert_eq!(super::format_window(30), "30 s");
        assert_eq!(super::format_window(60), "1 min");
        assert_eq!(super::format_window(720), "12 min");
        assert_eq!(super::format_window(3600 * 3), "3 h");
        assert_eq!(super::format_window(3600 * 24 * 3), "3 d");
    }

    #[test]
    fn health_coverage_gaps_render_section() {
        use crate::health_types::*;

        let root = PathBuf::from("/project");
        let mut report = empty_report();
        report.coverage_gaps = Some(CoverageGaps {
            summary: CoverageGapSummary {
                runtime_files: 1,
                covered_files: 0,
                file_coverage_pct: 0.0,
                untested_files: 1,
                untested_exports: 1,
            },
            files: vec![UntestedFileFinding::with_actions(
                UntestedFile {
                    path: root.join("src/app.ts"),
                    value_export_count: 2,
                },
                &root,
            )],
            exports: vec![UntestedExportFinding::with_actions(
                UntestedExport {
                    path: root.join("src/app.ts"),
                    export_name: "loader".into(),
                    line: 12,
                    col: 4,
                },
                &root,
            )],
        });

        let text = plain(&build_health_human_lines(&report, &root));
        assert!(
            text.contains("Coverage gaps (1 untested file, 1 untested export, 0.0% file coverage)")
        );
        assert!(text.contains("src/app.ts"));
        assert!(text.contains("loader"));
    }

    #[test]
    fn fmt_trend_val_percentage() {
        assert_eq!(fmt_trend_val(15.5, "%"), "15.5%");
        assert_eq!(fmt_trend_val(0.0, "%"), "0.0%");
    }

    #[test]
    fn fmt_trend_val_integer_when_round() {
        assert_eq!(fmt_trend_val(72.0, ""), "72");
        assert_eq!(fmt_trend_val(5.0, "pts"), "5");
    }

    #[test]
    fn fmt_trend_val_decimal_when_fractional() {
        assert_eq!(fmt_trend_val(4.7, ""), "4.7");
        assert_eq!(fmt_trend_val(1.3, "pts"), "1.3");
    }

    #[test]
    fn fmt_trend_delta_percentage() {
        assert_eq!(fmt_trend_delta(2.5, "%"), "+2.5%");
        assert_eq!(fmt_trend_delta(-1.3, "%"), "-1.3%");
    }

    #[test]
    fn fmt_trend_delta_integer_when_round() {
        assert_eq!(fmt_trend_delta(5.0, ""), "+5");
        assert_eq!(fmt_trend_delta(-3.0, "pts"), "-3");
    }

    #[test]
    fn fmt_trend_delta_decimal_when_fractional() {
        assert_eq!(fmt_trend_delta(4.9, ""), "+4.9");
        assert_eq!(fmt_trend_delta(-0.7, "pts"), "-0.7");
    }

    #[test]
    fn health_score_grade_a_display() {
        let root = PathBuf::from("/project");
        let mut report = empty_report();
        report.health_score = Some(crate::health_types::HealthScore {
            formula_version: crate::health_types::HEALTH_SCORE_FORMULA_VERSION,
            score: 92.0,
            grade: "A",
            penalties: crate::health_types::HealthScorePenalties {
                dead_files: Some(3.0),
                dead_exports: Some(2.0),
                complexity: 1.5,
                p90_complexity: 1.5,
                maintainability: Some(0.0),
                hotspots: Some(0.0),
                unused_deps: Some(0.0),
                circular_deps: Some(0.0),
                unit_size: None,
                coupling: None,
                duplication: None,
            },
        });
        let lines = build_health_human_lines(&report, &root);
        let text = plain(&lines);
        assert!(text.contains("Health score:"));
        assert!(text.contains("92 A"));
        assert!(text.contains("dead files -3.0"));
        assert!(text.contains("dead exports -2.0"));
        assert!(text.contains("complexity -1.5"));
        assert!(text.contains("p90 -1.5"));
    }

    #[test]
    fn health_score_grade_b_display() {
        let root = PathBuf::from("/project");
        let mut report = empty_report();
        report.health_score = Some(crate::health_types::HealthScore {
            formula_version: crate::health_types::HEALTH_SCORE_FORMULA_VERSION,
            score: 76.0,
            grade: "B",
            penalties: crate::health_types::HealthScorePenalties {
                dead_files: Some(5.0),
                dead_exports: Some(6.0),
                complexity: 3.0,
                p90_complexity: 2.0,
                maintainability: Some(4.0),
                hotspots: Some(2.0),
                unused_deps: Some(1.0),
                circular_deps: Some(1.0),
                unit_size: None,
                coupling: None,
                duplication: None,
            },
        });
        let lines = build_health_human_lines(&report, &root);
        let text = plain(&lines);
        assert!(text.contains("76 B"));
        assert!(text.contains("dead exports -6.0"));
        assert!(text.contains("maintainability -4.0"));
        assert!(text.contains("hotspots -2.0"));
        assert!(text.contains("unused deps -1.0"));
        assert!(text.contains("circular deps -1.0"));
    }

    #[test]
    fn health_score_grade_c_display() {
        let root = PathBuf::from("/project");
        let mut report = empty_report();
        report.health_score = Some(crate::health_types::HealthScore {
            formula_version: crate::health_types::HEALTH_SCORE_FORMULA_VERSION,
            score: 60.0,
            grade: "C",
            penalties: crate::health_types::HealthScorePenalties {
                dead_files: Some(10.0),
                dead_exports: Some(10.0),
                complexity: 10.0,
                p90_complexity: 5.0,
                maintainability: Some(5.0),
                hotspots: None,
                unused_deps: None,
                circular_deps: None,
                unit_size: None,
                coupling: None,
                duplication: None,
            },
        });
        let lines = build_health_human_lines(&report, &root);
        let text = plain(&lines);
        assert!(text.contains("60 C"));
    }

    #[test]
    fn health_score_grade_f_display() {
        let root = PathBuf::from("/project");
        let mut report = empty_report();
        report.health_score = Some(crate::health_types::HealthScore {
            formula_version: crate::health_types::HEALTH_SCORE_FORMULA_VERSION,
            score: 30.0,
            grade: "F",
            penalties: crate::health_types::HealthScorePenalties {
                dead_files: Some(15.0),
                dead_exports: Some(15.0),
                complexity: 20.0,
                p90_complexity: 10.0,
                maintainability: Some(10.0),
                hotspots: None,
                unused_deps: None,
                circular_deps: None,
                unit_size: None,
                coupling: None,
                duplication: None,
            },
        });
        let lines = build_health_human_lines(&report, &root);
        let text = plain(&lines);
        assert!(text.contains("30 F"));
    }

    #[test]
    fn health_score_na_components_shown() {
        let root = PathBuf::from("/project");
        let mut report = empty_report();
        report.health_score = Some(crate::health_types::HealthScore {
            formula_version: crate::health_types::HEALTH_SCORE_FORMULA_VERSION,
            score: 90.0,
            grade: "A",
            penalties: crate::health_types::HealthScorePenalties {
                dead_files: None,
                dead_exports: None,
                complexity: 0.0,
                p90_complexity: 0.0,
                maintainability: None,
                hotspots: None,
                unused_deps: None,
                circular_deps: None,
                unit_size: None,
                coupling: None,
                duplication: None,
            },
        });
        let lines = build_health_human_lines(&report, &root);
        let text = plain(&lines);
        assert!(text.contains("N/A: dead code, maintainability, hotspots"));
        assert!(text.contains("enable the corresponding analysis flags"));
    }

    #[test]
    fn health_score_no_na_when_all_present() {
        let root = PathBuf::from("/project");
        let mut report = empty_report();
        report.health_score = Some(crate::health_types::HealthScore {
            formula_version: crate::health_types::HEALTH_SCORE_FORMULA_VERSION,
            score: 85.0,
            grade: "A",
            penalties: crate::health_types::HealthScorePenalties {
                dead_files: Some(0.0),
                dead_exports: Some(0.0),
                complexity: 0.0,
                p90_complexity: 0.0,
                maintainability: Some(0.0),
                hotspots: Some(0.0),
                unused_deps: Some(0.0),
                circular_deps: Some(0.0),
                unit_size: None,
                coupling: None,
                duplication: None,
            },
        });
        let lines = build_health_human_lines(&report, &root);
        let text = plain(&lines);
        assert!(!text.contains("N/A:"));
    }

    #[test]
    fn health_score_zero_penalties_suppressed() {
        let root = PathBuf::from("/project");
        let mut report = empty_report();
        report.health_score = Some(crate::health_types::HealthScore {
            formula_version: crate::health_types::HEALTH_SCORE_FORMULA_VERSION,
            score: 100.0,
            grade: "A",
            penalties: crate::health_types::HealthScorePenalties {
                dead_files: Some(0.0),
                dead_exports: Some(0.0),
                complexity: 0.0,
                p90_complexity: 0.0,
                maintainability: Some(0.0),
                hotspots: Some(0.0),
                unused_deps: Some(0.0),
                circular_deps: Some(0.0),
                unit_size: None,
                coupling: None,
                duplication: None,
            },
        });
        let lines = build_health_human_lines(&report, &root);
        let text = plain(&lines);
        assert!(!text.contains("dead files"));
        assert!(!text.contains("complexity -"));
    }

    #[test]
    fn health_trend_improving_display() {
        let root = PathBuf::from("/project");
        let mut report = empty_report();
        report.health_trend = Some(crate::health_types::HealthTrend {
            compared_to: crate::health_types::TrendPoint {
                timestamp: "2026-03-25T14:30:00Z".into(),
                git_sha: Some("abc1234".into()),
                score: Some(72.0),
                grade: Some("B".into()),
                coverage_model: None,
                snapshot_schema_version: None,
            },
            metrics: vec![
                crate::health_types::TrendMetric {
                    name: "score",
                    label: "Health Score",
                    previous: 72.0,
                    current: 85.0,
                    delta: 13.0,
                    direction: crate::health_types::TrendDirection::Improving,
                    unit: "",
                    previous_count: None,
                    current_count: None,
                },
                crate::health_types::TrendMetric {
                    name: "dead_file_pct",
                    label: "Dead Files",
                    previous: 10.0,
                    current: 5.0,
                    delta: -5.0,
                    direction: crate::health_types::TrendDirection::Improving,
                    unit: "%",
                    previous_count: None,
                    current_count: None,
                },
            ],
            snapshots_loaded: 2,
            overall_direction: crate::health_types::TrendDirection::Improving,
        });
        let lines = build_health_human_lines(&report, &root);
        let text = plain(&lines);
        assert!(text.contains("Trend:"));
        assert!(text.contains("improving"));
        assert!(text.contains("vs 2026-03-25"));
        assert!(text.contains("abc1234"));
        assert!(text.contains("Health Score"));
        assert!(text.contains("+13"));
        assert!(text.contains("Dead Files"));
        assert!(text.contains("-5.0%"));
    }

    #[test]
    fn health_trend_declining_display() {
        let root = PathBuf::from("/project");
        let mut report = empty_report();
        report.health_trend = Some(crate::health_types::HealthTrend {
            compared_to: crate::health_types::TrendPoint {
                timestamp: "2026-03-20T10:00:00Z".into(),
                git_sha: None,
                score: None,
                grade: None,
                coverage_model: None,
                snapshot_schema_version: None,
            },
            metrics: vec![crate::health_types::TrendMetric {
                name: "unused_deps",
                label: "Unused Deps",
                previous: 5.0,
                current: 10.0,
                delta: 5.0,
                direction: crate::health_types::TrendDirection::Declining,
                unit: "",
                previous_count: None,
                current_count: None,
            }],
            snapshots_loaded: 1,
            overall_direction: crate::health_types::TrendDirection::Declining,
        });
        let lines = build_health_human_lines(&report, &root);
        let text = plain(&lines);
        assert!(text.contains("declining"));
        assert!(text.contains("Unused Deps"));
    }

    #[test]
    fn health_trend_all_stable_collapsed() {
        let root = PathBuf::from("/project");
        let mut report = empty_report();
        report.health_trend = Some(crate::health_types::HealthTrend {
            compared_to: crate::health_types::TrendPoint {
                timestamp: "2026-03-25T14:30:00Z".into(),
                git_sha: Some("def5678".into()),
                score: Some(80.0),
                grade: Some("B".into()),
                coverage_model: None,
                snapshot_schema_version: None,
            },
            metrics: vec![
                crate::health_types::TrendMetric {
                    name: "score",
                    label: "Health Score",
                    previous: 80.0,
                    current: 80.0,
                    delta: 0.0,
                    direction: crate::health_types::TrendDirection::Stable,
                    unit: "",
                    previous_count: None,
                    current_count: None,
                },
                crate::health_types::TrendMetric {
                    name: "avg_cyclomatic",
                    label: "Avg Cyclomatic",
                    previous: 2.0,
                    current: 2.0,
                    delta: 0.0,
                    direction: crate::health_types::TrendDirection::Stable,
                    unit: "",
                    previous_count: None,
                    current_count: None,
                },
            ],
            snapshots_loaded: 3,
            overall_direction: crate::health_types::TrendDirection::Stable,
        });
        let lines = build_health_human_lines(&report, &root);
        let text = plain(&lines);
        assert!(text.contains("stable"));
        assert!(text.contains("All 2 metrics unchanged"));
        assert!(!text.contains("Health Score"));
    }

    #[test]
    fn health_trend_without_sha() {
        let root = PathBuf::from("/project");
        let mut report = empty_report();
        report.health_trend = Some(crate::health_types::HealthTrend {
            compared_to: crate::health_types::TrendPoint {
                timestamp: "2026-03-20T10:00:00Z".into(),
                git_sha: None,
                score: None,
                grade: None,
                coverage_model: None,
                snapshot_schema_version: None,
            },
            metrics: vec![crate::health_types::TrendMetric {
                name: "score",
                label: "Health Score",
                previous: 80.0,
                current: 82.0,
                delta: 2.0,
                direction: crate::health_types::TrendDirection::Improving,
                unit: "",
                previous_count: None,
                current_count: None,
            }],
            snapshots_loaded: 1,
            overall_direction: crate::health_types::TrendDirection::Improving,
        });
        let lines = build_health_human_lines(&report, &root);
        let text = plain(&lines);
        assert!(text.contains("vs 2026-03-20"));
        assert!(!text.contains("\u{00b7}"));
    }

    #[test]
    fn vital_signs_shown_without_trend() {
        let root = PathBuf::from("/project");
        let mut report = empty_report();
        report.vital_signs = Some(crate::health_types::VitalSigns {
            dead_file_pct: Some(3.2),
            dead_export_pct: Some(8.1),
            avg_cyclomatic: 4.7,
            p90_cyclomatic: 12,
            duplication_pct: None,
            hotspot_count: Some(2),
            maintainability_avg: Some(72.4),
            unused_dep_count: Some(3),
            circular_dep_count: Some(1),
            counts: None,
            unit_size_profile: None,
            unit_interfacing_profile: None,
            p95_fan_in: None,
            coupling_high_pct: None,
            total_loc: 42_381,
            ..Default::default()
        });
        report.hotspot_summary = Some(crate::health_types::HotspotSummary {
            since: "6 months".to_string(),
            min_commits: 3,
            files_analyzed: 50,
            files_excluded: 20,
            shallow_clone: false,
        });
        let lines = build_health_human_lines(&report, &root);
        let text = plain(&lines);
        assert!(text.contains("42,381 LOC"));
        assert!(text.contains("dead files 3.2%"));
        assert!(text.contains("dead exports 8.1%"));
        assert!(text.contains("avg cyclomatic 4.7"));
        assert!(text.contains("p90 cyclomatic 12"));
        assert!(text.contains("maintainability 72.4"));
        assert!(text.contains("2 churn hotspots (since 6 months)"));
        assert!(text.contains("3 unused deps"));
        assert!(text.contains("1 circular dep"));
    }

    #[test]
    fn vital_signs_zero_hotspots_still_show_window() {
        let root = PathBuf::from("/project");
        let mut report = empty_report();
        report.vital_signs = Some(crate::health_types::VitalSigns {
            avg_cyclomatic: 2.0,
            p90_cyclomatic: 5,
            hotspot_count: Some(0),
            total_loc: 1_000,
            ..Default::default()
        });
        report.hotspot_summary = Some(crate::health_types::HotspotSummary {
            since: "90 days".to_string(),
            min_commits: 3,
            files_analyzed: 10,
            files_excluded: 0,
            shallow_clone: false,
        });
        let lines = build_health_human_lines(&report, &root);
        let text = plain(&lines);
        assert!(text.contains("0 churn hotspots (since 90 days)"));
        assert!(!text.contains("Hotspots ("));
    }

    #[test]
    fn vital_signs_hotspot_count_without_summary_omits_window() {
        let root = PathBuf::from("/project");
        let mut report = empty_report();
        report.vital_signs = Some(crate::health_types::VitalSigns {
            avg_cyclomatic: 2.0,
            p90_cyclomatic: 5,
            hotspot_count: Some(1),
            total_loc: 1_000,
            ..Default::default()
        });
        report.hotspot_summary = None;
        let lines = build_health_human_lines(&report, &root);
        let text = plain(&lines);
        assert!(text.contains("1 churn hotspot"));
        assert!(!text.contains("(since"));
    }

    #[test]
    fn vital_signs_suppressed_when_trend_active() {
        let root = PathBuf::from("/project");
        let mut report = empty_report();
        report.vital_signs = Some(crate::health_types::VitalSigns {
            dead_file_pct: Some(3.2),
            dead_export_pct: Some(8.1),
            avg_cyclomatic: 4.7,
            p90_cyclomatic: 12,
            duplication_pct: None,
            hotspot_count: Some(2),
            maintainability_avg: Some(72.4),
            unused_dep_count: None,
            circular_dep_count: None,
            counts: None,
            unit_size_profile: None,
            unit_interfacing_profile: None,
            p95_fan_in: None,
            coupling_high_pct: None,
            total_loc: 0,
            ..Default::default()
        });
        report.health_trend = Some(crate::health_types::HealthTrend {
            compared_to: crate::health_types::TrendPoint {
                timestamp: "2026-03-25T14:30:00Z".into(),
                git_sha: None,
                score: None,
                grade: None,
                coverage_model: None,
                snapshot_schema_version: None,
            },
            metrics: vec![],
            snapshots_loaded: 1,
            overall_direction: crate::health_types::TrendDirection::Stable,
        });
        let lines = build_health_human_lines(&report, &root);
        let text = plain(&lines);
        assert!(!text.contains("dead files"));
        assert!(!text.contains("avg cyclomatic"));
    }

    #[test]
    fn vital_signs_optional_fields_omitted_when_none() {
        let root = PathBuf::from("/project");
        let mut report = empty_report();
        report.vital_signs = Some(crate::health_types::VitalSigns {
            dead_file_pct: None,
            dead_export_pct: None,
            avg_cyclomatic: 2.0,
            p90_cyclomatic: 5,
            duplication_pct: None,
            hotspot_count: None,
            maintainability_avg: None,
            unused_dep_count: None,
            circular_dep_count: None,
            counts: None,
            unit_size_profile: None,
            unit_interfacing_profile: None,
            p95_fan_in: None,
            coupling_high_pct: None,
            total_loc: 0,
            ..Default::default()
        });
        let lines = build_health_human_lines(&report, &root);
        let text = plain(&lines);
        assert!(!text.contains("dead files"));
        assert!(!text.contains("dead exports"));
        assert!(!text.contains("maintainability "));
        assert!(!text.contains("hotspot"));
        assert!(text.contains("avg cyclomatic 2.0"));
        assert!(text.contains("p90 cyclomatic 5"));
    }

    #[test]
    fn vital_signs_zero_counts_suppressed() {
        let root = PathBuf::from("/project");
        let mut report = empty_report();
        report.vital_signs = Some(crate::health_types::VitalSigns {
            dead_file_pct: None,
            dead_export_pct: None,
            avg_cyclomatic: 1.0,
            p90_cyclomatic: 2,
            duplication_pct: None,
            hotspot_count: None,
            maintainability_avg: None,
            unused_dep_count: Some(0),
            circular_dep_count: Some(0),
            counts: None,
            unit_size_profile: None,
            unit_interfacing_profile: None,
            p95_fan_in: None,
            coupling_high_pct: None,
            total_loc: 0,
            ..Default::default()
        });
        let lines = build_health_human_lines(&report, &root);
        let text = plain(&lines);
        assert!(!text.contains("unused dep"));
        assert!(!text.contains("circular dep"));
    }

    #[test]
    fn vital_signs_plural_vs_singular() {
        let root = PathBuf::from("/project");
        let mut report = empty_report();
        report.vital_signs = Some(crate::health_types::VitalSigns {
            dead_file_pct: None,
            dead_export_pct: None,
            avg_cyclomatic: 1.0,
            p90_cyclomatic: 2,
            duplication_pct: None,
            hotspot_count: Some(1),
            maintainability_avg: None,
            unused_dep_count: Some(1),
            circular_dep_count: Some(2),
            counts: None,
            unit_size_profile: None,
            unit_interfacing_profile: None,
            p95_fan_in: None,
            coupling_high_pct: None,
            total_loc: 0,
            ..Default::default()
        });
        let lines = build_health_human_lines(&report, &root);
        let text = plain(&lines);
        assert!(text.contains("1 churn hotspot"));
        assert!(!text.contains("1 churn hotspots"));
        assert!(text.contains("1 unused dep"));
        assert!(!text.contains("1 unused deps"));
        assert!(text.contains("2 circular deps"));
    }

    #[test]
    fn file_scores_single_entry() {
        let root = PathBuf::from("/project");
        let mut report = empty_report();
        report.file_scores = vec![crate::health_types::FileHealthScore {
            path: root.join("src/utils.ts"),
            fan_in: 5,
            fan_out: 3,
            dead_code_ratio: 0.15,
            complexity_density: 0.42,
            maintainability_index: 85.3,
            total_cyclomatic: 12,
            total_cognitive: 8,
            function_count: 4,
            lines: 200,
            crap_max: 0.0,
            crap_above_threshold: 0,
        }];
        let lines = build_health_human_lines(&report, &root);
        let text = plain(&lines);
        assert!(text.contains("File health scores (1 files)"));
        assert!(text.contains("85.3"));
        assert!(text.contains("src/utils.ts"));
        assert!(text.contains("200 LOC"));
        assert!(text.contains("5 fan-in"));
        assert!(text.contains("3 fan-out"));
        assert!(text.contains("15% dead"));
        assert!(text.contains("0.42 density"));
    }

    #[test]
    fn file_scores_concern_tag_marks_risk_vs_structure() {
        let root = PathBuf::from("/project");
        let mut report = empty_report();
        report.file_scores = vec![
            crate::health_types::FileHealthScore {
                path: root.join("src/risky.ts"),
                fan_in: 0,
                fan_out: 0,
                dead_code_ratio: 0.0,
                complexity_density: 0.2,
                maintainability_index: 85.0,
                total_cyclomatic: 10,
                total_cognitive: 8,
                function_count: 1,
                lines: 100,
                crap_max: 552.0,
                crap_above_threshold: 1,
            },
            crate::health_types::FileHealthScore {
                path: root.join("src/messy.ts"),
                fan_in: 0,
                fan_out: 0,
                dead_code_ratio: 0.0,
                complexity_density: 0.3,
                maintainability_index: 30.0,
                total_cyclomatic: 5,
                total_cognitive: 3,
                function_count: 1,
                lines: 100,
                crap_max: 2.0,
                crap_above_threshold: 0,
            },
        ];
        let text = plain(&build_health_human_lines(&report, &root));
        let risky_line = text
            .lines()
            .find(|l| l.contains("risky.ts"))
            .expect("risky path line");
        assert!(
            risky_line.trim_end().ends_with("risk"),
            "expected risk tag, got: {risky_line:?}"
        );
        let messy_line = text
            .lines()
            .find(|l| l.contains("messy.ts"))
            .expect("messy path line");
        assert!(
            messy_line.trim_end().ends_with("structure"),
            "expected structure tag, got: {messy_line:?}"
        );
    }

    #[test]
    fn file_scores_mi_color_thresholds() {
        let root = PathBuf::from("/project");
        let mut report = empty_report();
        report.file_scores = vec![
            crate::health_types::FileHealthScore {
                path: root.join("src/good.ts"),
                fan_in: 1,
                fan_out: 1,
                dead_code_ratio: 0.0,
                complexity_density: 0.1,
                maintainability_index: 90.0, // green: >= 80
                total_cyclomatic: 2,
                total_cognitive: 1,
                function_count: 1,
                lines: 50,
                crap_max: 0.0,
                crap_above_threshold: 0,
            },
            crate::health_types::FileHealthScore {
                path: root.join("src/okay.ts"),
                fan_in: 2,
                fan_out: 3,
                dead_code_ratio: 0.1,
                complexity_density: 0.3,
                maintainability_index: 65.0, // yellow: >= 50
                total_cyclomatic: 8,
                total_cognitive: 5,
                function_count: 3,
                lines: 100,
                crap_max: 0.0,
                crap_above_threshold: 0,
            },
            crate::health_types::FileHealthScore {
                path: root.join("src/bad.ts"),
                fan_in: 8,
                fan_out: 12,
                dead_code_ratio: 0.5,
                complexity_density: 0.9,
                maintainability_index: 30.0, // red: < 50
                total_cyclomatic: 40,
                total_cognitive: 30,
                function_count: 10,
                lines: 500,
                crap_max: 0.0,
                crap_above_threshold: 0,
            },
        ];
        let lines = build_health_human_lines(&report, &root);
        let text = plain(&lines);
        assert!(text.contains("File health scores (3 files)"));
        assert!(text.contains("90.0"));
        assert!(text.contains("65.0"));
        assert!(text.contains("30.0"));
    }

    #[test]
    fn file_scores_truncation_above_max_flat_items() {
        let root = PathBuf::from("/project");
        let mut report = empty_report();
        for i in 0..12 {
            report
                .file_scores
                .push(crate::health_types::FileHealthScore {
                    path: root.join(format!("src/file{i}.ts")),
                    fan_in: 1,
                    fan_out: 1,
                    dead_code_ratio: 0.0,
                    complexity_density: 0.1,
                    maintainability_index: 80.0,
                    total_cyclomatic: 2,
                    total_cognitive: 1,
                    function_count: 1,
                    lines: 50,
                    crap_max: 0.0,
                    crap_above_threshold: 0,
                });
        }
        let lines = build_health_human_lines(&report, &root);
        let text = plain(&lines);
        assert!(text.contains("File health scores (12 files)"));
        assert!(text.contains("... and 2 more files"));
        assert!(text.contains("file0.ts"));
        assert!(text.contains("file9.ts"));
        assert!(!text.contains("file10.ts"));
        assert!(!text.contains("file11.ts"));
    }

    #[test]
    fn file_scores_docs_link() {
        let root = PathBuf::from("/project");
        let mut report = empty_report();
        report.file_scores = vec![crate::health_types::FileHealthScore {
            path: root.join("src/a.ts"),
            fan_in: 1,
            fan_out: 1,
            dead_code_ratio: 0.0,
            complexity_density: 0.1,
            maintainability_index: 80.0,
            total_cyclomatic: 2,
            total_cognitive: 1,
            function_count: 1,
            lines: 50,
            crap_max: 0.0,
            crap_above_threshold: 0,
        }];
        let lines = build_health_human_lines(&report, &root);
        let text = plain(&lines);
        assert!(text.contains("docs.fallow.tools/explanations/health#file-health-scores"));
    }

    #[test]
    fn hotspots_accelerating_trend() {
        let root = PathBuf::from("/project");
        let mut report = empty_report();
        report.hotspots = vec![
            crate::health_types::HotspotEntry {
                path: root.join("src/core.ts"),
                score: 75.0,
                commits: 42,
                weighted_commits: 30.0,
                lines_added: 500,
                lines_deleted: 200,
                complexity_density: 0.85,
                fan_in: 10,
                trend: fallow_core::churn::ChurnTrend::Accelerating,
                ownership: None,
                is_test_path: false,
            }
            .into(),
        ];
        let lines = build_health_human_lines(&report, &root);
        let text = plain(&lines);
        assert!(text.contains("Hotspots (1 files)"));
        assert!(text.contains("75.0"));
        assert!(text.contains("src/core.ts"));
        assert!(text.contains("42 commits"));
        assert!(text.contains("700 churn"));
        assert!(text.contains("0.85 density"));
        assert!(text.contains("10 fan-in"));
        assert!(text.contains("accelerating"));
    }

    #[test]
    fn hotspots_cooling_trend() {
        let root = PathBuf::from("/project");
        let mut report = empty_report();
        report.hotspots = vec![
            crate::health_types::HotspotEntry {
                path: root.join("src/old.ts"),
                score: 20.0,
                commits: 5,
                weighted_commits: 2.0,
                lines_added: 50,
                lines_deleted: 30,
                complexity_density: 0.3,
                fan_in: 2,
                trend: fallow_core::churn::ChurnTrend::Cooling,
                ownership: None,
                is_test_path: false,
            }
            .into(),
        ];
        let lines = build_health_human_lines(&report, &root);
        let text = plain(&lines);
        assert!(text.contains("20.0"));
        assert!(text.contains("cooling"));
    }

    #[test]
    fn hotspots_stable_trend() {
        let root = PathBuf::from("/project");
        let mut report = empty_report();
        report.hotspots = vec![
            crate::health_types::HotspotEntry {
                path: root.join("src/mid.ts"),
                score: 45.0,
                commits: 15,
                weighted_commits: 10.0,
                lines_added: 200,
                lines_deleted: 100,
                complexity_density: 0.5,
                fan_in: 5,
                trend: fallow_core::churn::ChurnTrend::Stable,
                ownership: None,
                is_test_path: false,
            }
            .into(),
        ];
        let lines = build_health_human_lines(&report, &root);
        let text = plain(&lines);
        assert!(text.contains("45.0"));
        assert!(text.contains("stable"));
    }

    #[test]
    fn hotspots_with_summary_and_since() {
        let root = PathBuf::from("/project");
        let mut report = empty_report();
        report.hotspots = vec![
            crate::health_types::HotspotEntry {
                path: root.join("src/a.ts"),
                score: 50.0,
                commits: 10,
                weighted_commits: 8.0,
                lines_added: 100,
                lines_deleted: 50,
                complexity_density: 0.4,
                fan_in: 3,
                trend: fallow_core::churn::ChurnTrend::Stable,
                ownership: None,
                is_test_path: false,
            }
            .into(),
        ];
        report.hotspot_summary = Some(crate::health_types::HotspotSummary {
            since: "6 months".to_string(),
            min_commits: 3,
            files_analyzed: 50,
            files_excluded: 20,
            shallow_clone: false,
        });
        let lines = build_health_human_lines(&report, &root);
        let text = plain(&lines);
        assert!(text.contains("Hotspots (1 files, since 6 months)"));
        assert!(text.contains("20 files excluded (< 3 commits)"));
    }

    #[test]
    fn hotspots_summary_no_exclusions() {
        let root = PathBuf::from("/project");
        let mut report = empty_report();
        report.hotspots = vec![
            crate::health_types::HotspotEntry {
                path: root.join("src/a.ts"),
                score: 50.0,
                commits: 10,
                weighted_commits: 8.0,
                lines_added: 100,
                lines_deleted: 50,
                complexity_density: 0.4,
                fan_in: 3,
                trend: fallow_core::churn::ChurnTrend::Stable,
                ownership: None,
                is_test_path: false,
            }
            .into(),
        ];
        report.hotspot_summary = Some(crate::health_types::HotspotSummary {
            since: "3 months".to_string(),
            min_commits: 2,
            files_analyzed: 50,
            files_excluded: 0,
            shallow_clone: false,
        });
        let lines = build_health_human_lines(&report, &root);
        let text = plain(&lines);
        assert!(!text.contains("files excluded"));
    }

    #[test]
    fn hotspots_docs_link() {
        let root = PathBuf::from("/project");
        let mut report = empty_report();
        report.hotspots = vec![
            crate::health_types::HotspotEntry {
                path: root.join("src/a.ts"),
                score: 50.0,
                commits: 10,
                weighted_commits: 8.0,
                lines_added: 100,
                lines_deleted: 50,
                complexity_density: 0.4,
                fan_in: 3,
                trend: fallow_core::churn::ChurnTrend::Stable,
                ownership: None,
                is_test_path: false,
            }
            .into(),
        ];
        let lines = build_health_human_lines(&report, &root);
        let text = plain(&lines);
        assert!(text.contains("docs.fallow.tools/explanations/health#hotspot-metrics"));
    }

    #[test]
    fn refactoring_targets_single_low_effort() {
        let root = PathBuf::from("/project");
        let mut report = empty_report();
        report.targets = vec![
            crate::health_types::RefactoringTarget {
                path: root.join("src/legacy.ts"),
                priority: 65.0,
                efficiency: 65.0,
                recommendation: "Extract complex logic into helper functions".to_string(),
                category: crate::health_types::RecommendationCategory::ExtractComplexFunctions,
                effort: crate::health_types::EffortEstimate::Low,
                confidence: crate::health_types::Confidence::High,
                factors: vec![],
                evidence: None,
            }
            .into(),
        ];
        let lines = build_health_human_lines(&report, &root);
        let text = plain(&lines);
        assert!(text.contains("Refactoring targets (1)"));
        assert!(text.contains("1 low effort"));
        assert!(text.contains("65.0"));
        assert!(text.contains("pri:65.0"));
        assert!(text.contains("src/legacy.ts"));
        assert!(text.contains("complexity"));
        assert!(text.contains("effort:low"));
        assert!(text.contains("confidence:high"));
        assert!(text.contains("Extract complex logic into helper functions"));
    }

    #[test]
    fn refactoring_targets_mixed_effort() {
        let root = PathBuf::from("/project");
        let mut report = empty_report();
        report.targets = vec![
            crate::health_types::RefactoringTarget {
                path: root.join("src/a.ts"),
                priority: 80.0,
                efficiency: 80.0,
                recommendation: "Remove dead exports".to_string(),
                category: crate::health_types::RecommendationCategory::RemoveDeadCode,
                effort: crate::health_types::EffortEstimate::Low,
                confidence: crate::health_types::Confidence::High,
                factors: vec![],
                evidence: None,
            }
            .into(),
            crate::health_types::RefactoringTarget {
                path: root.join("src/b.ts"),
                priority: 60.0,
                efficiency: 30.0,
                recommendation: "Split into smaller modules".to_string(),
                category: crate::health_types::RecommendationCategory::SplitHighImpact,
                effort: crate::health_types::EffortEstimate::Medium,
                confidence: crate::health_types::Confidence::Medium,
                factors: vec![],
                evidence: None,
            }
            .into(),
            crate::health_types::RefactoringTarget {
                path: root.join("src/c.ts"),
                priority: 50.0,
                efficiency: 16.7,
                recommendation: "Break circular dependency".to_string(),
                category: crate::health_types::RecommendationCategory::BreakCircularDependency,
                effort: crate::health_types::EffortEstimate::High,
                confidence: crate::health_types::Confidence::Low,
                factors: vec![],
                evidence: None,
            }
            .into(),
        ];
        let lines = build_health_human_lines(&report, &root);
        let text = plain(&lines);
        assert!(text.contains("Refactoring targets (3)"));
        assert!(text.contains("1 low effort"));
        assert!(text.contains("1 medium"));
        assert!(text.contains("1 high"));
        assert!(text.contains("effort:low"));
        assert!(text.contains("effort:medium"));
        assert!(text.contains("effort:high"));
        assert!(text.contains("confidence:high"));
        assert!(text.contains("confidence:medium"));
        assert!(text.contains("confidence:low"));
    }

    #[test]
    fn refactoring_targets_truncation_above_max_flat_items() {
        let root = PathBuf::from("/project");
        let mut report = empty_report();
        for i in 0..12 {
            report.targets.push(
                crate::health_types::RefactoringTarget {
                    path: root.join(format!("src/target{i}.ts")),
                    priority: 50.0,
                    efficiency: 25.0,
                    recommendation: format!("Fix target {i}"),
                    category: crate::health_types::RecommendationCategory::ExtractComplexFunctions,
                    effort: crate::health_types::EffortEstimate::Medium,
                    confidence: crate::health_types::Confidence::Medium,
                    factors: vec![],
                    evidence: None,
                }
                .into(),
            );
        }
        let lines = build_health_human_lines(&report, &root);
        let text = plain(&lines);
        assert!(text.contains("Refactoring targets (12)"));
        assert!(text.contains("... and 2 more targets"));
        assert!(text.contains("target0.ts"));
        assert!(text.contains("target9.ts"));
        assert!(!text.contains("target10.ts"));
    }

    #[test]
    fn refactoring_targets_docs_link() {
        let root = PathBuf::from("/project");
        let mut report = empty_report();
        report.targets = vec![
            crate::health_types::RefactoringTarget {
                path: root.join("src/a.ts"),
                priority: 50.0,
                efficiency: 50.0,
                recommendation: "Fix it".to_string(),
                category: crate::health_types::RecommendationCategory::ExtractDependencies,
                effort: crate::health_types::EffortEstimate::Low,
                confidence: crate::health_types::Confidence::High,
                factors: vec![],
                evidence: None,
            }
            .into(),
        ];
        let lines = build_health_human_lines(&report, &root);
        let text = plain(&lines);
        assert!(text.contains("docs.fallow.tools/explanations/health#refactoring-targets"));
    }

    #[test]
    fn refactoring_targets_all_categories() {
        let root = PathBuf::from("/project");
        let mut report = empty_report();
        let categories = [
            (
                crate::health_types::RecommendationCategory::UrgentChurnComplexity,
                "churn+complexity",
            ),
            (
                crate::health_types::RecommendationCategory::BreakCircularDependency,
                "circular dependency",
            ),
            (
                crate::health_types::RecommendationCategory::SplitHighImpact,
                "high impact",
            ),
            (
                crate::health_types::RecommendationCategory::RemoveDeadCode,
                "dead code",
            ),
            (
                crate::health_types::RecommendationCategory::ExtractComplexFunctions,
                "complexity",
            ),
            (
                crate::health_types::RecommendationCategory::ExtractDependencies,
                "coupling",
            ),
            (
                crate::health_types::RecommendationCategory::AddTestCoverage,
                "untested risk",
            ),
        ];
        for (i, (cat, _label)) in categories.iter().enumerate() {
            report.targets.push(
                crate::health_types::RefactoringTarget {
                    path: root.join(format!("src/cat{i}.ts")),
                    priority: 50.0,
                    efficiency: 50.0,
                    recommendation: format!("Fix cat{i}"),
                    category: cat.clone(),
                    effort: crate::health_types::EffortEstimate::Low,
                    confidence: crate::health_types::Confidence::High,
                    factors: vec![],
                    evidence: None,
                }
                .into(),
            );
        }
        let lines = build_health_human_lines(&report, &root);
        let text = plain(&lines);
        for (_cat, label) in &categories {
            assert!(
                text.contains(label),
                "Expected category label '{label}' in output"
            );
        }
    }

    #[test]
    fn refactoring_targets_efficiency_color_thresholds() {
        let root = PathBuf::from("/project");
        let mut report = empty_report();
        report.targets = vec![
            crate::health_types::RefactoringTarget {
                path: root.join("src/high.ts"),
                priority: 50.0,
                efficiency: 50.0, // green: >= 40
                recommendation: "High eff".to_string(),
                category: crate::health_types::RecommendationCategory::RemoveDeadCode,
                effort: crate::health_types::EffortEstimate::Low,
                confidence: crate::health_types::Confidence::High,
                factors: vec![],
                evidence: None,
            }
            .into(),
            crate::health_types::RefactoringTarget {
                path: root.join("src/mid.ts"),
                priority: 50.0,
                efficiency: 25.0, // yellow: >= 20
                recommendation: "Mid eff".to_string(),
                category: crate::health_types::RecommendationCategory::RemoveDeadCode,
                effort: crate::health_types::EffortEstimate::Medium,
                confidence: crate::health_types::Confidence::Medium,
                factors: vec![],
                evidence: None,
            }
            .into(),
            crate::health_types::RefactoringTarget {
                path: root.join("src/low.ts"),
                priority: 50.0,
                efficiency: 10.0, // dimmed: < 20
                recommendation: "Low eff".to_string(),
                category: crate::health_types::RecommendationCategory::RemoveDeadCode,
                effort: crate::health_types::EffortEstimate::High,
                confidence: crate::health_types::Confidence::Low,
                factors: vec![],
                evidence: None,
            }
            .into(),
        ];
        let lines = build_health_human_lines(&report, &root);
        let text = plain(&lines);
        assert!(text.contains("50.0"));
        assert!(text.contains("25.0"));
        assert!(text.contains("10.0"));
    }

    #[test]
    fn all_sections_combined() {
        let root = PathBuf::from("/project");
        let mut report = empty_report();
        report.summary.functions_above_threshold = 1;
        report.findings = vec![
            crate::health_types::ComplexityViolation {
                path: root.join("src/complex.ts"),
                name: "bigFn".to_string(),
                line: 10,
                col: 0,
                cyclomatic: 25,
                cognitive: 20,
                line_count: 80,
                param_count: 0,
                exceeded: crate::health_types::ExceededThreshold::Both,
                severity: crate::health_types::FindingSeverity::Moderate,
                crap: None,
                coverage_pct: None,
                coverage_tier: None,
                coverage_source: None,
                inherited_from: None,
                component_rollup: None,
                contributions: Vec::new(),
            }
            .into(),
        ];
        report.health_score = Some(crate::health_types::HealthScore {
            formula_version: crate::health_types::HEALTH_SCORE_FORMULA_VERSION,
            score: 75.0,
            grade: "B",
            penalties: crate::health_types::HealthScorePenalties {
                dead_files: Some(5.0),
                dead_exports: Some(5.0),
                complexity: 5.0,
                p90_complexity: 2.0,
                maintainability: Some(3.0),
                hotspots: Some(2.0),
                unused_deps: Some(2.0),
                circular_deps: Some(1.0),
                unit_size: None,
                coupling: None,
                duplication: None,
            },
        });
        report.file_scores = vec![crate::health_types::FileHealthScore {
            path: root.join("src/complex.ts"),
            fan_in: 5,
            fan_out: 3,
            dead_code_ratio: 0.1,
            complexity_density: 0.5,
            maintainability_index: 60.0,
            total_cyclomatic: 15,
            total_cognitive: 10,
            function_count: 3,
            lines: 200,
            crap_max: 0.0,
            crap_above_threshold: 0,
        }];
        report.hotspots = vec![
            crate::health_types::HotspotEntry {
                path: root.join("src/complex.ts"),
                score: 65.0,
                commits: 20,
                weighted_commits: 15.0,
                lines_added: 300,
                lines_deleted: 100,
                complexity_density: 0.5,
                fan_in: 5,
                trend: fallow_core::churn::ChurnTrend::Accelerating,
                ownership: None,
                is_test_path: false,
            }
            .into(),
        ];
        report.targets = vec![
            crate::health_types::RefactoringTarget {
                path: root.join("src/complex.ts"),
                priority: 70.0,
                efficiency: 70.0,
                recommendation: "Extract complex functions".to_string(),
                category: crate::health_types::RecommendationCategory::ExtractComplexFunctions,
                effort: crate::health_types::EffortEstimate::Low,
                confidence: crate::health_types::Confidence::High,
                factors: vec![],
                evidence: None,
            }
            .into(),
        ];
        let lines = build_health_human_lines(&report, &root);
        let text = plain(&lines);
        assert!(text.contains("Health score:"));
        assert!(text.contains("High complexity functions"));
        assert!(text.contains("File health scores"));
        assert!(text.contains("Hotspots"));
        assert!(text.contains("Refactoring targets"));
    }

    #[test]
    fn completely_empty_report_produces_no_lines() {
        let root = PathBuf::from("/project");
        let report = empty_report();
        let lines = build_health_human_lines(&report, &root);
        assert!(lines.is_empty());
    }

    #[test]
    fn finding_only_cyclomatic_exceeds() {
        let root = PathBuf::from("/project");
        let mut report = empty_report();
        report.summary.functions_above_threshold = 1;
        report.findings = vec![
            crate::health_types::ComplexityViolation {
                path: root.join("src/a.ts"),
                name: "fn1".to_string(),
                line: 1,
                col: 0,
                cyclomatic: 25, // exceeds 20
                cognitive: 10,  // does not exceed 15
                line_count: 50,
                param_count: 0,
                exceeded: crate::health_types::ExceededThreshold::Cyclomatic,
                severity: crate::health_types::FindingSeverity::Moderate,
                crap: None,
                coverage_pct: None,
                coverage_tier: None,
                coverage_source: None,
                inherited_from: None,
                component_rollup: None,
                contributions: Vec::new(),
            }
            .into(),
        ];
        let lines = build_health_human_lines(&report, &root);
        let text = plain(&lines);
        assert!(text.contains("25 cyclomatic"));
        assert!(text.contains("10 cognitive"));
    }

    #[test]
    fn finding_only_cognitive_exceeds() {
        let root = PathBuf::from("/project");
        let mut report = empty_report();
        report.summary.functions_above_threshold = 1;
        report.findings = vec![
            crate::health_types::ComplexityViolation {
                path: root.join("src/a.ts"),
                name: "fn1".to_string(),
                line: 1,
                col: 0,
                cyclomatic: 10, // does not exceed 20
                cognitive: 25,  // exceeds 15
                line_count: 50,
                param_count: 0,
                exceeded: crate::health_types::ExceededThreshold::Cognitive,
                severity: crate::health_types::FindingSeverity::High,
                crap: None,
                coverage_pct: None,
                coverage_tier: None,
                coverage_source: None,
                inherited_from: None,
                component_rollup: None,
                contributions: Vec::new(),
            }
            .into(),
        ];
        let lines = build_health_human_lines(&report, &root);
        let text = plain(&lines);
        assert!(text.contains("10 cyclomatic"));
        assert!(text.contains("25 cognitive"));
    }

    #[test]
    fn findings_across_multiple_files() {
        let root = PathBuf::from("/project");
        let mut report = empty_report();
        report.summary.functions_above_threshold = 2;
        report.findings = vec![
            crate::health_types::ComplexityViolation {
                path: root.join("src/a.ts"),
                name: "fn1".to_string(),
                line: 1,
                col: 0,
                cyclomatic: 25,
                cognitive: 20,
                line_count: 50,
                param_count: 0,
                exceeded: crate::health_types::ExceededThreshold::Both,
                severity: crate::health_types::FindingSeverity::Moderate,
                crap: None,
                coverage_pct: None,
                coverage_tier: None,
                coverage_source: None,
                inherited_from: None,
                component_rollup: None,
                contributions: Vec::new(),
            }
            .into(),
            crate::health_types::ComplexityViolation {
                path: root.join("src/b.ts"),
                name: "fn2".to_string(),
                line: 5,
                col: 0,
                cyclomatic: 22,
                cognitive: 18,
                line_count: 40,
                param_count: 0,
                exceeded: crate::health_types::ExceededThreshold::Both,
                severity: crate::health_types::FindingSeverity::Moderate,
                crap: None,
                coverage_pct: None,
                coverage_tier: None,
                coverage_source: None,
                inherited_from: None,
                component_rollup: None,
                contributions: Vec::new(),
            }
            .into(),
        ];
        let lines = build_health_human_lines(&report, &root);
        let text = plain(&lines);
        assert!(text.contains("src/a.ts"));
        assert!(text.contains("src/b.ts"));
    }

    #[test]
    fn findings_docs_link() {
        let root = PathBuf::from("/project");
        let mut report = empty_report();
        report.summary.functions_above_threshold = 1;
        report.findings = vec![
            crate::health_types::ComplexityViolation {
                path: root.join("src/a.ts"),
                name: "fn1".to_string(),
                line: 1,
                col: 0,
                cyclomatic: 25,
                cognitive: 20,
                line_count: 50,
                param_count: 0,
                exceeded: crate::health_types::ExceededThreshold::Both,
                severity: crate::health_types::FindingSeverity::Moderate,
                crap: None,
                coverage_pct: None,
                coverage_tier: None,
                coverage_source: None,
                inherited_from: None,
                component_rollup: None,
                contributions: Vec::new(),
            }
            .into(),
        ];
        let lines = build_health_human_lines(&report, &root);
        let text = plain(&lines);
        assert!(text.contains("docs.fallow.tools/explanations/health#complexity-metrics"));
    }

    #[test]
    fn hotspot_score_high_medium_low() {
        let root = PathBuf::from("/project");
        let mut report = empty_report();
        report.hotspots = vec![
            crate::health_types::HotspotEntry {
                path: root.join("src/high.ts"),
                score: 80.0, // red: >= 70
                commits: 30,
                weighted_commits: 25.0,
                lines_added: 400,
                lines_deleted: 200,
                complexity_density: 0.9,
                fan_in: 8,
                trend: fallow_core::churn::ChurnTrend::Accelerating,
                ownership: None,
                is_test_path: false,
            }
            .into(),
            crate::health_types::HotspotEntry {
                path: root.join("src/medium.ts"),
                score: 45.0, // yellow: >= 30
                commits: 15,
                weighted_commits: 10.0,
                lines_added: 200,
                lines_deleted: 100,
                complexity_density: 0.5,
                fan_in: 4,
                trend: fallow_core::churn::ChurnTrend::Stable,
                ownership: None,
                is_test_path: false,
            }
            .into(),
            crate::health_types::HotspotEntry {
                path: root.join("src/low.ts"),
                score: 15.0, // green: < 30
                commits: 5,
                weighted_commits: 3.0,
                lines_added: 50,
                lines_deleted: 20,
                complexity_density: 0.2,
                fan_in: 1,
                trend: fallow_core::churn::ChurnTrend::Cooling,
                ownership: None,
                is_test_path: false,
            }
            .into(),
        ];
        let lines = build_health_human_lines(&report, &root);
        let text = plain(&lines);
        assert!(text.contains("80.0"));
        assert!(text.contains("45.0"));
        assert!(text.contains("15.0"));
        assert!(text.contains("Hotspots (3 files)"));
    }

    #[test]
    fn rollup_breakdown_renders_workspace_relative_template_path() {
        let root = PathBuf::from("/project");
        let template =
            root.join("apps/admin/src/app/payments/payment-list/payment-list.component.html");
        let finding = crate::health_types::ComplexityViolation {
            path: root.join("apps/admin/src/app/payments/payment-list/payment-list.component.ts"),
            name: "<component>".to_string(),
            line: 1,
            col: 0,
            cyclomatic: 25,
            cognitive: 28,
            line_count: 0,
            param_count: 0,
            exceeded: crate::health_types::ExceededThreshold::Both,
            severity: crate::health_types::FindingSeverity::High,
            crap: None,
            coverage_pct: None,
            coverage_tier: None,
            coverage_source: None,
            inherited_from: None,
            component_rollup: Some(crate::health_types::ComponentRollup {
                component: "PaymentListComponent".to_string(),
                class_worst_function: "ngOnInit".to_string(),
                class_cyclomatic: 12,
                class_cognitive: 16,
                template_path: template,
                template_cyclomatic: 13,
                template_cognitive: 12,
            }),
            contributions: Vec::new(),
        };
        let line = render_component_rollup_breakdown(&finding, &root)
            .expect("rollup payload should render a breakdown line");
        assert!(
            line.contains("apps/admin/src/app/payments/payment-list/payment-list.component.html"),
            "breakdown must include workspace-relative template path: {line}"
        );
        assert!(
            !line.contains(" payment-list.component.html"),
            "bare basename token must not be the rendered template: {line}"
        );
    }

    #[test]
    fn inherited_from_renders_workspace_relative_owner_path() {
        let root = PathBuf::from("/project");
        let owner = root.join("apps/admin/src/app/auth/permissions/permissions.component.ts");
        let template_path =
            root.join("apps/admin/src/app/auth/permissions/permissions.component.html");
        let report = crate::health_types::HealthReport {
            findings: vec![
                crate::health_types::ComplexityViolation {
                    path: template_path,
                    name: "<template>".to_string(),
                    line: 1,
                    col: 0,
                    cyclomatic: 12,
                    cognitive: 14,
                    line_count: 0,
                    param_count: 0,
                    exceeded: crate::health_types::ExceededThreshold::Both,
                    severity: crate::health_types::FindingSeverity::High,
                    crap: Some(45.0),
                    coverage_pct: None,
                    coverage_tier: Some(crate::health_types::CoverageTier::Partial),
                    coverage_source: Some(
                        crate::health_types::CoverageSource::EstimatedComponentInherited,
                    ),
                    inherited_from: Some(owner),
                    component_rollup: None,
                    contributions: Vec::new(),
                }
                .into(),
            ],
            summary: crate::health_types::HealthSummary {
                files_analyzed: 1,
                functions_analyzed: 1,
                functions_above_threshold: 1,
                ..Default::default()
            },
            ..Default::default()
        };
        let lines = build_health_human_lines(&report, &root);
        let text = plain(&lines);
        assert!(
            text.contains(
                "(inherited from apps/admin/src/app/auth/permissions/permissions.component.ts)"
            ),
            "inherited-from suffix must use workspace-relative path: {text}"
        );
        assert!(
            !text.contains("(inherited from permissions.component.ts)"),
            "bare basename suffix must not be rendered: {text}"
        );
    }
}
