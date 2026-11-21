//! Markdown sections for `health.thresholdOverrides` entries, and compact
//! and markdown rows for `fallow-override-threshold-next-line` comments.

use std::fmt::Write as _;
use std::path::Path;

use fallow_output::{
    HealthReport, InlineThresholdOverrideState, ThresholdOverrideDimension,
    ThresholdOverrideStatus, markdown_table_code_span, normalize_uri,
};

fn relative_path<'a>(path: &'a Path, root: &Path) -> &'a Path {
    path.strip_prefix(root).unwrap_or(path)
}

const fn status_label(status: ThresholdOverrideStatus) -> &'static str {
    match status {
        ThresholdOverrideStatus::Active => "active",
        ThresholdOverrideStatus::Stale => "stale",
        ThresholdOverrideStatus::Insufficient => "insufficient",
        ThresholdOverrideStatus::NoMatch => "no_match",
    }
}

const fn dimension_label(dimension: ThresholdOverrideDimension) -> &'static str {
    match dimension {
        ThresholdOverrideDimension::Complexity => "complexity",
        ThresholdOverrideDimension::Crap => "crap",
    }
}

fn outstanding_list(state: &InlineThresholdOverrideState, separator: &str) -> String {
    state
        .outstanding
        .iter()
        .map(|value| dimension_label(*value))
        .collect::<Vec<_>>()
        .join(separator)
}

/// One compact line per comment row and per comment with a problem:
/// `threshold-override-comment:<dimension>:<status>:<target>:comment_line=<n>`
/// and `threshold-override-comment-problem:<path>:<line>:<problem>`.
pub fn push_inline_threshold_compact(
    lines: &mut Vec<String>,
    report: &HealthReport,
    display_path: impl Fn(&Path) -> String,
) {
    for state in &report.inline_threshold_overrides {
        let metrics = state.metrics.map_or(String::new(), |metrics| {
            let crap = metrics
                .crap
                .map_or(String::new(), |value| format!(",crap={value:.1}"));
            format!(
                ",cyclomatic={},cognitive={}{crap}",
                metrics.cyclomatic, metrics.cognitive
            )
        });
        let outstanding = if state.outstanding.is_empty() {
            String::new()
        } else {
            format!(",outstanding={}", outstanding_list(state, "|"))
        };
        lines.push(format!(
            "threshold-override-comment:{}:{}:{}:comment_line={}{metrics}{outstanding}",
            dimension_label(state.dimension),
            status_label(state.status),
            state.target_label(&display_path(&state.path)),
            state.comment_line,
        ));
    }
    for problem in &report.inline_threshold_override_problems {
        let problem_name = serde_json::to_value(problem.problem)
            .ok()
            .and_then(|value| value.as_str().map(str::to_string))
            .unwrap_or_default();
        lines.push(format!(
            "threshold-override-comment-problem:{}:{}:{problem_name}",
            display_path(&problem.path),
            problem.line,
        ));
    }
}

/// A markdown section with one table row per comment row and per comment
/// with a problem. Writes nothing when the report has neither.
pub fn write_inline_threshold_markdown(
    out: &mut String,
    report: &HealthReport,
    display_path: impl Fn(&Path) -> String,
) {
    if report.inline_threshold_overrides.is_empty()
        && report.inline_threshold_override_problems.is_empty()
    {
        return;
    }
    if !out.is_empty() && !out.ends_with("\n\n") {
        out.push('\n');
    }
    out.push_str("## Threshold Override Comments\n\n");
    if !report.inline_threshold_overrides.is_empty() {
        out.push_str("| Comment | Dimension | Status | Target | Metrics | Outstanding |\n");
        out.push_str("|:--------|:----------|:-------|:-------|:--------|:------------|\n");
        for state in &report.inline_threshold_overrides {
            let path = display_path(&state.path);
            let metrics = state.metrics.map_or_else(
                || "-".to_string(),
                |metrics| {
                    let crap = metrics
                        .crap
                        .map_or(String::new(), |value| format!(", CRAP {value:.1}"));
                    format!(
                        "cyclomatic {}, cognitive {}{crap}",
                        metrics.cyclomatic, metrics.cognitive
                    )
                },
            );
            let outstanding = if state.outstanding.is_empty() {
                "-".to_string()
            } else {
                outstanding_list(state, ", ")
            };
            let _ = writeln!(
                out,
                "| {} | {} | {} | {} | {metrics} | {outstanding} |",
                markdown_table_code_span(&format!("{path}:{}", state.comment_line)),
                dimension_label(state.dimension),
                status_label(state.status),
                markdown_table_code_span(&state.target_label(&path)),
            );
        }
        out.push('\n');
    }
    if !report.inline_threshold_override_problems.is_empty() {
        out.push_str("| Comment | Problem |\n");
        out.push_str("|:--------|:--------|\n");
        for problem in &report.inline_threshold_override_problems {
            let _ = writeln!(
                out,
                "| {} | {} |",
                markdown_table_code_span(&format!(
                    "{}:{}",
                    display_path(&problem.path),
                    problem.line
                )),
                problem.problem.description(),
            );
        }
        out.push('\n');
    }
}

