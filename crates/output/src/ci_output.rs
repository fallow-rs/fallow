//! Shared CI comment output contracts for CLI and programmatic consumers.

use std::fmt::Write as _;

use crate::{
    CodeClimateIssue, CodeClimateSeverity, DiffIndex, GitHubReviewComment, GitHubReviewSide,
    GitLabReviewComment, GitLabReviewPosition, GitLabReviewPositionType, ReviewCheckConclusion,
    ReviewComment, ReviewEnvelopeEvent, ReviewEnvelopeMeta, ReviewEnvelopeOutput,
    ReviewEnvelopeSchema, ReviewEnvelopeSummary, ReviewProvider, default_marker_regex,
    default_marker_regex_flags,
};
use serde_json::Value;

/// Supported CI review providers for generated comments.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CiProvider {
    Github,
    Gitlab,
}

impl CiProvider {
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Github => "GitHub",
            Self::Gitlab => "GitLab",
        }
    }
}

/// Normalized CodeClimate issue used by CI comment renderers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CiIssue {
    pub rule_id: String,
    pub description: String,
    pub severity: String,
    pub path: String,
    pub line: u64,
    pub fingerprint: String,
}

/// Inputs for rendering a sticky PR/MR summary comment.
pub struct PrCommentRenderInput<'a> {
    pub command: &'a str,
    pub provider: CiProvider,
    pub issues: &'a [CiIssue],
    pub marker_id: String,
    pub max_comments: usize,
    pub category_for_rule: &'a dyn Fn(&str) -> &'static str,
}

/// GitLab diff refs for a review-envelope position.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReviewGitlabDiffRefs {
    pub base_sha: String,
    pub start_sha: String,
    pub head_sha: String,
}

/// Truncation signals produced while rendering a review envelope.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReviewEnvelopeTruncation {
    pub body: bool,
    pub comment_limit: bool,
}

/// Rendered review envelope plus side-channel signals for CLI telemetry.
#[derive(Debug)]
pub struct ReviewEnvelopeRenderResult {
    pub envelope: ReviewEnvelopeOutput,
    pub truncation: ReviewEnvelopeTruncation,
}

/// Inputs for rendering a GitHub/GitLab review envelope.
pub struct ReviewEnvelopeRenderInput<'a> {
    pub command: &'a str,
    pub provider: CiProvider,
    pub issues: &'a [CiIssue],
    pub diff_index: Option<&'a DiffIndex>,
    pub max_comments: usize,
    pub gitlab_diff_refs: Option<&'a ReviewGitlabDiffRefs>,
    pub include_guidance: bool,
    pub suggestion_block: &'a dyn Fn(CiProvider, &CiIssue) -> Option<String>,
    pub guidance_block: &'a dyn Fn(&CiIssue) -> Option<String>,
}

/// Marker prefix appended to every v2 review-comment body.
pub const MARKER_PREFIX_V2: &str = "<!-- fallow-fingerprint:v2: ";

/// Closing of the v2 marker, after the fingerprint string.
pub const MARKER_SUFFIX_V2: &str = " -->";

pub const MAX_COMMENT_BODY_BYTES: usize = 65_536;
const TRUNCATION_SUFFIX: &str = "\n\n<!-- fallow-truncated -->\n> Body truncated by fallow.";

#[must_use]
pub fn issues_from_codeclimate(value: &Value) -> Vec<CiIssue> {
    let mut issues = value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(issue_from_codeclimate)
        .collect::<Vec<_>>();
    sort_ci_issues(&mut issues);
    issues
}

#[must_use]
pub fn issues_from_codeclimate_issues(issues: &[CodeClimateIssue]) -> Vec<CiIssue> {
    let mut issues = issues
        .iter()
        .map(issue_from_codeclimate_issue)
        .collect::<Vec<_>>();
    sort_ci_issues(&mut issues);
    issues
}

