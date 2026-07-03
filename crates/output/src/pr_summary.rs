//! Pure renderer for sticky PR summary comments.

use std::fmt::Write as _;

use crate::{CiProvider, PrCommentEnvelope, PrCommentTruncation, command_title, escape_md};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrSummaryStatus {
    Pass,
    Warn,
    Fail,
    Info,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrSummaryScope {
    Project,
    Diff,
    ChangedFiles,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrSummaryArea {
    pub name: String,
    pub status: PrSummaryStatus,
    pub result: String,
    pub threshold: Option<String>,
    pub details: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrSummaryFinding {
    pub severity: String,
    pub rule_id: String,
    pub location: String,
    pub description: String,
    pub fix: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrCommentLayout {
    Default,
    Compact,
    GateOnly,
    Details,
}

pub struct PrSummaryInput<'a> {
    pub command: &'a str,
    pub provider: CiProvider,
    pub marker_id: String,
    pub scope: PrSummaryScope,
    pub areas: &'a [PrSummaryArea],
    pub findings: &'a [PrSummaryFinding],
    pub max_findings: usize,
    pub details_url: Option<&'a str>,
    pub layout: PrCommentLayout,
}

#[must_use]
pub fn render_pr_summary(input: &PrSummaryInput<'_>) -> PrCommentEnvelope {
    let max_findings = input.max_findings.max(1);
    let is_clean = input.findings.is_empty()
        && input
            .areas
            .iter()
            .all(|area| matches!(area.status, PrSummaryStatus::Pass | PrSummaryStatus::Info));
    let status = summary_status(input.areas);
    let marker = format!("<!-- fallow-id: {} -->", input.marker_id);
    let mut body = String::new();
    body.push_str(&marker);
    body.push('\n');
    render_header(&mut body, input);
    render_callout(&mut body, status, is_clean, input.findings.len());
    match input.layout {
        PrCommentLayout::Default | PrCommentLayout::Details => {
            render_area_table(&mut body, input.areas);
            render_top_findings(&mut body, input.findings, max_findings);
        }
        PrCommentLayout::GateOnly => {
            render_area_table(&mut body, input.areas);
        }
        PrCommentLayout::Compact => {
            render_compact_gates(&mut body, input.areas);
        }
    }
    render_footer(&mut body);

    let shown_findings = input.findings.len().min(max_findings);
    PrCommentEnvelope {
        marker_id: input.marker_id.clone(),
        body,
        is_clean,
        details_url: input.details_url.map(str::to_owned),
        check_summary: Some(status_label(status).to_owned()),
        truncation: PrCommentTruncation {
            truncated: input.findings.len() > max_findings,
            shown_findings,
            total_findings: input.findings.len(),
        },
    }
}

fn render_header(out: &mut String, input: &PrSummaryInput<'_>) {
    let title = command_title(input.command);
    let scope = scope_label(input.scope);
    let provider = input.provider.name();
    let target = provider_target_label(input.provider);
    let _ = writeln!(out, "# Fallow {title}\n");
    let _ = writeln!(out, "_{provider} {target} summary, scope: {scope}_\n");
}

fn render_callout(out: &mut String, status: PrSummaryStatus, is_clean: bool, finding_count: usize) {
    let kind = callout_kind(status, is_clean);
    let message = callout_message(status, is_clean, finding_count);
    let _ = writeln!(out, "> [!{kind}]");
    let _ = writeln!(out, "> {message}\n");
}

fn summary_status(areas: &[PrSummaryArea]) -> PrSummaryStatus {
    if areas
        .iter()
        .any(|area| area.status == PrSummaryStatus::Fail)
    {
        return PrSummaryStatus::Fail;
    }
    if areas
        .iter()
        .any(|area| area.status == PrSummaryStatus::Warn)
    {
        return PrSummaryStatus::Warn;
    }
    if areas
        .iter()
        .any(|area| area.status == PrSummaryStatus::Info)
    {
        return PrSummaryStatus::Info;
    }
    PrSummaryStatus::Pass
}

fn callout_kind(status: PrSummaryStatus, is_clean: bool) -> &'static str {
    if is_clean {
        return "NOTE";
    }
    match status {
        PrSummaryStatus::Fail => "IMPORTANT",
        PrSummaryStatus::Warn => "WARNING",
        PrSummaryStatus::Pass | PrSummaryStatus::Info => "NOTE",
    }
}

fn callout_message(status: PrSummaryStatus, is_clean: bool, finding_count: usize) -> String {
    if is_clean {
        return "No review-visible findings were produced for this run.".to_owned();
    }
    let noun = if finding_count == 1 {
        "finding"
    } else {
        "findings"
    };
    match status {
        PrSummaryStatus::Fail => {
            format!("Quality gates need attention. Found {finding_count} {noun}.")
        }
        PrSummaryStatus::Warn => format!("Review recommended. Found {finding_count} {noun}."),
        PrSummaryStatus::Pass | PrSummaryStatus::Info => {
            format!("No blocking gates failed. Showing {finding_count} {noun}.")
        }
    }
}

fn scope_label(scope: PrSummaryScope) -> &'static str {
    match scope {
        PrSummaryScope::Project => "project",
        PrSummaryScope::Diff => "diff",
        PrSummaryScope::ChangedFiles => "changed files",
    }
}

fn provider_target_label(provider: CiProvider) -> &'static str {
    match provider {
        CiProvider::Github => "PR",
        CiProvider::Gitlab => "MR",
    }
}

