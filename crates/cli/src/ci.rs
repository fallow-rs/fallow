use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use fallow_config::OutputFormat;
use serde_json::Value;

use crate::api::{ResponseBodyReader, api_agent};
use crate::error::emit_error;

pub enum CiCommand {
    ReconcileReview {
        provider: CiProvider,
        target: Option<String>,
        envelope: PathBuf,
        repo: Option<String>,
        project_id: Option<String>,
        api_url: Option<String>,
        dry_run: bool,
    },
}

#[derive(Clone, Copy, Debug)]
pub enum CiProvider {
    Github,
    Gitlab,
}

pub fn run(command: CiCommand, output: OutputFormat) -> ExitCode {
    match command {
        CiCommand::ReconcileReview {
            provider,
            target,
            envelope,
            repo,
            project_id,
            api_url,
            dry_run,
        } => reconcile_review(
            provider,
            target.as_deref(),
            &envelope,
            ReconcileOptions {
                repo: repo.as_deref(),
                project_id: project_id.as_deref(),
                api_url: api_url.as_deref(),
                dry_run,
            },
            output,
        ),
    }
}

#[derive(Clone, Copy)]
struct ReconcileOptions<'a> {
    repo: Option<&'a str>,
    project_id: Option<&'a str>,
    api_url: Option<&'a str>,
    dry_run: bool,
}

fn reconcile_review(
    provider: CiProvider,
    target: Option<&str>,
    envelope: &Path,
    opts: ReconcileOptions<'_>,
    output: OutputFormat,
) -> ExitCode {
    let envelope = match read_envelope(envelope) {
        Ok(value) => value,
        Err(e) => {
            return emit_error(&e, 2, output);
        }
    };
    let current = envelope_fingerprints(&envelope);
    let state = match provider {
        CiProvider::Github => match load_github_state(target, opts) {
            Ok(state) => Some(state),
            Err(e) if opts.dry_run => {
                let plan = ReconcilePlan::without_provider(&current, e);
                return emit_reconcile_result(
                    provider,
                    target,
                    &envelope,
                    opts,
                    &plan,
                    &ApplyResult::default(),
                );
            }
            Err(e) => return emit_error(&e, crate::api::NETWORK_EXIT_CODE, output),
        },
        CiProvider::Gitlab => match load_gitlab_state(target, opts) {
            Ok(state) => Some(state),
            Err(e) if opts.dry_run => {
                let plan = ReconcilePlan::without_provider(&current, e);
                return emit_reconcile_result(
                    provider,
                    target,
                    &envelope,
                    opts,
                    &plan,
                    &ApplyResult::default(),
                );
            }
            Err(e) => return emit_error(&e, crate::api::NETWORK_EXIT_CODE, output),
        },
    };
    let Some(state) = state else {
        return emit_error(
            "internal error: provider state was not loaded for review reconciliation",
            2,
            output,
        );
    };
    let plan = reconcile_sets(&current, &state.fingerprints);

    let applied = if opts.dry_run {
        ApplyResult::default()
    } else {
        match provider {
            CiProvider::Github => apply_github_reconcile(&plan, &state, target, opts),
            CiProvider::Gitlab => apply_gitlab_reconcile(&plan, &state, target, opts),
        }
    };

    emit_reconcile_result(provider, target, &envelope, opts, &plan, &applied)
}