/// The config override section, then the comment section.
pub fn write_threshold_override_markdown(out: &mut String, report: &HealthReport, root: &Path) {
    write_threshold_overrides_section(out, report, root);
    write_inline_threshold_markdown(out, report, |path| {
        normalize_uri(&relative_path(path, root).display().to_string())
    });
}

fn write_threshold_overrides_section(
    out: &mut String,
    report: &fallow_output::HealthReport,
    root: &Path,
) {
    if report.threshold_overrides.is_empty() {
        return;
    }
    if !out.is_empty() && !out.ends_with("\n\n") {
        out.push('\n');
    }
    out.push_str("## Health Threshold Overrides\n\n");
    out.push_str("| Override | Dimension | Status | Target | Metrics | Outstanding |\n");
    out.push_str("|---------:|:----------|:-------|:-------|:--------|:------------|\n");
    for entry in &report.threshold_overrides {
        let status = match entry.status {
            fallow_output::ThresholdOverrideStatus::Active => "active",
            fallow_output::ThresholdOverrideStatus::Stale => "stale",
            fallow_output::ThresholdOverrideStatus::Insufficient => "insufficient",
            fallow_output::ThresholdOverrideStatus::NoMatch => "no_match",
        };
        let dimension = threshold_override_dimension_label(entry.dimension);
        let outstanding = if entry.outstanding.is_empty() {
            "-".to_string()
        } else {
            entry
                .outstanding
                .iter()
                .map(|value| threshold_override_dimension_label(*value))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let target = entry.path.as_ref().map_or_else(
            || "<no matching file or function>".to_string(),
            |path| {
                entry.target_label(&normalize_uri(
                    &relative_path(path, root).display().to_string(),
                ))
            },
        );
        let metrics = entry.metrics.map_or_else(
            || "-".to_string(),
            |metrics| {
                let crap = metrics
                    .crap
                    .map_or(String::new(), |value| format!(", CRAP {value:.1}"));
                let line_count = metrics
                    .line_count
                    .map_or(String::new(), |value| format!(", {value} lines"));
                format!(
                    "cyclomatic {}, cognitive {}{}{}",
                    metrics.cyclomatic, metrics.cognitive, line_count, crap
                )
            },
        );
        // Mirror of the human renderer's copy: a crap-dimension row against a
        // template-family unit reports a dimension the unit is not scored on,
        // so the row must read as removable, not as an unqualified success.
        let metrics = if threshold_override_crap_not_applicable(entry) {
            format!("{metrics}; not scored on CRAP (this entry can be removed)")
        } else {
            metrics
        };
        let _ = writeln!(
            out,
            "| {} | {} | {} | {} | {} | {} |",
            entry.override_index,
            dimension,
            status,
            markdown_table_code_span(&target),
            metrics,
            outstanding
        );
    }
    out.push('\n');
}

fn threshold_override_dimension_label(
    dimension: fallow_output::ThresholdOverrideDimension,
) -> &'static str {
    match dimension {
        fallow_output::ThresholdOverrideDimension::Complexity => "complexity",
        fallow_output::ThresholdOverrideDimension::Crap => "crap",
    }
}

