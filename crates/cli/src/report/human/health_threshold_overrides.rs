//! Human lines for `health.thresholdOverrides` entries and for
//! `fallow-override-threshold-next-line` comments.

use std::path::Path;

use colored::Colorize;
use fallow_output::{HealthReport, InlineThresholdOverrideState};

use super::MAX_FLAT_ITEMS;
use crate::report::format_display_path;

/// Render the comment rows and the comments with a problem. Rows that need a
/// code change (`no_match`, `insufficient`) come first. JSON keeps every row.
pub(super) fn render_inline_threshold_overrides(
    lines: &mut Vec<String>,
    report: &HealthReport,
    root: &Path,
) {
    render_states(lines, &report.inline_threshold_overrides, root);
    render_problems(lines, report, root);
}

fn render_states(lines: &mut Vec<String>, states: &[InlineThresholdOverrideState], root: &Path) {
    if states.is_empty() {
        return;
    }
    let mut comments: Vec<(&Path, u32)> = states
        .iter()
        .map(|state| (state.path.as_path(), state.comment_line))
        .collect();
    comments.sort_unstable();
    comments.dedup();
    lines.push(format!(
        "{} {}",
        "\u{25cf}".yellow(),
        format!("Threshold override comments ({})", comments.len())
            .yellow()
            .bold()
    ));
    let mut display: Vec<&InlineThresholdOverrideState> = states.iter().collect();
    display.sort_by_key(|state| threshold_override_status_display_rank(state.status));
    for state in display.iter().take(MAX_FLAT_ITEMS) {
        let target = state.target_label(&format_display_path(&state.path, root));
        let metrics = state.metrics.map_or(String::new(), |metrics| {
            let crap = metrics
                .crap
                .map_or(String::new(), |value| format!(" crap={value:.1}"));
            format!(
                " cyclomatic={} cognitive={}{crap}",
                metrics.cyclomatic, metrics.cognitive
            )
        });
        lines.push(format!(
            "    {dimension} {status} {target}{metrics}{outstanding} (comment, line {line})",
            dimension = threshold_override_dimension_label(state.dimension),
            status = threshold_override_status_label(state.status),
            outstanding = threshold_override_outstanding_suffix(&state.outstanding),
            line = state.comment_line,
        ));
    }
    if display.len() > MAX_FLAT_ITEMS {
        lines.push(format!(
            "    {}",
            format!(
                "... and {} more rows (--format json for full list)",
                display.len() - MAX_FLAT_ITEMS
            )
            .dimmed()
        ));
    }
    lines.push(String::new());
}

fn render_problems(lines: &mut Vec<String>, report: &HealthReport, root: &Path) {
    let problems = &report.inline_threshold_override_problems;
    if problems.is_empty() {
        return;
    }
    lines.push(format!(
        "{} {}",
        "\u{25cf}".yellow(),
        format!(
            "Threshold override comments with a problem ({})",
            problems.len()
        )
        .yellow()
        .bold()
    ));
    for problem in problems.iter().take(MAX_FLAT_ITEMS) {
        let token = problem
            .token
            .as_deref()
            .map_or(String::new(), |token| format!(" `{token}`:"));
        lines.push(format!(
            "    {}:{}{token} {}",
            format_display_path(&problem.path, root),
            problem.line,
            problem.problem.description()
        ));
    }
    if problems.len() > MAX_FLAT_ITEMS {
        lines.push(format!(
            "    {}",
            format!(
                "... and {} more (--format json for full list)",
                problems.len() - MAX_FLAT_ITEMS
            )
            .dimmed()
        ));
    }
    lines.push(String::new());
}

pub(super) fn render_threshold_overrides(
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
