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
            // Only honour resolved-fingerprint markers when the comment was
            // posted by a bot. A human commenter who pastes the marker into
            // their own comment could otherwise trick the apply step into
            // skipping a real "Resolved in `<sha>`" reply on a stale finding.
            if is_github_bot_comment(comment)
                && let Some(fingerprint) = extract_marker(body, "fallow-resolved-fingerprint:")
            {
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
        // Idempotency: check the (fingerprint, sha) marker, not the bare
        // fingerprint. Re-runs on the same commit must not post duplicate
        // "Resolved in `<sha>`" replies; legacy markers without a SHA suffix
        // still match on bare fingerprint to keep first-run-after-upgrade
        // clean.
        let marker_key = resolved_marker_key(fingerprint, sha.as_deref());
        let already_resolved = state.github_resolved_markers.contains(&marker_key)
            || state.github_resolved_markers.contains(fingerprint);
        if !already_resolved {
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
                // Same authorship gate as GitHub: only honour resolved
                // markers from bot-authored notes so a human cannot suppress
                // legitimate "Resolved in `<sha>`" replies by impersonating the
                // marker in their own comment.
                if is_gitlab_bot_note(note)
                    && let Some(fingerprint) = extract_marker(body, "fallow-resolved-fingerprint:")
                {
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
        // Idempotency: same approach as GitHub apply. (fingerprint, sha)
        // marker, with bare-fingerprint legacy fallback.
        let marker_key = resolved_marker_key(fingerprint, sha.as_deref());
        let already_resolved = state.gitlab_resolved_markers.contains(&marker_key)
            || state.gitlab_resolved_markers.contains(fingerprint);
        for discussion_id in state
            .gitlab_discussions_by_fingerprint
            .get(fingerprint)
            .into_iter()
            .flatten()
        {
            if !already_resolved {
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
    with_rate_limit_retry("GitHub", || {
        agent
            .get(url)
            .header("Authorization", &format!("Bearer {token}"))
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .header("User-Agent", "fallow-cli")
            .call()
    })
}

fn github_post_json(
    agent: &ureq::Agent,
    url: &str,
    token: &str,
    payload: &Value,
) -> Result<Value, String> {
    with_rate_limit_retry("GitHub", || {
        agent
            .post(url)
            .header("Authorization", &format!("Bearer {token}"))
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .header("User-Agent", "fallow-cli")
            .send_json(payload)
    })
}

fn gitlab_get_json(agent: &ureq::Agent, url: &str, token: &str) -> Result<Value, String> {
    with_rate_limit_retry("GitLab", || {
        agent
            .get(url)
            .header("PRIVATE-TOKEN", token)
            .header("User-Agent", "fallow-cli")
            .call()
    })
}

fn gitlab_post_json(
    agent: &ureq::Agent,
    url: &str,
    token: &str,
    payload: &Value,
) -> Result<Value, String> {
    with_rate_limit_retry("GitLab", || {
        agent
            .post(url)
            .header("PRIVATE-TOKEN", token)
            .header("Content-Type", "application/json")
            .header("User-Agent", "fallow-cli")
            .send_json(payload)
    })
}

fn gitlab_put_json(
    agent: &ureq::Agent,
    url: &str,
    token: &str,
    payload: &Value,
) -> Result<Value, String> {
    with_rate_limit_retry("GitLab", || {
        agent
            .put(url)
            .header("PRIVATE-TOKEN", token)
            .header("Content-Type", "application/json")
            .header("User-Agent", "fallow-cli")
            .send_json(payload)
    })
}

/// Maximum per-attempt sleep, even when the server's `Retry-After` is larger.
///
/// A misbehaving server (or a malicious upstream proxy) sending
/// `Retry-After: 86400` would otherwise stall the runner for a whole day.
/// 60s is enough headroom for genuine GitHub / GitLab rate-limit recovery
/// while bounding worst-case workflow latency at `RETRY_MAX_WAIT_SECONDS *
/// FALLOW_API_RETRIES = 180s` for the default retry count.
const RETRY_MAX_WAIT_SECONDS: u64 = 60;

/// Return `true` for HTTP statuses worth retrying (rate-limit + transient
/// 5xx). Persistent server faults (`500`, `501`) and all 4xx other than `429`
/// surface immediately so a real bug doesn't burn the full retry budget.
const fn should_retry_status(status: u16) -> bool {
    status == 429 || matches!(status, 502..=504)
}

/// Wrap an HTTP request closure with rate-limit + transient-5xx retry.
///
/// Mirrors the bash `gh_api_retry` / `curl_retry` helpers in the action and
/// CI scripts so the binary is no less robust than the bash glue around it
/// when a workflow re-runs against a rate-limited GitHub Enterprise or a
/// GitLab instance under load. Retries on `429 Too Many Requests` and on
/// `502/503/504` (Bad Gateway, Service Unavailable, Gateway Timeout); other
/// 5xx codes (`500`, `501`, ...) surface immediately so persistent server
/// faults don't burn the full retry budget.
///
/// `FALLOW_API_RETRIES` (default 3) caps the total attempts; `FALLOW_API_RETRY_DELAY`
/// (default 2s) is the floor between attempts. The actual sleep uses
/// `Retry-After` from the server when present, falling back to the floor;
/// either way it's clamped to `RETRY_MAX_WAIT_SECONDS` so a runaway server
/// can't strand the runner.
fn with_rate_limit_retry<F>(provider: &str, mut op: F) -> Result<Value, String>
where
    F: FnMut() -> Result<http::Response<ureq::Body>, ureq::Error>,
{
    let max_attempts = retries_from_env();
    let floor_delay = retry_delay_from_env();
    let mut attempt: u32 = 0;
    loop {
        attempt += 1;
        match op() {
            Ok(mut response) => {
                let status = response.status().as_u16();
                if should_retry_status(status) && attempt < max_attempts {
                    let wait = compute_retry_wait(response.headers(), floor_delay, provider);
                    let label = if status == 429 {
                        "rate-limited"
                    } else {
                        "transient server error"
                    };
                    eprintln!(
                        "fallow: {provider} {label} ({status}); retrying in {wait}s ({attempt}/{max_attempts})"
                    );
                    std::thread::sleep(std::time::Duration::from_secs(wait));
                    continue;
                }
                return read_json_response(&mut response, provider);
            }
            Err(e) => return Err(format!("{provider} request failed: {e}")),
        }
    }
}

/// Pick a sleep duration for a 429 retry attempt.
///
/// Precedence (highest first):
/// 1. `Retry-After` integer-seconds, clamped to `[1, RETRY_MAX_WAIT_SECONDS]`.
/// 2. `Retry-After` HTTP-date: not parsed; emit a one-time warning and fall
///    back to the floor delay so the user knows their server's Retry-After
///    contract was ignored.
/// 3. `floor_delay` from `FALLOW_API_RETRY_DELAY`, clamped to the ceiling.
fn compute_retry_wait(headers: &http::HeaderMap, floor_delay: u64, provider: &str) -> u64 {
    if let Some(seconds) = parse_retry_after(headers) {
        return seconds.clamp(1, RETRY_MAX_WAIT_SECONDS);
    }
    if let Some(raw) = headers
        .get("Retry-After")
        .and_then(|value| value.to_str().ok())
    {
        eprintln!(
            "fallow: {provider} returned non-numeric Retry-After {raw:?}; \
             falling back to {floor_delay}s floor"
        );
    }
    floor_delay.clamp(1, RETRY_MAX_WAIT_SECONDS)
}

fn retries_from_env() -> u32 {
    std::env::var("FALLOW_API_RETRIES")
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(3)
}

fn retry_delay_from_env() -> u64 {
    std::env::var("FALLOW_API_RETRY_DELAY")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(2)
}

fn parse_retry_after(headers: &http::HeaderMap) -> Option<u64> {
    let header = headers.get("Retry-After")?;
    let raw = header.to_str().ok()?.trim();
    raw.parse::<u64>().ok()
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

/// Determine whether a GitHub PR review comment was authored by a bot account.
///
/// We trust resolved-fingerprint markers only from bot identities so a human
/// commenter can't paste `<!-- fallow-resolved-fingerprint: <fp> -->` into
/// their own comment and trick the apply step into skipping a legitimate
/// "Resolved in `<sha>`" reply on a stale finding.
///
/// GitHub identifies bot identities through `user.type == "Bot"` (e.g.
/// `github-actions[bot]`, `dependabot[bot]`, custom GitHub Apps). The
/// fallback `FALLOW_BOT_LOGIN` env var lets self-hosted runners pin a
/// specific human-account login that posts on behalf of fallow when no Bot
/// type is available (uncommon but supported for legacy setups).
fn is_github_bot_comment(comment: &Value) -> bool {
    let user = comment.get("user");
    let user_type = user.and_then(|u| u.get("type")).and_then(Value::as_str);
    if user_type == Some("Bot") {
        return true;
    }
    let login = user.and_then(|u| u.get("login")).and_then(Value::as_str);
    if let Some(login) = login
        && let Ok(allow) = std::env::var("FALLOW_BOT_LOGIN")
        && !allow.trim().is_empty()
        && login == allow.trim()
    {
        return true;
    }
    false
}

/// Determine whether a GitLab MR discussion note was authored by a bot.
///
/// GitLab marks bot-authored notes with `system: true` (system-generated)
/// or, for project access tokens, the author's `bot: true` flag. Personal
/// access tokens posting on behalf of a human carry the human's identity;
/// callers that use a PAT must set `FALLOW_BOT_LOGIN` to the human's
/// username (or the project access token's bot username) to opt in.
fn is_gitlab_bot_note(note: &Value) -> bool {
    if note.get("system").and_then(Value::as_bool).unwrap_or(false) {
        return true;
    }
    let author = note.get("author");
    if author
        .and_then(|a| a.get("bot"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return true;
    }
    let username = author
        .and_then(|a| a.get("username"))
        .and_then(Value::as_str);
    if let Some(username) = username
        && let Ok(allow) = std::env::var("FALLOW_BOT_LOGIN")
        && !allow.trim().is_empty()
        && username == allow.trim()
    {
        return true;
    }
    false
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

/// Compute the idempotency marker for a (fingerprint, sha) pair. The marker
/// is what we look up to decide whether a resolution comment for this
/// fingerprint at this commit already exists, so re-runs of the workflow on
/// the same commit don't post duplicate "Resolved in `<sha>`" comments.
fn resolved_marker_key(fingerprint: &str, sha: Option<&str>) -> String {
    match sha.and_then(|value| value.get(..7)) {
        Some(short) => format!("{fingerprint}@{short}"),
        None => fingerprint.to_owned(),
    }
}

fn resolved_body(fingerprint: &str, sha: Option<&str>) -> String {
    let marker = resolved_marker_key(fingerprint, sha);
    match sha.and_then(|value| value.get(..7)) {
        Some(short) => {
            format!("Resolved in `{short}`.\n\n<!-- fallow-resolved-fingerprint: {marker} -->")
        }
        None => format!("Resolved.\n\n<!-- fallow-resolved-fingerprint: {marker} -->"),
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

    fn headers_with_retry_after(value: &'static str) -> http::HeaderMap {
        let mut map = http::HeaderMap::new();
        map.insert("Retry-After", http::HeaderValue::from_static(value));
        map
    }

    #[test]
    fn github_bot_check_accepts_bot_user_type() {
        let comment = serde_json::json!({
            "user": { "type": "Bot", "login": "github-actions[bot]" },
        });
        assert!(is_github_bot_comment(&comment));
    }

    #[test]
    fn github_bot_check_rejects_human_user_type() {
        // Critical security test: a human pasting a resolved-fingerprint
        // marker into their own comment must not be honoured.
        let comment = serde_json::json!({
            "user": { "type": "User", "login": "alice" },
            "body": "<!-- fallow-resolved-fingerprint: abc123 -->",
        });
        assert!(!is_github_bot_comment(&comment));
    }

    #[test]
    #[allow(unsafe_code, reason = "test-only env mutation, single-threaded run")]
    fn github_bot_check_accepts_explicit_login_override() {
        let comment = serde_json::json!({
            "user": { "type": "User", "login": "fallow-bot-account" },
        });
        // SAFETY: tests run sequentially within the bin target.
        unsafe {
            std::env::set_var("FALLOW_BOT_LOGIN", "fallow-bot-account");
        }
        assert!(is_github_bot_comment(&comment));
        // SAFETY: see above.
        unsafe {
            std::env::remove_var("FALLOW_BOT_LOGIN");
        }
    }

    #[test]
    fn gitlab_bot_check_accepts_system_and_bot_flag() {
        let system_note = serde_json::json!({ "system": true });
        assert!(is_gitlab_bot_note(&system_note));
        let bot_author = serde_json::json!({
            "system": false,
            "author": { "bot": true, "username": "project-bot" },
        });
        assert!(is_gitlab_bot_note(&bot_author));
    }

    #[test]
    fn gitlab_bot_check_rejects_human_author() {
        // Same security premise as GitHub.
        let human = serde_json::json!({
            "system": false,
            "author": { "bot": false, "username": "alice" },
        });
        assert!(!is_gitlab_bot_note(&human));
    }

    #[test]
    fn parse_retry_after_reads_integer_seconds() {
        assert_eq!(parse_retry_after(&headers_with_retry_after("12")), Some(12));
    }

    #[test]
    fn parse_retry_after_returns_none_for_missing_header() {
        assert_eq!(parse_retry_after(&http::HeaderMap::new()), None);
    }

    #[test]
    fn compute_retry_wait_clamps_huge_retry_after() {
        // A malicious or misconfigured server returning a day-long
        // Retry-After must NOT strand the runner.
        let headers = headers_with_retry_after("86400");
        assert_eq!(
            compute_retry_wait(&headers, 2, "GitHub"),
            RETRY_MAX_WAIT_SECONDS
        );
    }

    #[test]
    fn compute_retry_wait_clamps_zero_retry_after() {
        // A zero Retry-After (no wait) is a server bug; floor at 1s so we
        // don't tight-loop.
        let headers = headers_with_retry_after("0");
        assert_eq!(compute_retry_wait(&headers, 5, "GitLab"), 1);
    }

    #[test]
    fn compute_retry_wait_falls_back_to_floor_for_http_date() {
        // HTTP-date Retry-After values aren't parsed; we fall back to the
        // floor with a stderr warning (asserted via the public delay value).
        let headers = headers_with_retry_after("Wed, 21 Oct 2026 07:28:00 GMT");
        assert_eq!(compute_retry_wait(&headers, 7, "GitHub"), 7);
    }

    #[test]
    fn parse_retry_after_returns_none_for_http_date() {
        // Per RFC 9110 the header may carry an HTTP-date; we don't parse
        // those, the caller falls back to the floor delay.
        assert_eq!(
            parse_retry_after(&headers_with_retry_after("Wed, 21 Oct 2026 07:28:00 GMT")),
            None
        );
    }

    #[test]
    fn should_retry_status_covers_429_and_transient_5xx() {
        // 429 (rate-limit) and 502/503/504 (transient gateway errors) are the
        // statuses both bash gh_api_retry / curl_retry helpers and this
        // function retry on. Reverting the 5xx branch to 429-only would fail
        // the 502/503/504 assertions.
        assert!(should_retry_status(429));
        assert!(should_retry_status(502));
        assert!(should_retry_status(503));
        assert!(should_retry_status(504));
    }

    #[test]
    fn should_retry_status_skips_persistent_5xx_and_4xx() {
        // Persistent server faults (500, 501) and all 4xx other than 429
        // surface immediately so a real bug doesn't burn the full retry
        // budget on the runner.
        assert!(!should_retry_status(500));
        assert!(!should_retry_status(501));
        assert!(!should_retry_status(505));
        assert!(!should_retry_status(400));
        assert!(!should_retry_status(401));
        assert!(!should_retry_status(403));
        assert!(!should_retry_status(404));
        assert!(!should_retry_status(422));
        assert!(!should_retry_status(200));
    }

    #[test]
    fn resolved_marker_key_includes_short_sha() {
        // (fingerprint, sha) marker keeps re-runs idempotent on the same
        // commit while letting a force-push to a new SHA produce a fresh
        // resolution comment.
        assert_eq!(
            resolved_marker_key("abc", Some("1234567890")),
            "abc@1234567"
        );
        assert_eq!(resolved_marker_key("abc", None), "abc");
        assert_ne!(
            resolved_marker_key("abc", Some("1111111")),
            resolved_marker_key("abc", Some("2222222"))
        );
    }

    #[test]
    fn resolved_body_includes_short_sha_and_per_sha_marker() {
        let body = resolved_body("abc", Some("1234567890"));
        assert!(body.contains("`1234567`"));
        // Marker now encodes both fingerprint AND short SHA so re-runs on
        // the same commit can detect prior posts; force-push to new SHA
        // produces a new marker.
        assert!(body.contains("fallow-resolved-fingerprint: abc@1234567"));
    }
}
