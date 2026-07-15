use std::collections::BTreeSet;
use std::path::Path;
use std::process::ExitCode;

use fallow_config::OutputFormat;
use serde::Serialize;
use serde_json::Value;

use crate::api::try_api_agent;
use crate::error::emit_error_with_style;

use super::{
    ApplyResult, CiProvider, PlannedReconcile, ReconcileOptions, apply_provider_reconcile,
    emit_ci_command_json, github_post_json, github_token, gitlab_post_json, load_provider_state,
    read_envelope, require_target, url_encode_path_segment,
};

#[derive(Clone, Copy)]
pub(super) struct PostReviewInput<'a> {
    pub provider: CiProvider,
    pub target: Option<&'a str>,
    pub envelope: &'a Path,
    pub repo: Option<&'a str>,
    pub project_id: Option<&'a str>,
    pub api_url: Option<&'a str>,
    pub dry_run: bool,
}

#[derive(Debug, Default, Serialize)]
struct PostReviewResult {
    action: &'static str,
    provider: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    target: Option<String>,
    dry_run: bool,
    comments_total: usize,
    comments_posted: usize,
    comments_skipped: usize,
    summary_posted: bool,
    summary_updated: bool,
    stale_fingerprints: usize,
    resolution_comments_posted: usize,
    threads_resolved: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    apply_hint: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    apply_errors: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    post_errors: Vec<String>,
}

pub(super) fn post_review(
    input: PostReviewInput<'_>,
    output: OutputFormat,
    json_style: crate::json_style::JsonStyle,
) -> ExitCode {
    let envelope = match read_envelope(input.envelope) {
        Ok(value) => value,
        Err(e) => return emit_error_with_style(&e, 2, output, json_style),
    };
    let opts = ReconcileOptions {
        repo: input.repo,
        project_id: input.project_id,
        api_url: input.api_url,
        dry_run: input.dry_run,
    };
    let provider_state = match load_provider_state(input.provider, input.target, opts) {
        Ok(state) => state,
        Err(e) => {
            return emit_error_with_style(&e, crate::api::NETWORK_EXIT_CODE, output, json_style);
        }
    };
    let current = envelope_fingerprints(&envelope);
    let planned = PlannedReconcile::new(&current, &provider_state);

    let mut result = match input.provider {
        CiProvider::Github => post_github_review(input, &envelope, &provider_state.fingerprints),
        CiProvider::Gitlab => post_gitlab_review(input, &envelope, &provider_state.fingerprints),
    };
    if !result.post_errors.is_empty() {
        return emit_post_review_result(&result, output, json_style);
    }

    let applied = if input.dry_run {
        ApplyResult::default()
    } else {
        apply_provider_reconcile(input.provider, &planned, input.target, opts)
    };
    attach_reconcile_result(&mut result, &planned.plan.stale, applied);
    emit_post_review_result(&result, output, json_style)
}

fn post_github_review(
    input: PostReviewInput<'_>,
    envelope: &Value,
    existing_fingerprints: &BTreeSet<String>,
) -> PostReviewResult {
    let pr = match require_target("GitHub pull request", input.target) {
        Ok(pr) => pr,
        Err(e) => return result_with_error(input, e),
    };
    let repo = match github_repo(input.repo) {
        Ok(repo) => repo,
        Err(e) => return result_with_error(input, e),
    };
    let token = match github_token() {
        Ok(token) => token,
        Err(e) => return result_with_error(input, e),
    };
    let api = input
        .api_url
        .unwrap_or("https://api.github.com")
        .trim_end_matches('/');
    let agent = match try_api_agent() {
        Ok(agent) => agent,
        Err(e) => return result_with_error(input, e.to_string()),
    };
    let comments = new_comments(envelope, existing_fingerprints);
    let mut result = base_result(input, "github", comments.len());
    if envelope_comments_len(envelope) == 0 {
        result.action = "skip";
        return result;
    }
    result.comments_skipped = envelope_comments_len(envelope).saturating_sub(comments.len());
    if comments.is_empty() {
        result.action = "skip";
        return result;
    }
    if input.dry_run {
        result.action = "post_review";
        result.comments_posted = comments.len();
        return result;
    }
    let payload = github_review_payload(envelope, &comments);
    let url = format!("{api}/repos/{repo}/pulls/{pr}/reviews");
    match github_post_json(&agent, &url, &token, &payload) {
        Ok(_) => {
            result.action = "post_review";
            result.comments_posted = comments.len();
        }
        Err(e) => result.post_errors.push(e),
    }
    result
}