fn issue_from_codeclimate(value: &Value) -> Option<CiIssue> {
    let path = value.pointer("/location/path")?.as_str()?.to_string();
    let line = value
        .pointer("/location/lines/begin")
        .and_then(Value::as_u64)
        .unwrap_or(1);
    Some(CiIssue {
        rule_id: value
            .get("check_name")
            .and_then(Value::as_str)
            .unwrap_or("fallow/finding")
            .to_string(),
        description: value
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("Fallow finding")
            .to_string(),
        severity: value
            .get("severity")
            .and_then(Value::as_str)
            .unwrap_or("minor")
            .to_string(),
        fingerprint: value
            .get("fingerprint")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        path,
        line,
    })
}

fn issue_from_codeclimate_issue(issue: &CodeClimateIssue) -> CiIssue {
    CiIssue {
        rule_id: issue.check_name.clone(),
        description: issue.description.clone(),
        severity: codeclimate_severity_label(issue.severity).to_owned(),
        path: issue.location.path.clone(),
        line: u64::from(issue.location.lines.begin),
        fingerprint: issue.fingerprint.clone(),
    }
}

const fn codeclimate_severity_label(severity: CodeClimateSeverity) -> &'static str {
    match severity {
        CodeClimateSeverity::Info => "info",
        CodeClimateSeverity::Minor => "minor",
        CodeClimateSeverity::Major => "major",
        CodeClimateSeverity::Critical => "critical",
        CodeClimateSeverity::Blocker => "blocker",
    }
}

fn sort_ci_issues(issues: &mut [CiIssue]) {
    issues
        .sort_by(|a, b| (&a.path, a.line, &a.fingerprint).cmp(&(&b.path, b.line, &b.fingerprint)));
}

fn fingerprint_hash(parts: &[&str]) -> String {
    crate::codeclimate_fingerprint_hash(parts)
}

#[must_use]
#[expect(clippy::expect_used, reason = "formatting into String is infallible")]
pub fn render_pr_comment(input: &PrCommentRenderInput<'_>) -> String {
    let marker = format!("<!-- fallow-id: {} -->", input.marker_id);
    let title = command_title(input.command);
    let count = input.issues.len();
    let noun = if count == 1 { "finding" } else { "findings" };

    let mut out = String::new();
    out.push_str(&marker);
    out.push('\n');
    write!(&mut out, "### Fallow {title}\n\n").expect("write to string");
    if count == 0 {
        writeln!(
            &mut out,
            "No {provider} PR/MR findings.",
            provider = input.provider.name()
        )
        .expect("write to string");
    } else {
        write!(&mut out, "Found **{count}** {noun}.\n\n").expect("write to string");
        let groups = group_by_category(input.issues, input.category_for_rule);
        if let [(_, group_issues)] = groups.as_slice() {
            render_findings_table(&mut out, group_issues, input.max_comments, "Details");
        } else {
            for (category, group_issues) in &groups {
                let summary_label = summary_label(category, group_issues.len(), input.max_comments);
                render_findings_table(&mut out, group_issues, input.max_comments, &summary_label);
            }
        }
    }
    out.push_str("\nGenerated by fallow.");
    out
}

/// Rule ids whose findings describe project-wide config state rather than a
/// change touching a specific source line.
pub const PROJECT_LEVEL_RULE_IDS: &[&str] = &[
    "fallow/unused-catalog-entry",
    "fallow/empty-catalog-group",
    "fallow/unresolved-catalog-reference",
    "fallow/unused-dependency-override",
    "fallow/misconfigured-dependency-override",
    "fallow/unused-dependency",
    "fallow/unused-dev-dependency",
    "fallow/unused-optional-dependency",
    "fallow/type-only-dependency",
    "fallow/test-only-dependency",
    "fallow/dev-dependency-in-production",
];

#[must_use]
pub fn is_project_level_rule(rule_id: &str) -> bool {
    PROJECT_LEVEL_RULE_IDS.contains(&rule_id)
}

const CATEGORY_ORDER: [&str; 6] = [
    "Dead code",
    "Dependencies",
    "Duplication",
    "Health",
    "Architecture",
    "Suppressions",
];

