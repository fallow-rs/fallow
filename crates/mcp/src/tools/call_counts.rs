//! The finding count of an in-process tool result, for opt-in telemetry.
//!
//! The CLI notes one count per analysis workflow, for example the total
//! issues of a dead-code run or the clone groups of a duplication run. The
//! telemetry event turns that count into `findings_present` and a coarse
//! `result_count_bucket`. For a tool call that runs in the server process,
//! this module reads the same count from the JSON result, so the event of
//! the in-process route and the event of the CLI route agree. Only the
//! count leaves this module; the result itself never reaches telemetry.

use serde::Deserialize;
use serde::de::IgnoredAny;

/// The fields that hold a count, in the JSON of the analysis tools.
/// `serde` skips every other field without building it.
#[derive(Debug, Default, Deserialize)]
struct Counts {
    total_issues: Option<usize>,
    total_flags: Option<usize>,
    stats: Option<DupesStats>,
    findings: Option<Vec<IgnoredAny>>,
    summary: Option<AuditSummary>,
}

#[derive(Debug, Deserialize)]
struct DupesStats {
    clone_groups: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct AuditSummary {
    dead_code_issues: Option<usize>,
    complexity_findings: Option<usize>,
    duplication_clone_groups: Option<usize>,
}

/// The finding count of a successful result of `tool`, counted as the
/// matching CLI command counts it. `None` for a tool without such a count,
/// and for a result that is not the expected JSON.
pub(super) fn result_count(tool: &str, text: &str) -> Option<usize> {
    if !has_count(tool) {
        return None;
    }
    let counts: Counts = serde_json::from_str(text).ok()?;
    match tool {
        "analyze" | "check_changed" | "check_architecture" => counts.total_issues,
        "find_dupes" => counts.stats?.clone_groups,
        "check_health" => counts.findings.map(|findings| findings.len()),
        "feature_flags" => counts.total_flags,
        "audit" => {
            let summary = counts.summary?;
            Some(
                summary.dead_code_issues?
                    + summary.complexity_findings?
                    + summary.duplication_clone_groups?,
            )
        }
        _ => None,
    }
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
            | "feature_flags"
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
    fn dupes_counts_every_clone_group_not_only_the_shown_ones() {
        let text = r#"{"clone_groups":[{}],"clone_groups_omitted":4,"stats":{"clone_groups":5}}"#;
        assert_eq!(result_count("find_dupes", text), Some(5));
    }

    #[test]
    fn health_counts_its_findings() {
        let text = r#"{"findings":[{"a":1},{"b":2}],"summary":{"files_analyzed":3}}"#;
        assert_eq!(result_count("check_health", text), Some(2));
    }

    #[test]
    fn flags_count_the_total_flags() {
        assert_eq!(
            result_count("feature_flags", r#"{"total_flags":0}"#),
            Some(0)
        );
    }

    #[test]
    fn audit_adds_the_three_analysis_counts() {
        let text = r#"{"summary":{"dead_code_issues":11,"complexity_findings":1,"duplication_clone_groups":2,"max_cyclomatic":17}}"#;
        assert_eq!(result_count("audit", text), Some(14));
    }

    #[test]
    fn no_count_for_other_tools_or_unexpected_results() {
        assert_eq!(result_count("project_info", r#"{"total_issues":3}"#), None);
        assert_eq!(result_count("analyze", "not json"), None);
        assert_eq!(result_count("analyze", r#"{"error":true}"#), None);
        assert_eq!(
            result_count("audit", r#"{"summary":{"dead_code_issues":1}}"#),
            None
        );
    }
}
