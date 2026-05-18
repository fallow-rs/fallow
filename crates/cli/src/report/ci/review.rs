use std::process::ExitCode;

use serde_json::Value;

use super::pr_comment::{CiIssue, Provider, command_title, escape_md};
use super::severity;
use crate::output_envelope::{
    GitHubReviewComment, GitHubReviewSide, GitLabReviewComment, GitLabReviewPosition,
    GitLabReviewPositionType, ReviewCheckConclusion, ReviewComment, ReviewEnvelopeEvent,
    ReviewEnvelopeMeta, ReviewEnvelopeOutput, ReviewEnvelopeSchema, ReviewProvider,
};
use crate::report::emit_json;

#[must_use]
pub fn render_review_envelope(
    command: &str,
    provider: Provider,
    issues: &[CiIssue],
) -> ReviewEnvelopeOutput {
    let max = std::env::var("FALLOW_MAX_COMMENTS")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(50);
    let gitlab_diff_refs = (provider == Provider::Gitlab)
        .then(gitlab_diff_refs_from_env)
        .flatten();
    let body = format!(
        "### Fallow {}\n\n{} inline finding{} selected for {} review.\n\n<!-- fallow-review -->",
        command_title(command),
        issues.len().min(max),
        if issues.len().min(max) == 1 { "" } else { "s" },
        provider.name(),
    );
    let comments: Vec<ReviewComment> = issues
        .iter()
        .take(max)
        .map(|issue| render_comment(provider, issue, gitlab_diff_refs.as_ref()))
        .collect();

    match provider {
        Provider::Github => ReviewEnvelopeOutput {
            event: Some(ReviewEnvelopeEvent::Comment),
            body,
            comments,
            meta: ReviewEnvelopeMeta {
                schema: ReviewEnvelopeSchema::V1,
                provider: ReviewProvider::Github,
                check_conclusion: Some(github_check_conclusion(issues)),
            },
        },
        Provider::Gitlab => ReviewEnvelopeOutput {
            event: None,
            body,
            comments,
            meta: ReviewEnvelopeMeta {
                schema: ReviewEnvelopeSchema::V1,
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
    let envelope = render_review_envelope(command, provider, &issues);
    let value =
        serde_json::to_value(&envelope).expect("ReviewEnvelopeOutput serializes infallibly");
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

fn render_comment(
    provider: Provider,
    issue: &CiIssue,
    gitlab_diff_refs: Option<&GitlabDiffRefs>,
) -> ReviewComment {
    let label = review_label_from_codeclimate(&issue.severity);
    let mut body = format!(
        "**{}** `{}`: {}\n\n<!-- fallow-fingerprint: {} -->",
        label,
        escape_md(&issue.rule_id),
        escape_md(&issue.description),
        issue.fingerprint
    );
    if let Some(suggestion) = super::suggestion::suggestion_block(provider, issue) {
        body.push_str(&suggestion);
    }
    match provider {
        // Fallow findings point at the current file state. GitHub deletion-side
        // review comments are intentionally not modeled in this envelope yet.
        Provider::Github => ReviewComment::GitHub(GitHubReviewComment {
            path: issue.path.clone(),
            // `CiIssue.line` is `u64` for legacy reasons but every callsite
            // populates it from a `u32` line number (`begin_line: Option<u32>`
            // in `cc_issue`); the typed envelope locks the wire to `u32`.
            // Follow-up: narrow `CiIssue.line` to `u32` at construction time
            // in `pr_comment.rs::issues_from_codeclimate` so this cast goes
            // away entirely (out of scope for the #384 ladder migration).
            line: u32::try_from(issue.line).unwrap_or(u32::MAX),
            side: GitHubReviewSide::Right,
            body,
            fingerprint: issue.fingerprint.clone(),
        }),
        Provider::Gitlab => {
            let position = GitLabReviewPosition {
                base_sha: gitlab_diff_refs.map(|r| r.base_sha.clone()),
                start_sha: gitlab_diff_refs.map(|r| r.start_sha.clone()),
                head_sha: gitlab_diff_refs.map(|r| r.head_sha.clone()),
                position_type: GitLabReviewPositionType::Text,
                old_path: issue.path.clone(),
                new_path: issue.path.clone(),
                // Same `u64 -> u32` narrowing as the GitHub branch above;
                // see the follow-up note there.
                new_line: u32::try_from(issue.line).unwrap_or(u32::MAX),
            };
            ReviewComment::GitLab(GitLabReviewComment {
                body,
                position,
                fingerprint: issue.fingerprint.clone(),
            })
        }
    }
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

    fn to_value(envelope: &ReviewEnvelopeOutput) -> Value {
        serde_json::to_value(envelope).expect("ReviewEnvelopeOutput serializes infallibly")
    }

    fn comment_to_value(comment: &ReviewComment) -> Value {
        serde_json::to_value(comment).expect("ReviewComment serializes infallibly")
    }

    #[test]
    fn github_review_envelope_matches_api_shape() {
        let issues = vec![CiIssue {
            rule_id: "fallow/unused-file".into(),
            description: "File is unused".into(),
            severity: "minor".into(),
            path: "src/a.ts".into(),
            line: 1,
            fingerprint: "abc".into(),
        }];
        let envelope = to_value(&render_review_envelope("check", Provider::Github, &issues));
        assert_eq!(envelope["event"], "COMMENT");
        assert_eq!(envelope["comments"][0]["path"], "src/a.ts");
        assert!(
            envelope["comments"][0]["body"]
                .as_str()
                .unwrap()
                .contains("fallow-fingerprint")
        );
    }

    #[test]
    fn github_comments_target_current_state_side() {
        let issue = CiIssue {
            rule_id: "fallow/unused-file".into(),
            description: "File is unused".into(),
            severity: "minor".into(),
            path: "src/a.ts".into(),
            line: 1,
            fingerprint: "abc".into(),
        };
        let comment = comment_to_value(&render_comment(Provider::Github, &issue, None));
        assert_eq!(comment["side"], "RIGHT");
    }

    #[test]
    fn labels_major_issues_as_errors() {
        let issue = CiIssue {
            rule_id: "fallow/unused-file".into(),
            description: "File is unused".into(),
            severity: "major".into(),
            path: "src/a.ts".into(),
            line: 1,
            fingerprint: "abc".into(),
        };
        let comment = comment_to_value(&render_comment(Provider::Github, &issue, None));
        assert!(comment["body"].as_str().unwrap().starts_with("**error**"));
    }

    #[test]
    fn gitlab_comment_accepts_diff_refs() {
        let issue = CiIssue {
            rule_id: "fallow/unused-file".into(),
            description: "File is unused".into(),
            severity: "minor".into(),
            path: "src/a.ts".into(),
            line: 1,
            fingerprint: "abc".into(),
        };
        let refs = GitlabDiffRefs {
            base_sha: "base".into(),
            start_sha: "start".into(),
            head_sha: "head".into(),
        };
        let comment = comment_to_value(&render_comment(Provider::Gitlab, &issue, Some(&refs)));
        assert_eq!(comment["position"]["position_type"], "text");
        assert_eq!(comment["position"]["base_sha"], "base");
        assert_eq!(comment["position"]["start_sha"], "start");
        assert_eq!(comment["position"]["head_sha"], "head");
    }
}