fn post_gitlab_review(
    input: PostReviewInput<'_>,
    envelope: &Value,
    existing_fingerprints: &BTreeSet<String>,
) -> PostReviewResult {
    let mr = match require_target("GitLab merge request", input.target) {
        Ok(mr) => mr,
        Err(e) => return result_with_error(input, e),
    };
    let project_id = match gitlab_project_id(input.project_id) {
        Ok(project_id) => project_id,
        Err(e) => return result_with_error(input, e),
    };
    let token = match gitlab_token() {
        Ok(token) => token,
        Err(e) => return result_with_error(input, e),
    };
    let api = gitlab_api_url(input.api_url);
    let encoded_project = url_encode_path_segment(&project_id);
    let agent = match try_api_agent() {
        Ok(agent) => agent,
        Err(e) => return result_with_error(input, e.to_string()),
    };
    let comments = new_comments(envelope, existing_fingerprints);
    let mut result = base_result(input, "gitlab", comments.len());
    if envelope_comments_len(envelope) == 0 {
        result.action = "skip";
        return result;
    }
    result.comments_skipped = envelope_comments_len(envelope).saturating_sub(comments.len());
    if comments.is_empty() {
        result.action = "skip";
        return result;
    }
    result.action = "post_review";
    if input.dry_run {
        result.comments_posted = comments.len();
        return result;
    }
    for comment in &comments {
        match post_gitlab_inline_comment(&agent, &api, &encoded_project, mr, &token, comment) {
            Ok(()) => result.comments_posted += 1,
            Err(e) => result.post_errors.push(e),
        }
    }
    result
}

fn github_review_payload(envelope: &Value, comments: &[Value]) -> Value {
    let comments = comments
        .iter()
        .map(|comment| {
            serde_json::json!({
                "path": comment.get("path").cloned().unwrap_or(Value::Null),
                "line": comment.get("line").cloned().unwrap_or(Value::Null),
                "side": comment.get("side").cloned().unwrap_or(Value::Null),
                "body": comment.get("body").cloned().unwrap_or(Value::Null),
            })
        })
        .collect::<Vec<_>>();
    serde_json::json!({
        "event": envelope.get("event").and_then(Value::as_str).unwrap_or("COMMENT"),
        "body": envelope.get("body").and_then(Value::as_str).unwrap_or(""),
        "comments": comments,
    })
}

fn post_gitlab_inline_comment(
    agent: &ureq::Agent,
    api: &str,
    encoded_project: &str,
    mr: &str,
    token: &str,
    comment: &Value,
) -> Result<(), String> {
    let position = comment.get("position").cloned().unwrap_or(Value::Null);
    let body = comment
        .get("body")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    if position.get("base_sha").and_then(Value::as_str).is_some()
        && position.get("head_sha").and_then(Value::as_str).is_some()
    {
        let payload = serde_json::json!({ "body": body, "position": position });
        let url = format!("{api}/projects/{encoded_project}/merge_requests/{mr}/discussions");
        return gitlab_post_json(agent, &url, token, &payload).map(|_| ());
    }
    let path = position
        .get("new_path")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let line = position
        .get("new_line")
        .and_then(Value::as_u64)
        .unwrap_or(1);
    let fallback_body = format!("Warning: **{path}:{line}**\n\n{body}");
    let payload = serde_json::json!({ "body": fallback_body });
    let url = format!("{api}/projects/{encoded_project}/merge_requests/{mr}/notes");
    gitlab_post_json(agent, &url, token, &payload).map(|_| ())
}

fn attach_reconcile_result(result: &mut PostReviewResult, stale: &[String], applied: ApplyResult) {
    result.stale_fingerprints = stale.len();
    result.resolution_comments_posted = applied.resolution_comments_posted;
    result.threads_resolved = applied.threads_resolved;
    result.apply_hint = applied.hint();
    result.apply_errors = applied.errors;
}

