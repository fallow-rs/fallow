use std::process::ExitCode;

use serde_json::Value;

use super::diff_filter::DiffIndex;
use super::fingerprint::{composite_fingerprint, summary_fingerprint};
use super::pr_comment::{
    CiIssue, Provider, command_title, escape_md, issues_from_codeclimate_issues,
};
use super::severity;
use crate::output_envelope::{
    CodeClimateIssue, GitHubReviewComment, GitHubReviewSide, GitLabReviewComment,
    GitLabReviewPosition, GitLabReviewPositionType, ReviewCheckConclusion, ReviewComment,
    ReviewEnvelopeEvent, ReviewEnvelopeMeta, ReviewEnvelopeOutput, ReviewEnvelopeSchema,
    ReviewEnvelopeSummary, ReviewProvider, default_marker_regex, default_marker_regex_flags,
};
use crate::report::emit_json;

/// Conservative body-size floor across the two supported review providers.
/// GitLab accepts ~1,000,000 chars per `Note#note` validation (see
/// <https://docs.gitlab.com/administration/instance_limits/>) and GitHub
/// empirically enforces a 65,536-character cap on PR review comments
/// (undocumented but reproducible: a 65,537-char body returns
/// `Body is too long (maximum is 65536 characters)`). We pick 65,536 BYTES
/// here so the cap is safe under either vendor regardless of whether the
/// limit is enforced in bytes or chars, and regardless of multi-byte UTF-8
/// expansion. Hardcoded for now; if a real consumer needs it tunable, expose
/// a `FALLOW_REVIEW_MAX_BODY_BYTES` env var.
const MAX_COMMENT_BODY_BYTES: usize = 65_536;

/// Marker prefix appended to every v2 review-comment body. Mirrored by
/// [`crate::output_envelope::MARKER_REGEX_V2`]; both must change together
/// because consumers extract the fingerprint by running the regex over a
/// body whose marker line uses this prefix. The `:v2:` namespace prevents
/// collision with v1 historical markers and reduces user-paste spoofing
/// risk (typing `:v2:` by accident is unlikely).
pub const MARKER_PREFIX_V2: &str = "<!-- fallow-fingerprint:v2: ";

/// Closing of the v2 marker, after the fingerprint string.
const MARKER_SUFFIX_V2: &str = " -->";

/// Human-readable truncation breadcrumb appended to the body when the
/// rendered content exceeds [`MAX_COMMENT_BODY_BYTES`]. The HTML comment is
/// machine-detectable; the blockquote that follows is a human-readable
/// breadcrumb that reads as fallow speaking (matching the existing
/// `> Run \`fallow fix --files\` or delete this file.` convention from the
/// unused-file suggestion block). Three signals total (typed
/// `truncated: bool` on the comment, this HTML marker, and the blockquote
/// text) so consumers don't need to choose a primary detection channel.
const TRUNCATION_SUFFIX: &str = "\n\n<!-- fallow-truncated -->\n> Body truncated by fallow.";

#[must_use]
pub fn render_review_envelope(
    command: &str,
    provider: Provider,
    issues: &[CiIssue],
) -> ReviewEnvelopeOutput {
    render_review_envelope_with_diff(
        command,
        provider,
        issues,
        super::diff_filter::shared_diff_index(),
    )
}

