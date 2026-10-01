//! The `## Health by <mode>` section of a grouped health run.
//!
//! One renderer for `--format markdown` and the GitHub job summary, so the two
//! tables cannot drift. It reads the JSON shape of `groups[]`, because the job
//! summary renders a saved envelope (`fallow report --from`) that has no typed
//! grouping. The live Markdown path serializes its typed groups first.

use std::fmt::Write;

use fallow_output::{markdown_code_span, markdown_table_code_span};
use serde_json::Value;

/// Findings shown per group inside its collapsible block.
const GROUP_DETAILS_MAX_FINDINGS: usize = 10;

/// Group label of the files that no CODEOWNERS rule matches.
const UNOWNED_GROUP_KEY: &str = "(unowned)";

/// Render the per-group section of a grouped health envelope.
///
/// Reads `grouped_by`, `groups` and `group_filter` from `envelope`. Returns an
/// empty string when the envelope is not grouped. Paths that start with
/// `root_prefix` lose that prefix, so a live run (absolute paths) and a saved
/// envelope (relative paths) render the same text.
#[must_use]
pub fn build_health_groups_markdown(envelope: &Value, root_prefix: &str) -> String {
    let Some(mode) = envelope.get("grouped_by").and_then(Value::as_str) else {
        return String::new();
    };
    let Some(groups) = envelope.get("groups").and_then(Value::as_array) else {
        return String::new();
    };
    let mut out = String::new();
    let _ = write!(out, "## Health by {mode}\n\n");
    if let Some(filter) = envelope.get("group_filter").and_then(Value::as_array) {
        let patterns: Vec<String> = filter
            .iter()
            .filter_map(Value::as_str)
            .map(|pattern| markdown_code_span(&collapse_line_endings(pattern)))
            .collect();
        let _ = write!(
            out,
            "Groups selected with `--group` {}. Project-level sections are not filtered.\n\n",
            patterns.join(", ")
        );
    }
    if groups.is_empty() {
        out.push_str("No group matched.\n");
        return out;
    }
    let ordered = groups_in_display_order(groups);
    let show_p90 = groups
        .iter()
        .any(|group| group.get("vital_signs").is_some());
    out.push_str("| Group | Score | Grade | Delta | Files | Critical | Hotspots |");
    out.push_str(if show_p90 { " P90 |\n" } else { "\n" });
    out.push_str("|:------|------:|:------|:------|------:|---------:|---------:|");
    out.push_str(if show_p90 { "----:|\n" } else { "\n" });
    for group in &ordered {
        write_group_row(&mut out, group, show_p90);
    }
    for group in &ordered {
        write_group_details(&mut out, group, root_prefix);
    }
    out
}