fn group_by_category<'a>(
    issues: &'a [CiIssue],
    category_for_rule: &dyn Fn(&str) -> &'static str,
) -> Vec<(&'static str, Vec<&'a CiIssue>)> {
    let mut buckets: std::collections::BTreeMap<&'static str, Vec<&CiIssue>> =
        std::collections::BTreeMap::new();
    for issue in issues {
        let category = category_for_rule(&issue.rule_id);
        buckets.entry(category).or_default().push(issue);
    }
    let mut ordered: Vec<(&'static str, Vec<&CiIssue>)> = Vec::with_capacity(buckets.len());
    for category in CATEGORY_ORDER {
        if let Some(items) = buckets.remove(category) {
            ordered.push((category, items));
        }
    }
    for (category, items) in buckets {
        ordered.push((category, items));
    }
    ordered
}

#[must_use]
pub fn summary_label(category: &str, total: usize, max: usize) -> String {
    if total > max {
        format!("{category} ({total}, showing {max})")
    } else {
        format!("{category} ({total})")
    }
}

#[expect(clippy::expect_used, reason = "formatting into String is infallible")]
fn render_findings_table(out: &mut String, issues: &[&CiIssue], max: usize, summary: &str) {
    writeln!(out, "<details>\n<summary>{summary}</summary>\n").expect("write to string");
    out.push_str("| Severity | Rule | Location | Description |\n");
    out.push_str("| --- | --- | --- | --- |\n");
    for issue in issues.iter().take(max) {
        writeln!(
            out,
            "| {} | `{}` | `{}`:{} | {} |",
            escape_md(&issue.severity),
            escape_md(&issue.rule_id),
            escape_md(&issue.path),
            issue.line,
            escape_md(&issue.description),
        )
        .expect("write to string");
    }
    if issues.len() > max {
        writeln!(
            out,
            "\nShowing {max} of {} findings. Run fallow locally or inspect the CI output for the full report.",
            issues.len(),
        )
        .expect("write to string");
    }
    out.push_str("\n</details>\n\n");
}

#[must_use]
pub fn command_title(command: &str) -> &'static str {
    match command {
        "dead-code" | "check" => "dead-code report",
        "dupes" => "duplication report",
        "health" => "health report",
        "audit" => "audit report",
        "" | "combined" => "combined report",
        _ => "report",
    }
}

/// Escape a string for inclusion in a Markdown table cell.
#[must_use]
pub fn escape_md(value: &str) -> String {
    let collapsed = value.replace('\n', " ");
    let mut out = String::with_capacity(collapsed.len());
    for ch in collapsed.chars() {
        if matches!(
            ch,
            '\\' | '`'
                | '*'
                | '_'
                | '['
                | ']'
                | '('
                | ')'
                | '!'
                | '<'
                | '>'
                | '#'
                | '|'
                | '~'
                | '&'
        ) {
            out.push('\\');
        }
        out.push(ch);
    }
    out.trim().to_owned()
}

/// Render a provider-specific review envelope from typed CI issues.
#[must_use]
pub fn render_review_envelope(input: &ReviewEnvelopeRenderInput<'_>) -> ReviewEnvelopeRenderResult {
    let grouped = group_review_issues_by_path_line(input.issues, input.max_comments);

    let comments: Vec<ReviewComment> = grouped
        .groups
        .iter()
        .map(|group| {
            render_review_comment_for_group(&ReviewCommentRenderInput {
                provider: input.provider,
                group,
                gitlab_diff_refs: input.gitlab_diff_refs,
                diff_index: input.diff_index,
                include_guidance: input.include_guidance,
                suggestion_block: input.suggestion_block,
                guidance_block: input.guidance_block,
            })
        })
        .collect();

    let summary_text =
        review_summary_text(input.command, input.provider, comments.len(), input.issues);
    let summary_fp = summary_fingerprint(&summary_text);
    let summary_marker = format!("\n\n{MARKER_PREFIX_V2}{summary_fp}{MARKER_SUFFIX_V2}");
    let body = format!("{summary_text}{summary_marker}");
    let summary = ReviewEnvelopeSummary {
        body: body.clone(),
        fingerprint: summary_fp,
    };

    let truncation = ReviewEnvelopeTruncation {
        body: comments.iter().any(review_comment_truncated),
        comment_limit: grouped.truncated,
    };

    ReviewEnvelopeRenderResult {
        envelope: build_review_envelope_output(
            input.provider,
            body,
            summary,
            comments,
            input.issues,
        ),
        truncation,
    }
}