/// Render path the print site uses. Exposed so unit tests can pass a
/// hand-crafted `DiffIndex` without poking the process-wide `SHARED_DIFF`
/// cache (which is `OnceLock`-bounded and not reentrant under cargo test's
/// parallel runner).
#[must_use]
pub fn render_review_envelope_with_diff(
    command: &str,
    provider: Provider,
    issues: &[CiIssue],
    diff_index: Option<&DiffIndex>,
) -> ReviewEnvelopeOutput {
    let max = std::env::var("FALLOW_MAX_COMMENTS")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(50);
    let gitlab_diff_refs = (provider == Provider::Gitlab)
        .then(gitlab_diff_refs_from_env)
        .flatten();
    let include_guidance = review_guidance_enabled();

    let merged_groups = group_by_path_line(issues, max);

    let comments: Vec<ReviewComment> = merged_groups
        .iter()
        .map(|group| {
            render_merged_comment(
                provider,
                group,
                gitlab_diff_refs.as_ref(),
                diff_index,
                include_guidance,
            )
        })
        .collect();

    let summary_text = format!(
        "### Fallow {}\n\n{} inline finding{} selected for {} review.\n\n<!-- fallow-review -->",
        command_title(command),
        comments.len(),
        if comments.len() == 1 { "" } else { "s" },
        provider.name(),
    );
    let summary_fp = summary_fingerprint(&summary_text);
    let summary_marker = format!("\n\n{MARKER_PREFIX_V2}{summary_fp}{MARKER_SUFFIX_V2}");
    let body = format!("{summary_text}{summary_marker}");
    let summary = ReviewEnvelopeSummary {
        body: body.clone(),
        fingerprint: summary_fp,
    };

    match provider {
        Provider::Github => ReviewEnvelopeOutput {
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
        Provider::Gitlab => ReviewEnvelopeOutput {
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
pub fn print_review_envelope(command: &str, provider: Provider, codeclimate: &Value) -> ExitCode {
    let issues = super::diff_filter::filter_issues_from_env(
        super::pr_comment::issues_from_codeclimate(codeclimate),
    );
    print_review_envelope_from_ci_issues(command, provider, &issues)
}

#[must_use]
pub fn print_review_envelope_from_codeclimate_issues(
    command: &str,
    provider: Provider,
    codeclimate: &[CodeClimateIssue],
) -> ExitCode {
    let issues =
        super::diff_filter::filter_issues_from_env(issues_from_codeclimate_issues(codeclimate));
    print_review_envelope_from_ci_issues(command, provider, &issues)
}

#[must_use]
#[expect(
    clippy::expect_used,
    reason = "review envelope contains only infallibly serializable fields"
)]
fn print_review_envelope_from_ci_issues(
    command: &str,
    provider: Provider,
    issues: &[CiIssue],
) -> ExitCode {
    let envelope = render_review_envelope(command, provider, issues);
    let value = crate::output_envelope::serialize_root_output(
        crate::output_envelope::FallowOutput::ReviewEnvelope(envelope),
    )
    .expect("ReviewEnvelopeOutput serializes infallibly");
    emit_json(&value, "review envelope")
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[expect(
    clippy::struct_field_names,
    reason = "GitLab API names these diff refs base_sha/start_sha/head_sha"
)]
struct GitlabDiffRefs {
    base_sha: String,
    start_sha: String,
    head_sha: String,
}

fn gitlab_diff_refs_from_env() -> Option<GitlabDiffRefs> {
    let base_sha = env_nonempty("FALLOW_GITLAB_BASE_SHA")
        .or_else(|| env_nonempty("CI_MERGE_REQUEST_DIFF_BASE_SHA"))?;
    let start_sha = env_nonempty("FALLOW_GITLAB_START_SHA").unwrap_or_else(|| base_sha.clone());
    let head_sha =
        env_nonempty("FALLOW_GITLAB_HEAD_SHA").or_else(|| env_nonempty("CI_COMMIT_SHA"))?;
    Some(GitlabDiffRefs {
        base_sha,
        start_sha,
        head_sha,
    })
}

fn env_nonempty(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
}

fn review_guidance_enabled() -> bool {
    std::env::var("FALLOW_REVIEW_GUIDANCE").is_ok_and(|value| env_truthy(&value))
}

fn env_truthy(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

/// Group consecutive same-(path, line) issues. Input is already sorted by
/// `(path, line, fingerprint)` so a single linear pass collects runs.
fn group_by_path_line(issues: &[CiIssue], max_groups: usize) -> Vec<Vec<&CiIssue>> {
    if max_groups == 0 {
        return Vec::new();
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
                    return groups;
                }
            }
            current_key = Some(key);
        }
        current.push(issue);
    }
    if !current.is_empty() && groups.len() < max_groups {
        groups.push(current);
    }
    groups
}

/// Render one comment from a group of 1+ issues that share the same
/// `(path, line)`. Single-element groups produce the v1-shaped body
/// (modulo the `:v2:` marker shape); multi-element groups stack each
/// finding's `**label** \`rule\`: desc` paragraph under a
/// `merged:<16-char hash>` composite fingerprint over sorted constituent
/// fingerprints. The composite identity shifts whenever the set of
/// constituents changes, so consumers' skip-if-fingerprint-exists logic
/// correctly re-posts on content change.
#[expect(clippy::expect_used, reason = "formatting into String is infallible")]
fn render_merged_comment(
    provider: Provider,
    group: &[&CiIssue],
    gitlab_diff_refs: Option<&GitlabDiffRefs>,
    diff_index: Option<&DiffIndex>,
    include_guidance: bool,
) -> ReviewComment {
    assert!(!group.is_empty(), "group_by_path_line never yields empty");
    let representative = group[0];
    let fingerprint = if group.len() == 1 {
        representative.fingerprint.clone()
    } else {
        let constituents: Vec<&str> = group.iter().map(|i| i.fingerprint.as_str()).collect();
        composite_fingerprint(&constituents)
    };

    use std::fmt::Write as _;
    let mut content = String::new();
    for (index, issue) in group.iter().enumerate() {
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
        if let Some(suggestion) = super::suggestion::suggestion_block(provider, issue) {
            content.push_str(&suggestion);
        }
        if include_guidance && let Some(guidance) = review_guidance_block(issue) {
            content.push_str(&guidance);
        }
    }

    let marker_line = format!("\n\n{MARKER_PREFIX_V2}{fingerprint}{MARKER_SUFFIX_V2}");
    let (body, truncated) = cap_body_with_marker(&content, &marker_line);

    match provider {
        Provider::Github => ReviewComment::GitHub(GitHubReviewComment {
            path: representative.path.clone(),
            line: u32::try_from(representative.line).unwrap_or(u32::MAX),
            side: GitHubReviewSide::Right,
            body,
            fingerprint,
            truncated,
        }),
        Provider::Gitlab => {
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

fn review_guidance_block(issue: &CiIssue) -> Option<String> {
    let rule = crate::explain::rule_by_id(&issue.rule_id)?;
    let guide = crate::explain::rule_guide(rule);
    let docs_url = crate::explain::rule_docs_url(rule);

    Some(format!(
        "\n\n<details><summary>What to do</summary>\n\n{}\n\n[Read the rule docs]({docs_url})\n\n</details>",
        guide.how_to_fix
    ))
}

/// Truncate `content` if appending `marker_line` would exceed
/// [`MAX_COMMENT_BODY_BYTES`], preserving the marker at the tail and
/// inserting a [`TRUNCATION_SUFFIX`] breadcrumb. Truncation walks back to
/// the nearest UTF-8 char boundary so multi-byte characters straddling the
/// cut are not chopped mid-codepoint. Returns `(final_body, truncated)`.
fn cap_body_with_marker(content: &str, marker_line: &str) -> (String, bool) {
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

fn review_label_from_codeclimate(severity_name: &str) -> &'static str {
    match severity_name {
        "major" | "critical" | "blocker" => severity::review_label(fallow_config::Severity::Error),
        _ => severity::review_label(fallow_config::Severity::Warn),
    }
}

fn github_check_conclusion(issues: &[CiIssue]) -> ReviewCheckConclusion {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output_envelope::MARKER_REGEX_V2;

    fn to_value(envelope: &ReviewEnvelopeOutput) -> Value {
        serde_json::to_value(envelope).expect("ReviewEnvelopeOutput serializes infallibly")
    }

    fn comment_to_value(comment: &ReviewComment) -> Value {
        serde_json::to_value(comment).expect("ReviewComment serializes infallibly")
    }

    fn issue(rule: &str, sev: &str, path: &str, line: u64, fp: &str) -> CiIssue {
        CiIssue {
            rule_id: rule.into(),
            description: "desc".into(),
            severity: sev.into(),
            path: path.into(),
            line,
            fingerprint: fp.into(),
        }
    }

    fn issue_with_desc(
        rule: &str,
        desc: impl Into<String>,
        sev: &str,
        path: &str,
        line: u64,
        fp: &str,
    ) -> CiIssue {
        CiIssue {
            rule_id: rule.into(),
            description: desc.into(),
            severity: sev.into(),
            path: path.into(),
            line,
            fingerprint: fp.into(),
        }
    }

    #[test]
    fn github_review_envelope_matches_api_shape() {
        let issues = vec![issue(
            "fallow/unused-file",
            "minor",
            "src/a.ts",
            1,
            "abc1234567890def",
        )];
        let envelope = to_value(&render_review_envelope("check", Provider::Github, &issues));
        assert_eq!(envelope["event"], "COMMENT");
        assert_eq!(envelope["meta"]["schema"], "fallow-review-envelope/v2");
        assert_eq!(envelope["comments"][0]["path"], "src/a.ts");
        assert!(
            envelope["comments"][0]["body"]
                .as_str()
                .unwrap()
                .contains("fallow-fingerprint:v2:")
        );
    }

    #[test]
    fn github_comments_target_current_state_side() {
        let issue = issue("fallow/unused-file", "minor", "src/a.ts", 1, "abc");
        let comment = comment_to_value(&render_merged_comment(
            Provider::Github,
            &[&issue],
            None,
            None,
            false,
        ));
        assert_eq!(comment["side"], "RIGHT");
    }

    #[test]
    fn labels_major_issues_as_errors() {
        let issue = issue("fallow/unused-file", "major", "src/a.ts", 1, "abc");
        let comment = comment_to_value(&render_merged_comment(
            Provider::Github,
            &[&issue],
            None,
            None,
            false,
        ));
        assert!(comment["body"].as_str().unwrap().starts_with("**error**"));
    }

    #[test]
    fn gitlab_comment_accepts_diff_refs() {
        let issue = issue("fallow/unused-file", "minor", "src/a.ts", 1, "abc");
        let refs = GitlabDiffRefs {
            base_sha: "base".into(),
            start_sha: "start".into(),
            head_sha: "head".into(),
        };
        let comment = comment_to_value(&render_merged_comment(
            Provider::Gitlab,
            &[&issue],
            Some(&refs),
            None,
            false,
        ));
        assert_eq!(comment["position"]["position_type"], "text");
        assert_eq!(comment["position"]["base_sha"], "base");
        assert_eq!(comment["position"]["start_sha"], "start");
        assert_eq!(comment["position"]["head_sha"], "head");
    }

    #[test]
    fn guidance_toggle_accepts_common_truthy_values() {
        for value in ["1", "true", "TRUE", "yes", "on", " On "] {
            assert!(env_truthy(value), "{value:?} should enable guidance");
        }
        for value in ["", "0", "false", "no", "off", "enabled"] {
            assert!(!env_truthy(value), "{value:?} should not enable guidance");
        }
    }

    #[test]
    fn guidance_disabled_omits_details_block() {
        let issue = issue(
            "fallow/high-complexity",
            "major",
            "src/a.ts",
            10,
            "abc1234567890def",
        );
        let comment = comment_to_value(&render_merged_comment(
            Provider::Github,
            &[&issue],
            None,
            None,
            false,
        ));
        let body = comment["body"].as_str().unwrap();
        assert!(!body.contains("<details><summary>What to do</summary>"));
        assert!(!body.contains("For function findings"));
    }

    #[test]
    fn guidance_enabled_appends_rule_guide_details() {
        let issue = issue(
            "fallow/high-complexity",
            "major",
            "src/a.ts",
            10,
            "abc1234567890def",
        );
        let comment = comment_to_value(&render_merged_comment(
            Provider::Github,
            &[&issue],
            None,
            None,
            true,
        ));
        let body = comment["body"].as_str().unwrap();
        assert!(body.contains("<details><summary>What to do</summary>"));
        assert!(body.contains("For function findings"));
        assert!(body.contains("[Read the rule docs]("));
        assert!(
            body.find("</details>").unwrap() < body.find("fallow-fingerprint:v2:").unwrap(),
            "guidance should render before the marker"
        );
    }

    #[test]
    fn guidance_attaches_to_each_merged_finding() {
        let complexity = issue("fallow/high-complexity", "major", "src/foo.ts", 42, "fp_a");
        let duplication = issue("fallow/code-duplication", "minor", "src/foo.ts", 42, "fp_b");
        let comment = comment_to_value(&render_merged_comment(
            Provider::Github,
            &[&complexity, &duplication],
            None,
            None,
            true,
        ));
        let body = comment["body"].as_str().unwrap();
        assert_eq!(
            body.matches("<details><summary>What to do</summary>")
                .count(),
            2
        );
        assert!(body.contains("For function findings"));
        assert!(body.contains("Extract the shared logic"));
    }

    #[test]
    fn envelope_emits_marker_regex_field_at_root() {
        let issues = vec![issue("fallow/unused-file", "minor", "src/a.ts", 1, "abc")];
        let env = to_value(&render_review_envelope("check", Provider::Github, &issues));
        let regex = env["marker_regex"].as_str().expect("marker_regex present");
        assert_eq!(regex, MARKER_REGEX_V2);
        assert!(regex.contains("[0-9a-f]{16}"));
        assert!(regex.starts_with('^'));
        assert!(regex.ends_with("\\s*$"));
        assert!(!regex.contains("(?m)"));
        assert!(regex.contains("((?:[a-z]+:)?[0-9a-f]{16})"));
        let flags = env["marker_regex_flags"]
            .as_str()
            .expect("marker_regex_flags present");
        assert_eq!(flags, "m");
    }

    #[test]
    fn envelope_emits_summary_block_with_fingerprint() {
        let issues = vec![issue("fallow/unused-file", "minor", "src/a.ts", 1, "abc")];
        let env = to_value(&render_review_envelope("check", Provider::Github, &issues));
        assert_eq!(env["summary"]["body"], env["body"]);
        let summary_fp = env["summary"]["fingerprint"].as_str().expect("fingerprint");
        assert_eq!(summary_fp.len(), 16);
        assert!(summary_fp.chars().all(|c| c.is_ascii_hexdigit()));
        let body_str = env["body"].as_str().unwrap();
        let marker_line = format!("{MARKER_PREFIX_V2}{summary_fp}{MARKER_SUFFIX_V2}");
        assert!(
            body_str.contains(&marker_line),
            "body must carry summary marker:\nbody={body_str}\nmarker={marker_line}"
        );
    }

    #[test]
    fn same_line_findings_merge_into_one_comment_with_composite_fingerprint() {
        let a = issue("fallow/unused-export", "minor", "src/foo.ts", 42, "fp_a");
        let b = issue("fallow/duplicate-export", "minor", "src/foo.ts", 42, "fp_b");
        let env = to_value(&render_review_envelope("check", Provider::Github, &[a, b]));
        assert_eq!(
            env["comments"].as_array().unwrap().len(),
            1,
            "two same-line findings must collapse to one comment"
        );
        let merged = &env["comments"][0];
        let fp = merged["fingerprint"].as_str().unwrap();
        assert!(
            fp.starts_with("merged:"),
            "merged comment fingerprint must start with merged:, got {fp}"
        );
        assert_eq!(fp.len(), 23);
        let body = merged["body"].as_str().unwrap();
        assert!(body.contains("fallow/unused-export"));
        assert!(body.contains("fallow/duplicate-export"));
        assert_eq!(
            body.matches("fallow-fingerprint:v2:").count(),
            1,
            "merged body must carry exactly one fingerprint marker"
        );
        assert!(
            merged.get("constituent_fingerprints").is_none(),
            "v2 hashed-composite design does not emit constituent_fingerprints"
        );
    }

    #[test]
    fn group_by_path_line_respects_max_groups_without_splitting_same_line_findings() {
        let a = issue("fallow/unused-export", "minor", "src/foo.ts", 42, "fp_a");
        let b = issue("fallow/duplicate-export", "minor", "src/foo.ts", 42, "fp_b");
        let c = issue("fallow/unused-type", "minor", "src/z.ts", 7, "fp_c");
        let issues = vec![a, b, c];

        assert!(group_by_path_line(&issues, 0).is_empty());

        let max_one = group_by_path_line(&issues, 1);
        assert_eq!(max_one.len(), 1);
        assert_eq!(max_one[0].len(), 2);
        assert_eq!(max_one[0][0].path, "src/foo.ts");
        assert_eq!(max_one[0][0].line, 42);

        let max_two = group_by_path_line(&issues, 2);
        assert_eq!(max_two.len(), 2);
        assert_eq!(max_two[0].len(), 2);
        assert_eq!(max_two[1].len(), 1);
        assert_eq!(
            max_two[0]
                .iter()
                .map(|issue| issue.fingerprint.as_str())
                .collect::<Vec<_>>(),
            ["fp_a", "fp_b"]
        );
    }

    #[test]
    fn single_finding_keeps_v1_fingerprint_shape() {
        let issues = vec![issue(
            "fallow/unused-file",
            "minor",
            "src/a.ts",
            1,
            "abc1234567890def",
        )];
        let env = to_value(&render_review_envelope("check", Provider::Github, &issues));
        let comment = &env["comments"][0];
        assert_eq!(comment["fingerprint"], "abc1234567890def");
        assert!(
            comment.get("constituent_fingerprints").is_none(),
            "single-finding comment must NOT emit constituent_fingerprints"
        );
        assert!(
            comment.get("truncated").is_none(),
            "non-truncated comment must NOT emit truncated"
        );
    }

    #[test]
    fn composite_fingerprint_shifts_when_constituents_change() {
        let a = issue("fallow/unused-export", "minor", "src/foo.ts", 42, "fp_a");
        let b = issue("fallow/duplicate-export", "minor", "src/foo.ts", 42, "fp_b");
        let c = issue("fallow/unused-type", "minor", "src/foo.ts", 42, "fp_c");
        let run1 = to_value(&render_review_envelope(
            "check",
            Provider::Github,
            &[a.clone(), b, c.clone()],
        ));
        let run2_drop_b = to_value(&render_review_envelope("check", Provider::Github, &[a, c]));
        assert_ne!(
            run1["comments"][0]["fingerprint"], run2_drop_b["comments"][0]["fingerprint"],
            "primary fingerprint must shift when a constituent drops"
        );
    }

    #[test]
    fn gitlab_old_path_pulls_from_diff_rename_map() {
        let rename_diff = "\
diff --git a/src/old.ts b/src/new.ts
similarity index 90%
rename from src/old.ts
rename to src/new.ts
--- a/src/old.ts
+++ b/src/new.ts
@@ -1,2 +1,3 @@
 keep
+added
 still
";
        let diff_index = DiffIndex::from_unified_diff(rename_diff);
        let issue = issue("fallow/unused-export", "minor", "src/new.ts", 2, "abc");
        let envelope = to_value(&render_review_envelope_with_diff(
            "check",
            Provider::Gitlab,
            &[issue],
            Some(&diff_index),
        ));
        let position = &envelope["comments"][0]["position"];
        assert_eq!(position["old_path"], "src/old.ts");
        assert_eq!(position["new_path"], "src/new.ts");
    }

    #[test]
    fn gitlab_old_path_falls_back_to_new_path_without_rename() {
        let issue = issue("fallow/unused-export", "minor", "src/edit.ts", 5, "abc");
        let envelope = to_value(&render_review_envelope_with_diff(
            "check",
            Provider::Gitlab,
            &[issue],
            None,
        ));
        let position = &envelope["comments"][0]["position"];
        assert_eq!(position["old_path"], "src/edit.ts");
        assert_eq!(position["new_path"], "src/edit.ts");
    }

    #[test]
    fn oversized_body_truncates_at_char_boundary_and_preserves_marker() {
        let huge_desc = "x".repeat(MAX_COMMENT_BODY_BYTES * 2);
        let issue = CiIssue {
            rule_id: "fallow/unused-export".into(),
            description: huge_desc,
            severity: "minor".into(),
            path: "src/a.ts".into(),
            line: 1,
            fingerprint: "abc1234567890def".into(),
        };
        let comment = comment_to_value(&render_merged_comment(
            Provider::Github,
            &[&issue],
            None,
            None,
            false,
        ));
        let body = comment["body"].as_str().unwrap();
        assert!(
            body.len() <= MAX_COMMENT_BODY_BYTES,
            "body len {} must not exceed cap {MAX_COMMENT_BODY_BYTES}",
            body.len()
        );
        assert!(
            body.contains("fallow-fingerprint:v2:"),
            "marker must be preserved under truncation"
        );
        assert!(body.contains("<!-- fallow-truncated -->"));
        assert!(body.contains("> Body truncated by fallow."));
        assert_eq!(comment["truncated"], true);
        assert!(std::str::from_utf8(body.as_bytes()).is_ok());
    }

    #[test]
    fn oversized_guidance_body_truncates_and_preserves_marker() {
        let issue = issue_with_desc(
            "fallow/high-complexity",
            "x".repeat(MAX_COMMENT_BODY_BYTES * 2),
            "major",
            "src/a.ts",
            1,
            "abc1234567890def",
        );
        let comment = comment_to_value(&render_merged_comment(
            Provider::Github,
            &[&issue],
            None,
            None,
            true,
        ));
        let body = comment["body"].as_str().unwrap();
        assert!(body.len() <= MAX_COMMENT_BODY_BYTES);
        assert!(body.contains("<!-- fallow-truncated -->"));
        assert!(body.contains("fallow-fingerprint:v2:"));
        assert_eq!(comment["truncated"], true);
    }

    #[test]
    fn multibyte_body_truncates_at_char_boundary() {
        let huge_desc: String = "あ".repeat(MAX_COMMENT_BODY_BYTES);
        let issue = CiIssue {
            rule_id: "fallow/unused-export".into(),
            description: huge_desc,
            severity: "minor".into(),
            path: "src/a.ts".into(),
            line: 1,
            fingerprint: "abc1234567890def".into(),
        };
        let comment = comment_to_value(&render_merged_comment(
            Provider::Github,
            &[&issue],
            None,
            None,
            false,
        ));
        let body = comment["body"].as_str().unwrap();
        assert!(std::str::from_utf8(body.as_bytes()).is_ok());
        assert!(body.len() <= MAX_COMMENT_BODY_BYTES);
        assert_eq!(comment["truncated"], true);
    }
}
