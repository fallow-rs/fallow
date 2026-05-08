use std::process::ExitCode;

use serde_json::Value;

use super::pr_comment::{CiIssue, Provider, command_title, escape_md};
use super::severity;
use crate::report::emit_json;

#[must_use]
pub fn render_review_envelope(command: &str, provider: Provider, issues: &[CiIssue]) -> Value {
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
    let comments = issues
        .iter()
        .take(max)
        .map(|issue| render_comment(provider, issue, gitlab_diff_refs.as_ref()))
        .collect::<Vec<_>>();

    match provider {
        Provider::Github => serde_json::json!({
            "event": "COMMENT",
            "body": body,
            "comments": comments,
            "meta": {
                "schema": "fallow-review-envelope/v1",
                "provider": "github",
                "check_conclusion": github_check_conclusion(issues),
            }
        }),
        Provider::Gitlab => serde_json::json!({
            "body": body,
            "comments": comments,
            "meta": {
                "schema": "fallow-review-envelope/v1",
                "provider": "gitlab"
            }
        }),
    }
}

#[must_use]
pub fn print_review_envelope(command: &str, provider: Provider, codeclimate: &Value) -> ExitCode {
    let issues = super::diff_filter::filter_issues_from_env(
        super::pr_comment::issues_from_codeclimate(codeclimate),
    );
    let envelope = render_review_envelope(command, provider, &issues);
    emit_json(&envelope, "review envelope")
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
) -> Value {
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
        Provider::Github => serde_json::json!({
            "path": issue.path,
            "line": issue.line,
            "side": "RIGHT",
            "body": body,
            "fingerprint": issue.fingerprint,
        }),
        Provider::Gitlab => {
            let mut position = serde_json::json!({
                "position_type": "text",
                "old_path": issue.path,
                "new_path": issue.path,
                "new_line": issue.line,
            });
            if let Some(diff_refs) = gitlab_diff_refs {
                position["base_sha"] = serde_json::json!(diff_refs.base_sha);
                position["start_sha"] = serde_json::json!(diff_refs.start_sha);
                position["head_sha"] = serde_json::json!(diff_refs.head_sha);
            }
            serde_json::json!({
                "body": body,
                "position": position,
                "fingerprint": issue.fingerprint,
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

fn github_check_conclusion(issues: &[CiIssue]) -> &'static str {
    if issues
        .iter()
        .any(|issue| matches!(issue.severity.as_str(), "major" | "critical" | "blocker"))
    {
        severity::github_check_conclusion(fallow_config::Severity::Error)
    } else if issues.is_empty() {
        severity::github_check_conclusion(fallow_config::Severity::Off)
    } else {
        severity::github_check_conclusion(fallow_config::Severity::Warn)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let envelope = render_review_envelope("check", Provider::Github, &issues);
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
        let comment = render_comment(Provider::Github, &issue, None);
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
        let comment = render_comment(Provider::Github, &issue, None);
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
        let comment = render_comment(Provider::Gitlab, &issue, Some(&refs));
        assert_eq!(comment["position"]["position_type"], "text");
        assert_eq!(comment["position"]["base_sha"], "base");
        assert_eq!(comment["position"]["start_sha"], "start");
        assert_eq!(comment["position"]["head_sha"], "head");
    }
}