/// True for a crap-dimension override row recorded against a synthetic
/// template-family unit: measured complexity metrics present, CRAP value
/// absent because the unit is excluded from the CRAP dimension.
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

#[cfg(test)]
mod tests {
    use super::*;
    use fallow_output::{
        HealthConfiguredThresholds, HealthEffectiveThresholds, InlineThresholdOverrideProblem,
        InlineThresholdProblemKind,
    };

    fn report() -> HealthReport {
        HealthReport {
            inline_threshold_overrides: vec![InlineThresholdOverrideState {
                status: ThresholdOverrideStatus::Insufficient,
                dimension: ThresholdOverrideDimension::Complexity,
                outstanding: vec![ThresholdOverrideDimension::Complexity],
                path: "/root/src/a.ts".into(),
                comment_line: 4,
                function: Some("parse".to_string()),
                line: Some(5),
                col: Some(0),
                configured_thresholds: HealthConfiguredThresholds {
                    max_cyclomatic: None,
                    max_cognitive: Some(5),
                    max_crap: None,
                    max_unit_size: None,
                    max_file_lines: None,
                },
                effective_thresholds: HealthEffectiveThresholds {
                    max_cyclomatic: 20,
                    max_cognitive: 5,
                    max_crap: 30.0,
                    max_unit_size: 60,
                },
                metrics: None,
                reason: "cap".to_string(),
            }],
            inline_threshold_override_problems: vec![InlineThresholdOverrideProblem {
                path: "/root/src/a.ts".into(),
                line: 9,
                problem: InlineThresholdProblemKind::MissingReason,
                token: None,
            }],
            ..HealthReport::default()
        }
    }

    fn display(path: &Path) -> String {
        path.strip_prefix("/root")
            .unwrap_or(path)
            .display()
            .to_string()
            .trim_start_matches('/')
            .to_string()
    }

    #[test]
    fn compact_lines_name_the_comment_and_the_problem() {
        let mut lines = Vec::new();
        push_inline_threshold_compact(&mut lines, &report(), display);
        assert_eq!(
            lines,
            vec![
                "threshold-override-comment:complexity:insufficient:src/a.ts:5:parse:comment_line=4,outstanding=complexity",
                "threshold-override-comment-problem:src/a.ts:9:missing_reason",
            ]
        );
    }

    #[test]
    fn markdown_lists_rows_and_problems() {
        let mut out = String::new();
        write_inline_threshold_markdown(&mut out, &report(), display);
        assert!(out.contains("## Threshold Override Comments"), "{out}");
        assert!(
            out.contains(
                "| `src/a.ts:4` | complexity | insufficient | `src/a.ts:5:parse` | - | complexity |"
            ),
            "{out}"
        );
        assert!(
            out.contains("| `src/a.ts:9` | the comment has no reason"),
            "{out}"
        );
    }

    #[test]
    fn markdown_cells_escape_a_backtick_and_a_pipe_in_the_path() {
        let mut report = report();
        report.inline_threshold_overrides[0].path = "/root/src/a`b|c.ts".into();
        report.inline_threshold_override_problems[0].path = "/root/src/a`b|c.ts".into();
        let mut out = String::new();
        write_inline_threshold_markdown(&mut out, &report, display);
        for line in out.lines().filter(|line| line.contains("a`b")) {
            let cells = line.matches(" | ").count();
            let escaped_pipes = line.matches("\\|").count();
            assert!(escaped_pipes > 0, "{line}");
            assert!(
                line.contains("``"),
                "a path with a backtick needs a longer code fence: {line}"
            );
            assert!(cells >= 1, "{line}");
        }
        let row = out
            .lines()
            .find(|line| line.contains("insufficient"))
            .expect("row");
        let unescaped = row.replace("\\|", "");
        assert_eq!(unescaped.matches('|').count(), 7, "{row}");
    }

    #[test]
    fn markdown_writes_nothing_without_comments() {
        let mut out = String::new();
        write_inline_threshold_markdown(&mut out, &HealthReport::default(), display);
        assert!(out.is_empty());
    }
}