/// Score ascending (worst first) with the unowned group last, when any group
/// carries a score. Otherwise the resolver order of the envelope.
fn groups_in_display_order(groups: &[Value]) -> Vec<&Value> {
    let mut ordered: Vec<&Value> = groups.iter().collect();
    if groups.iter().any(|group| group_score(group).is_some()) {
        ordered.sort_by(|a, b| {
            let unowned =
                (group_key(a) == UNOWNED_GROUP_KEY).cmp(&(group_key(b) == UNOWNED_GROUP_KEY));
            let a_score = group_score(a).unwrap_or(f64::INFINITY);
            let b_score = group_score(b).unwrap_or(f64::INFINITY);
            unowned.then(
                a_score
                    .partial_cmp(&b_score)
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
        });
    }
    ordered
}

fn group_key(group: &Value) -> &str {
    group.get("key").and_then(Value::as_str).unwrap_or_default()
}

fn group_score(group: &Value) -> Option<f64> {
    group.get("health_score")?.get("score")?.as_f64()
}

fn count(group: &Value, key: &str) -> u64 {
    group.get(key).and_then(Value::as_u64).unwrap_or(0)
}

/// A `--group` pattern is free user text. A line ending in it would end the
/// paragraph, so line endings collapse to spaces before the code span.
fn collapse_line_endings(value: &str) -> String {
    value.replace("\r\n", " ").replace(['\n', '\r'], " ")
}

/// Write one table row. The P90 cell is present only when `show_p90` is set,
/// which is when at least one group carries vital signs.
fn write_group_row(out: &mut String, group: &Value, show_p90: bool) {
    let score = group_score(group).map_or_else(|| "-".to_owned(), |score| format!("{score:.1}"));
    let grade = group
        .get("health_score")
        .and_then(|score| score.get("grade"))
        .and_then(Value::as_str)
        .unwrap_or("-");
    let _ = write!(
        out,
        "| {} | {score} | {grade} | {} | {} | {} | {} |",
        markdown_table_code_span(group_key(group)),
        score_delta(group),
        count(group, "files_analyzed"),
        count(group, "severity_critical_count"),
        count(group, "hotspot_count"),
    );
    if show_p90 {
        let p90 = group
            .get("vital_signs")
            .and_then(|vitals| vitals.get("p90_cyclomatic"))
            .and_then(Value::as_u64)
            .map_or_else(|| "-".to_owned(), |p90| p90.to_string());
        let _ = write!(out, " {p90} |");
    }
    out.push('\n');
}

/// `+2.3 ↑` for a compared group, `new` for a group the baseline does not
/// hold, `-` otherwise. Mirrors `fallow_output::group_score_delta_label`.
fn score_delta(group: &Value) -> String {
    match group.get("trend_status").and_then(Value::as_str) {
        Some("new_group") => "new".to_owned(),
        Some("compared") => group
            .get("trend")
            .and_then(|trend| trend.get("metrics"))
            .and_then(Value::as_array)
            .and_then(|metrics| {
                metrics
                    .iter()
                    .find(|metric| metric.get("name").and_then(Value::as_str) == Some("score"))
            })
            .and_then(|metric| {
                let delta = metric.get("delta")?.as_f64()?;
                let arrow = match metric.get("direction")?.as_str()? {
                    "improving" => "\u{2191}",
                    "declining" => "\u{2193}",
                    _ => "\u{2192}",
                };
                Some(format!("{delta:+.1} {arrow}"))
            })
            .unwrap_or_else(|| "-".to_owned()),
        _ => "-".to_owned(),
    }
}

fn write_group_details(out: &mut String, group: &Value, root_prefix: &str) {
    let Some(findings) = group.get("findings").and_then(Value::as_array) else {
        return;
    };
    if findings.is_empty() {
        return;
    }
    let total = group_finding_total(group).max(findings.len() as u64);
    let shown = findings.len().min(GROUP_DETAILS_MAX_FINDINGS);
    let _ = write!(
        out,
        "\n<details>\n<summary><code>{}</code>: {total} finding{}</summary>\n\n",
        html_escape(group_key(group)),
        if total == 1 { "" } else { "s" },
    );
    out.push_str("| File | Function | Severity | Cyclomatic | Cognitive | CRAP | Lines |\n");
    out.push_str("|:-----|:---------|:---------|:-----------|:----------|:-----|:------|\n");
    for finding in findings.iter().take(shown) {
        write_finding_row(out, finding, root_prefix);
    }
    let hidden = total.saturating_sub(shown as u64);
    if hidden > 0 {
        let _ = write!(out, "\n... and {hidden} more.\n");
    }
    out.push_str("\n</details>\n");
}

/// The number of findings of a group before `--top`.
///
/// `functions_above_threshold` counts only the findings that the group shows
/// after `--top`. The severity counts are taken before `--top`, so their sum
/// is the total. An envelope without severity counts falls back to
/// `functions_above_threshold`.
fn group_finding_total(group: &Value) -> u64 {
    let by_severity = count(group, "severity_critical_count")
        + count(group, "severity_high_count")
        + count(group, "severity_moderate_count");
    if by_severity > 0 {
        by_severity
    } else {
        count(group, "functions_above_threshold")
    }
}

fn write_finding_row(out: &mut String, finding: &Value, root_prefix: &str) {
    let path = finding
        .get("path")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let path = path
        .strip_prefix(root_prefix)
        .filter(|rest| !root_prefix.is_empty() && rest.starts_with(['/', '\\']))
        .map_or(path, |rest| rest.trim_start_matches(['/', '\\']))
        .replace('\\', "/");
    let line = count(finding, "line");
    let crap = finding
        .get("crap")
        .and_then(Value::as_f64)
        .map_or_else(|| "-".to_owned(), |crap| format!("{crap:.1}"));
    let _ = writeln!(
        out,
        "| {} | {} | {} | {} | {} | {crap} | {} |",
        markdown_table_code_span(&format!("{path}:{line}")),
        markdown_table_code_span(
            finding
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
        ),
        finding
            .get("severity")
            .and_then(Value::as_str)
            .unwrap_or("-"),
        count(finding, "cyclomatic"),
        count(finding, "cognitive"),
        count(finding, "line_count"),
    );
}

fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn envelope() -> Value {
        serde_json::json!({
            "grouped_by": "owner",
            "groups": [
                {
                    "key": "(unowned)",
                    "files_analyzed": 4,
                    "functions_above_threshold": 0,
                    "severity_critical_count": 0,
                    "hotspot_count": 0,
                    "health_score": { "score": 10.0, "grade": "F" }
                },
                {
                    "key": "@team/b",
                    "files_analyzed": 2,
                    "functions_above_threshold": 1,
                    "severity_critical_count": 1,
                    "hotspot_count": 3,
                    "health_score": { "score": 60.0, "grade": "C" },
                    "vital_signs": { "p90_cyclomatic": 9 },
                    "trend_status": "compared",
                    "trend": { "metrics": [
                        { "name": "score", "delta": -2.5, "direction": "declining" }
                    ] },
                    "findings": [
                        {
                            "path": "/repo/src/b.ts",
                            "line": 3,
                            "name": "big",
                            "severity": "critical",
                            "cyclomatic": 30,
                            "cognitive": 40,
                            "line_count": 90
                        }
                    ]
                },
                {
                    "key": "@team/a",
                    "files_analyzed": 3,
                    "functions_above_threshold": 0,
                    "severity_critical_count": 0,
                    "hotspot_count": 0,
                    "health_score": { "score": 90.0, "grade": "A" },
                    "trend_status": "new_group"
                }
            ]
        })
    }

    #[test]
    fn renders_rows_worst_first_with_unowned_last() {
        let out = build_health_groups_markdown(&envelope(), "/repo");
        let b = out.find("| `@team/b` |").expect("row b");
        let a = out.find("| `@team/a` |").expect("row a");
        let unowned = out.find("| `(unowned)` |").expect("row unowned");
        assert!(b < a && a < unowned, "{out}");
        assert!(out.contains("| `@team/b` | 60.0 | C | -2.5 \u{2193} | 2 | 1 | 3 | 9 |"));
        assert!(out.contains("| `@team/a` | 90.0 | A | new | 3 | 0 | 0 | - |"));
    }

    #[test]
    fn details_strip_the_root_prefix() {
        let out = build_health_groups_markdown(&envelope(), "/repo");
        assert!(out.contains("<summary><code>@team/b</code>: 1 finding</summary>"));
        assert!(out.contains("| `src/b.ts:3` | `big` | critical | 30 | 40 | - | 90 |"));
    }

    #[test]
    fn details_count_the_findings_before_top() {
        let envelope = serde_json::json!({
            "grouped_by": "owner",
            "groups": [{
                "key": "@team/c",
                "files_analyzed": 1,
                "functions_above_threshold": 1,
                "severity_critical_count": 2,
                "severity_high_count": 1,
                "severity_moderate_count": 4,
                "findings": [{ "path": "src/c.ts", "line": 1, "name": "c" }]
            }]
        });
        let out = build_health_groups_markdown(&envelope, "");
        assert!(
            out.contains("<summary><code>@team/c</code>: 7 findings</summary>"),
            "{out}"
        );
        assert!(out.contains("... and 6 more."), "{out}");
    }

    #[test]
    fn ungrouped_envelope_renders_nothing() {
        assert!(build_health_groups_markdown(&serde_json::json!({}), "/repo").is_empty());
    }

    #[test]
    fn empty_selection_says_so() {
        let envelope = serde_json::json!({
            "grouped_by": "owner",
            "groups": [],
            "group_filter": ["@nobody"]
        });
        let out = build_health_groups_markdown(&envelope, "/repo");
        assert!(out.contains("Groups selected with `--group` `@nobody`."));
        assert!(out.contains("No group matched."));
    }

    #[test]
    fn group_filter_pattern_with_backtick_stays_one_code_span() {
        let envelope = serde_json::json!({
            "grouped_by": "owner",
            "groups": [],
            "group_filter": ["@a`b", "line\none"]
        });
        let out = build_health_groups_markdown(&envelope, "/repo");
        assert!(
            out.contains("Groups selected with `--group` ``@a`b``, `line one`."),
            "{out}"
        );
    }

    #[test]
    fn p90_column_hidden_when_no_group_has_vital_signs() {
        let envelope = serde_json::json!({
            "grouped_by": "owner",
            "groups": [{
                "key": "@team/a",
                "files_analyzed": 3,
                "health_score": { "score": 90.0, "grade": "A" }
            }]
        });
        let out = build_health_groups_markdown(&envelope, "/repo");
        assert!(!out.contains("P90"), "{out}");
        assert!(
            out.contains("| Group | Score | Grade | Delta | Files | Critical | Hotspots |\n"),
            "{out}"
        );
        assert!(
            out.contains("|:------|------:|:------|:------|------:|---------:|---------:|\n"),
            "{out}"
        );
        assert!(
            out.contains("| `@team/a` | 90.0 | A | - | 3 | 0 | 0 |\n"),
            "{out}"
        );
    }
}