fn review_summary_text(
    command: &str,
    provider: CiProvider,
    comment_count: usize,
    issues: &[CiIssue],
) -> String {
    let verdict = review_summary_verdict(issues);
    format!(
        "### Fallow {}\n\n**{}**\n\n{} inline finding{} selected for {} review.\n\n<!-- fallow-review -->",
        command_title(command),
        verdict,
        comment_count,
        if comment_count == 1 { "" } else { "s" },
        provider.name(),
    )
}

fn review_summary_verdict(issues: &[CiIssue]) -> &'static str {
    match github_check_conclusion(issues) {
        ReviewCheckConclusion::Failure => "Quality gate failed",
        ReviewCheckConclusion::Neutral => "Review needed",
        ReviewCheckConclusion::Success => "Quality gate passed",
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct GroupedReviewIssues<'a> {
    pub groups: Vec<Vec<&'a CiIssue>>,
    pub truncated: bool,
}

/// Group consecutive same-(path, line) issues. Input is already sorted by
/// `(path, line, fingerprint)` so a single linear pass collects runs.
#[must_use]
pub fn group_review_issues_by_path_line(
    issues: &[CiIssue],
    max_groups: usize,
) -> GroupedReviewIssues<'_> {
    if max_groups == 0 {
        return GroupedReviewIssues {
            groups: Vec::new(),
            truncated: !issues.is_empty(),
        };
    }
    let mut groups: Vec<Vec<&CiIssue>> = Vec::with_capacity(max_groups.min(issues.len()));
    let mut current: Vec<&CiIssue> = Vec::new();
    let mut current_key: Option<(&str, u64)> = None;
    for issue in issues {
        let key = (issue.path.as_str(), issue.line);
        if Some(key) != current_key {
            if !current.is_empty() {
                groups.push(std::mem::take(&mut current));
                if groups.len() == max_groups {
                    return GroupedReviewIssues {
                        groups,
                        truncated: true,
                    };
                }
            }
            current_key = Some(key);
        }
        current.push(issue);
    }
    if !current.is_empty() && groups.len() < max_groups {
        groups.push(current);
    }
    GroupedReviewIssues {
        groups,
        truncated: false,
    }
}

fn review_comment_truncated(comment: &ReviewComment) -> bool {
    match comment {
        ReviewComment::GitHub(comment) => comment.truncated,
        ReviewComment::GitLab(comment) => comment.truncated,
    }
}

pub struct ReviewCommentRenderInput<'a, 'group> {
    pub provider: CiProvider,
    pub group: &'a [&'group CiIssue],
    pub gitlab_diff_refs: Option<&'a ReviewGitlabDiffRefs>,
    pub diff_index: Option<&'a DiffIndex>,
    pub include_guidance: bool,
    pub suggestion_block: &'a dyn Fn(CiProvider, &CiIssue) -> Option<String>,
    pub guidance_block: &'a dyn Fn(&CiIssue) -> Option<String>,
}

/// Render one comment from a group of issues sharing the same `(path, line)`.
#[must_use]
pub fn render_review_comment_for_group(input: &ReviewCommentRenderInput<'_, '_>) -> ReviewComment {
    assert!(
        !input.group.is_empty(),
        "group_review_issues_by_path_line never yields empty"
    );
    let representative = input.group[0];
    let fingerprint = if input.group.len() == 1 {
        representative.fingerprint.clone()
    } else {
        let constituents: Vec<&str> = input.group.iter().map(|i| i.fingerprint.as_str()).collect();
        composite_fingerprint(&constituents)
    };

    let content = build_merged_comment_content(input);
    let marker_line = format!("\n\n{MARKER_PREFIX_V2}{fingerprint}{MARKER_SUFFIX_V2}");
    let (body, truncated) = cap_body_with_marker(&content, &marker_line);

    build_review_comment(ReviewCommentInput {
        provider: input.provider,
        representative,
        gitlab_diff_refs: input.gitlab_diff_refs,
        diff_index: input.diff_index,
        body,
        fingerprint,
        truncated,
    })
}