fn new_comments(envelope: &Value, existing: &BTreeSet<String>) -> Vec<Value> {
    envelope
        .get("comments")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|comment| {
            comment
                .get("fingerprint")
                .and_then(Value::as_str)
                .is_none_or(|fingerprint| !existing.contains(fingerprint))
        })
        .cloned()
        .collect()
}

fn envelope_comments_len(envelope: &Value) -> usize {
    envelope
        .get("comments")
        .and_then(Value::as_array)
        .map_or(0, Vec::len)
}

fn envelope_fingerprints(envelope: &Value) -> BTreeSet<String> {
    envelope
        .get("comments")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|comment| comment.get("fingerprint").and_then(Value::as_str))
        .filter(|fingerprint| !fingerprint.trim().is_empty())
        .map(str::to_owned)
        .collect()
}

fn base_result(
    input: PostReviewInput<'_>,
    provider: &'static str,
    comments_total: usize,
) -> PostReviewResult {
    PostReviewResult {
        action: "skip",
        provider,
        target: input.target.map(str::to_owned),
        dry_run: input.dry_run,
        comments_total,
        ..PostReviewResult::default()
    }
}

fn result_with_error(input: PostReviewInput<'_>, error: String) -> PostReviewResult {
    let mut result = base_result(
        input,
        match input.provider {
            CiProvider::Github => "github",
            CiProvider::Gitlab => "gitlab",
        },
        0,
    );
    result.post_errors.push(error);
    result
}

fn emit_post_review_result(
    result: &PostReviewResult,
    output: OutputFormat,
    json_style: crate::json_style::JsonStyle,
) -> ExitCode {
    emit_ci_command_json(result, "review post result", output, json_style)
}

fn github_repo(explicit: Option<&str>) -> Result<String, String> {
    explicit
        .map(str::to_owned)
        .or_else(|| std::env::var("GH_REPO").ok())
        .or_else(|| std::env::var("GITHUB_REPOSITORY").ok())
        .ok_or_else(|| {
            "GitHub review posting requires --repo, GH_REPO, or GITHUB_REPOSITORY".to_owned()
        })
}

fn gitlab_project_id(explicit: Option<&str>) -> Result<String, String> {
    explicit
        .map(str::to_owned)
        .or_else(|| std::env::var("CI_PROJECT_ID").ok())
        .ok_or_else(|| "GitLab review posting requires --project-id or CI_PROJECT_ID".to_owned())
}

fn gitlab_token() -> Result<String, String> {
    std::env::var("GITLAB_TOKEN")
        .map_err(|_| "GitLab review posting requires GITLAB_TOKEN".to_owned())
}

fn gitlab_api_url(explicit: Option<&str>) -> String {
    explicit
        .map(str::to_owned)
        .or_else(|| std::env::var("CI_API_V4_URL").ok())
        .unwrap_or_else(|| "https://gitlab.com/api/v4".to_owned())
        .trim_end_matches('/')
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn envelope() -> Value {
        serde_json::json!({
            "event": "COMMENT",
            "body": "body",
            "comments": [
                { "path": "src/a.ts", "line": 1, "side": "RIGHT", "body": "A", "fingerprint": "a" },
                { "path": "src/b.ts", "line": 2, "side": "RIGHT", "body": "B", "fingerprint": "b" }
            ]
        })
    }

    #[test]
    fn new_comments_filters_existing_fingerprints() {
        let existing = BTreeSet::from(["a".to_owned()]);

        let comments = new_comments(&envelope(), &existing);

        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0]["fingerprint"], "b");
    }

    #[test]
    fn github_review_payload_keeps_retryable_input_body() {
        let comments = new_comments(&envelope(), &BTreeSet::new());

        let payload = github_review_payload(&envelope(), &comments);

        assert_eq!(payload["event"], "COMMENT");
        assert_eq!(payload["comments"][0]["path"], "src/a.ts");
        assert_eq!(payload["comments"][0]["side"], "RIGHT");
    }
}
