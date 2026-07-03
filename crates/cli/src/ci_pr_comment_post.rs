use std::path::Path;
use std::process::ExitCode;

use fallow_config::OutputFormat;
use serde_json::Value;

use crate::api::try_api_agent;
use crate::error::emit_error;

use super::{
    CiProvider, emit_pr_comment_post_plan, github_get_json, github_post_json, github_token,
    gitlab_get_json, gitlab_post_json, gitlab_put_json, read_text_file, require_target,
    url_encode_path_segment,
};

#[derive(Clone)]
pub(super) struct PostPrCommentInput<'a> {
    pub provider: CiProvider,
    pub target: Option<&'a str>,
    pub body: &'a Path,
    pub envelope: Option<&'a Path>,
    pub marker_id: String,
    pub clean: bool,
    pub repo: Option<&'a str>,
    pub project_id: Option<&'a str>,
    pub api_url: Option<&'a str>,
    pub dry_run: bool,
}

pub(super) fn post_pr_comment(input: &PostPrCommentInput<'_>, output: OutputFormat) -> ExitCode {
    match input.provider {
        CiProvider::Github => post_github_pr_comment(input, output),
        CiProvider::Gitlab => post_gitlab_mr_comment(input, output),
    }
}

fn post_github_pr_comment(input: &PostPrCommentInput<'_>, output: OutputFormat) -> ExitCode {
    let pr = match require_target("GitHub pull request", input.target) {
        Ok(pr) => pr,
        Err(e) => return emit_error(&e, 2, output),
    };
    let envelope =
        match read_pr_comment_envelope(input.envelope, input.body, &input.marker_id, input.clean) {
            Ok(envelope) => envelope,
            Err(e) => return emit_error(&e, 2, output),
        };
    let repo = match github_repo(input.repo) {
        Ok(repo) => repo,
        Err(e) => return emit_error(&e, 2, output),
    };
    let token = match github_token() {
        Ok(token) => token,
        Err(e) => return emit_error(&e, crate::api::NETWORK_EXIT_CODE, output),
    };
    let api = input
        .api_url
        .unwrap_or("https://api.github.com")
        .trim_end_matches('/');
    let agent = match try_api_agent() {
        Ok(agent) => agent,
        Err(err) => return emit_error(&err.to_string(), crate::api::NETWORK_EXIT_CODE, output),
    };
    let existing =
        match find_github_sticky_comment(&agent, api, &repo, pr, &token, &input.marker_id) {
            Ok(existing) => existing,
            Err(e) => return emit_error(&e, crate::api::NETWORK_EXIT_CODE, output),
        };
    let plan = fallow_output::plan_pr_comment_post(&fallow_output::PrCommentPostPlanInput {
        envelope: &envelope,
        existing: existing.as_ref(),
    });
    if !input.dry_run
        && let Err(e) = apply_github_pr_comment_plan(&agent, api, &repo, pr, &token, &plan)
    {
        return emit_error(&e, crate::api::NETWORK_EXIT_CODE, output);
    }
    emit_pr_comment_post_plan(&plan, output)
}

fn post_gitlab_mr_comment(input: &PostPrCommentInput<'_>, output: OutputFormat) -> ExitCode {
    let mr = match require_target("GitLab merge request", input.target) {
        Ok(mr) => mr,
        Err(e) => return emit_error(&e, 2, output),
    };
    let envelope =
        match read_pr_comment_envelope(input.envelope, input.body, &input.marker_id, input.clean) {
            Ok(envelope) => envelope,
            Err(e) => return emit_error(&e, 2, output),
        };
    let project_id = match gitlab_project_id(input.project_id) {
        Ok(project_id) => project_id,
        Err(e) => return emit_error(&e, 2, output),
    };
    let token = match gitlab_token() {
        Ok(token) => token,
        Err(e) => return emit_error(&e, crate::api::NETWORK_EXIT_CODE, output),
    };
    let api = gitlab_api_url(input.api_url);
    let agent = match try_api_agent() {
        Ok(agent) => agent,
        Err(err) => return emit_error(&err.to_string(), crate::api::NETWORK_EXIT_CODE, output),
    };
    let encoded_project = url_encode_path_segment(&project_id);
    let existing =
        match find_gitlab_sticky_note(&agent, &api, &encoded_project, mr, &token, &input.marker_id)
        {
            Ok(existing) => existing,
            Err(e) => return emit_error(&e, crate::api::NETWORK_EXIT_CODE, output),
        };
    let plan = fallow_output::plan_pr_comment_post(&fallow_output::PrCommentPostPlanInput {
        envelope: &envelope,
        existing: existing.as_ref(),
    });
    if !input.dry_run
        && let Err(e) =
            apply_gitlab_mr_comment_plan(&agent, &api, &encoded_project, mr, &token, &plan)
    {
        return emit_error(&e, crate::api::NETWORK_EXIT_CODE, output);
    }
    emit_pr_comment_post_plan(&plan, output)
}