fn emit_reconcile_result(
    provider: CiProvider,
    target: Option<&str>,
    envelope: &Value,
    opts: ReconcileOptions<'_>,
    plan: &ReconcilePlan,
    applied: &ApplyResult,
) -> ExitCode {
    crate::report::emit_json(
        &serde_json::json!({
            "schema": "fallow-review-reconcile/v1",
            "provider": match provider {
                CiProvider::Github => "github",
                CiProvider::Gitlab => "gitlab",
            },
            "target": target,
            "dry_run": opts.dry_run,
            "comments": envelope_comments_len(envelope),
            "current_fingerprints": plan.current.len(),
            "existing_fingerprints": plan.existing.len(),
            "new_fingerprints": plan.new.len(),
            "stale_fingerprints": plan.stale.len(),
            "new": &plan.new,
            "stale": &plan.stale,
            "provider_warning": &plan.provider_warning,
            "resolution_comments_posted": applied.resolution_comments_posted,
            "threads_resolved": applied.threads_resolved,
            "apply_errors": applied.errors,
        }),
        "review reconcile",
    )
}

fn read_envelope(path: &Path) -> Result<Value, String> {
    let data = std::fs::read_to_string(path)
        .map_err(|e| format!("failed to read review envelope '{}': {e}", path.display()))?;
    serde_json::from_str(&data)
        .map_err(|e| format!("failed to parse review envelope '{}': {e}", path.display()))
}

fn envelope_comments_len(value: &Value) -> usize {
    value
        .get("comments")
        .and_then(Value::as_array)
        .map_or(0, Vec::len)
}

fn envelope_fingerprints(value: &Value) -> BTreeSet<String> {
    value
        .get("comments")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|comment| comment.get("fingerprint").and_then(Value::as_str))
        .filter(|fingerprint| !fingerprint.trim().is_empty())
        .map(str::to_owned)
        .collect()
}

#[derive(Debug, Default)]
struct ProviderState {
    fingerprints: BTreeSet<String>,
    github_comments_by_fingerprint: BTreeMap<String, Vec<u64>>,
    github_threads_by_fingerprint: BTreeMap<String, Vec<String>>,
    github_resolved_markers: BTreeSet<String>,
    gitlab_discussions_by_fingerprint: BTreeMap<String, Vec<String>>,
    gitlab_resolved_markers: BTreeSet<String>,
}

#[derive(Debug, Default)]
struct ReconcilePlan {
    current: Vec<String>,
    existing: Vec<String>,
    new: Vec<String>,
    stale: Vec<String>,
    provider_warning: Option<String>,
}

impl ReconcilePlan {
    fn without_provider(current: &BTreeSet<String>, warning: String) -> Self {
        Self {
            current: current.iter().cloned().collect(),
            new: current.iter().cloned().collect(),
            provider_warning: Some(warning),
            ..Self::default()
        }
    }
}

fn reconcile_sets(current: &BTreeSet<String>, existing: &BTreeSet<String>) -> ReconcilePlan {
    ReconcilePlan {
        current: current.iter().cloned().collect(),
        existing: existing.iter().cloned().collect(),
        new: current.difference(existing).cloned().collect(),
        stale: existing.difference(current).cloned().collect(),
        provider_warning: None,
    }
}

#[derive(Debug, Default)]
struct ApplyResult {
    resolution_comments_posted: usize,
    threads_resolved: usize,
    errors: Vec<String>,
}

fn load_github_state(
    target: Option<&str>,
    opts: ReconcileOptions<'_>,
) -> Result<ProviderState, String> {
    let pr = require_target("GitHub pull request", target)?;
    let repo = opts
        .repo
        .map(str::to_owned)
        .or_else(|| std::env::var("GH_REPO").ok())
        .or_else(|| std::env::var("GITHUB_REPOSITORY").ok())
        .ok_or_else(|| {
            "GitHub reconciliation requires --repo, GH_REPO, or GITHUB_REPOSITORY".to_owned()
        })?;
    let token = github_token()?;
    let api = opts
        .api_url
        .unwrap_or("https://api.github.com")
        .trim_end_matches('/');
    let agent = api_agent();
    let mut state = ProviderState::default();

    for page in 1..=100 {
        let url = format!("{api}/repos/{repo}/pulls/{pr}/comments?per_page=100&page={page}");
        let value = github_get_json(&agent, &url, &token)?;
        let comments = value
            .as_array()
            .ok_or_else(|| "GitHub review comments response was not an array".to_owned())?;
        if comments.is_empty() {
            break;
        }
        for comment in comments {
            let body = comment.get("body").and_then(Value::as_str).unwrap_or("");
            if let Some(fingerprint) = extract_marker(body, "fallow-fingerprint:") {
                state.fingerprints.insert(fingerprint.clone());
                if let Some(id) = comment.get("id").and_then(Value::as_u64) {
                    state
                        .github_comments_by_fingerprint
                        .entry(fingerprint)
                        .or_default()
                        .push(id);
                }
            }
            if let Some(fingerprint) = extract_marker(body, "fallow-resolved-fingerprint:") {
                state.github_resolved_markers.insert(fingerprint);
            }
        }
        if comments.len() < 100 {
            break;
        }
    }

    load_github_review_threads(&mut state, &agent, &repo, pr, &token, api)?;
    Ok(state)
}

