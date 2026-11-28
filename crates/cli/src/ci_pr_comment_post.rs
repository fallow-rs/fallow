use std::path::Path;
use std::process::ExitCode;

use fallow_config::OutputFormat;
use serde_json::Value;

use crate::api::try_api_agent;
use crate::error::{
    emit_coded_error_with_style, emit_error_with_style, emit_input_error_with_style,
};
use crate::exit_codes::NETWORK_FAILURE;

use super::{
    BotLogin, CiProvider, GITLAB_PER_PAGE, GitlabAuthenticatedUser, emit_pr_comment_post_plan,
    github_create_json, github_get_json, github_repo, github_token, gitlab_api_url,
    gitlab_create_json, gitlab_get_page, gitlab_has_more_pages,
    gitlab_note_has_native_bot_metadata, gitlab_project_id, gitlab_put_json, gitlab_token,
    is_github_bot_comment, is_gitlab_bot_note, load_gitlab_authenticated_user, read_json_response,
    read_text_file, require_target, should_retry_status, url_encode_path_segment,
    with_retryable_response,
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

pub(super) fn post_pr_comment(
    input: &PostPrCommentInput<'_>,
    output: OutputFormat,
    json_style: crate::json_style::JsonStyle,
) -> ExitCode {
    match input.provider {
        CiProvider::Github => post_github_pr_comment(input, output, json_style),
        CiProvider::Gitlab => post_gitlab_mr_comment(input, output, json_style),
    }
}

fn post_github_pr_comment(
    input: &PostPrCommentInput<'_>,
    output: OutputFormat,
    json_style: crate::json_style::JsonStyle,
) -> ExitCode {
    let pr = match require_target("GitHub pull request", input.target) {
        Ok(pr) => pr,
        Err(e) => return emit_input_error_with_style(&e, output, json_style),
    };
    let envelope =
        match read_pr_comment_envelope(input.envelope, input.body, &input.marker_id, input.clean) {
            Ok(envelope) => envelope,
            Err(e) => return emit_input_error_with_style(&e, output, json_style),
        };
    let repo = match github_repo(input.repo, "PR comment") {
        Ok(repo) => repo,
        Err(e) => return emit_input_error_with_style(&e, output, json_style),
    };
    let token = match github_token() {
        Ok(token) => token,
        Err(e) => {
            return emit_error_with_style(&e, NETWORK_FAILURE, output, json_style);
        }
    };
    let api = input
        .api_url
        .unwrap_or("https://api.github.com")
        .trim_end_matches('/');
    let agent = match try_api_agent() {
        Ok(agent) => agent,
        Err(err) => {
            return emit_error_with_style(&err.to_string(), NETWORK_FAILURE, output, json_style);
        }
    };
    let existing =
        match find_github_sticky_comment(&agent, api, &repo, pr, &token, &input.marker_id) {
            Ok(existing) => existing,
            Err(e) => {
                return emit_coded_error_with_style(
                    &e,
                    Some(super::ProviderStateError::LOOKUP_CODE),
                    NETWORK_FAILURE,
                    output,
                    json_style,
                );
            }
        };
    let plan = fallow_output::plan_pr_comment_post(&fallow_output::PrCommentPostPlanInput {
        envelope: &envelope,
        existing: existing.as_ref(),
    });
    if !input.dry_run
        && let Err(e) = apply_github_pr_comment_plan(&agent, api, &repo, pr, &token, &plan)
    {
        return emit_error_with_style(&e, NETWORK_FAILURE, output, json_style);
    }
    emit_pr_comment_post_plan(&plan, output, json_style)
}

fn post_gitlab_mr_comment(
    input: &PostPrCommentInput<'_>,
    output: OutputFormat,
    json_style: crate::json_style::JsonStyle,
) -> ExitCode {
    let mr = match require_target("GitLab merge request", input.target) {
        Ok(mr) => mr,
        Err(e) => return emit_input_error_with_style(&e, output, json_style),
    };
    let envelope =
        match read_pr_comment_envelope(input.envelope, input.body, &input.marker_id, input.clean) {
            Ok(envelope) => envelope,
            Err(e) => return emit_input_error_with_style(&e, output, json_style),
        };
    let project_id = match gitlab_project_id(input.project_id, "MR comment") {
        Ok(project_id) => project_id,
        Err(e) => return emit_input_error_with_style(&e, output, json_style),
    };
    let token = match gitlab_token("MR comment") {
        Ok(token) => token,
        Err(e) => {
            return emit_error_with_style(&e, NETWORK_FAILURE, output, json_style);
        }
    };
    let api = gitlab_api_url(input.api_url);
    let agent = match try_api_agent() {
        Ok(agent) => agent,
        Err(err) => {
            return emit_error_with_style(&err.to_string(), NETWORK_FAILURE, output, json_style);
        }
    };
    let encoded_project = url_encode_path_segment(&project_id);
    let existing =
        match find_gitlab_sticky_note(&agent, &api, &encoded_project, mr, &token, &input.marker_id)
        {
            Ok(existing) => existing,
            Err(e) => {
                return emit_coded_error_with_style(
                    &e,
                    Some(super::ProviderStateError::LOOKUP_CODE),
                    NETWORK_FAILURE,
                    output,
                    json_style,
                );
            }
        };
    let plan = fallow_output::plan_pr_comment_post(&fallow_output::PrCommentPostPlanInput {
        envelope: &envelope,
        existing: existing.as_ref(),
    });
    if !input.dry_run
        && let Err(e) =
            apply_gitlab_mr_comment_plan(&agent, &api, &encoded_project, mr, &token, &plan)
    {
        return emit_error_with_style(&e, NETWORK_FAILURE, output, json_style);
    }
    emit_pr_comment_post_plan(&plan, output, json_style)
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

fn find_github_sticky_comment(
    agent: &ureq::Agent,
    api: &str,
    repo: &str,
    pr: &str,
    token: &str,
    marker_id: &str,
) -> Result<Option<fallow_output::ExistingPrComment>, String> {
    let bot_login_var = std::env::var("FALLOW_BOT_LOGIN");
    let bot_login = BotLogin::from_var(&bot_login_var);
    let mut token_login: Option<Option<String>> = None;
    let marker = format!("<!-- fallow-id: {marker_id} -->");
    for page in 1..=100 {
        let url = format!("{api}/repos/{repo}/issues/{pr}/comments?per_page=100&page={page}");
        let value = github_get_json(agent, &url, token)?;
        let comments = value
            .as_array()
            .ok_or_else(|| "GitHub issue comments response was not an array".to_owned())?;
        if token_login.is_none() && sticky_comment_needs_token_login(comments, &marker, bot_login) {
            token_login = Some(github_token_login(agent, api, token)?);
        }
        let token_login = token_login.as_ref().and_then(Option::as_deref);
        if let Some(existing) = github_sticky_comment(comments, &marker, bot_login, token_login) {
            return Ok(Some(existing));
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
    let bot_login_var = std::env::var("FALLOW_BOT_LOGIN");
    let bot_login = BotLogin::from_var(&bot_login_var);
    let mut authenticated_user = None;
    let marker = format!("<!-- fallow-id: {marker_id} -->");
    for page in 1..=100 {
        let url = format!(
            "{api}/projects/{encoded_project}/merge_requests/{mr}/notes?per_page={GITLAB_PER_PAGE}&page={page}"
        );
        let (value, next_page) = gitlab_get_page(agent, &url, token)?;
        let notes = value
            .as_array()
            .ok_or_else(|| "GitLab notes response was not an array".to_owned())?;
        if authenticated_user.is_none() && sticky_note_needs_user(notes, &marker, bot_login) {
            authenticated_user = Some(load_gitlab_authenticated_user(agent, api, token)?);
        }
        if let Some(existing) =
            gitlab_sticky_note(notes, &marker, authenticated_user.as_ref(), bot_login)
        {
            return Ok(Some(existing));
        }
        if !gitlab_has_more_pages(next_page, notes.len()) {
            break;
        }
    }
    Ok(None)
}

/// The first comment of a page that the bot wrote and that holds the sticky
/// `marker` as a full line. A comment of another account is never updated.
///
/// Without `FALLOW_BOT_LOGIN`, the bot is an account of type `Bot`, or the
/// login of the token (`token_login`) when `/user` gave one. A personal
/// access token posts as an account of type `User`.
fn github_sticky_comment(
    comments: &[Value],
    marker: &str,
    bot_login: BotLogin<'_>,
    token_login: Option<&str>,
) -> Option<fallow_output::ExistingPrComment> {
    let is_token_login = |comment: &Value| {
        matches!(bot_login, BotLogin::Unset)
            && token_login.is_some_and(|login| github_comment_login(comment) == Some(login))
    };
    comments
        .iter()
        .filter(|comment| has_sticky_marker(comment, marker))
        .filter(|comment| is_github_bot_comment(comment, bot_login) || is_token_login(comment))
        .find_map(existing_comment)
}

/// Whether a candidate comment needs the login of the token for the owner
/// check: no `FALLOW_BOT_LOGIN` and a candidate that is not of type `Bot`.
fn sticky_comment_needs_token_login(
    comments: &[Value],
    marker: &str,
    bot_login: BotLogin<'_>,
) -> bool {
    matches!(bot_login, BotLogin::Unset)
        && comments.iter().any(|comment| {
            has_sticky_marker(comment, marker) && !is_github_bot_comment(comment, bot_login)
        })
}

fn github_comment_login(comment: &Value) -> Option<&str> {
    comment
        .get("user")
        .and_then(|user| user.get("login"))
        .and_then(Value::as_str)
}

/// The login of the token from `GET /user`. `GITHUB_TOKEN` and app tokens
/// cannot read it and get 401 or 403; that gives `None`. Any other failure is
/// an error, so the lookup never falls back to an unchecked comment.
fn github_token_login(
    agent: &ureq::Agent,
    api: &str,
    token: &str,
) -> Result<Option<String>, String> {
    let url = format!("{api}/user");
    with_retryable_response(
        "GitHub",
        should_retry_status,
        || {
            agent
                .get(&url)
                .header("Authorization", &format!("Bearer {token}"))
                .header("Accept", "application/vnd.github+json")
                .header("X-GitHub-Api-Version", "2022-11-28")
                .header("User-Agent", "fallow-cli")
                .call()
        },
        |response| {
            if matches!(response.status().as_u16(), 401 | 403) {
                return Ok(None);
            }
            let value = read_json_response(response, "GitHub")?;
            value
                .get("login")
                .and_then(Value::as_str)
                .filter(|login| !login.is_empty())
                .map(|login| Some(login.to_owned()))
                .ok_or_else(|| {
                    "GitHub current-user response did not contain a non-empty login".to_owned()
                })
        },
    )
}

/// The first note of a page that the bot wrote and that holds the sticky
/// `marker` as a full line. fallow never writes a system note, so a system
/// note is not a candidate.
fn gitlab_sticky_note(
    notes: &[Value],
    marker: &str,
    authenticated_user: Option<&GitlabAuthenticatedUser>,
    bot_login: BotLogin<'_>,
) -> Option<fallow_output::ExistingPrComment> {
    notes
        .iter()
        .filter(|note| has_sticky_marker(note, marker))
        .filter(|note| is_gitlab_bot_note(note, authenticated_user, bot_login))
        .find_map(existing_comment)
}

/// Whether the owner check of a candidate note needs the user of the token.
fn sticky_note_needs_user(notes: &[Value], marker: &str, bot_login: BotLogin<'_>) -> bool {
    matches!(bot_login, BotLogin::Unset)
        && notes.iter().any(|note| {
            has_sticky_marker(note, marker) && !gitlab_note_has_native_bot_metadata(note)
        })
}

fn has_sticky_marker(comment: &Value, marker: &str) -> bool {
    comment.get("system").and_then(Value::as_bool) != Some(true)
        && comment
            .get("body")
            .and_then(Value::as_str)
            .is_some_and(|body| body.lines().any(|line| line.trim_end() == marker))
}

fn existing_comment(comment: &Value) -> Option<fallow_output::ExistingPrComment> {
    let id = comment.get("id").and_then(Value::as_u64)?;
    let body = comment.get("body").and_then(Value::as_str)?;
    Some(fallow_output::ExistingPrComment {
        id: id.to_string(),
        body: body.to_owned(),
    })
}

fn apply_github_pr_comment_plan(
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
            github_create_json(agent, &url, token, &payload)
                .and_then(|value| validate_created_comment(&value, body, "GitHub"))
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
            gitlab_create_json(agent, &url, token, &payload)
                .and_then(|value| validate_created_comment(&value, body, "GitLab"))
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

fn validate_created_comment(
    value: &Value,
    expected_body: &str,
    provider: &str,
) -> Result<(), String> {
    let confirmed = value.get("id").and_then(Value::as_u64).is_some()
        && value.get("body").and_then(Value::as_str) == Some(expected_body);
    confirmed.then_some(()).ok_or_else(|| {
        format!("{provider} comment response did not confirm the created comment: {value}")
    })
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
            gitlab_project_id(Some("group/project"), "MR comment").as_deref(),
            Ok("group/project")
        );
    }

    const MARKER: &str = "<!-- fallow-id: fallow-results -->";

    /// Another account can post a comment with the sticky marker. The post
    /// step updates only the comment of the bot.
    #[test]
    fn github_sticky_comment_skips_a_comment_of_another_author() {
        let comments = serde_json::json!([
            {
                "id": 1,
                "body": format!("{MARKER}\ntext of a person"),
                "user": { "login": "alice", "type": "User" }
            },
            {
                "id": 2,
                "body": format!("quoted: {MARKER}"),
                "user": { "login": "other[bot]", "type": "Bot" }
            },
            {
                "id": 3,
                "body": format!("{MARKER}\n### report"),
                "user": { "login": "github-actions[bot]", "type": "Bot" }
            }
        ]);
        let comments = comments.as_array().unwrap();

        let found = github_sticky_comment(comments, MARKER, BotLogin::Unset, None);
        assert_eq!(found.map(|c| c.id).as_deref(), Some("3"));

        let found = github_sticky_comment(comments, MARKER, BotLogin::Configured("alice"), None);
        assert_eq!(found.map(|c| c.id).as_deref(), Some("1"));

        let found = github_sticky_comment(comments, MARKER, BotLogin::Unset, Some("alice"));
        assert_eq!(found.map(|c| c.id).as_deref(), Some("1"));
        assert!(sticky_comment_needs_token_login(
            comments,
            MARKER,
            BotLogin::Unset
        ));
        assert!(!sticky_comment_needs_token_login(
            comments,
            MARKER,
            BotLogin::Configured("alice")
        ));
    }

    #[test]
    fn gitlab_sticky_note_skips_a_note_of_another_author() {
        let notes = serde_json::json!([
            {
                "id": 1,
                "body": format!("{MARKER}\ntext of a person"),
                "author": { "id": 7, "username": "alice", "bot": false }
            },
            {
                "id": 2,
                "system": true,
                "body": format!("added 1 commit\n\n{MARKER}"),
                "author": { "id": 7, "username": "alice" }
            },
            {
                "id": 3,
                "body": format!("{MARKER}\n### report"),
                "author": { "id": 9, "username": "ci-runner", "bot": false }
            }
        ]);
        let notes = notes.as_array().unwrap();
        let user = GitlabAuthenticatedUser {
            id: 9,
            username: "ci-runner".to_owned(),
        };

        assert!(sticky_note_needs_user(notes, MARKER, BotLogin::Unset));
        let found = gitlab_sticky_note(notes, MARKER, Some(&user), BotLogin::Unset);
        assert_eq!(found.map(|c| c.id).as_deref(), Some("3"));
        assert!(gitlab_sticky_note(notes, MARKER, None, BotLogin::Unset).is_none());
    }

    #[test]
    fn created_comment_response_requires_id_and_exact_body() {
        assert!(validate_created_comment(&serde_json::json!({}), "body", "GitHub").is_err());
        assert!(
            validate_created_comment(
                &serde_json::json!({ "id": 7, "body": "body" }),
                "body",
                "GitLab",
            )
            .is_ok()
        );
    }
}
