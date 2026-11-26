//! The finding count of an in-process tool result, for opt-in telemetry.
//!
//! The CLI notes one count per analysis workflow, for example the total
//! issues of a dead-code run or the clone groups of a duplication run. The
//! telemetry event turns that count into `findings_present` and a coarse
//! `result_count_bucket`. For a tool call that runs in the server process,
//! this module reads the same count from the JSON result, so the event of
//! the in-process route and the event of the CLI route agree. Only the
//! count leaves this module; the result itself never reaches telemetry.
//!
//! Each tool has its own small struct, so `serde` reads only the fields of
//! that tool and skips the rest without building it.

use serde::Deserialize;
use serde::de::IgnoredAny;

/// `analyze`, `check_changed` and `check_architecture`: the CLI notes
/// `total_issues`.
#[derive(Deserialize)]
struct DeadCode {
    total_issues: usize,
}

/// `find_dupes`: the CLI notes the clone groups after `top`, which the JSON
/// names `clone_groups_shown`.
#[derive(Deserialize)]
struct Dupes {
    clone_groups_shown: usize,
}

/// `check_health`: the CLI notes the findings, but a run whose only
/// findings are coverage gaps counts as "found, count unknown".
#[derive(Deserialize)]
struct Health {
    findings: Vec<IgnoredAny>,
    coverage_gaps: Option<CoverageGaps>,
}

#[derive(Deserialize)]
struct CoverageGaps {
    #[serde(default)]
    files: Vec<IgnoredAny>,
    #[serde(default)]
    exports: Vec<IgnoredAny>,
}

/// `audit`: the CLI adds the three analysis counts of the summary.
#[derive(Deserialize)]
struct Audit {
    summary: AuditSummary,
}

#[derive(Deserialize)]
struct AuditSummary {
    dead_code_issues: usize,
    complexity_findings: usize,
    duplication_clone_groups: usize,
}

/// The finding count of a successful result of `tool`, counted as the
/// matching CLI command counts it. `None` for a tool without such a count,
/// for a result that is not the expected JSON, and for a count that the CLI
/// also does not know.
pub(super) fn result_count(tool: &str, text: &str) -> Option<usize> {
    match tool {
        "analyze" | "check_changed" | "check_architecture" => {
            serde_json::from_str::<DeadCode>(text)
                .ok()
                .map(|out| out.total_issues)
        }
        "find_dupes" => serde_json::from_str::<Dupes>(text)
            .ok()
            .map(|out| out.clone_groups_shown),
        "check_health" => health_count(serde_json::from_str(text).ok()?),
        "audit" => serde_json::from_str::<Audit>(text).ok().map(|out| {
            out.summary.dead_code_issues
                + out.summary.complexity_findings
                + out.summary.duplication_clone_groups
        }),
        _ => None,
    }
}

fn health_count(health: Health) -> Option<usize> {
    let gaps = health
        .coverage_gaps
        .is_some_and(|gaps| !gaps.files.is_empty() || !gaps.exports.is_empty());
    if health.findings.is_empty() && gaps {
        return None;
    }
    Some(health.findings.len())
}

/// True for a tool whose result holds a finding count. The server copies a
/// result for telemetry only for these tools.
pub(super) fn has_count(tool: &str) -> bool {
    matches!(
        tool,
        "analyze"
            | "check_changed"
            | "check_architecture"
            | "find_dupes"
            | "check_health"
            | "audit"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dead_code_tools_count_the_total_issues() {
        let text = r#"{"kind":"dead-code","total_issues":7,"summary":{"total_issues":7}}"#;
        for tool in ["analyze", "check_changed", "check_architecture"] {
            assert_eq!(result_count(tool, text), Some(7), "{tool}");
        }
    }

    #[test]
    fn dupes_counts_the_clone_groups_after_top_like_the_cli() {
        let text = r#"{"clone_groups":[{}],"clone_groups_shown":1,"clone_groups_omitted":4,"stats":{"clone_groups":5}}"#;
        assert_eq!(result_count("find_dupes", text), Some(1));
    }

    #[test]
    fn health_counts_its_findings() {
        let text = r#"{"findings":[{"a":1},{"b":2}],"summary":{"files_analyzed":3}}"#;
        assert_eq!(result_count("check_health", text), Some(2));
    }

    #[test]
    fn health_with_only_coverage_gaps_has_no_count() {
        let text = r#"{"findings":[],"coverage_gaps":{"summary":{},"files":[{"path":"a.ts"}]}}"#;
        assert_eq!(result_count("check_health", text), None);
        let clean = r#"{"findings":[],"coverage_gaps":{"summary":{}}}"#;
        assert_eq!(result_count("check_health", clean), Some(0));
    }

    #[test]
    fn audit_adds_the_three_analysis_counts() {
        let text = r#"{"summary":{"dead_code_issues":11,"complexity_findings":1,"duplication_clone_groups":2,"max_cyclomatic":17}}"#;
        assert_eq!(result_count("audit", text), Some(14));
    }

    #[test]
    fn an_unrelated_field_of_another_type_does_not_drop_the_count() {
        let text = r#"{"total_issues":2,"summary":"text","stats":[1],"findings":3}"#;
        assert_eq!(result_count("analyze", text), Some(2));
    }

    #[test]
    fn no_count_for_other_tools_or_unexpected_results() {
        assert_eq!(result_count("project_info", r#"{"total_issues":3}"#), None);
        assert_eq!(result_count("feature_flags", r#"{"total_flags":3}"#), None);
        assert_eq!(result_count("analyze", "not json"), None);
        assert_eq!(result_count("analyze", r#"{"error":true}"#), None);
        assert_eq!(
            result_count("audit", r#"{"summary":{"dead_code_issues":1}}"#),
            None
        );
        assert!(!has_count("feature_flags"));
    }
}