fn load_github_review_threads(
    state: &mut ProviderState,
    agent: &ureq::Agent,
    repo: &str,
    pr: &str,
    token: &str,
    api: &str,
) -> Result<(), String> {
    let (owner, name) = repo
        .split_once('/')
        .ok_or_else(|| format!("GitHub repo must be owner/name, got '{repo}'"))?;
    let number = pr
        .parse::<u64>()
        .map_err(|_| format!("GitHub PR must be numeric, got '{pr}'"))?;
    let mut cursor: Option<String> = None;
    for _ in 0..100 {
        let query = r"
query($owner:String!, $name:String!, $number:Int!, $cursor:String) {
  repository(owner:$owner, name:$name) {
    pullRequest(number:$number) {
      reviewThreads(first:100, after:$cursor) {
        nodes {
          id
          isResolved
          comments(first:50) {
            nodes { body }
          }
        }
        pageInfo { hasNextPage endCursor }
      }
    }
  }
}";
        let payload = serde_json::json!({
            "query": query,
            "variables": {
                "owner": owner,
                "name": name,
                "number": number,
                "cursor": cursor,
            }
        });
        let value = github_post_json(agent, &format!("{api}/graphql"), token, &payload)?;
        if value.get("errors").is_some() {
            return Err(format!(
                "GitHub GraphQL reviewThreads query failed: {value}"
            ));
        }
        let threads = value
            .pointer("/data/repository/pullRequest/reviewThreads/nodes")
            .and_then(Value::as_array)
            .ok_or_else(|| "GitHub reviewThreads response did not contain nodes".to_owned())?;
        for thread in threads {
            if thread
                .get("isResolved")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                continue;
            }
            let Some(thread_id) = thread.get("id").and_then(Value::as_str) else {
                continue;
            };
            let comments = thread
                .pointer("/comments/nodes")
                .and_then(Value::as_array)
                .into_iter()
                .flatten();
            for comment in comments {
                let body = comment.get("body").and_then(Value::as_str).unwrap_or("");
                if let Some(fingerprint) = extract_marker(body, "fallow-fingerprint:") {
                    state
                        .github_threads_by_fingerprint
                        .entry(fingerprint)
                        .or_default()
                        .push(thread_id.to_owned());
                }
            }
        }
        let page_info = value
            .pointer("/data/repository/pullRequest/reviewThreads/pageInfo")
            .unwrap_or(&Value::Null);
        if !page_info
            .get("hasNextPage")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            break;
        }
        cursor = page_info
            .get("endCursor")
            .and_then(Value::as_str)
            .map(str::to_owned);
    }
    Ok(())
}