#[expect(clippy::expect_used, reason = "formatting into String is infallible")]
fn build_merged_comment_content(input: &ReviewCommentRenderInput<'_, '_>) -> String {
    let mut content = String::new();
    for (index, issue) in input.group.iter().enumerate() {
        let label = review_label_from_codeclimate(&issue.severity);
        if index > 0 {
            content.push_str("\n\n");
        }
        write!(
            content,
            "**{}** `{}`: {}",
            label,
            escape_md(&issue.rule_id),
            escape_md(&issue.description)
        )
        .expect("write to String is infallible");
        if let Some(suggestion) = (input.suggestion_block)(input.provider, issue) {
            content.push_str(&suggestion);
        }
        if input.include_guidance
            && let Some(guidance) = (input.guidance_block)(issue)
        {
            content.push_str(&guidance);
        }
    }
    content
}

struct ReviewCommentInput<'a> {
    provider: CiProvider,
    representative: &'a CiIssue,
    gitlab_diff_refs: Option<&'a ReviewGitlabDiffRefs>,
    diff_index: Option<&'a DiffIndex>,
    body: String,
    fingerprint: String,
    truncated: bool,
}

fn build_review_comment(input: ReviewCommentInput<'_>) -> ReviewComment {
    let ReviewCommentInput {
        provider,
        representative,
        gitlab_diff_refs,
        diff_index,
        body,
        fingerprint,
        truncated,
    } = input;
    match provider {
        CiProvider::Github => ReviewComment::GitHub(GitHubReviewComment {
            path: representative.path.clone(),
            line: u32::try_from(representative.line).unwrap_or(u32::MAX),
            side: GitHubReviewSide::Right,
            body,
            fingerprint,
            truncated,
        }),
        CiProvider::Gitlab => {
            let new_path = representative.path.clone();
            let old_path = diff_index
                .and_then(|di| di.old_path_for(&new_path))
                .map_or_else(|| new_path.clone(), str::to_owned);
            let position = GitLabReviewPosition {
                base_sha: gitlab_diff_refs.map(|r| r.base_sha.clone()),
                start_sha: gitlab_diff_refs.map(|r| r.start_sha.clone()),
                head_sha: gitlab_diff_refs.map(|r| r.head_sha.clone()),
                position_type: GitLabReviewPositionType::Text,
                old_path,
                new_path,
                new_line: u32::try_from(representative.line).unwrap_or(u32::MAX),
            };
            ReviewComment::GitLab(GitLabReviewComment {
                body,
                position,
                fingerprint,
                truncated,
            })
        }
    }
}

#[must_use]
pub fn cap_body_with_marker(content: &str, marker_line: &str) -> (String, bool) {
    let intact_len = content.len() + marker_line.len();
    if intact_len <= MAX_COMMENT_BODY_BYTES {
        let mut out = String::with_capacity(intact_len);
        out.push_str(content);
        out.push_str(marker_line);
        return (out, false);
    }
    let reserved = marker_line.len() + TRUNCATION_SUFFIX.len();
    let budget = MAX_COMMENT_BODY_BYTES.saturating_sub(reserved);
    let mut cut = budget.min(content.len());
    while cut > 0 && !content.is_char_boundary(cut) {
        cut -= 1;
    }
    let mut out = String::with_capacity(MAX_COMMENT_BODY_BYTES);
    out.push_str(&content[..cut]);
    out.push_str(TRUNCATION_SUFFIX);
    out.push_str(marker_line);
    (out, true)
}

#[must_use]
pub const fn review_label_from_codeclimate(severity_name: &str) -> &'static str {
    match severity_name.as_bytes() {
        b"major" | b"critical" | b"blocker" => "error",
        _ => "warn",
    }
}