fn render_area_table(out: &mut String, areas: &[PrSummaryArea]) {
    if areas.is_empty() {
        return;
    }
    out.push_str("## Checks\n\n");
    out.push_str("| Area | Status | Result | Threshold | Details |\n");
    out.push_str("| --- | --- | --- | --- | --- |\n");
    for area in areas {
        let threshold = area.threshold.as_deref().unwrap_or("n/a");
        let details = area.details.as_deref().unwrap_or("");
        let _ = writeln!(
            out,
            "| {} | {} | {} | {} | {} |",
            escape_md(&area.name),
            status_label(area.status),
            escape_md(&area.result),
            escape_md(threshold),
            escape_md(details)
        );
    }
    out.push('\n');
}

fn render_compact_gates(out: &mut String, areas: &[PrSummaryArea]) {
    let notable = areas
        .iter()
        .filter(|area| !matches!(area.status, PrSummaryStatus::Pass | PrSummaryStatus::Info))
        .collect::<Vec<_>>();
    if notable.is_empty() {
        out.push_str("All PR gates passed.\n\n");
        return;
    }
    out.push_str("## Gates\n\n");
    for area in notable {
        let _ = writeln!(
            out,
            "- {}: {} ({})",
            escape_md(&area.name),
            status_label(area.status),
            escape_md(&area.result)
        );
    }
    out.push('\n');
}

fn render_top_findings(out: &mut String, findings: &[PrSummaryFinding], max_findings: usize) {
    if findings.is_empty() {
        return;
    }
    let summary = if findings.len() > max_findings {
        format!("Top fixes (showing {max_findings} of {})", findings.len())
    } else {
        "Top fixes".to_owned()
    };
    let _ = writeln!(out, "<details open>\n<summary>{summary}</summary>\n");
    out.push_str("| Severity | Fix | Location | Why |\n");
    out.push_str("| --- | --- | --- | --- |\n");
    for finding in findings.iter().take(max_findings) {
        render_finding_row(out, finding);
    }
    if findings.len() > max_findings {
        let _ = writeln!(
            out,
            "\nShowing {max_findings} of {} findings. Inspect the CI artifact for the full report.",
            findings.len()
        );
    }
    out.push_str("\n</details>\n\n");
}

fn render_finding_row(out: &mut String, finding: &PrSummaryFinding) {
    let _ = writeln!(
        out,
        "| {} | {} | `{}` | {} |",
        escape_md(&finding.severity),
        escape_md(finding.fix.as_deref().unwrap_or(&finding.rule_id)),
        escape_md(&finding.location),
        escape_md(&finding.description)
    );
}

fn status_label(status: PrSummaryStatus) -> &'static str {
    match status {
        PrSummaryStatus::Pass => "pass",
        PrSummaryStatus::Warn => "warn",
        PrSummaryStatus::Fail => "fail",
        PrSummaryStatus::Info => "info",
    }
}

