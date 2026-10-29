use crate::report::sink::outln;
use std::borrow::Cow;
use std::fmt::Write as _;
use std::path::Path;
use std::time::Duration;

use colored::Colorize;

use super::health_hotspots::{render_change_coupling, render_hotspots};
use super::health_runtime::render_runtime_coverage;
use super::health_targets::render_refactoring_targets;
use super::{
    MAX_FLAT_ITEMS, format_path, plural, print_explain_tip_if_tty, push_docs_footer,
    split_dir_filename, thousands,
};
use crate::health::scoring::{
    FileScoreConcern, file_score_concern_axis, file_score_crap_signal, file_score_fully_crap_exempt,
};
use crate::report::format_display_path;

/// Docs base URL for health explanations.
pub(super) const DOCS_HEALTH: &str = "https://fallow.tools/docs/explanations/health/";

pub(in crate::report) struct PrintHealthHumanInput<'a> {
    pub(in crate::report) report: &'a fallow_output::HealthReport,
    pub(in crate::report) root: &'a Path,
    pub(in crate::report) elapsed: Duration,
    pub(in crate::report) quiet: bool,
    pub(in crate::report) show_explain_tip: bool,
    pub(in crate::report) explain: bool,
    pub(in crate::report) skip_score_and_trend: bool,
    /// Whether `--css` was requested. Drives the empty-CSS explanatory note when
    /// no stylesheet was import-reachable; defaults `false` for non-css callers.
    pub(in crate::report) css_requested: bool,
    pub(in crate::report) type_aware: Option<&'a fallow_types::envelope::TypeAwareMeta>,
    /// Whether a gate of this run fails it. Picks the mark of the final
    /// status line.
    pub(in crate::report) run_fails: bool,
    /// The files that did not parse cleanly. The report names them in the body
    /// next to the score, because their functions and imports can be missing
    /// from it.
    pub(in crate::report) parse_degraded: &'a [fallow_output::GateFile],
}