#[must_use]
pub fn github_check_conclusion(issues: &[CiIssue]) -> ReviewCheckConclusion {
    if issues
        .iter()
        .any(|issue| matches!(issue.severity.as_str(), "major" | "critical" | "blocker"))
    {
        ReviewCheckConclusion::Failure
    } else if issues.is_empty() {
        ReviewCheckConclusion::Success
    } else {
        ReviewCheckConclusion::Neutral
    }
}

fn build_review_envelope_output(
    provider: CiProvider,
    body: String,
    summary: ReviewEnvelopeSummary,
    comments: Vec<ReviewComment>,
    issues: &[CiIssue],
) -> ReviewEnvelopeOutput {
    match provider {
        CiProvider::Github => ReviewEnvelopeOutput {
            event: Some(ReviewEnvelopeEvent::Comment),
            body,
            summary,
            comments,
            marker_regex: default_marker_regex(),
            marker_regex_flags: default_marker_regex_flags(),
            meta: ReviewEnvelopeMeta {
                schema: ReviewEnvelopeSchema::V2,
                provider: ReviewProvider::Github,
                check_conclusion: Some(github_check_conclusion(issues)),
            },
        },
        CiProvider::Gitlab => ReviewEnvelopeOutput {
            event: None,
            body,
            summary,
            comments,
            marker_regex: default_marker_regex(),
            marker_regex_flags: default_marker_regex_flags(),
            meta: ReviewEnvelopeMeta {
                schema: ReviewEnvelopeSchema::V2,
                provider: ReviewProvider::Gitlab,
                check_conclusion: None,
            },
        },
    }
}

#[must_use]
pub fn summary_fingerprint(body: &str) -> String {
    fingerprint_hash(&[body])
}