fn apply_github_reconcile(
    plan: &ReconcilePlan,
    state: &ProviderState,
    target: Option<&str>,
    opts: ReconcileOptions<'_>,
) -> ApplyResult {
    let mut result = ApplyResult::default();
    let pr = target.unwrap_or_default();
    let repo = opts
        .repo
        .map(str::to_owned)
        .or_else(|| std::env::var("GH_REPO").ok())
        .or_else(|| std::env::var("GITHUB_REPOSITORY").ok())
        .unwrap_or_default();
    let token = match github_token() {
        Ok(token) => token,
        Err(e) => {
            result.errors.push(e);
            return result;
        }
    };
    let api = opts
        .api_url
        .unwrap_or("https://api.github.com")
        .trim_end_matches('/');
    let agent = api_agent();
    let sha = std::env::var("GITHUB_SHA")
        .ok()
        .or_else(|| std::env::var("PR_HEAD_SHA").ok());

    for fingerprint in &plan.stale {
        if !state.github_resolved_markers.contains(fingerprint) {
            for comment_id in state
                .github_comments_by_fingerprint
                .get(fingerprint)
                .into_iter()
                .flatten()
            {
                let body = resolved_body(fingerprint, sha.as_deref());
                let payload = serde_json::json!({ "body": body });
                let url = format!("{api}/repos/{repo}/pulls/{pr}/comments/{comment_id}/replies");
                match github_post_json(&agent, &url, &token, &payload) {
                    Ok(_) => result.resolution_comments_posted += 1,
                    Err(e) => result.errors.push(e),
                }
            }
        }
        for thread_id in state
            .github_threads_by_fingerprint
            .get(fingerprint)
            .into_iter()
            .flatten()
        {
            let payload = serde_json::json!({
                "query": "mutation($threadId:ID!){resolveReviewThread(input:{threadId:$threadId}){thread{id isResolved}}}",
                "variables": { "threadId": thread_id },
            });
            match github_post_json(&agent, &format!("{api}/graphql"), &token, &payload) {
                Ok(value) if value.get("errors").is_none() => result.threads_resolved += 1,
                Ok(value) => result
                    .errors
                    .push(format!("GitHub resolveReviewThread failed: {value}")),
                Err(e) => result.errors.push(e),
            }
        }
    }
    result
}

fn load_gitlab_state(
    target: Option<&str>,
    opts: ReconcileOptions<'_>,
) -> Result<ProviderState, String> {
    let mr = require_target("GitLab merge request", target)?;
    let project_id = opts
        .project_id
        .map(str::to_owned)
        .or_else(|| std::env::var("CI_PROJECT_ID").ok())
        .ok_or_else(|| "GitLab reconciliation requires --project-id or CI_PROJECT_ID".to_owned())?;
    let token = std::env::var("GITLAB_TOKEN")
        .map_err(|_| "GitLab reconciliation requires GITLAB_TOKEN".to_owned())?;
    let api = opts
        .api_url
        .map(str::to_owned)
        .or_else(|| std::env::var("CI_API_V4_URL").ok())
        .unwrap_or_else(|| "https://gitlab.com/api/v4".to_owned());
    let api = api.trim_end_matches('/').to_owned();
    let agent = api_agent();
    let mut state = ProviderState::default();

    for page in 1..=100 {
        let url = format!(
            "{api}/projects/{}/merge_requests/{mr}/discussions?per_page=100&page={page}",
            url_encode_path_segment(&project_id)
        );
        let value = gitlab_get_json(&agent, &url, &token)?;
        let discussions = value
            .as_array()
            .ok_or_else(|| "GitLab discussions response was not an array".to_owned())?;
        if discussions.is_empty() {
            break;
        }
        for discussion in discussions {
            let Some(discussion_id) = discussion.get("id").and_then(Value::as_str) else {
                continue;
            };
            let notes = discussion
                .get("notes")
                .and_then(Value::as_array)
                .into_iter()
                .flatten();
            for note in notes {
                let body = note.get("body").and_then(Value::as_str).unwrap_or("");
                if let Some(fingerprint) = extract_marker(body, "fallow-fingerprint:") {
                    state.fingerprints.insert(fingerprint.clone());
                    state
                        .gitlab_discussions_by_fingerprint
                        .entry(fingerprint)
                        .or_default()
                        .push(discussion_id.to_owned());
                }
                if let Some(fingerprint) = extract_marker(body, "fallow-resolved-fingerprint:") {
                    state.gitlab_resolved_markers.insert(fingerprint);
                }
            }
        }
        if discussions.len() < 100 {
            break;
        }
    }
    Ok(state)
}