fn read_pr_comment_envelope(
    envelope_path: Option<&Path>,
    body_path: &Path,
    marker_id: &str,
    clean: bool,
) -> Result<fallow_output::PrCommentEnvelope, String> {
    if let Some(path) = envelope_path {
        let text = read_text_file(path, "PR comment envelope")?;
        return serde_json::from_str(&text).map_err(|e| {
            format!(
                "failed to parse PR comment envelope '{}': {e}",
                path.display()
            )
        });
    }
    let body = read_text_file(body_path, "PR comment body")?;
    Ok(fallow_output::PrCommentEnvelope {
        marker_id: marker_id.to_owned(),
        body,
        is_clean: clean,
        details_url: None,
        check_summary: None,
        truncation: fallow_output::PrCommentTruncation::default(),
    })
}

fn github_repo(explicit: Option<&str>) -> Result<String, String> {
    explicit
        .map(str::to_owned)
        .or_else(|| std::env::var("GH_REPO").ok())
        .or_else(|| std::env::var("GITHUB_REPOSITORY").ok())
        .ok_or_else(|| {
            "GitHub PR comment posting requires --repo, GH_REPO, or GITHUB_REPOSITORY".to_owned()
        })
}

fn gitlab_project_id(explicit: Option<&str>) -> Result<String, String> {
    explicit
        .map(str::to_owned)
        .or_else(|| std::env::var("CI_PROJECT_ID").ok())
        .ok_or_else(|| {
            "GitLab MR comment posting requires --project-id or CI_PROJECT_ID".to_owned()
        })
}

fn gitlab_token() -> Result<String, String> {
    std::env::var("GITLAB_TOKEN")
        .map_err(|_| "GitLab MR comment posting requires GITLAB_TOKEN".to_owned())
}

fn gitlab_api_url(explicit: Option<&str>) -> String {
    explicit
        .map(str::to_owned)
        .or_else(|| std::env::var("CI_API_V4_URL").ok())
        .unwrap_or_else(|| "https://gitlab.com/api/v4".to_owned())
        .trim_end_matches('/')
        .to_owned()
}

fn find_github_sticky_comment(
    agent: &ureq::Agent,
    api: &str,
    repo: &str,
    pr: &str,
    token: &str,
    marker_id: &str,
) -> Result<Option<fallow_output::ExistingPrComment>, String> {
    let marker = format!("<!-- fallow-id: {marker_id} -->");
    for page in 1..=100 {
        let url = format!("{api}/repos/{repo}/issues/{pr}/comments?per_page=100&page={page}");
        let value = github_get_json(agent, &url, token)?;
        let comments = value
            .as_array()
            .ok_or_else(|| "GitHub issue comments response was not an array".to_owned())?;
        for comment in comments {
            let body = comment.get("body").and_then(Value::as_str).unwrap_or("");
            if body.contains(&marker)
                && let Some(id) = comment.get("id").and_then(Value::as_u64)
            {
                return Ok(Some(fallow_output::ExistingPrComment {
                    id: id.to_string(),
                    body: body.to_owned(),
                }));
            }
        }
        if comments.len() < 100 {
            break;
        }
    }
    Ok(None)
}