pub(in crate::report) fn print_health_human(input: &PrintHealthHumanInput<'_>) {
    let report = input.report;
    let root = input.root;
    let elapsed = input.elapsed;
    let quiet = input.quiet;
    let show_explain_tip = input.show_explain_tip;
    let explain = input.explain;
    let skip_score_and_trend = input.skip_score_and_trend;
    let css_requested = input.css_requested;
    let type_coupling = input
        .type_aware
        .and_then(|meta| meta.type_coupling.as_ref());
    if !quiet {
        eprintln!();
    }

    let has_score = report.health_score.is_some();
    let parse_note = parse_degraded_note_lines(input.parse_degraded);
    if report.findings.is_empty()
        && report.file_scores.is_empty()
        && report.coverage_gaps.is_none()
        && report.hotspots.is_empty()
        && report.targets.is_empty()
        && report.runtime_coverage.is_none()
        && report.coverage_intelligence.is_none()
        && report.threshold_overrides.is_empty()
        && report.css_analytics.is_none()
        && !css_requested
        && type_coupling.is_none()
        && !has_score
    {
        for line in parse_note {
            outln!("{line}");
        }
        print_health_empty_state(report, elapsed, quiet);
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

    let mut lines = build_health_human_lines_with_explain(
        report,
        root,
        explain,
        skip_score_and_trend,
        css_requested,
        input.type_aware,
    );
    insert_parse_degraded_note(&mut lines, parse_note, has_score && !skip_score_and_trend);
    for line in lines {
        outln!("{line}");
    }

    if !quiet {
        if type_coupling.is_some()
            && report.findings.is_empty()
            && report.coverage_gaps.is_none()
            && report.runtime_coverage.is_none()
        {
            eprintln!(
                "{}",
                format!(
                    "\u{2713} Type coupling analyzed ({:.2}s)",
                    elapsed.as_secs_f64()
                )
                .green()
                .bold()
            );
        } else {
            print_health_final_status(report, elapsed, has_findings, input.run_fails);
        }
    }
}

fn print_health_empty_state(report: &fallow_output::HealthReport, elapsed: Duration, quiet: bool) {
    if quiet {
        return;
    }

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

/// The final status line of a health run. A run with no finding and no
/// failing gate shows the success mark. Otherwise the mark follows the gate
/// result, see [`super::findings_status_line`].
fn print_health_final_status(
    report: &fallow_output::HealthReport,
    elapsed: Duration,
    has_findings: bool,
    run_fails: bool,
) {
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

/// The body note for files that did not parse cleanly, empty when every file
/// parsed. The score is unchanged: the note says why a function or an import
/// can be missing from it, and the stderr warning alone was easy to miss.
fn parse_degraded_note_lines(files: &[fallow_output::GateFile]) -> Vec<String> {
    if files.is_empty() {
        return Vec::new();
    }
    let count = files.len();
    let noun = if count == 1 { "file" } else { "files" };
    let mut lines = vec![format!(
        "  {} fallow could not fully parse {count} {noun}. Functions and imports in {} can be missing from this report.",
        "Parse errors:".yellow().bold(),
        if count == 1 { "it" } else { "them" },
    )];
    lines.extend(files.iter().map(|file| {
        format!(
            "    {}: {}",
            file.path,
            crate::gates::parse_error_reason(file).dimmed()
        )
    }));
    lines.push(String::new());
    lines
}

/// Put the parse-error note directly under the score block when the report
/// shows a score, else at the top of the body.
fn insert_parse_degraded_note(lines: &mut Vec<String>, note: Vec<String>, after_score: bool) {
    if note.is_empty() {
        return;
    }
    // `render_health_score` ends its block with one blank line.
    let at = if after_score {
        lines
            .iter()
            .position(String::is_empty)
            .map_or(0, |blank| blank + 1)
    } else {
        0
    };
    lines.splice(at..at, note);
}

/// Build human-readable output lines for health (complexity) findings.
///
#[cfg(test)]
fn build_health_human_lines(report: &fallow_output::HealthReport, root: &Path) -> Vec<String> {
    build_health_human_lines_with_explain(report, root, false, false, false, None)
}

fn build_health_human_lines_with_explain(
    report: &fallow_output::HealthReport,
    root: &Path,
    explain: bool,
    skip_score_and_trend: bool,
    css_requested: bool,
    type_aware: Option<&fallow_types::envelope::TypeAwareMeta>,
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
    render_type_coupling(&mut lines, type_aware);
    render_render_fan_in(&mut lines, report);
    render_large_functions(&mut lines, report, root);
    render_findings(&mut lines, report, root);
    render_threshold_overrides(&mut lines, report, root);
    render_coverage_gaps(&mut lines, report, root);
    render_file_scores(&mut lines, report, root);
    render_hotspots(&mut lines, report, root);
    render_change_coupling(&mut lines, report, root);
    render_refactoring_targets(&mut lines, report, root);
    render_css_analytics(&mut lines, report, css_requested);
    if explain {
        inject_explain_blocks(lines)
    } else {
        lines
    }
}

fn render_type_coupling(
    lines: &mut Vec<String>,
    type_aware: Option<&fallow_types::envelope::TypeAwareMeta>,
) {
    let Some(coupling) = type_aware.and_then(|meta| meta.type_coupling.as_ref()) else {
        return;
    };
    lines.push(String::new());
    lines.push("Type coupling".bold().to_string());
    lines.push(format!(
        "  {}",
        crate::report::human_status_line(
            crate::report::semantic_status(coupling.status),
            format_args!("Proof: {} ({:?})", coupling.assertion, coupling.status)
        )
    ));
    if let Some(summary) = &coupling.summary {
        lines.push(format!(
            "  Scope: {}, {}, {} of {} files coupled ({:.1}%), {} edges",
            summary.scope,
            summary.direction,
            summary.distinct_coupled_files,
            summary.project_size,
            summary.coupled_file_pct,
            summary.edge_count
        ));
        lines.push(format!(
            "  Connections: p50 {:.1}, p90 {:.1}; public API depends on: p95 {:.1}, public types used by: p95 {:.1}",
            summary.p50_distinct_connections,
            summary.p90_distinct_connections,
            summary.p95_public_api_depends_on,
            summary.p95_public_types_used_by
        ));
        lines.push(format!(
            "  High coupling: {:.1}%, top concentration: {:.1}%, type cycles: {}",
            summary.high_coupling_pct,
            summary.concentration * 100.0,
            summary.cycle_count
        ));
    }
    for file in coupling.top_contributors.iter().take(10) {
        lines.push(format!(
            "    {}: depends on {}, used by {}",
            file.path.display(),
            file.public_api_depends_on,
            file.public_types_used_by
        ));
    }
    for cycle in coupling.cycles.iter().take(5) {
        lines.push(format!(
            "    Cycle: {}",
            cycle
                .files
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(" -> ")
        ));
    }
    for action in &coupling.actions {
        lines.push(format!("  Next: {action}"));
    }
}

/// Render the opt-in `--css` structural CSS analytics section: a one-line
/// stylesheet summary plus the most structurally notable rules (highest
/// specificity, then complexity, then `!important`).
fn render_css_analytics(
    lines: &mut Vec<String>,
    report: &fallow_output::HealthReport,
    css_requested: bool,
) {
    let Some(ref css) = report.css_analytics else {
        // `--css` was requested but no stylesheet was import-reachable (so neither
        // `css_analytics` nor `styling_health` was produced). Render the header
        // plus a one-line reason instead of silently dropping the section, so the
        // empty result is explained. A plain `fallow health` run (css not
        // requested) renders nothing, leaving non-css output byte-unchanged.
        if css_requested && report.styling_health.is_none() {
            lines.push(String::new());
            lines.push("CSS health".bold().to_string());
            lines.push(format!(
                "  {}",
                "No stylesheets analyzed (none reachable from entry points; install dependencies for full resolution)."
                    .dimmed()
            ));
        }
        return;
    };

    lines.push(String::new());
    lines.push("CSS health".bold().to_string());
    render_styling_health(lines, report);
    render_css_analytics_summary(lines, &css.summary);
    render_css_preprocessor_caveat(lines, &css.summary);
    render_css_keyframe_candidates(lines, css);
    render_css_unused_at_rules(lines, css);
    render_css_scoped_unused(lines, css);
    render_css_duplicate_blocks(lines, css);
    render_css_tailwind_arbitrary(lines, css);
    render_css_unresolved_classes(lines, css);
    render_css_unreferenced_classes(lines, css);
    render_css_unused_font_faces(lines, css);
    render_css_unused_theme_tokens(lines, css);
    render_css_near_duplicate_theme_tokens(lines, css);
    render_css_near_duplicate_css_in_js_tokens(lines, css);
    render_css_font_size_unit_mix(lines, css);
    render_css_notable_rules(lines, css);
}

/// Render the styling-health grade line: the SECOND health axis (the CSS /
/// design-system grade), styled like the code `Health score:` line. Present only
/// alongside `--css`; the code grade is rendered separately and is unaffected.
fn render_styling_health(lines: &mut Vec<String>, report: &fallow_output::HealthReport) {
    let Some(ref styling) = report.styling_health else {
        return;
    };
    lines.push(format!(
        "  {} {} {}",
        "Styling health:".cyan().bold(),
        styling_health_colored(styling),
        "(CSS quality, scored separately from the code health score)".dimmed(),
    ));

    // Low confidence: the grade above is rendered dimmed (no green/yellow/red
    // band) and a caveat carries the reason in TEXT, so the signal survives a
    // non-color terminal or piped output (agents read piped human output too).
    // The caveat sits before `Deductions:` so the reader sees the qualifier
    // before the penalty breakdown they might act on.
    if let Some(reason) = &styling.confidence_reason {
        lines.push(format!("  {} {reason}", "Low confidence:".yellow().bold()));
    }

    let penalties = styling_health_penalties(&styling.penalties);
    if !penalties.is_empty() {
        lines.push(format!(
            "  {} {}",
            "Deductions:".dimmed(),
            render_health_score_penalties(&penalties)
        ));
    }
}

/// Build the styling-health deduction list (non-zero penalties, sorted by
/// magnitude descending) mirroring `health_score_penalties` for the code score.
/// Unlike the code penalties these fields are plain `f64` (never `Option`), so a
/// zero means "evaluated and clean" rather than "not evaluated".
fn styling_health_penalties(p: &fallow_output::StylingHealthPenalties) -> Vec<(&'static str, f64)> {
    let mut penalties = vec![
        ("duplication", p.duplication),
        ("dead surface", p.dead_surface),
        ("broken refs", p.broken_references),
        ("token erosion", p.token_erosion),
        ("structural", p.structural),
    ];
    penalties.retain(|&(_, v)| v > 0.0);
    penalties.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    penalties
}

fn styling_health_colored(styling: &fallow_output::StylingHealth) -> String {
    // A low-confidence grade is dimmed and prefixed with `~` regardless of score
    // band: rendering a sparse-surface 89 in confident green-bold would fight the
    // "Low confidence:" caveat line. The band coloring is reserved for grades the
    // analyzed CSS surface is large enough to back.
    if matches!(
        styling.confidence,
        fallow_output::StylingHealthConfidence::Low
    ) {
        return format!("~{:.0} {}", styling.score, styling.grade)
            .dimmed()
            .to_string();
    }
    let text = format!("{:.0} {}", styling.score, styling.grade);
    if styling.score >= 85.0 {
        text.green().bold().to_string()
    } else if styling.score >= 70.0 {
        text.yellow().bold().to_string()
    } else if styling.score >= 55.0 {
        text.yellow().to_string()
    } else {
        text.red().bold().to_string()
    }
}

fn render_css_analytics_summary(
    lines: &mut Vec<String>,
    summary: &fallow_output::CssAnalyticsSummary,
) {
    let important_pct = if summary.total_declarations > 0 {
        f64::from(summary.important_declarations) / f64::from(summary.total_declarations) * 100.0
    } else {
        0.0
    };
    lines.push(format!(
        "  {} stylesheet{} \u{00b7} {} rule{} \u{00b7} {important_pct:.1}% !important \u{00b7} {} empty \u{00b7} max nesting {}",
        summary.files_analyzed,
        plural(summary.files_analyzed as usize),
        summary.total_rules,
        plural(summary.total_rules as usize),
        summary.empty_rules,
        summary.max_nesting_depth,
    ));
    render_css_value_sprawl(lines, summary);
    if summary.notable_truncated_files > 0 {
        lines.push(
            format!(
                "  (per-rule detail truncated in {} file{}; see --format json)",
                summary.notable_truncated_files,
                plural(summary.notable_truncated_files as usize),
            )
            .dimmed()
            .to_string(),
        );
    }
    // Cleanup candidates. Custom properties stay a count in BOTH directions
    // (high-cardinality and often JS/cross-app consumed, so locating them would
    // be net-noise); the lower-cardinality @keyframes are located.
    if summary.custom_properties_defined > 0 || summary.custom_properties_undefined > 0 {
        let undefined = if summary.custom_properties_undefined > 0 {
            format!(", {} undefined", summary.custom_properties_undefined)
        } else {
            String::new()
        };
        lines.push(format!(
            "  custom properties: {} defined, {} unreferenced in CSS{undefined} (candidates; may be set from JS)",
            summary.custom_properties_defined,
            summary.custom_properties_unreferenced,
        ));
    }
}

fn render_css_scoped_unused(lines: &mut Vec<String>, css: &fallow_output::CssAnalyticsReport) {
    let summary = &css.summary;
    if !css.scoped_unused.is_empty() {
        let class_word = if summary.scoped_unused_classes == 1 {
            "class"
        } else {
            "classes"
        };
        lines.push(format!(
            "  {} unused scoped {class_word} in {} Vue SFC{} (candidates; verify)",
            summary.scoped_unused_classes,
            css.scoped_unused.len(),
            plural(css.scoped_unused.len()),
        ));
        for entry in css.scoped_unused.iter().take(5) {
            lines.push(format!("  {}: {}", entry.path, entry.classes.join(", ")));
        }
        if css.scoped_unused.len() > 5 {
            let more = css.scoped_unused.len() - 5;
            lines.push(
                format!(
                    "  ... and {more} more SFC{} (--format json for full list)",
                    plural(more),
                )
                .dimmed()
                .to_string(),
            );
        }
    }
}

fn sorted_css_notable_rules(
    css: &fallow_output::CssAnalyticsReport,
) -> Vec<(&str, &fallow_types::extract::CssRuleMetric)> {
    let mut notable: Vec<(&str, &fallow_types::extract::CssRuleMetric)> = css
        .files
        .iter()
        .flat_map(|file| {
            file.analytics
                .notable_rules
                .iter()
                .map(move |rule| (file.path.as_str(), rule))
        })
        .collect();
    notable.sort_by(|a, b| {
        let key = |m: &fallow_types::extract::CssRuleMetric| {
            (
                m.specificity_a,
                m.specificity_b,
                m.specificity_c,
                m.complexity,
                m.important_count,
            )
        };
        // Sort by salience descending; tie-break on (path, line) so the human
        // top-5 stays deterministic when two rules share identical metrics.
        key(b.1)
            .cmp(&key(a.1))
            .then_with(|| (a.0, a.1.line).cmp(&(b.0, b.1.line)))
    });
    notable
}

fn render_css_notable_rules(lines: &mut Vec<String>, css: &fallow_output::CssAnalyticsReport) {
    let notable = sorted_css_notable_rules(css);
    let total_notable = notable.len();

    for (path, rule) in notable.iter().take(5) {
        lines.push(format!(
            "  {path}:{}  specificity ({},{},{}) \u{00b7} complexity {} \u{00b7} {} !important \u{00b7} nesting {}",
            rule.line,
            rule.specificity_a,
            rule.specificity_b,
            rule.specificity_c,
            rule.complexity,
            rule.important_count,
            rule.nesting_depth,
        ));
    }
    if total_notable > 5 {
        let more = total_notable - 5;
        lines.push(
            format!("  ... and {more} more (--format json for full list)")
                .dimmed()
                .to_string(),
        );
    }
}

/// Render the two located `@keyframes` candidate lines: defined-but-unused
/// (`unreferenced`) and used-but-defined-nowhere (`undefined`).
fn render_css_keyframe_candidates(
    lines: &mut Vec<String>,
    css: &fallow_output::CssAnalyticsReport,
) {
    let summary = &css.summary;
    if summary.keyframes_defined > 0 {
        if css.unreferenced_keyframes.is_empty() {
            lines.push(format!(
                "  @keyframes: {} defined, 0 unreferenced",
                summary.keyframes_defined,
            ));
        } else {
            let listed = join_located_keyframes(
                css.unreferenced_keyframes
                    .iter()
                    .map(|kf| (kf.name.as_str(), kf.path.as_str())),
                css.unreferenced_keyframes.len(),
            );
            lines.push(format!(
                "  @keyframes: {} defined, {} unreferenced (candidates; verify): {listed}",
                summary.keyframes_defined, summary.keyframes_unreferenced,
            ));
        }
    }
    if !css.undefined_keyframes.is_empty() {
        let listed = join_located_keyframes(
            css.undefined_keyframes
                .iter()
                .map(|kf| (kf.name.as_str(), kf.path.as_str())),
            css.undefined_keyframes.len(),
        );
        lines.push(format!(
            "  undefined @keyframes: {} referenced but defined nowhere (candidates; likely typo or defined in CSS-in-JS): {listed}",
            summary.keyframes_undefined,
        ));
    }
}

/// Render unused CSS at-rule entities: `@property` registrations never read via
/// `var()`, and cascade layers declared but never populated. One line per kind,
/// shown only when present, with up to 5 located names.
fn render_css_unused_at_rules(lines: &mut Vec<String>, css: &fallow_output::CssAnalyticsReport) {
    use fallow_output::UnusedAtRuleKind;
    if css.unused_at_rules.is_empty() {
        return;
    }
    let render_kind = |lines: &mut Vec<String>, kind: UnusedAtRuleKind, label: &str, what: &str| {
        let named: Vec<String> = css
            .unused_at_rules
            .iter()
            .filter(|e| e.kind == kind)
            .take(5)
            .map(|e| format!("{} ({})", e.name, e.path))
            .collect();
        if named.is_empty() {
            return;
        }
        let total = css
            .unused_at_rules
            .iter()
            .filter(|e| e.kind == kind)
            .count();
        let more = if total > 5 {
            format!(", +{} more", total - 5)
        } else {
            String::new()
        };
        lines.push(format!(
            "  {label}: {total} {what} (candidates; verify): {}{more}",
            named.join(", ")
        ));
    };
    render_kind(
        lines,
        UnusedAtRuleKind::PropertyRegistration,
        "unused @property",
        "registered but never used via var()",
    );
    render_kind(
        lines,
        UnusedAtRuleKind::Layer,
        "unused @layer",
        "declared but never populated",
    );
}

/// Render likely class-name typos: static markup class tokens one edit from a
/// defined CSS class, with the suggested class. Up to 5 located entries.
fn render_css_unresolved_classes(lines: &mut Vec<String>, css: &fallow_output::CssAnalyticsReport) {
    if css.unresolved_class_references.is_empty() {
        return;
    }
    let total = css.unresolved_class_references.len();
    lines.push(format!(
        "  {total} likely class typo{} (candidates; verify, may be defined in CSS-in-JS or an external stylesheet):",
        plural(total),
    ));
    for entry in css.unresolved_class_references.iter().take(5) {
        lines.push(format!(
            "  {}:{}: \"{}\" -> did you mean \"{}\"?",
            entry.path, entry.line, entry.class, entry.suggestion,
        ));
    }
    if total > 5 {
        let more = total - 5;
        lines.push(
            format!("  ... and {more} more (--format json for full list)")
                .dimmed()
                .to_string(),
        );
    }
}

/// Render global CSS classes referenced by no in-project markup, with the
/// "we did not scan emails / server templates / CMS" disclosure. Up to 5 located.
fn render_css_unreferenced_classes(
    lines: &mut Vec<String>,
    css: &fallow_output::CssAnalyticsReport,
) {
    if css.unreferenced_css_classes.is_empty() {
        return;
    }
    let total = css.unreferenced_css_classes.len();
    lines.push(format!(
        "  {total} global CSS class{} referenced by no in-project markup (candidates; verify no email / server template / CMS / Markdown applies them):",
        if total == 1 { "" } else { "es" },
    ));
    for entry in css.unreferenced_css_classes.iter().take(5) {
        lines.push(format!("  {}:{}: .{}", entry.path, entry.line, entry.class));
    }
    if total > 5 {
        let more = total - 5;
        lines.push(
            format!("  ... and {more} more (--format json for full list)")
                .dimmed()
                .to_string(),
        );
    }
}

/// Render `@font-face` families declared but never applied (dead web-font
/// payload), with the residual inline-style / JS caveat. Up to 5 located.
fn render_css_unused_font_faces(lines: &mut Vec<String>, css: &fallow_output::CssAnalyticsReport) {
    if css.unused_font_faces.is_empty() {
        return;
    }
    let total = css.unused_font_faces.len();
    lines.push(format!(
        "  {total} unused @font-face{} (declared but applied by no font-family; candidates, may be set from JS/inline):",
        plural(total),
    ));
    for entry in css.unused_font_faces.iter().take(5) {
        lines.push(format!("  {}: {}", entry.path, entry.family));
    }
    if total > 5 {
        let more = total - 5;
        lines.push(
            format!("  ... and {more} more (--format json for full list)")
                .dimmed()
                .to_string(),
        );
    }
}

/// Render Tailwind v4 `@theme` design tokens whose generated utility, `var()`
/// reads, and `@apply` uses appear nowhere (dead design tokens), with the
/// residual plugin / downstream-consumer caveat. Up to 5 located.
fn render_css_unused_theme_tokens(
    lines: &mut Vec<String>,
    css: &fallow_output::CssAnalyticsReport,
) {
    if css.unused_theme_tokens.is_empty() {
        return;
    }
    let total = css.unused_theme_tokens.len();
    lines.push(format!(
        "  {total} Tailwind @theme token{} used by no utility, var(), or @apply (candidates; verify not consumed by a plugin or a downstream repo):",
        plural(total),
    ));
    for entry in css.unused_theme_tokens.iter().take(5) {
        lines.push(format!("  {}:{}: {}", entry.path, entry.line, entry.token));
    }
    if total > 5 {
        let more = total - 5;
        lines.push(
            format!("  ... and {more} more (--format json for full list)")
                .dimmed()
                .to_string(),
        );
    }
}

/// Render Tailwind v4 `@theme` tokens whose comparable values are close to an
/// existing token, naming the nearest reuse target. Up to 5 located entries.
fn render_css_near_duplicate_theme_tokens(
    lines: &mut Vec<String>,
    css: &fallow_output::CssAnalyticsReport,
) {
    if css.near_duplicate_theme_tokens.is_empty() {
        return;
    }
    let total = css.near_duplicate_theme_tokens.len();
    lines.push(format!(
        "  {total} near-duplicate Tailwind @theme token{} (candidates; verify semantic intent before reusing the nearest token):",
        plural(total),
    ));
    for entry in css.near_duplicate_theme_tokens.iter().take(5) {
        lines.push(format!(
            "  {}:{}: {} ~= {} (distance {:.2})",
            entry.path,
            entry.line,
            entry.token,
            entry.nearest_token.name,
            entry.nearest_token.distance
        ));
    }
    if total > 5 {
        let more = total - 5;
        lines.push(
            format!("  ... and {more} more (--format json for full list)")
                .dimmed()
                .to_string(),
        );
    }
}

/// Render CSS-in-JS tokens whose comparable values are close to another project
/// token, naming the nearest reuse target. Up to 5 located entries.
fn render_css_near_duplicate_css_in_js_tokens(
    lines: &mut Vec<String>,
    css: &fallow_output::CssAnalyticsReport,
) {
    if css.near_duplicate_css_in_js_tokens.is_empty() {
        return;
    }
    let total = css.near_duplicate_css_in_js_tokens.len();
    lines.push(format!(
        "  {total} near-duplicate CSS-in-JS token{} (candidates; verify semantic intent before reusing the nearest token):",
        plural(total),
    ));
    for entry in css.near_duplicate_css_in_js_tokens.iter().take(5) {
        lines.push(format!(
            "  {}:{}: {} ~= {} (distance {:.2})",
            entry.path,
            entry.line,
            entry.token,
            entry.nearest_token.name,
            entry.nearest_token.distance
        ));
    }
    if total > 5 {
        let more = total - 5;
        lines.push(
            format!("  ... and {more} more (--format json for full list)")
                .dimmed()
                .to_string(),
        );
    }
}

/// Render the font-size unit-mix candidate: the project authors its type scale
/// in several length units (a px/rem accessibility smell), with the per-unit
/// distinct-value breakdown. Advisory; one line plus the breakdown.
fn render_css_font_size_unit_mix(lines: &mut Vec<String>, css: &fallow_output::CssAnalyticsReport) {
    let Some(mix) = &css.font_size_unit_mix else {
        return;
    };
    let breakdown = mix
        .notations
        .iter()
        .map(|n| format!("{} {}", n.count, n.notation))
        .collect::<Vec<_>>()
        .join(", ");
    lines.push(format!(
        "  font sizes mix {} units ({breakdown}; candidate, standardize unless intentional)",
        mix.notations.len(),
    ));
}

fn render_css_preprocessor_caveat(
    lines: &mut Vec<String>,
    summary: &fallow_output::CssAnalyticsSummary,
) {
    if !summary.preprocessor_reachability_abstained {
        return;
    }
    lines.push(format!(
        "  {}",
        format!(
            "Sass/Less reachability skipped: {} preprocessor stylesheet{} outnumber plain CSS, so generated classes were not treated as dead or broken.",
            summary.preprocessor_stylesheets,
            plural(summary.preprocessor_stylesheets as usize)
        )
        .dimmed()
    ));
}

/// Render the value-sprawl lines: colors / font-sizes / z-indexes always, plus a
/// continuation line for shadow / radius / line-height scales when present.
fn render_css_value_sprawl(lines: &mut Vec<String>, summary: &fallow_output::CssAnalyticsSummary) {
    lines.push(format!(
        "  value sprawl: {} distinct color{} \u{00b7} {} font size{} \u{00b7} {} z-index value{}",
        summary.unique_colors,
        plural(summary.unique_colors as usize),
        summary.unique_font_sizes,
        plural(summary.unique_font_sizes as usize),
        summary.unique_z_indexes,
        plural(summary.unique_z_indexes as usize),
    ));
    let mut extra: Vec<String> = Vec::new();
    if summary.unique_box_shadows > 0 {
        extra.push(format!(
            "{} shadow{}",
            summary.unique_box_shadows,
            plural(summary.unique_box_shadows as usize)
        ));
    }
    if summary.unique_border_radii > 0 {
        extra.push(format!(
            "{} radius value{}",
            summary.unique_border_radii,
            plural(summary.unique_border_radii as usize)
        ));
    }
    if summary.unique_line_heights > 0 {
        extra.push(format!(
            "{} line-height{}",
            summary.unique_line_heights,
            plural(summary.unique_line_heights as usize)
        ));
    }
    if !extra.is_empty() {
        // Shadow / radius / line-height sprawl feeds the styling-health
        // `token_erosion` drift sub-term (formula v3), so this line carries the
        // same tokenization-candidate suffix its siblings do (the `font sizes mix`
        // and `Tailwind arbitrary values` lines), bridging the "value sprawl" ->
        // "token erosion" vocabulary gap. Soft advisory (renders for any count > 0,
        // even below the scoring baseline), never a penalty claim.
        lines.push(format!(
            "  value sprawl (cont.): {} (candidates; tokenize repeated values via custom properties)",
            extra.join(" \u{00b7} ")
        ));
    }
}

/// Render the Tailwind arbitrary-value section: a summary line plus the top 5
/// most-used tokens (token, use count, first location). Present only when the
/// project uses Tailwind and any arbitrary values were found.
fn render_css_tailwind_arbitrary(lines: &mut Vec<String>, css: &fallow_output::CssAnalyticsReport) {
    if css.tailwind_arbitrary_values.is_empty() {
        return;
    }
    let summary = &css.summary;
    lines.push(format!(
        "  Tailwind arbitrary values: {} distinct ({} use{}) bypassing the scale (candidates; add a scale token or confirm one-off)",
        summary.tailwind_arbitrary_values,
        summary.tailwind_arbitrary_value_uses,
        plural(summary.tailwind_arbitrary_value_uses as usize),
    ));
    for arb in css.tailwind_arbitrary_values.iter().take(5) {
        lines.push(format!(
            "    {} ({}x): {}:{}",
            arb.value, arb.count, arb.path, arb.line
        ));
    }
    if css.tailwind_arbitrary_values.len() > 5 {
        let more = css.tailwind_arbitrary_values.len() - 5;
        lines.push(
            format!("  ... and {more} more (--format json for full list)")
                .dimmed()
                .to_string(),
        );
    }
}

/// Render the duplicate-declaration-block candidate section: a summary line plus
/// the top 5 groups (declaration count, occurrence count, located occurrences).
fn render_css_duplicate_blocks(lines: &mut Vec<String>, css: &fallow_output::CssAnalyticsReport) {
    if css.duplicate_declaration_blocks.is_empty() {
        return;
    }
    let summary = &css.summary;
    let group_word = if summary.duplicate_declaration_blocks == 1 {
        "group"
    } else {
        "groups"
    };
    lines.push(format!(
        "  duplicate declaration blocks: {} {group_word}, {} declarations removable (candidates; consolidate or confirm intentional overrides)",
        summary.duplicate_declaration_blocks, summary.duplicate_declarations_total,
    ));
    for block in css.duplicate_declaration_blocks.iter().take(5) {
        let locs = block
            .occurrences
            .iter()
            .take(3)
            .map(|occ| format!("{}:{}", occ.path, occ.line))
            .collect::<Vec<_>>()
            .join(", ");
        let extra = block.occurrences.len().saturating_sub(3);
        let more = if extra > 0 {
            format!(", +{extra} more")
        } else {
            String::new()
        };
        lines.push(format!(
            "    {} declarations in {} rules: {locs}{more}",
            block.declaration_count, block.occurrence_count,
        ));
    }
    if css.duplicate_declaration_blocks.len() > 5 {
        let more = css.duplicate_declaration_blocks.len() - 5;
        lines.push(
            format!("  ... and {more} more (--format json for full list)")
                .dimmed()
                .to_string(),
        );
    }
}

/// Join the first 5 located keyframe names as `name (path)`, with a `, +N more`
/// suffix when `total` exceeds 5.
fn join_located_keyframes<'a>(
    items: impl Iterator<Item = (&'a str, &'a str)>,
    total: usize,
) -> String {
    let named = items
        .take(5)
        .map(|(name, path)| format!("{name} ({path})"))
        .collect::<Vec<_>>()
        .join(", ");
    let extra = total.saturating_sub(5);
    if extra > 0 {
        format!("{named}, +{extra} more")
    } else {
        named
    }
}

fn render_coverage_intelligence(
    lines: &mut Vec<String>,
    report: &fallow_output::HealthReport,
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
        let relative = format_display_path(&finding.path, root);
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
            format_path(&relative),
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
            "The score starts at 100 and subtracts capped penalties for available metrics. N/A means a metric was unavailable and added no deduction; it does not mean the project has no issue in that area. Duplication adds no penalty through 5%, then subtracts one point per percentage point above 5%, up to 10 points."
                .to_string(),
        );
    }
    if line.contains("Cyclomatic units:") {
        return Some(
            "Cyclomatic averages and percentiles include authored functions, module scopes, and templates. JSON population counts and sums reconstruct the mean. Module scopes contribute to metrics without function findings."
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

pub fn render_health_score(lines: &mut Vec<String>, report: &fallow_output::HealthReport) {
    let Some(ref hs) = report.health_score else {
        return;
    };

    lines.push(format!(
        "{} {} {}",
        "\u{25cf}".cyan(),
        "Health score:".cyan().bold(),
        health_score_colored(hs),
    ));

    let p = &hs.penalties;
    let penalties = health_score_penalties(p);
    if !penalties.is_empty() {
        lines.push(format!(
            "  {} {}",
            "Deductions:".dimmed(),
            render_health_score_penalties(&penalties)
        ));
    }
    if let Some(na_line) = health_score_na_line(p) {
        lines.push(format!("  {}", na_line.dimmed()));
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

fn health_score_colored(hs: &fallow_output::HealthScore) -> String {
    let score_str = format!("{:.0}", hs.score);
    let grade_str = hs.grade;
    if hs.score >= 85.0 {
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
    }
}

fn health_score_penalties(p: &fallow_output::HealthScorePenalties) -> Vec<(&'static str, f64)> {
    let mut penalties = Vec::new();
    push_optional_penalty(&mut penalties, "dead files", p.dead_files);
    push_optional_penalty(&mut penalties, "dead exports", p.dead_exports);
    penalties.push(("complexity", p.complexity));
    penalties.push(("p90", p.p90_complexity));
    push_optional_penalty(&mut penalties, "maintainability", p.maintainability);
    push_optional_penalty(&mut penalties, "hotspots", p.hotspots);
    push_optional_penalty(&mut penalties, "unused deps", p.unused_deps);
    push_optional_penalty(&mut penalties, "circular deps", p.circular_deps);
    push_optional_penalty(&mut penalties, "unit size", p.unit_size);
    push_optional_penalty(&mut penalties, "coupling", p.coupling);
    push_optional_penalty(&mut penalties, "duplication", p.duplication);
    penalties.retain(|&(_, v)| v > 0.0);
    penalties.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    penalties
}

fn push_optional_penalty(
    penalties: &mut Vec<(&'static str, f64)>,
    label: &'static str,
    value: Option<f64>,
) {
    if let Some(value) = value {
        penalties.push((label, value));
    }
}

fn render_health_score_penalties(penalties: &[(&str, f64)]) -> String {
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
    parts.join(&format!(" {} ", "\u{00b7}".dimmed()))
}

fn health_score_na_line(p: &fallow_output::HealthScorePenalties) -> Option<String> {
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
    (!na_parts.is_empty()).then(|| {
        format!(
            "N/A: {} (enable the corresponding analysis flags)",
            na_parts.join(", ")
        )
    })
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

pub fn render_health_trend(lines: &mut Vec<String>, report: &fallow_output::HealthReport) {
    let Some(ref trend) = report.health_trend else {
        return;
    };

    use fallow_output::TrendDirection;

    push_trend_header_line(lines, trend);
    push_trend_model_notes(lines, trend, report);

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

    push_trend_metric_rows(lines, trend);
    lines.push(String::new());
}

/// Renders the trend header line with date, SHA, and colored overall direction.
fn push_trend_header_line(lines: &mut Vec<String>, trend: &fallow_output::HealthTrend) {
    use fallow_output::TrendDirection;

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
}

/// Renders score-formula, CRAP-model and snapshot-schema compatibility notes.
fn push_trend_model_notes(
    lines: &mut Vec<String>,
    trend: &fallow_output::HealthTrend,
    report: &fallow_output::HealthReport,
) {
    if let Some(current) = report.health_score.as_ref()
        && trend.compared_to.score.is_some()
        && !trend.metrics.iter().any(|metric| metric.name == "score")
        && let Some(note) = fallow_output::health_score_comparison_note(
            trend.compared_to.score_formula_version,
            Some(current.formula_version),
        )
    {
        lines.push(format!("  {}", note.yellow()));
    }
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
        && prev_version < fallow_output::SNAPSHOT_SCHEMA_VERSION
    {
        lines.push(format!(
            "  {}",
            format!(
                "note: compared snapshot uses schema v{prev_version}; available raw metrics remain comparable"
            )
            .yellow()
        ));
    }
}

/// Renders one row per trend metric with previous/current values and direction.
fn push_trend_metric_rows(lines: &mut Vec<String>, trend: &fallow_output::HealthTrend) {
    use fallow_output::TrendDirection;

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
}

fn render_vital_signs(lines: &mut Vec<String>, report: &fallow_output::HealthReport) {
    let Some(ref vs) = report.vital_signs else {
        return;
    };

    if report.health_trend.is_some() {
        render_cyclomatic_population(lines, vs);
        return;
    }

    let mut parts = Vec::new();
    push_vital_core_parts(&mut parts, vs);
    push_vital_signal_parts(&mut parts, vs, report);
    lines.push(format!(
        "{} {} {}",
        "\u{25a0}".dimmed(),
        "Metrics:".dimmed(),
        parts.join(" \u{00b7} ").dimmed()
    ));
    render_cyclomatic_population(lines, vs);
    lines.push(String::new());
}

fn render_cyclomatic_population(lines: &mut Vec<String>, vs: &fallow_output::VitalSigns) {
    let Some(population) = &vs.cyclomatic_population else {
        return;
    };
    lines.push(format!(
        "  Cyclomatic units: functions {}, module scopes {}, templates {}",
        population.functions.count, population.modules.count, population.templates.count
    ));
    if let Some(max) = population.modules.max {
        lines.push(format!(
            "  Module scope: max cyclomatic {max}; included in metrics, not function findings"
        ));
    }
}

/// Appends the LOC, dead-code, cyclomatic, and maintainability vital-sign parts.
fn push_vital_core_parts(parts: &mut Vec<String>, vs: &fallow_output::VitalSigns) {
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
}

/// Appends the hotspot, circular-dep, unused-dep, and duplication vital-sign parts.
fn push_vital_signal_parts(
    parts: &mut Vec<String>,
    vs: &fallow_output::VitalSigns,
    report: &fallow_output::HealthReport,
) {
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
}

fn render_risk_profiles(lines: &mut Vec<String>, report: &fallow_output::HealthReport) {
    let Some(ref vs) = report.vital_signs else {
        return;
    };

    let format_profile = |profile: &fallow_output::RiskProfile| -> String {
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

/// Render the descriptive render-fan-in (blast-radius) list of the highest
/// fan-in React/Preact components, e.g.
/// `Render fan-in: <Button> 31 parents (146 incl. repeats) ...`.
///
/// The HEADLINE axis is distinct parents (the honest count of distinct render
/// LOCATIONS); render sites is shown as secondary "incl. repeats" context.
/// Mirrors `render_risk_profiles`: descriptive vital-sign context, gated on a
/// non-empty `top_render_fan_in` so non-React output is byte-identical. The
/// list is already sorted by distinct parents descending and project-relativized
/// in the health layer; this only renders a capped, dimmed summary line.
fn render_render_fan_in(lines: &mut Vec<String>, report: &fallow_output::HealthReport) {
    let Some(ref vs) = report.vital_signs else {
        return;
    };
    if vs.top_render_fan_in.is_empty() {
        return;
    }

    const MAX_SHOWN: usize = 5;
    let shown: Vec<String> = vs
        .top_render_fan_in
        .iter()
        .take(MAX_SHOWN)
        .map(|c| {
            format!(
                "<{}> {} parent{} ({} incl. repeats)",
                c.component,
                c.distinct_parents,
                plural(c.distinct_parents as usize),
                c.render_sites,
            )
        })
        .collect();

    lines.push(format!(
        "  {} {}",
        "Render fan-in:".dimmed(),
        shown.join(" \u{00b7} ").dimmed()
    ));
    lines.push(String::new());
}

fn render_large_functions(
    lines: &mut Vec<String>,
    report: &fallow_output::HealthReport,
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
        let file_str = format_display_path(&entry.path, root);
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
    let unit_ceiling = report.summary.max_unit_size_threshold;
    let unit_word = if unit_ceiling == 1 { "line" } else { "lines" };
    push_docs_footer(
        lines,
        &format!("Functions exceeding {unit_ceiling} {unit_word} of code (very high risk)"),
        &format!("{DOCS_HEALTH}#unit-size"),
    );
    if shown < total {
        lines.push(format!(
            "  {}",
            format!("use --top {total} to see all").dimmed()
        ));
    }
    lines.push(String::new());
}

/// Human twin of the `suppress-line` action built for SFC templates. Sources the
/// comment from `fallow_output` so the hint and the JSON action cannot drift. The
/// template unit is anchored at its FIRST contributing construct, not at the top
/// of the file, so the comment has to sit on the line directly above the
/// reported line.
fn sfc_template_suppression_hint() -> String {
    format!(
        "To suppress SFC templates: {} on the line directly above the reported line",
        fallow_output::SFC_TEMPLATE_SUPPRESS_COMMENT
    )
}

/// True when the extension names a single-file-component dialect whose template
/// lives in markup, not in a TypeScript decorator literal.
fn is_sfc_template_extension(ext: &str) -> bool {
    ext.eq_ignore_ascii_case("svelte")
        || ext.eq_ignore_ascii_case("vue")
        || ext.eq_ignore_ascii_case("astro")
}

fn template_finding_extension(finding: &fallow_output::ComplexityViolation) -> Option<&str> {
    if !fallow_types::extract::is_synthetic_template_unit(&finding.name) {
        return None;
    }
    finding.path.extension().and_then(|ext| ext.to_str())
}

/// Append per-finding-kind suppression hints to the findings section footer.
///
/// External `.html` templates take a file-level HTML comment; SFC markup
/// (`.svelte`, `.vue`, `.astro`) takes an HTML comment on the line above the
/// reported anchor line; inline `@Component` templates take a line-level TS
/// comment placed directly above the decorator. `<component>` rollups suppress
/// through the worst class method (the rollup anchors at that method's line).
/// Generic function findings get the catch-all hint above a `>=3` noise
/// threshold. Extracted from `render_findings` to keep that function under the
/// SIG unit-size threshold.
fn append_suppression_hints(lines: &mut Vec<String>, report: &fallow_output::HealthReport) {
    let has_html_template = report.findings.iter().any(|finding| {
        template_finding_extension(finding).is_some_and(|ext| ext.eq_ignore_ascii_case("html"))
    });
    let has_sfc_template = report
        .findings
        .iter()
        .any(|finding| template_finding_extension(finding).is_some_and(is_sfc_template_extension));
    // Exact complement of the `above-angular-decorator` fallthrough in
    // `fallow_output::build_suppress_action`: every template-family unit that
    // is neither `.html` nor SFC markup gets the Angular hint there, so
    // narrowing this to an extension allowlist would leave the human surface
    // silent while JSON still advertises the action. Keep the family guard
    // explicit rather than routing through `template_finding_extension`,
    // whose `None` also covers ordinary function findings.
    let has_inline_template = report.findings.iter().any(|finding| {
        fallow_types::extract::is_synthetic_template_unit(&finding.name)
            && finding
                .path
                .extension()
                .and_then(|ext| ext.to_str())
                .is_none_or(|ext| {
                    !ext.eq_ignore_ascii_case("html") && !is_sfc_template_extension(ext)
                })
    });
    let has_component_rollup = report
        .findings
        .iter()
        .any(|finding| finding.name == "<component>");
    // Snippet units are template-family, so a snippet-only report must not
    // print the generic `//` function suppression hint (rendered text in
    // Svelte markup).
    let has_function_finding = report.findings.iter().any(|finding| {
        !fallow_types::extract::is_synthetic_template_unit(&finding.name)
            && finding.name != "<component>"
            && !finding.exceeded.is_file_lines()
    });
    let has_file_lines = report
        .findings
        .iter()
        .any(|finding| finding.exceeded.is_file_lines());
    if has_html_template {
        lines.push(format!(
            "  {}",
            "To suppress HTML templates: <!-- fallow-ignore-file complexity -->".dimmed()
        ));
    }
    if has_sfc_template {
        lines.push(format!("  {}", sfc_template_suppression_hint().dimmed()));
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
    if has_file_lines {
        lines.push(format!(
            "  {}",
            "To suppress a long file: // fallow-ignore-file complexity (at the top of the file)"
                .dimmed()
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
    finding: &fallow_output::ComplexityViolation,
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

fn render_findings(lines: &mut Vec<String>, report: &fallow_output::HealthReport, root: &Path) {
    if report.findings.is_empty() {
        return;
    }

    push_findings_header(lines, report);
    if let Some(note) = crap_coverage_note(report) {
        lines.push(format!("  {}", note.dimmed()));
    }
    if let Some(note) = mutation_report_note(report) {
        lines.push(format!("  {}", note.dimmed()));
    }
    if let Some(warning) = coverage_join_warning(report) {
        lines.push(format!("  {}", warning.yellow()));
    }

    let mut last_file = String::new();
    for finding in &report.findings {
        push_finding_file_header(lines, finding, root, &mut last_file);
        push_finding_metric_rows(lines, finding, report, root);
    }
    let scope = if has_synthetic_complexity_entries(report) {
        "Functions and synthetic template or component entries"
    } else {
        "Functions"
    };
    let file_lines_note = if report.findings.iter().any(|f| f.exceeded.is_file_lines()) {
        ", and files over the line threshold"
    } else {
        ""
    };
    push_docs_footer(
        lines,
        &format!(
            "{scope} exceeding cyclomatic, cognitive, or CRAP thresholds{file_lines_note}; ! marks the dimension that breached"
        ),
        &format!("{DOCS_HEALTH}#complexity-metrics"),
    );
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

fn push_findings_header(lines: &mut Vec<String>, report: &fallow_output::HealthReport) {
    let subject = if has_synthetic_complexity_entries(report) {
        "High complexity findings"
    } else {
        "High complexity functions"
    };
    let title = if report.findings.len() < report.summary.functions_above_threshold {
        format!(
            "{subject} ({} shown, {} total)",
            report.findings.len(),
            report.summary.functions_above_threshold
        )
    } else {
        format!("{subject} ({})", report.summary.functions_above_threshold)
    };
    lines.push(format!("{} {}", "\u{25cf}".red(), title.red().bold()));
}

fn has_synthetic_complexity_entries(report: &fallow_output::HealthReport) -> bool {
    report.findings.iter().any(|finding| {
        fallow_types::extract::is_synthetic_template_unit(&finding.name)
            || finding.name == "<component>"
            || finding.exceeded.is_file_lines()
    })
}

fn display_complexity_entry_name(name: &str) -> Cow<'_, str> {
    match name {
        "<template>" => Cow::Borrowed("<template> (template complexity)"),
        "<component>" => Cow::Borrowed("<component> (component rollup)"),
        fallow_output::FILE_LINES_FINDING_NAME => Cow::Borrowed("<file> (file length)"),
        name if fallow_types::extract::is_synthetic_template_unit(name) => {
            Cow::Owned(format!("{name} (snippet complexity)"))
        }
        _ => Cow::Borrowed(name),
    }
}

fn push_finding_file_header(
    lines: &mut Vec<String>,
    finding: &fallow_output::ComplexityViolation,
    root: &Path,
    last_file: &mut String,
) {
    let file_str = crate::report::format_display_path(&finding.path, root);
    if file_str != *last_file {
        lines.push(format!("  {}", format_path(&file_str)));
        *last_file = file_str;
    }
}

fn push_finding_metric_rows(
    lines: &mut Vec<String>,
    finding: &fallow_output::ComplexityViolation,
    report: &fallow_output::HealthReport,
    root: &Path,
) {
    if finding.exceeded.is_file_lines() {
        push_file_lines_rows(lines, finding);
        return;
    }
    let thresholds = finding_thresholds(finding, report);
    lines.push(format!(
        "    {} {}{}{}",
        format!(":{}", finding.line).dimmed(),
        display_complexity_entry_name(&finding.name).as_ref().bold(),
        finding_severity_tag(finding),
        finding_generated_tag(finding),
    ));
    lines.push(format!(
        "         {}{} cyclomatic  {}{} cognitive  {} lines",
        threshold_colored(finding.cyclomatic, thresholds.max_cyclomatic),
        breach_marker(finding.exceeded.includes_cyclomatic()),
        threshold_colored(finding.cognitive, thresholds.max_cognitive),
        breach_marker(finding.exceeded.includes_cognitive()),
        format!("{:>3}", finding.line_count).dimmed(),
    ));
    if let Some(line) = render_react_context(finding) {
        lines.push(line);
    }
    if let Some(line) = render_blast_radius_context(finding, report) {
        lines.push(line);
    }
    if let Some(line) = render_component_rollup_breakdown(finding, root) {
        lines.push(line);
    }
    if let Some(line) = finding_crap_line(finding, root) {
        lines.push(line);
    }
    if let Some(line) = finding_mutation_line(finding) {
        lines.push(line);
    }
    if let Some(line) = finding_override_thresholds_line(finding, thresholds) {
        lines.push(line);
    }
}

/// The rows of a file-length finding: the line count against its ceiling.
/// The complexity columns do not apply to a whole file.
fn push_file_lines_rows(lines: &mut Vec<String>, finding: &fallow_output::ComplexityViolation) {
    lines.push(format!(
        "    {} {}",
        format!(":{}", finding.line).dimmed(),
        display_complexity_entry_name(&finding.name).as_ref().bold(),
    ));
    lines.push(format!(
        "         {}{} lines (threshold: {})",
        finding.line_count.to_string().red().bold(),
        breach_marker(true).trim_end(),
        finding.max_file_lines.unwrap_or_default(),
    ));
}

/// Colour-independent marker for the dimension that actually breached.
///
/// Driven off `exceeded`, never a re-comparison, so human output can never
/// disagree with the JSON discriminant. Fixed width so the metric columns stay
/// aligned, and plain ASCII so the signal survives `NO_COLOR`, pipes, and
/// pasted terminal logs (issue #2163).
fn breach_marker(breached: bool) -> &'static str {
    if breached { " !" } else { "  " }
}

/// Name the ceilings a finding was actually measured against when a
/// `thresholdOverrides` entry moved them.
///
/// The report header prints the GLOBAL ceilings, so without this line a user
/// who raised `maxCyclomatic` and `maxCognitive` but left `maxCrap` alone has no
/// way to see which ceiling still applies (issue #2163). Renders only for
/// override-affected findings.
fn finding_override_thresholds_line(
    finding: &fallow_output::ComplexityViolation,
    thresholds: fallow_output::HealthEffectiveThresholds,
) -> Option<String> {
    if !matches!(
        finding.threshold_source,
        Some(fallow_output::ThresholdSource::Override)
    ) {
        return None;
    }
    Some(format!(
        "         {}",
        format!(
            "override: cyclomatic {}, cognitive {}, CRAP {:.1}, unit size {}",
            thresholds.max_cyclomatic,
            thresholds.max_cognitive,
            thresholds.max_crap,
            thresholds.max_unit_size,
        )
        .dimmed()
    ))
}

/// Render the descriptive React-shape context line for a complexity finding:
/// `react: 14 props, 9 hooks (3 state, 4 effect, 2 memo), max effect deps 5,
/// JSX depth 7`. Purely descriptive, never a threshold. Returns `None` when the
/// finding has no React
/// signals, so non-React findings render unchanged. The per-kind breakdown and
/// max effect deps come from the cached `hook_uses` IR (component-scope hooks
/// only) and are shown only when a profile was attributed; `react_hook_count`
/// stays the headline so the documented breakdown-vs-total divergence is a
/// non-issue.
fn render_react_context(finding: &fallow_output::ComplexityViolation) -> Option<String> {
    if finding.react_prop_count == 0
        && finding.react_hook_count == 0
        && finding.react_jsx_max_depth == 0
    {
        return None;
    }
    let mut parts: Vec<String> = Vec::new();
    if finding.react_prop_count > 0 {
        parts.push(format!(
            "{} prop{}",
            finding.react_prop_count,
            plural(usize::from(finding.react_prop_count))
        ));
    }
    if finding.react_hook_count > 0 {
        let breakdown = finding
            .react_hook_profile
            .as_ref()
            .map(hook_breakdown_fragment)
            .filter(|b| !b.is_empty());
        let hooks = format!(
            "{} hook{}",
            finding.react_hook_count,
            plural(usize::from(finding.react_hook_count))
        );
        match breakdown {
            Some(breakdown) => parts.push(format!("{hooks} ({breakdown})")),
            None => parts.push(hooks),
        }
    }
    if let Some(arity) = finding
        .react_hook_profile
        .as_ref()
        .and_then(|p| p.max_effect_dep_arity)
    {
        parts.push(format!("max effect deps {arity}"));
    }
    if finding.react_jsx_max_depth > 0 {
        parts.push(format!("JSX depth {}", finding.react_jsx_max_depth));
    }
    Some(format!(
        "         {}",
        format!("react: {}", parts.join(", ")).dimmed()
    ))
}

/// The minimum render-SITE count for the descriptive blast-radius line. A
/// component rendered in `<= 1` place is not a blast-radius amplifier (editing
/// it ripples to at most one site), so the line only surfaces for components
/// rendered in MULTIPLE places. Descriptive floor only, not a gate or threshold.
const BLAST_RADIUS_MIN_SITES: u32 = 2;

/// Render the descriptive blast-radius line for a complexity finding whose file
/// declares a high-render-fan-in component: `rendered in N places`. This is the
/// component-graph analogue of fan-in surfaced as context (a hotspot file
/// containing a shared component is a higher blast-radius hotspot: editing it
/// ripples to every render site). Purely descriptive (dimmed, sibling to the
/// `react: ...` line), never a threshold or finding. Returns `None` when the
/// file has no inspectable high-fan-in component (so non-React findings render
/// unchanged) or the metric was not computed (non-React run / score-only).
fn render_blast_radius_context(
    finding: &fallow_output::ComplexityViolation,
    report: &fallow_output::HealthReport,
) -> Option<String> {
    let (component, render_sites) = report.render_fan_in_top.get(&finding.path)?;
    if *render_sites < BLAST_RADIUS_MIN_SITES {
        return None;
    }
    Some(format!(
        "         {}",
        format!("blast radius: <{component}> rendered in {render_sites} places").dimmed()
    ))
}

/// Build the per-kind hook breakdown parenthetical (`3 state, 4 effect, 2
/// memo`), gating each segment on `> 0` so empty kinds never produce dangling
/// fragments. Returns an empty string when no kind was attributed.
fn hook_breakdown_fragment(profile: &fallow_output::ReactHookProfile) -> String {
    let mut segments: Vec<String> = Vec::new();
    if profile.state > 0 {
        segments.push(format!("{} state", profile.state));
    }
    if profile.effect > 0 {
        segments.push(format!("{} effect", profile.effect));
    }
    if profile.memo > 0 {
        segments.push(format!("{} memo", profile.memo));
    }
    if profile.callback > 0 {
        segments.push(format!("{} callback", profile.callback));
    }
    if profile.custom > 0 {
        segments.push(format!("{} custom", profile.custom));
    }
    segments.join(", ")
}

fn finding_thresholds(
    finding: &fallow_output::ComplexityViolation,
    report: &fallow_output::HealthReport,
) -> fallow_output::HealthEffectiveThresholds {
    finding.resolved_thresholds(&report.summary)
}

fn threshold_colored(value: u16, threshold: u16) -> String {
    let formatted = format!("{value:>3}");
    if value > threshold {
        formatted.red().bold().to_string()
    } else {
        formatted.dimmed().to_string()
    }
}

fn finding_severity_tag(finding: &fallow_output::ComplexityViolation) -> String {
    match finding.severity {
        fallow_output::FindingSeverity::Critical => format!(" {}", "CRITICAL".red().bold()),
        fallow_output::FindingSeverity::High => format!(" {}", "HIGH".yellow().bold()),
        fallow_output::FindingSeverity::Moderate => String::new(),
    }
}

fn finding_generated_tag(finding: &fallow_output::ComplexityViolation) -> String {
    if is_likely_generated(&finding.name, finding.cyclomatic) {
        format!(" {}", "(generated)".dimmed())
    } else {
        String::new()
    }
}

/// Render the CRAP row.
///
/// The CRAP value is highlighted only when CRAP is one of the dimensions that
/// produced the finding. It used to render red unconditionally, which made a
/// benign CRAP look identical to a breaching one and left the colour channel
/// carrying no information (issue #2163).
fn finding_crap_line(finding: &fallow_output::ComplexityViolation, root: &Path) -> Option<String> {
    let crap = finding.crap?;
    let breached = finding.exceeded.includes_crap();
    let formatted = format!("{crap:>5.1}");
    let crap_colored = if breached {
        formatted.red().bold().to_string()
    } else {
        formatted.dimmed().to_string()
    };
    let coverage_suffix = if let Some(pct) = finding.coverage_pct {
        format!("  ({pct:.0}% tested)")
    } else if matches!(
        finding.coverage_source,
        Some(fallow_output::CoverageSource::EstimatedComponentInherited)
    ) && let Some(ref owner) = finding.inherited_from
    {
        let owner_display = crate::report::format_display_path(owner, root);
        format!("  (inherited from {owner_display})")
    } else {
        String::new()
    };
    Some(format!(
        "         {crap_colored}{} CRAP{}",
        breach_marker(breached),
        coverage_suffix.dimmed(),
    ))
}

/// The mutation score from `--mutation-report`, under the CRAP line. A stale
/// file prints no score, so a changed function never shows an old value.
fn finding_mutation_line(finding: &fallow_output::ComplexityViolation) -> Option<String> {
    let mutation = finding.mutation.as_ref()?;
    let text = match (mutation.state, mutation.score, mutation.counts) {
        (fallow_output::HealthMutationState::Stale, _, _) => {
            let reason = match mutation.stale_reason {
                Some(fallow_output::HealthMutationStaleReason::FileMissing) => "file missing",
                Some(fallow_output::HealthMutationStaleReason::NoDigest) => "no digest",
                Some(fallow_output::HealthMutationStaleReason::SourceChanged) | None => {
                    "file changed since the run"
                }
            };
            format!("mutation score: stale ({reason})")
        }
        (_, Some(score), Some(counts)) => format!(
            "{:.0}% mutation score ({} killed, {} survived{})",
            score * 100.0,
            counts.killed + counts.timeout,
            counts.survived,
            if counts.no_coverage > 0 {
                format!(", {} no coverage", counts.no_coverage)
            } else {
                String::new()
            },
        ),
        (_, None, Some(counts)) if counts.no_coverage > 0 => {
            format!("mutation score: none ({} no coverage)", counts.no_coverage)
        }
        _ => "mutation score: none (no mutant ran)".to_string(),
    };
    Some(format!("         {}", text.dimmed()))
}

/// One line for the join of `--mutation-report`: matched findings and stale
/// files. The mutation score never changes CRAP or the health score.
fn mutation_report_note(report: &fallow_output::HealthReport) -> Option<String> {
    let summary = report.mutation.as_ref()?;
    let mut note = format!(
        "Mutation report: {} of {} findings matched",
        summary.findings_fresh + summary.findings_stale,
        report.findings.len(),
    );
    if !summary.stale_files.is_empty() {
        let _ = write!(
            note,
            "; {} {} no score",
            summary.stale_files.len(),
            if summary.stale_files.len() == 1 {
                "stale file has"
            } else {
                "stale files have"
            },
        );
    }
    if summary.completion == Some(fallow_output::MutationCompletionStatus::Partial) {
        note.push_str("; the run was partial");
    }
    note.push_str(". The score does not change CRAP or the health score.");
    Some(note)
}

fn render_threshold_overrides(
    lines: &mut Vec<String>,
    report: &fallow_output::HealthReport,
    root: &Path,
) {
    if report.threshold_overrides.is_empty() {
        return;
    }

    // Count configured overrides, not state rows: one override contributes one
    // row per dimension it participates in, and interpolating the row count read
    // as "you configured N overrides" (issue #2163).
    let configured_count = {
        let mut indexes: Vec<usize> = report
            .threshold_overrides
            .iter()
            .map(|entry| entry.override_index)
            .collect();
        indexes.sort_unstable();
        indexes.dedup();
        indexes.len()
    };
    lines.push(format!(
        "{} {}",
        "\u{25cf}".yellow(),
        format!("Health threshold overrides ({configured_count})")
            .yellow()
            .bold()
    ));
    // Rows are prioritized by actionability before the cap: `no_match` and
    // `insufficient` need a config or code change, `active` is confirmation,
    // and `stale` is pure noise at volume. A wide glob produces one row per
    // matched function, so an uncapped section drowned the actionable rows
    // (issue #2163 follow-up). The stable sort preserves the engine's
    // deterministic order within each status. JSON stays uncapped.
    let mut display: Vec<&fallow_output::ThresholdOverrideState> =
        report.threshold_overrides.iter().collect();
    display.sort_by_key(|entry| threshold_override_status_display_rank(entry.status));
    for entry in display.iter().take(MAX_FLAT_ITEMS) {
        let status = threshold_override_status_label(entry.status);
        let dimension = threshold_override_dimension_label(entry.dimension);
        let outstanding = threshold_override_outstanding_suffix(&entry.outstanding);
        let target = entry.path.as_ref().map_or_else(
            || "<no matching file or function>".to_string(),
            |path| entry.target_label(&crate::report::format_display_path(path, root)),
        );
        let metrics = entry.metrics.map_or(String::new(), |metrics| {
            let crap = metrics
                .crap
                .map_or(String::new(), |value| format!(" crap={value:.1}"));
            let line_count = metrics
                .line_count
                .map_or(String::new(), |value| format!(" lines={value}"));
            format!(
                " cyclomatic={} cognitive={}{}{}",
                metrics.cyclomatic, metrics.cognitive, line_count, crap
            )
        });
        // A crap-dimension row on a template-family unit is not a success or a
        // failure: the unit is not scored on CRAP at all, so the entry buys
        // nothing and can be deleted (issue #2235).
        let note = if threshold_override_crap_not_applicable(entry) {
            " (template units are not scored on CRAP; this entry can be removed)"
        } else {
            ""
        };
        lines.push(format!(
            "    #{idx} {dimension} {status} {target}{metrics}{outstanding}{note}",
            idx = entry.override_index
        ));
    }
    push_threshold_overrides_overflow(lines, &display);
    lines.push(String::new());
}

/// True for a crap-dimension override row recorded against a synthetic
/// template-family unit. Recognized by shape: the row carries measured
/// complexity metrics but no CRAP value, because the unit is excluded from
/// the CRAP dimension and the score was never computed.
fn threshold_override_crap_not_applicable(entry: &fallow_output::ThresholdOverrideState) -> bool {
    matches!(
        entry.dimension,
        fallow_output::ThresholdOverrideDimension::Crap
    ) && entry.metrics.is_some_and(|metrics| metrics.crap.is_none())
        && entry
            .function
            .as_deref()
            .is_some_and(fallow_types::extract::is_synthetic_template_unit)
}

fn threshold_override_status_label(status: fallow_output::ThresholdOverrideStatus) -> &'static str {
    match status {
        fallow_output::ThresholdOverrideStatus::Active => "active",
        fallow_output::ThresholdOverrideStatus::Stale => "stale",
        fallow_output::ThresholdOverrideStatus::Insufficient => "insufficient",
        fallow_output::ThresholdOverrideStatus::NoMatch => "no_match",
    }
}

fn threshold_override_status_display_rank(status: fallow_output::ThresholdOverrideStatus) -> u8 {
    match status {
        fallow_output::ThresholdOverrideStatus::NoMatch => 0,
        fallow_output::ThresholdOverrideStatus::Insufficient => 1,
        fallow_output::ThresholdOverrideStatus::Active => 2,
        fallow_output::ThresholdOverrideStatus::Stale => 3,
    }
}

fn push_threshold_overrides_overflow(
    lines: &mut Vec<String>,
    display: &[&fallow_output::ThresholdOverrideState],
) {
    if display.len() <= MAX_FLAT_ITEMS {
        return;
    }
    let hidden = &display[MAX_FLAT_ITEMS..];
    let mut counts: Vec<(&'static str, usize)> = Vec::new();
    for entry in hidden {
        let label = threshold_override_status_label(entry.status);
        match counts.iter_mut().find(|(name, _)| *name == label) {
            Some((_, count)) => *count += 1,
            None => counts.push((label, 1)),
        }
    }
    let breakdown = counts
        .iter()
        .map(|(name, count)| format!("{count} {name}"))
        .collect::<Vec<_>>()
        .join(", ");
    lines.push(format!(
        "    {}",
        format!(
            "... and {} more rows ({breakdown}) (--format json for full list)",
            hidden.len()
        )
        .dimmed()
    ));
}

fn threshold_override_dimension_label(
    dimension: fallow_output::ThresholdOverrideDimension,
) -> &'static str {
    match dimension {
        fallow_output::ThresholdOverrideDimension::Complexity => "complexity",
        fallow_output::ThresholdOverrideDimension::Crap => "crap",
    }
}

fn threshold_override_outstanding_suffix(
    outstanding: &[fallow_output::ThresholdOverrideDimension],
) -> String {
    if outstanding.is_empty() {
        return String::new();
    }
    let names: Vec<&str> = outstanding
        .iter()
        .map(|dimension| match dimension {
            fallow_output::ThresholdOverrideDimension::Complexity => "complexity",
            fallow_output::ThresholdOverrideDimension::Crap => "CRAP",
        })
        .collect();
    // "still breaches", not "finding still fires": the complexity dimension
    // covers `maxUnitSize`, which keeps a unit in the large-function list
    // without ever emitting a finding of its own.
    format!(" (still breaches: {})", names.join(", "))
}

/// Warn when the coverage file describes files that no analyzed file matched.
///
/// Without this, a coverage file written for a different root, a container
/// path prefix, or an older checkout reads exactly like code that has no
/// tests: every function falls back to its estimate and nothing says why.
fn coverage_join_warning(report: &fallow_output::HealthReport) -> Option<String> {
    let matched = report.summary.istanbul_files_matched?;
    let total = report.summary.istanbul_files_total?;
    if total == 0 || matched >= total {
        return None;
    }
    let file_word = if total == 1 { "file" } else { "files" };
    if matched == 0 {
        return Some(format!(
            "note: the coverage file describes {total} {file_word} that no analyzed file matched; check that it was generated for this project, or set --coverage-root."
        ));
    }
    Some(format!(
        "note: {matched} of {total} {file_word} in the coverage file matched an analyzed file; the rest scored from their estimate."
    ))
}

fn crap_coverage_note(report: &fallow_output::HealthReport) -> Option<String> {
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
        Some(fallow_output::CoverageModel::Istanbul)
    ) || has_istanbul_counts
    {
        let match_info = match (
            report.summary.istanbul_matched,
            report.summary.istanbul_total,
        ) {
            (Some(matched), Some(total)) if total > 0 && matched < total => {
                return Some(format!(
                    "CRAP scores use {} coverage where matched ({matched}/{total} functions); the rest fall back to the static estimate.",
                    measured_coverage_label(report)
                ));
            }
            (Some(matched), Some(total)) if total > 0 => {
                format!(" ({matched}/{total} functions matched)")
            }
            _ => String::new(),
        };
        return Some(format!(
            "CRAP scores use {} coverage data{match_info}.",
            measured_coverage_label(report)
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

fn render_file_scores(lines: &mut Vec<String>, report: &fallow_output::HealthReport, root: &Path) {
    if report.file_scores.is_empty() {
        return;
    }

    push_file_scores_header(lines, report.file_scores.len());

    let max_crap_threshold = report.summary.max_crap_threshold;
    let shown_scores = report.file_scores.len().min(MAX_FLAT_ITEMS);
    for score in &report.file_scores[..shown_scores] {
        render_file_score_row(lines, score, root, max_crap_threshold);
    }
    push_file_scores_overflow(lines, report.file_scores.len());
    let crap_note = file_scores_crap_note(report);
    let risk_bands = file_scores_risk_bands_note(max_crap_threshold);
    push_docs_footer(
        lines,
        &format!(
            "Sorted by triage concern: the larger of low-MI concern and CRAP risk. The risk / structure tag marks which one placed each file. MI reflects complexity, coupling, and dead code; risk reflects untested complexity (CRAP) and can diverge from MI. {risk_bands} {crap_note}"
        ),
        &format!("{DOCS_HEALTH}#file-health-scores"),
    );
    lines.push(String::new());
}

/// Risk band segment of the file-scores footnote, stated relative to the run's
/// configured CRAP ceiling so a raised `maxCrap` / `--max-crap` does not leave
/// the note claiming the default 15/30 bands.
fn file_scores_risk_bands_note(max_crap_threshold: f64) -> String {
    if max_crap_threshold <= 0.0 {
        return "CRAP enforcement is disabled (maxCrap 0): no function counts above threshold; exempt counts use the canonical 30 baseline.".to_string();
    }
    let high = format_risk_band_bound(max_crap_threshold);
    let low = format_risk_band_bound(max_crap_threshold / 2.0);
    format!("Risk: low <{low}, moderate {low}-{high}, high >={high}.")
}

/// Render a risk band bound without a trailing `.0`, so the default note keeps
/// reading `low <15, moderate 15-30, high >=30`.
fn format_risk_band_bound(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{value:.0}")
    } else {
        format!("{value:.1}")
    }
}

fn push_file_scores_header(lines: &mut Vec<String>, score_count: usize) {
    lines.push(format!(
        "{} {} {}",
        "\u{25cf}".cyan(),
        format!(
            "File health scores ({score_count} file{})",
            plural(score_count)
        )
        .cyan()
        .bold(),
        "\u{b7} sorted by triage concern".dimmed(),
    ));
    lines.push(String::new());
}

fn render_file_score_row(
    lines: &mut Vec<String>,
    score: &fallow_output::FileHealthScore,
    root: &Path,
    max_crap_threshold: f64,
) {
    let file_str = format_display_path(&score.path, root);
    let (dir, filename) = split_dir_filename(&file_str);
    const CONCERN_TAG_COLUMN: usize = 48;
    let pad = CONCERN_TAG_COLUMN
        .saturating_sub(file_str.chars().count())
        .max(2);
    lines.push(format!(
        "  {}    {}{}{}{}",
        maintainability_colored(score.maintainability_index),
        dir.dimmed(),
        filename,
        " ".repeat(pad),
        file_score_concern_colored(score, max_crap_threshold),
    ));
    lines.push(format!(
        "         {} LOC  {} fan-in  {} fan-out  {} dead  {} density{}{}",
        format!("{:>6}", score.lines).dimmed(),
        format!("{:>3}", score.fan_in).dimmed(),
        format!("{:>3}", score.fan_out).dimmed(),
        format!("{:>3.0}%", score.dead_code_ratio * 100.0).dimmed(),
        format!("{:.2}", score.complexity_density).dimmed(),
        file_score_risk_suffix(score, max_crap_threshold),
        file_score_exempt_marker(score, max_crap_threshold),
    ));
    lines.push(String::new());
}

fn maintainability_colored(mi: f64) -> String {
    let mi_str = format!("{mi:>5.1}");
    if mi >= 80.0 {
        mi_str.green().to_string()
    } else if mi >= 50.0 {
        mi_str.yellow().to_string()
    } else {
        mi_str.red().bold().to_string()
    }
}

fn file_score_concern_colored(
    score: &fallow_output::FileHealthScore,
    max_crap_threshold: f64,
) -> String {
    let label = file_score_concern_axis(score, max_crap_threshold).label();
    match file_score_concern_axis(score, max_crap_threshold) {
        FileScoreConcern::Risk => {
            let signal = file_score_crap_signal(score, max_crap_threshold);
            if signal.maximum >= signal.threshold {
                label.red().bold().to_string()
            } else if signal.maximum >= signal.threshold / 2.0 {
                label.yellow().to_string()
            } else {
                label.dimmed().to_string()
            }
        }
        FileScoreConcern::Structural => {
            if score.maintainability_index < 50.0 {
                label.red().bold().to_string()
            } else if score.maintainability_index < 80.0 {
                label.yellow().to_string()
            } else {
                label.dimmed().to_string()
            }
        }
    }
}

fn file_score_risk_suffix(
    score: &fallow_output::FileHealthScore,
    max_crap_threshold: f64,
) -> String {
    let signal = file_score_crap_signal(score, max_crap_threshold);
    if signal.maximum <= 0.0 {
        return String::new();
    }
    let risk_str = if signal.maximum > 999.0 {
        ">999".to_string()
    } else {
        format!("{:.1}", signal.maximum)
    };
    // A fully exempt row renders dimmed regardless of magnitude: the run has
    // zero CRAP findings for it, so a red-bold number would contradict the
    // findings story the configuration produced (issue #2228).
    let risk_colored = if file_score_fully_crap_exempt(score, max_crap_threshold) {
        risk_str.dimmed().to_string()
    } else if signal.maximum >= signal.threshold {
        risk_str.red().bold().to_string()
    } else if signal.maximum >= signal.threshold / 2.0 {
        risk_str.yellow().to_string()
    } else {
        risk_str.dimmed().to_string()
    };
    format!("  {risk_colored} risk")
}

/// Disclose suppression and configured exemptions separately because a
/// suppressed function can also be covered by a configured exemption.
fn file_score_exempt_marker(
    score: &fallow_output::FileHealthScore,
    max_crap_threshold: f64,
) -> String {
    if score.crap_exempted == 0 && score.crap_suppressed == 0 {
        return String::new();
    }
    let mut markers = Vec::new();
    if score.crap_suppressed > 0 {
        markers.push("complexity suppression".to_string());
    }
    if score.crap_exempted > 0 {
        if score.crap_suppressed > 0 {
            // The minimum ceiling does not identify which configured exemption
            // overlaps a suppression, so disclose both without guessing a cause.
            markers.push("configured exemptions".to_string());
        } else {
            let reason = if score.crap_effective_threshold.is_some() {
                "override"
            } else if max_crap_threshold <= 0.0 {
                "crap disabled"
            } else {
                "raised threshold"
            };
            markers.push(format!("exempt ({reason})"));
        }
    }
    format!("  {}", markers.join(", ").dimmed())
}

fn push_file_scores_overflow(lines: &mut Vec<String>, score_count: usize) {
    if score_count <= MAX_FLAT_ITEMS {
        return;
    }
    lines.push(format!(
        "  {}",
        format!(
            "... and {} more files (--format json for full list)",
            score_count - MAX_FLAT_ITEMS
        )
        .dimmed()
    ));
    lines.push(String::new());
}

/// Name of the coverage input behind measured CRAP scores.
fn measured_coverage_label(report: &fallow_output::HealthReport) -> &'static str {
    match report.summary.coverage_input_format {
        Some(fallow_output::CoverageInputFormat::V8) => "V8",
        _ => "Istanbul",
    }
}

fn file_scores_crap_note(report: &fallow_output::HealthReport) -> String {
    if matches!(
        report.summary.coverage_model,
        Some(fallow_output::CoverageModel::Istanbul)
    ) {
        let match_info = match (
            report.summary.istanbul_matched,
            report.summary.istanbul_total,
        ) {
            (Some(m), Some(t)) if t > 0 => format!(" ({m}/{t} functions matched)"),
            _ => String::new(),
        };
        format!(
            "CRAP from {} coverage data{match_info}.",
            measured_coverage_label(report)
        )
    } else {
        "CRAP estimated from export references (85% direct, 40% indirect, 0% untested). Run `fallow health --coverage <coverage-final.json>` for exact scores.".to_string()
    }
}

fn render_coverage_gaps(
    lines: &mut Vec<String>,
    report: &fallow_output::HealthReport,
    root: &Path,
) {
    let Some(ref gaps) = report.coverage_gaps else {
        return;
    };

    push_coverage_gaps_header(lines, gaps);
    push_coverage_gap_files(lines, gaps, root);
    push_coverage_gap_exports(lines, gaps, root);
    push_docs_footer(
        lines,
        "Static test dependency gaps (not line-level coverage)",
        &format!("{DOCS_HEALTH}#coverage-gaps"),
    );
    lines.push(String::new());
}

fn push_coverage_gaps_header(lines: &mut Vec<String>, gaps: &fallow_output::CoverageGaps) {
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
}

fn push_coverage_gap_files(
    lines: &mut Vec<String>,
    gaps: &fallow_output::CoverageGaps,
    root: &Path,
) {
    if gaps.files.is_empty() {
        return;
    }
    let shown_files = gaps.files.len().min(MAX_FLAT_ITEMS);
    lines.push(format!("  {}", "Files".dimmed()));
    for item in &gaps.files[..shown_files] {
        let file_str = format_display_path(&item.file.path, root);
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

fn push_coverage_gap_exports(
    lines: &mut Vec<String>,
    gaps: &fallow_output::CoverageGaps,
    root: &Path,
) {
    if gaps.exports.is_empty() {
        return;
    }
    lines.push(format!("  {}", "Exports".dimmed()));

    let by_file = group_coverage_gap_exports_by_file(&gaps.exports);
    let shown = push_coverage_gap_export_rows(lines, &by_file, root);
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

fn group_coverage_gap_exports_by_file(
    exports: &[fallow_output::UntestedExportFinding],
) -> Vec<(&std::path::Path, Vec<&fallow_output::UntestedExportFinding>)> {
    let mut by_file: Vec<(&std::path::Path, Vec<&fallow_output::UntestedExportFinding>)> =
        Vec::new();
    for item in exports {
        match by_file.last_mut() {
            Some((path, items)) if *path == item.export.path.as_path() => items.push(item),
            _ => by_file.push((item.export.path.as_path(), vec![item])),
        }
    }
    by_file
}

fn push_coverage_gap_export_rows(
    lines: &mut Vec<String>,
    by_file: &[(&std::path::Path, Vec<&fallow_output::UntestedExportFinding>)],
    root: &Path,
) -> usize {
    let mut shown = 0;
    for (file_path, exports) in by_file {
        if shown >= MAX_FLAT_ITEMS {
            break;
        }
        let file_str = format_display_path(file_path, root);
        if exports.len() > 10 {
            lines.push(format!(
                "  {} ({} untested re-exports)",
                file_str.dimmed(),
                exports.len(),
            ));
            shown += 1;
        } else {
            shown += push_coverage_gap_export_names(lines, exports, &file_str, shown);
        }
    }
    shown
}

fn push_coverage_gap_export_names(
    lines: &mut Vec<String>,
    exports: &[&fallow_output::UntestedExportFinding],
    file_str: &str,
    mut shown: usize,
) -> usize {
    let before = shown;
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
    shown - before
}

/// Print a concise health summary showing only aggregate statistics.
pub(in crate::report) fn print_health_summary(
    report: &fallow_output::HealthReport,
    elapsed: Duration,
    quiet: bool,
    heading: bool,
) {
    let s = &report.summary;

    if heading {
        outln!("{}", "Health Summary".bold());
        outln!();
    }
    print_health_summary_metrics(report);
    print_health_summary_coverage(report);

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

/// Prints the function-count, maintainability, and health-score summary rows.
fn print_health_summary_metrics(report: &fallow_output::HealthReport) {
    let s = &report.summary;
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
}

/// Prints the coverage-gap and runtime-coverage summary rows.
fn print_health_summary_coverage(report: &fallow_output::HealthReport) {
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
}

/// Render a per-group summary block beneath the project-level human report.
///
/// Layout: a header row (`key  score  grade  trend  files  crit  hot  p90`)
/// followed by one row per group. The `score`/`grade` columns are omitted
/// entirely when no group carries a health score (no `--score` requested).
/// The `trend` column shows the score delta against the baseline snapshot,
/// and is omitted when no group carries a trend status. The `p90` column is
/// omitted entirely when no group carries vital signs (`--score-only` was
/// active). `crit` counts critical findings and `hot` counts ranked hotspot
/// entries, both before `--top`.
///
/// When scores are present, groups are sorted ascending by score (worst
/// first) so the rows match the user's "where do I refactor first?"
/// question, with the unowned group last. Otherwise the resolver's own
/// ordering (descending by file count, unowned last) is preserved.
///
/// Grade is colored to match the project-level grade: A/B green, C yellow,
/// D/F red.
///
/// Goes to stdout (the rows are content, not progress) so the block survives
/// `fallow health --group-by package > out.txt`. The leading blank line,
/// the `(root)` legend, and the JSON-parity hint go to stderr because they
/// are display affordances, not data.
pub(in crate::report) fn print_health_grouping(
    grouping: &fallow_output::HealthGrouping,
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
    let columns = GroupingColumns {
        key_width,
        score: grouping.groups.iter().any(|g| g.health_score.is_some()),
        trend: grouping.groups.iter().any(|g| g.trend_status.is_some()),
        vitals: grouping.groups.iter().any(|g| g.vital_signs.is_some()),
    };

    outln!("{}", grouping_header(&columns).dimmed());

    let mut has_root_bucket = false;
    for group in fallow_output::health_groups_in_display_order(&grouping.groups) {
        if group.key == "(root)" {
            has_root_bucket = true;
        }
        outln!("{}", grouping_row(group, &columns));
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

/// Which optional columns the per-group block shows.
struct GroupingColumns {
    key_width: usize,
    score: bool,
    trend: bool,
    vitals: bool,
}

/// Width of the `trend` column: a signed delta, a space and an arrow.
const GROUP_TREND_WIDTH: usize = 7;

/// Builds the per-group header row, omitting score/grade, trend and p90
/// columns when no group carries that data.
fn grouping_header(columns: &GroupingColumns) -> String {
    let mut header = format!("  {:<width$}", "", width = columns.key_width);
    if columns.score {
        let _ = write!(header, "  {:>9}  grade", "score");
    }
    if columns.trend {
        let _ = write!(header, "  {:>width$}", "trend", width = GROUP_TREND_WIDTH);
    }
    let _ = write!(header, "  {:>5}", "files");
    let _ = write!(header, "  {:>4}", "crit");
    let _ = write!(header, "  {:>3}", "hot");
    if columns.vitals {
        let _ = write!(header, "  {:>3}", "p90");
    }
    header
}

/// Builds one per-group data row, matching the column layout of `grouping_header`.
fn grouping_row(group: &fallow_output::HealthGroup, columns: &GroupingColumns) -> String {
    let mut row = format!("  {:<width$}", group.key, width = columns.key_width);
    if columns.score {
        if let Some(ref hs) = group.health_score {
            let grade_colored = colorize_grade(hs.grade);
            // The color codes break `{:<5}`, so pad the grade to the
            // header width by hand.
            let pad = "grade".len().saturating_sub(hs.grade.len());
            let _ = write!(
                row,
                "  {:>9.1}  {grade_colored}{:pad$}",
                hs.score,
                "",
                pad = pad
            );
        } else {
            row.push_str("                  ");
        }
    }
    if columns.trend {
        let _ = write!(
            row,
            "  {:>width$}",
            fallow_output::group_score_delta_label(group),
            width = GROUP_TREND_WIDTH
        );
    }
    let _ = write!(row, "  {:>5}", group.files_analyzed);
    let _ = write!(row, "  {:>4}", group.severity_critical_count);
    let _ = write!(row, "  {:>3}", group.hotspot_count);
    if columns.vitals {
        if let Some(ref vs) = group.vital_signs {
            let _ = write!(row, "  {:>3}", vs.p90_cyclomatic);
        } else {
            row.push_str("     ");
        }
    }
    row
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
#[path = "health_tests.rs"]
mod tests;