fn apply_gitlab_reconcile(
    plan: &ReconcilePlan,
    state: &ProviderState,
    target: Option<&str>,
    opts: ReconcileOptions<'_>,
) -> ApplyResult {
    let mut result = ApplyResult::default();
    let mr = target.unwrap_or_default();
    let project_id = opts
        .project_id
        .map(str::to_owned)
        .or_else(|| std::env::var("CI_PROJECT_ID").ok())
        .unwrap_or_default();
    let Ok(token) = std::env::var("GITLAB_TOKEN") else {
        result
            .errors
            .push("GitLab reconciliation requires GITLAB_TOKEN".to_owned());
        return result;
    };
    let api = opts
        .api_url
        .map(str::to_owned)
        .or_else(|| std::env::var("CI_API_V4_URL").ok())
        .unwrap_or_else(|| "https://gitlab.com/api/v4".to_owned());
    let api = api.trim_end_matches('/').to_owned();
    let agent = api_agent();
    let sha = std::env::var("CI_COMMIT_SHA").ok();
    let encoded_project = url_encode_path_segment(&project_id);

    for fingerprint in &plan.stale {
        for discussion_id in state
            .gitlab_discussions_by_fingerprint
            .get(fingerprint)
            .into_iter()
            .flatten()
        {
            if !state.gitlab_resolved_markers.contains(fingerprint) {
                let body = resolved_body(fingerprint, sha.as_deref());
                let payload = serde_json::json!({ "body": body });
                let url = format!(
                    "{api}/projects/{encoded_project}/merge_requests/{mr}/discussions/{discussion_id}/notes"
                );
                match gitlab_post_json(&agent, &url, &token, &payload) {
                    Ok(_) => result.resolution_comments_posted += 1,
                    Err(e) => result.errors.push(e),
                }
            }
            let payload = serde_json::json!({ "resolved": true });
            let url = format!(
                "{api}/projects/{encoded_project}/merge_requests/{mr}/discussions/{discussion_id}"
            );
            match gitlab_put_json(&agent, &url, &token, &payload) {
                Ok(_) => result.threads_resolved += 1,
                Err(e) => result.errors.push(e),
            }
        }
    }
    result
}

fn require_target<'a>(label: &str, target: Option<&'a str>) -> Result<&'a str, String> {
    target
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("{label} id is required"))
}

fn github_token() -> Result<String, String> {
    std::env::var("GH_TOKEN")
        .or_else(|_| std::env::var("GITHUB_TOKEN"))
        .map_err(|_| "GitHub reconciliation requires GH_TOKEN or GITHUB_TOKEN".to_owned())
}

fn github_get_json(agent: &ureq::Agent, url: &str, token: &str) -> Result<Value, String> {
    let mut response = agent
        .get(url)
        .header("Authorization", &format!("Bearer {token}"))
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .header("User-Agent", "fallow-cli")
        .call()
        .map_err(|e| format!("GitHub request failed: {e}"))?;
    read_json_response(&mut response, "GitHub")
}

fn github_post_json(
    agent: &ureq::Agent,
    url: &str,
    token: &str,
    payload: &Value,
) -> Result<Value, String> {
    let mut response = agent
        .post(url)
        .header("Authorization", &format!("Bearer {token}"))
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .header("User-Agent", "fallow-cli")
        .send_json(payload)
        .map_err(|e| format!("GitHub request failed: {e}"))?;
    read_json_response(&mut response, "GitHub")
}