#[must_use]
pub fn composite_fingerprint(constituents: &[&str]) -> String {
    let mut sorted: Vec<&str> = constituents.to_vec();
    sorted.sort_unstable();
    let joined = sorted.join(":");
    format!("merged:{}", fingerprint_hash(&[joined.as_str()]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CodeClimateIssueKind, CodeClimateLines, CodeClimateLocation};

    fn category_for_rule(rule_id: &str) -> &'static str {
        match rule_id {
            "fallow/code-duplication" => "Duplication",
            "fallow/high-complexity" => "Health",
            "fallow/unused-dependency" => "Dependencies",
            _ => "Dead code",
        }
    }

    #[test]
    fn extracts_issues_from_codeclimate() {
        let value = serde_json::json!([{
            "check_name": "fallow/unused-export",
            "description": "Export x is never imported",
            "severity": "minor",
            "fingerprint": "abc",
            "location": { "path": "src/a.ts", "lines": { "begin": 7 } }
        }]);
        let issues = issues_from_codeclimate(&value);
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].path, "src/a.ts");
        assert_eq!(issues[0].line, 7);
    }

    #[test]
    fn typed_codeclimate_issues_extract_like_json_codeclimate() {
        let severities = [
            (CodeClimateSeverity::Info, "info"),
            (CodeClimateSeverity::Minor, "minor"),
            (CodeClimateSeverity::Major, "major"),
            (CodeClimateSeverity::Critical, "critical"),
            (CodeClimateSeverity::Blocker, "blocker"),
        ];
        let typed = severities
            .iter()
            .enumerate()
            .map(|(index, (severity, _))| CodeClimateIssue {
                kind: CodeClimateIssueKind::Issue,
                check_name: format!("fallow/rule-{index}"),
                description: format!("Finding {index}"),
                categories: vec!["Complexity".to_owned()],
                severity: *severity,
                fingerprint: format!("fp-{index}"),
                location: CodeClimateLocation {
                    path: format!("src/{index}.ts"),
                    lines: CodeClimateLines {
                        begin: u32::try_from(index + 1).expect("small fixture index"),
                    },
                },
                owner: None,
                group: None,
            })
            .collect::<Vec<_>>();
        let value = serde_json::to_value(&typed).expect("typed fixture serializes");

        assert_eq!(
            issues_from_codeclimate_issues(&typed),
            issues_from_codeclimate(&value)
        );
        let typed_labels = issues_from_codeclimate_issues(&typed)
            .into_iter()
            .map(|issue| issue.severity)
            .collect::<Vec<_>>();
        let expected_labels = severities
            .iter()
            .map(|(_, label)| (*label).to_owned())
            .collect::<Vec<_>>();
        assert_eq!(typed_labels, expected_labels);
    }

    #[test]
    fn renders_default_empty_comment() {
        let body = render_pr_comment(&PrCommentRenderInput {
            command: "check",
            provider: CiProvider::Github,
            issues: &[],
            marker_id: "fallow-results".to_owned(),
            max_comments: 50,
            category_for_rule: &category_for_rule,
        });
        assert!(body.contains("<!-- fallow-id: fallow-results"));
        assert!(body.contains("No GitHub PR/MR findings."));
    }

    #[test]
    fn escape_md_escapes_inline_commonmark_specials() {
        let raw = "foo*bar_baz [a](u) `c` <h> #x !i ~s | p";
        let escaped = escape_md(raw);
        for ch in [
            '*', '_', '[', ']', '(', ')', '`', '<', '>', '#', '!', '~', '|',
        ] {
            let raw_count = raw.chars().filter(|c| c == &ch).count();
            let escaped_count = escaped.matches(&format!("\\{ch}")).count();
            assert_eq!(
                raw_count, escaped_count,
                "char {ch:?}: raw {raw_count} occurrences, escaped {escaped_count} in {escaped:?}"
            );
        }
    }

    #[test]
    fn escape_md_escapes_ampersand_to_block_numeric_entity_bypass() {
        let raw = "value &#42;suspicious&#42; here";
        let escaped = escape_md(raw);
        assert!(escaped.contains(r"\&"), "got: {escaped}");
        assert!(escaped.contains(r"\#"), "got: {escaped}");
        assert!(!escaped.contains(" *suspicious"), "got: {escaped}");
    }

    #[test]
    fn summary_label_foreshadows_truncation() {
        assert_eq!(
            summary_label("Duplication", 160, 50),
            "Duplication (160, showing 50)"
        );
        assert_eq!(summary_label("Health", 12, 50), "Health (12)");
        assert_eq!(summary_label("Dependencies", 50, 50), "Dependencies (50)");
    }

    #[test]
    fn escape_md_does_not_escape_block_only_markers() {
        let raw = "fallow/test-only-dependency package.json:12";
        let escaped = escape_md(raw);
        assert!(!escaped.contains("\\-"), "should not escape `-`");
        assert!(!escaped.contains("\\."), "should not escape `.`");
        assert_eq!(escaped, raw);
    }

    #[test]
    fn escape_md_collapses_newlines_to_spaces() {
        let raw = "first\nsecond\nthird";
        assert_eq!(escape_md(raw), "first second third");
    }

    #[test]
    fn escape_md_leaves_safe_chars_unchanged() {
        let raw = "Export 'helperFn' is never imported by other modules";
        assert_eq!(
            escape_md(raw),
            r"Export 'helperFn' is never imported by other modules"
        );
    }

    #[test]
    fn is_project_level_rule_covers_config_anchored_dependency_findings() {
        for rule_id in PROJECT_LEVEL_RULE_IDS {
            assert!(
                is_project_level_rule(rule_id),
                "{rule_id} must be project-level"
            );
        }
        for rule_id in [
            "fallow/unused-file",
            "fallow/unused-export",
            "fallow/unused-type",
            "fallow/unused-enum-member",
            "fallow/unused-class-member",
            "fallow/unused-store-member",
            "fallow/unresolved-import",
            "fallow/unlisted-dependency",
            "fallow/duplicate-export",
            "fallow/circular-dependency",
            "fallow/re-export-cycle",
            "fallow/boundary-violation",
            "fallow/stale-suppression",
            "fallow/private-type-leak",
            "fallow/high-complexity",
            "fallow/high-crap-score",
        ] {
            assert!(
                !is_project_level_rule(rule_id),
                "{rule_id} must NOT be project-level"
            );
        }
    }

    #[test]
    fn escape_md_double_apply_is_safe() {
        let raw = "code with `backticks` and *stars*";
        let once = escape_md(raw);
        let twice = escape_md(&once);
        assert!(twice.contains(r"\\"));
    }
}