fn render_footer(out: &mut String) {
    out.push_str("Generated by fallow.");
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEFAULT_MAX_FINDINGS: usize = 50;

    fn input<'a>(
        areas: &'a [PrSummaryArea],
        findings: &'a [PrSummaryFinding],
    ) -> PrSummaryInput<'a> {
        PrSummaryInput {
            command: "combined",
            provider: CiProvider::Github,
            marker_id: "fallow-results".to_owned(),
            scope: PrSummaryScope::Project,
            areas,
            findings,
            max_findings: DEFAULT_MAX_FINDINGS,
            details_url: None,
            layout: PrCommentLayout::Default,
        }
    }

    #[test]
    fn clean_summary_marks_envelope_without_sentinel_body_policy() {
        let envelope = render_pr_summary(&input(&[], &[]));

        assert!(envelope.is_clean);
        assert!(envelope.body.contains("No review-visible findings"));
        assert!(!envelope.body.contains("fallow-clean-sentinel"));
    }

    #[test]
    fn gitlab_header_uses_mr_language() {
        let custom = PrSummaryInput {
            provider: CiProvider::Gitlab,
            ..input(&[], &[])
        };

        let envelope = render_pr_summary(&custom);

        assert!(envelope.body.contains("_GitLab MR summary"));
        assert!(!envelope.body.contains("_GitLab PR summary"));
    }

    #[test]
    fn warning_summary_leads_with_review_message_and_checks_table() {
        let areas = [PrSummaryArea {
            name: "Duplication".to_owned(),
            status: PrSummaryStatus::Warn,
            result: "2 clone groups".to_owned(),
            threshold: Some("<= 3% duplication".to_owned()),
            details: Some("9.1% duplicated lines".to_owned()),
        }];
        let findings = [PrSummaryFinding {
            severity: "minor".to_owned(),
            rule_id: "fallow/code-duplication".to_owned(),
            location: "src/a.ts:10".to_owned(),
            description: "Code clone group 1".to_owned(),
            fix: Some("Extract the repeated block.".to_owned()),
        }];

        let envelope = render_pr_summary(&input(&areas, &findings));

        assert!(!envelope.is_clean);
        assert!(envelope.body.contains("> [!WARNING]"));
        assert!(
            envelope
                .body
                .contains("| Duplication | warn | 2 clone groups |")
        );
        assert!(envelope.body.contains("<details open>"));
        assert!(envelope.body.contains("<summary>Top fixes</summary>"));
        assert!(envelope.body.contains("Extract the repeated block."));
    }

    #[test]
    fn findings_are_capped_and_mark_envelope_truncated() {
        let findings = [
            PrSummaryFinding {
                severity: "minor".to_owned(),
                rule_id: "fallow/a".to_owned(),
                location: "src/a.ts:1".to_owned(),
                description: "A".to_owned(),
                fix: None,
            },
            PrSummaryFinding {
                severity: "minor".to_owned(),
                rule_id: "fallow/b".to_owned(),
                location: "src/b.ts:1".to_owned(),
                description: "B".to_owned(),
                fix: None,
            },
        ];
        let custom = PrSummaryInput {
            max_findings: 1,
            ..input(&[], &findings)
        };

        let envelope = render_pr_summary(&custom);

        assert!(envelope.truncation.truncated);
        assert!(envelope.body.contains("showing 1 of 2"));
        assert!(envelope.body.contains("fallow/a"));
        assert!(!envelope.body.contains("fallow/b"));
    }

    #[test]
    fn details_url_is_preserved_on_the_envelope() {
        let custom = PrSummaryInput {
            details_url: Some("https://example.test/fallow"),
            ..input(&[], &[])
        };

        let envelope = render_pr_summary(&custom);

        assert_eq!(
            envelope.details_url.as_deref(),
            Some("https://example.test/fallow")
        );
    }

    #[test]
    fn gate_only_layout_skips_top_findings() {
        let areas = [PrSummaryArea {
            name: "Health".to_owned(),
            status: PrSummaryStatus::Warn,
            result: "1 finding".to_owned(),
            threshold: Some("configured rules".to_owned()),
            details: None,
        }];
        let findings = [PrSummaryFinding {
            severity: "minor".to_owned(),
            rule_id: "fallow/high-crap-score".to_owned(),
            location: "src/a.ts:10".to_owned(),
            description: "High CRAP score".to_owned(),
            fix: None,
        }];
        let custom = PrSummaryInput {
            layout: PrCommentLayout::GateOnly,
            ..input(&areas, &findings)
        };

        let envelope = render_pr_summary(&custom);

        assert!(envelope.body.contains("## Checks"));
        assert!(!envelope.body.contains("Top fixes"));
        assert!(!envelope.body.contains("High CRAP score"));
    }

    #[test]
    fn compact_layout_renders_failed_or_warning_gates_only() {
        let areas = [
            PrSummaryArea {
                name: "Dead code".to_owned(),
                status: PrSummaryStatus::Pass,
                result: "0 issues".to_owned(),
                threshold: None,
                details: None,
            },
            PrSummaryArea {
                name: "Duplication".to_owned(),
                status: PrSummaryStatus::Warn,
                result: "2 clone groups".to_owned(),
                threshold: None,
                details: None,
            },
        ];
        let custom = PrSummaryInput {
            layout: PrCommentLayout::Compact,
            ..input(&areas, &[])
        };

        let envelope = render_pr_summary(&custom);

        assert!(envelope.body.contains("## Gates"));
        assert!(
            envelope
                .body
                .contains("- Duplication: warn (2 clone groups)")
        );
        assert!(!envelope.body.contains("| Dead code |"));
        assert!(!envelope.body.contains("Top fixes"));
    }
}