fn gitlab_get_json(agent: &ureq::Agent, url: &str, token: &str) -> Result<Value, String> {
    let mut response = agent
        .get(url)
        .header("PRIVATE-TOKEN", token)
        .header("User-Agent", "fallow-cli")
        .call()
        .map_err(|e| format!("GitLab request failed: {e}"))?;
    read_json_response(&mut response, "GitLab")
}

fn gitlab_post_json(
    agent: &ureq::Agent,
    url: &str,
    token: &str,
    payload: &Value,
) -> Result<Value, String> {
    let mut response = agent
        .post(url)
        .header("PRIVATE-TOKEN", token)
        .header("Content-Type", "application/json")
        .header("User-Agent", "fallow-cli")
        .send_json(payload)
        .map_err(|e| format!("GitLab request failed: {e}"))?;
    read_json_response(&mut response, "GitLab")
}

fn gitlab_put_json(
    agent: &ureq::Agent,
    url: &str,
    token: &str,
    payload: &Value,
) -> Result<Value, String> {
    let mut response = agent
        .put(url)
        .header("PRIVATE-TOKEN", token)
        .header("Content-Type", "application/json")
        .header("User-Agent", "fallow-cli")
        .send_json(payload)
        .map_err(|e| format!("GitLab request failed: {e}"))?;
    read_json_response(&mut response, "GitLab")
}

fn read_json_response(
    response: &mut impl ResponseBodyReader,
    provider: &str,
) -> Result<Value, String> {
    if !(200..300).contains(&response.status()) {
        let status = response.status();
        let body = response.read_to_string().unwrap_or_default();
        return Err(format!(
            "{provider} request failed with HTTP {status}: {}",
            body.trim()
        ));
    }
    response
        .read_json::<Value>()
        .map_err(|e| format!("{provider} response was not valid JSON: {e}"))
}

fn extract_marker(body: &str, marker: &str) -> Option<String> {
    let rest = body.split(marker).nth(1)?.trim_start();
    let value = rest
        .split(|c: char| c.is_ascii_whitespace() || c == '<')
        .next()?
        .trim_matches('-')
        .trim();
    (!value.is_empty()).then(|| value.to_owned())
}

fn resolved_body(fingerprint: &str, sha: Option<&str>) -> String {
    match sha.and_then(|value| value.get(..7)) {
        Some(short) => {
            format!("Resolved in `{short}`.\n\n<!-- fallow-resolved-fingerprint: {fingerprint} -->")
        }
        None => format!("Resolved.\n\n<!-- fallow-resolved-fingerprint: {fingerprint} -->"),
    }
}

fn url_encode_path_segment(value: &str) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(char::from(byte));
            }
            _ => {
                use std::fmt::Write as _;
                write!(&mut out, "%{byte:02X}").expect("write to string");
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_fingerprint_marker() {
        assert_eq!(
            extract_marker(
                "**error**\n\n<!-- fallow-fingerprint: abc123 -->",
                "fallow-fingerprint:",
            )
            .as_deref(),
            Some("abc123")
        );
    }

    #[test]
    fn computes_reconcile_sets() {
        let current = BTreeSet::from(["a".to_owned(), "b".to_owned()]);
        let existing = BTreeSet::from(["b".to_owned(), "c".to_owned()]);
        let plan = reconcile_sets(&current, &existing);
        assert_eq!(plan.new, vec!["a"]);
        assert_eq!(plan.stale, vec!["c"]);
    }

    #[test]
    fn encodes_gitlab_project_path_as_one_segment() {
        assert_eq!(url_encode_path_segment("group/project"), "group%2Fproject");
    }

    #[test]
    fn resolved_body_includes_short_sha_and_marker() {
        let body = resolved_body("abc", Some("1234567890"));
        assert!(body.contains("`1234567`"));
        assert!(body.contains("fallow-resolved-fingerprint: abc"));
    }
}