fn find_gitlab_sticky_note(
    agent: &ureq::Agent,
    api: &str,
    encoded_project: &str,
    mr: &str,
    token: &str,
    marker_id: &str,
) -> Result<Option<fallow_output::ExistingPrComment>, String> {
    let marker = format!("<!-- fallow-id: {marker_id} -->");
    for page in 1..=100 {
        let url = format!(
            "{api}/projects/{encoded_project}/merge_requests/{mr}/notes?per_page=100&page={page}"
        );
        let value = gitlab_get_json(agent, &url, token)?;
        let notes = value
            .as_array()
            .ok_or_else(|| "GitLab notes response was not an array".to_owned())?;
        for note in notes {
            let body = note.get("body").and_then(Value::as_str).unwrap_or("");
            if body.contains(&marker)
                && let Some(id) = note.get("id").and_then(Value::as_u64)
            {
                return Ok(Some(fallow_output::ExistingPrComment {
                    id: id.to_string(),
                    body: body.to_owned(),
                }));
            }
        }
        if notes.len() < 100 {
            break;
        }
    }
    Ok(None)
}

pub(super) fn apply_github_pr_comment_plan(
    agent: &ureq::Agent,
    api: &str,
    repo: &str,
    pr: &str,
    token: &str,
    plan: &fallow_output::PrCommentPostPlan,
) -> Result<(), String> {
    match plan.action {
        fallow_output::PrCommentPostAction::Create => {
            let body = plan
                .body
                .as_deref()
                .ok_or_else(|| "create plan did not include a body".to_owned())?;
            let url = format!("{api}/repos/{repo}/issues/{pr}/comments");
            let payload = serde_json::json!({ "body": body });
            github_post_json(agent, &url, token, &payload).map(|_| ())
        }
        fallow_output::PrCommentPostAction::Update => {
            let body = plan
                .body
                .as_deref()
                .ok_or_else(|| "update plan did not include a body".to_owned())?;
            let comment_id = plan
                .comment_id
                .as_deref()
                .ok_or_else(|| "update plan did not include a comment id".to_owned())?;
            let url = format!("{api}/repos/{repo}/issues/comments/{comment_id}");
            let payload = serde_json::json!({ "body": body });
            super::github_patch_json(agent, &url, token, &payload).map(|_| ())
        }
        fallow_output::PrCommentPostAction::Skip => Ok(()),
    }
}

fn apply_gitlab_mr_comment_plan(
    agent: &ureq::Agent,
    api: &str,
    encoded_project: &str,
    mr: &str,
    token: &str,
    plan: &fallow_output::PrCommentPostPlan,
) -> Result<(), String> {
    match plan.action {
        fallow_output::PrCommentPostAction::Create => {
            let body = plan
                .body
                .as_deref()
                .ok_or_else(|| "create plan did not include a body".to_owned())?;
            let url = format!("{api}/projects/{encoded_project}/merge_requests/{mr}/notes");
            let payload = serde_json::json!({ "body": body });
            gitlab_post_json(agent, &url, token, &payload).map(|_| ())
        }
        fallow_output::PrCommentPostAction::Update => {
            let body = plan
                .body
                .as_deref()
                .ok_or_else(|| "update plan did not include a body".to_owned())?;
            let note_id = plan
                .comment_id
                .as_deref()
                .ok_or_else(|| "update plan did not include a note id".to_owned())?;
            let url =
                format!("{api}/projects/{encoded_project}/merge_requests/{mr}/notes/{note_id}");
            let payload = serde_json::json!({ "body": body });
            gitlab_put_json(agent, &url, token, &payload).map(|_| ())
        }
        fallow_output::PrCommentPostAction::Skip => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gitlab_api_url_trims_trailing_slash() {
        assert_eq!(
            gitlab_api_url(Some("https://gitlab.example/api/v4/")),
            "https://gitlab.example/api/v4"
        );
    }

    #[test]
    fn gitlab_project_id_accepts_explicit_path() {
        assert_eq!(
            gitlab_project_id(Some("group/project")).as_deref(),
            Ok("group/project")
        );
    }
}
