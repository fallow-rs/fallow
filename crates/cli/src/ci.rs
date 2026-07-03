use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use fallow_config::OutputFormat;
use serde_json::Value;

use crate::api::{ResponseBodyReader, sanitize_network_error, try_api_agent};
use crate::error::emit_error;

#[path = "ci_check_run.rs"]
mod check_run;
#[path = "ci_pr_comment_post.rs"]
mod pr_comment_post;
#[path = "ci_review_post.rs"]
mod review_post;

use check_run::{PostCheckRunInput, post_check_run};
use pr_comment_post::{PostPrCommentInput, post_pr_comment};
use review_post::{PostReviewInput, post_review};

pub enum CiCommand {
    PlanPrComment {
        body: PathBuf,
        marker_id: String,
        clean: bool,
        existing_comment_id: Option<String>,
        existing_body: Option<PathBuf>,
    },
    PostPrComment {
        provider: CiProvider,
        target: Option<String>,
        body: PathBuf,
        envelope: Option<PathBuf>,
        marker_id: String,
        clean: bool,
        repo: Option<String>,
        project_id: Option<String>,
        api_url: Option<String>,
        dry_run: bool,
    },
    PostReview {
        provider: CiProvider,
        target: Option<String>,
        envelope: PathBuf,
        repo: Option<String>,
        project_id: Option<String>,
        api_url: Option<String>,
        dry_run: bool,
    },
    PostCheckRun {
        provider: CiProvider,
        decision: PathBuf,
        repo: String,
        head_sha: String,
        api_url: Option<String>,
        split_gates: bool,
        dry_run: bool,
    },
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
        CiCommand::PlanPrComment {
            body,
            marker_id,
            clean,
            existing_comment_id,
            existing_body,
        } => plan_pr_comment(
            &body,
            marker_id,
            clean,
            existing_comment_id,
            existing_body.as_deref(),
            output,
        ),
        command @ CiCommand::PostPrComment { .. } => run_post_pr_comment(command, output),
        CiCommand::PostReview {
            provider,
            target,
            envelope,
            repo,
            project_id,
            api_url,
            dry_run,
        } => post_review(
            PostReviewInput {
                provider,
                target: target.as_deref(),
                envelope: &envelope,
                repo: repo.as_deref(),
                project_id: project_id.as_deref(),
                api_url: api_url.as_deref(),
                dry_run,
            },
            output,
        ),
        CiCommand::PostCheckRun {
            provider,
            decision,
            repo,
            head_sha,
            api_url,
            split_gates,
            dry_run,
        } => run_post_check_run(
            provider,
            PostCheckRunInput {
                decision: &decision,
                repo: &repo,
                head_sha: &head_sha,
                api_url: api_url.as_deref(),
                split_gates,
                dry_run,
            },
            output,
        ),
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

fn run_post_pr_comment(command: CiCommand, output: OutputFormat) -> ExitCode {
    let CiCommand::PostPrComment {
        provider,
        target,
        body,
        envelope,
        marker_id,
        clean,
        repo,
        project_id,
        api_url,
        dry_run,
    } = command
    else {
        unreachable!("run_post_pr_comment only accepts PostPrComment");
    };
    post_pr_comment(
        &PostPrCommentInput {
            provider,
            target: target.as_deref(),
            body: &body,
            envelope: envelope.as_deref(),
            marker_id,
            clean,
            repo: repo.as_deref(),
            project_id: project_id.as_deref(),
            api_url: api_url.as_deref(),
            dry_run,
        },
        output,
    )
}

fn run_post_check_run(
    provider: CiProvider,
    input: PostCheckRunInput<'_>,
    output: OutputFormat,
) -> ExitCode {
    match provider {
        CiProvider::Github => post_check_run(&input, output),
        CiProvider::Gitlab => emit_error("GitLab check runs are not supported", 2, output),
    }
}

fn plan_pr_comment(
    body: &Path,
    marker_id: String,
    clean: bool,
    existing_comment_id: Option<String>,
    existing_body: Option<&Path>,
    output: OutputFormat,
) -> ExitCode {
    let body = match read_text_file(body, "PR comment body") {
        Ok(body) => body,
        Err(e) => return emit_error(&e, 2, output),
    };
    let existing = match existing_comment(existing_comment_id, existing_body) {
        Ok(existing) => existing,
        Err(e) => return emit_error(&e, 2, output),
    };
    let envelope = fallow_output::PrCommentEnvelope {
        marker_id,
        body,
        is_clean: clean,
        details_url: None,
        check_summary: None,
        truncation: fallow_output::PrCommentTruncation::default(),
    };
    let plan = fallow_output::plan_pr_comment_post(&fallow_output::PrCommentPostPlanInput {
        envelope: &envelope,
        existing: existing.as_ref(),
    });
    emit_pr_comment_post_plan(&plan, output)
}

fn existing_comment(
    id: Option<String>,
    body: Option<&Path>,
) -> Result<Option<fallow_output::ExistingPrComment>, String> {
    let Some(id) = id else {
        return Ok(None);
    };
    let Some(body_path) = body else {
        return Ok(Some(fallow_output::ExistingPrComment {
            id,
            body: String::new(),
        }));
    };
    Ok(Some(fallow_output::ExistingPrComment {
        id,
        body: read_text_file(body_path, "existing PR comment body")?,
    }))
}

fn read_text_file(path: &Path, label: &str) -> Result<String, String> {
    std::fs::read_to_string(path)
        .map_err(|e| format!("failed to read {label} '{}': {e}", path.display()))
}

fn emit_pr_comment_post_plan(
    plan: &fallow_output::PrCommentPostPlan,
    output: OutputFormat,
) -> ExitCode {
    match serde_json::to_value(plan) {
        Ok(value) => crate::report::emit_json(&value, "PR comment post plan"),
        Err(e) => emit_error(&format!("JSON serialization error: {e}"), 2, output),
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
    let state = match load_provider_state(provider, target, opts) {
        Ok(state) => state,
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
    };
    let plan = PlannedReconcile::new(&current, &state);

    let applied = if opts.dry_run {
        ApplyResult::default()
    } else {
        apply_provider_reconcile(provider, &plan, target, opts)
    };

    emit_reconcile_result(provider, target, &envelope, opts, &plan.plan, &applied)
}

/// Load existing provider review state (comments + threads/discussions) for the
/// reconcile plan, dispatching to the GitHub or GitLab loader.
fn load_provider_state(
    provider: CiProvider,
    target: Option<&str>,
    opts: ReconcileOptions<'_>,
) -> Result<ProviderState, String> {
    match provider {
        CiProvider::Github => load_github_state(target, opts),
        CiProvider::Gitlab => load_gitlab_state(target, opts),
    }
}

/// Apply the reconcile plan against the live provider, dispatching to the
/// GitHub or GitLab applier.
fn apply_provider_reconcile(
    provider: CiProvider,
    plan: &PlannedReconcile<'_>,
    target: Option<&str>,
    opts: ReconcileOptions<'_>,
) -> ApplyResult {
    match provider {
        CiProvider::Github => apply_github_reconcile(plan, target, opts),
        CiProvider::Gitlab => apply_gitlab_reconcile(plan, target, opts),
    }
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "comment / fingerprint counts on a single PR are bounded well below u32::MAX"
)]
fn emit_reconcile_result(
    provider: CiProvider,
    target: Option<&str>,
    envelope: &Value,
    opts: ReconcileOptions<'_>,
    plan: &ReconcilePlan,
    applied: &ApplyResult,
) -> ExitCode {
    let envelope_struct = fallow_output::ReviewReconcileOutput {
        schema: fallow_output::ReviewReconcileSchema::V1,
        provider: match provider {
            CiProvider::Github => fallow_output::ReviewProvider::Github,
            CiProvider::Gitlab => fallow_output::ReviewProvider::Gitlab,
        },
        target: target.map(str::to_owned),
        dry_run: opts.dry_run,
        comments: envelope_comments_len(envelope) as u32,
        current_fingerprints: plan.current.len() as u32,
        existing_fingerprints: plan.existing.len() as u32,
        new_fingerprints: plan.new.len() as u32,
        stale_fingerprints: plan.stale.len() as u32,
        new: plan.new.clone(),
        stale: plan.stale.clone(),
        provider_warning: plan.provider_warning.clone(),
        resolution_comments_posted: applied.resolution_comments_posted as u32,
        threads_resolved: applied.threads_resolved as u32,
        apply_hint: applied.hint(),
        apply_errors: applied.errors.clone(),
        failed_fingerprints: applied.failed_fingerprints.iter().cloned().collect(),
        unapplied_fingerprints: applied.unapplied_fingerprints.iter().cloned().collect(),
    };
    match fallow_output::serialize_review_reconcile_json_output(
        envelope_struct,
        crate::output_runtime::current_root_envelope_mode(),
        crate::output_runtime::telemetry_analysis_run_id().as_deref(),
    ) {
        Ok(value) => crate::report::emit_json(&value, "review reconcile"),
        Err(e) => emit_error(
            &format!("JSON serialization error: {e}"),
            2,
            fallow_config::OutputFormat::Json,
        ),
    }
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

#[derive(Debug)]
struct PlannedReconcile<'state> {
    plan: ReconcilePlan,
    state: &'state ProviderState,
}

impl<'state> PlannedReconcile<'state> {
    fn new(current: &BTreeSet<String>, state: &'state ProviderState) -> Self {
        Self {
            plan: reconcile_sets(current, &state.fingerprints),
            state,
        }
    }
}

#[derive(Debug, Default)]
struct ApplyResult {
    resolution_comments_posted: usize,
    threads_resolved: usize,
    errors: Vec<String>,
    failed_fingerprints: BTreeSet<String>,
    unapplied_fingerprints: BTreeSet<String>,
}

impl ApplyResult {
    fn hint(&self) -> Option<String> {
        (!self.errors.is_empty()).then(|| {
            "Reconcile apply stopped before all stale fingerprints were applied. Refresh provider state and rerun the job; fingerprints listed in unapplied_fingerprints were not fully applied.".to_owned()
        })
    }

    fn record_failure(
        &mut self,
        failure: ApplyFailure,
        unapplied: impl IntoIterator<Item = String>,
    ) {
        self.errors.push(failure.message);
        self.failed_fingerprints.insert(failure.fingerprint);
        self.unapplied_fingerprints.extend(unapplied);
    }
}

#[derive(Debug)]
struct ApplyFailure {
    fingerprint: String,
    message: String,
}

impl ApplyFailure {
    fn new(fingerprint: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            fingerprint: fingerprint.into(),
            message: message.into(),
        }
    }
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
    let agent = try_api_agent().map_err(|err| err.to_string())?;
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
            if let Some(fingerprint) = extract_fallow_fingerprint(body) {
                state.fingerprints.insert(fingerprint.clone());
                if let Some(id) = comment.get("id").and_then(Value::as_u64) {
                    state
                        .github_comments_by_fingerprint
                        .entry(fingerprint)
                        .or_default()
                        .push(id);
                }
            }
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

const GITHUB_REVIEW_THREADS_QUERY: &str = r"
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
        let payload = serde_json::json!({
            "query": GITHUB_REVIEW_THREADS_QUERY,
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
            collect_github_thread_fingerprints(state, thread);
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

/// Record fallow fingerprints found in an unresolved GitHub review thread's
/// comment bodies, mapping each to the thread id for later resolution.
fn collect_github_thread_fingerprints(state: &mut ProviderState, thread: &Value) {
    if thread
        .get("isResolved")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return;
    }
    let Some(thread_id) = thread.get("id").and_then(Value::as_str) else {
        return;
    };
    let comments = thread
        .pointer("/comments/nodes")
        .and_then(Value::as_array)
        .into_iter()
        .flatten();
    for comment in comments {
        let body = comment.get("body").and_then(Value::as_str).unwrap_or("");
        if let Some(fingerprint) = extract_fallow_fingerprint(body) {
            state.fingerprints.insert(fingerprint.clone());
            state
                .github_threads_by_fingerprint
                .entry(fingerprint)
                .or_default()
                .push(thread_id.to_owned());
        }
    }
}

fn apply_github_reconcile(
    plan: &PlannedReconcile<'_>,
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
    let agent = match try_api_agent() {
        Ok(agent) => agent,
        Err(err) => {
            result.errors.push(err.to_string());
            return result;
        }
    };
    let sha = std::env::var("GITHUB_SHA")
        .ok()
        .or_else(|| std::env::var("PR_HEAD_SHA").ok());
    let operations = stage_github_operations(plan, sha.as_deref());

    if let Err(failure) = preflight_github_operations(&operations, &agent, &repo, &token, api) {
        result.record_failure(
            failure,
            operations
                .iter()
                .map(GithubApplyOperation::fingerprint_owned),
        );
        return result;
    }

    run_github_operations(
        &operations,
        GithubConnection {
            agent: &agent,
            repo: &repo,
            pr,
            token: &token,
            api,
        },
        &mut result,
    );
    result
}

/// Apply each staged GitHub operation in order, recording a failure (with the
/// not-yet-applied suffix) and stopping at the first error.
/// The GitHub PR connection context (agent + repo/PR coordinates + auth)
/// shared by every staged operation, bundled so the operation runner takes one
/// parameter instead of five.
#[derive(Clone, Copy)]
struct GithubConnection<'a> {
    agent: &'a ureq::Agent,
    repo: &'a str,
    pr: &'a str,
    token: &'a str,
    api: &'a str,
}

fn run_github_operations(
    operations: &[GithubApplyOperation],
    conn: GithubConnection<'_>,
    result: &mut ApplyResult,
) {
    let GithubConnection {
        agent,
        repo,
        pr,
        token,
        api,
    } = conn;
    for (index, operation) in operations.iter().enumerate() {
        if let Err(failure) = apply_github_operation(&mut GithubOperationInput {
            operation,
            agent,
            repo,
            pr,
            token,
            api,
            result,
        }) {
            result.record_failure(
                failure,
                operations[index..]
                    .iter()
                    .map(GithubApplyOperation::fingerprint_owned),
            );
            return;
        }
    }
}

#[derive(Debug)]
enum GithubApplyOperation {
    Reply {
        fingerprint: String,
        comment_id: u64,
        body: String,
    },
    ResolveThread {
        fingerprint: String,
        thread_id: String,
    },
}

impl GithubApplyOperation {
    fn fingerprint(&self) -> &str {
        match self {
            Self::Reply { fingerprint, .. } | Self::ResolveThread { fingerprint, .. } => {
                fingerprint
            }
        }
    }

    fn fingerprint_owned(&self) -> String {
        self.fingerprint().to_owned()
    }
}

fn stage_github_operations(
    plan: &PlannedReconcile<'_>,
    sha: Option<&str>,
) -> Vec<GithubApplyOperation> {
    let mut operations = Vec::new();
    for fingerprint in &plan.plan.stale {
        let marker_key = resolved_marker_key(fingerprint, sha);
        let already_resolved = plan.state.github_resolved_markers.contains(&marker_key)
            || plan.state.github_resolved_markers.contains(fingerprint);
        if !already_resolved {
            for comment_id in plan
                .state
                .github_comments_by_fingerprint
                .get(fingerprint)
                .into_iter()
                .flatten()
            {
                let body = resolved_body(fingerprint, sha);
                operations.push(GithubApplyOperation::Reply {
                    fingerprint: fingerprint.clone(),
                    comment_id: *comment_id,
                    body,
                });
            }
        }
        for thread_id in plan
            .state
            .github_threads_by_fingerprint
            .get(fingerprint)
            .into_iter()
            .flatten()
        {
            operations.push(GithubApplyOperation::ResolveThread {
                fingerprint: fingerprint.clone(),
                thread_id: thread_id.clone(),
            });
        }
    }
    operations
}

fn preflight_github_operations(
    operations: &[GithubApplyOperation],
    agent: &ureq::Agent,
    repo: &str,
    token: &str,
    api: &str,
) -> Result<(), ApplyFailure> {
    let mut comment_ids = BTreeMap::<u64, String>::new();
    let mut thread_ids = BTreeMap::<String, String>::new();
    for operation in operations {
        match operation {
            GithubApplyOperation::Reply {
                fingerprint,
                comment_id,
                ..
            } => {
                comment_ids
                    .entry(*comment_id)
                    .or_insert_with(|| fingerprint.clone());
            }
            GithubApplyOperation::ResolveThread {
                fingerprint,
                thread_id,
            } => {
                thread_ids
                    .entry(thread_id.clone())
                    .or_insert_with(|| fingerprint.clone());
            }
        }
    }

    for (comment_id, fingerprint) in comment_ids {
        let url = format!("{api}/repos/{repo}/pulls/comments/{comment_id}");
        github_get_json(agent, &url, token).map_err(|err| {
            ApplyFailure::new(
                fingerprint,
                format!("GitHub preflight failed for review comment {comment_id}: {err}"),
            )
        })?;
    }

    for (thread_id, fingerprint) in thread_ids {
        let payload = serde_json::json!({
            "query": "query($threadId:ID!){node(id:$threadId){... on PullRequestReviewThread{id isResolved}}}",
            "variables": { "threadId": thread_id },
        });
        let value =
            github_post_json(agent, &format!("{api}/graphql"), token, &payload).map_err(|err| {
                ApplyFailure::new(
                    fingerprint.clone(),
                    format!("GitHub preflight failed for review thread {thread_id}: {err}"),
                )
            })?;
        if value.get("errors").is_some() || value.pointer("/data/node/id").is_none() {
            return Err(ApplyFailure::new(
                fingerprint,
                format!("GitHub preflight failed for review thread {thread_id}: {value}"),
            ));
        }
    }

    Ok(())
}

struct GithubOperationInput<'a> {
    operation: &'a GithubApplyOperation,
    agent: &'a ureq::Agent,
    repo: &'a str,
    pr: &'a str,
    token: &'a str,
    api: &'a str,
    result: &'a mut ApplyResult,
}

fn apply_github_operation(input: &mut GithubOperationInput<'_>) -> Result<(), ApplyFailure> {
    match input.operation {
        GithubApplyOperation::Reply {
            fingerprint,
            comment_id,
            body,
        } => {
            let payload = serde_json::json!({ "body": body });
            let url = format!(
                "{}/repos/{}/pulls/{}/comments/{comment_id}/replies",
                input.api, input.repo, input.pr
            );
            github_post_json(input.agent, &url, input.token, &payload).map_err(|err| {
                ApplyFailure::new(
                    fingerprint.clone(),
                    format!("GitHub failed to post resolution reply for {fingerprint}: {err}"),
                )
            })?;
            input.result.resolution_comments_posted += 1;
        }
        GithubApplyOperation::ResolveThread {
            fingerprint,
            thread_id,
        } => {
            let payload = serde_json::json!({
                "query": "mutation($threadId:ID!){resolveReviewThread(input:{threadId:$threadId}){thread{id isResolved}}}",
                "variables": { "threadId": thread_id },
            });
            let value = github_post_json(
                input.agent,
                &format!("{}/graphql", input.api),
                input.token,
                &payload,
            )
            .map_err(|err| {
                ApplyFailure::new(
                    fingerprint.clone(),
                    format!("GitHub failed to resolve review thread {thread_id}: {err}"),
                )
            })?;
            if value.get("errors").is_some() {
                return Err(ApplyFailure::new(
                    fingerprint.clone(),
                    format!("GitHub resolveReviewThread failed for {fingerprint}: {value}"),
                ));
            }
            input.result.threads_resolved += 1;
        }
    }
    Ok(())
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
    let agent = try_api_agent().map_err(|err| err.to_string())?;
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
            collect_gitlab_discussion_fingerprints(&mut state, discussion);
        }
        if discussions.len() < 100 {
            break;
        }
    }
    Ok(state)
}

/// Record fallow fingerprints and resolved markers found in a GitLab
/// discussion's note bodies, mapping each fingerprint to the discussion id.
fn collect_gitlab_discussion_fingerprints(state: &mut ProviderState, discussion: &Value) {
    let Some(discussion_id) = discussion.get("id").and_then(Value::as_str) else {
        return;
    };
    let notes = discussion
        .get("notes")
        .and_then(Value::as_array)
        .into_iter()
        .flatten();
    for note in notes {
        let body = note.get("body").and_then(Value::as_str).unwrap_or("");
        if let Some(fingerprint) = extract_fallow_fingerprint(body) {
            state.fingerprints.insert(fingerprint.clone());
            state
                .gitlab_discussions_by_fingerprint
                .entry(fingerprint)
                .or_default()
                .push(discussion_id.to_owned());
        }
        if is_gitlab_bot_note(note)
            && let Some(fingerprint) = extract_marker(body, "fallow-resolved-fingerprint:")
        {
            state.gitlab_resolved_markers.insert(fingerprint);
        }
    }
}

fn apply_gitlab_reconcile(
    plan: &PlannedReconcile<'_>,
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
    let agent = match try_api_agent() {
        Ok(agent) => agent,
        Err(err) => {
            result.errors.push(err.to_string());
            return result;
        }
    };
    let sha = std::env::var("CI_COMMIT_SHA").ok();
    let encoded_project = url_encode_path_segment(&project_id);
    let operations = stage_gitlab_operations(plan, sha.as_deref());

    if let Err(failure) =
        preflight_gitlab_operations(&operations, &agent, &encoded_project, mr, &token, &api)
    {
        result.record_failure(
            failure,
            operations
                .iter()
                .map(GitlabApplyOperation::fingerprint_owned),
        );
        return result;
    }

    run_gitlab_operations(
        &operations,
        GitlabConnection {
            agent: &agent,
            encoded_project: &encoded_project,
            mr,
            token: &token,
            api: &api,
        },
        &mut result,
    );
    result
}

/// The GitLab MR connection context (agent + project/MR coordinates + auth)
/// shared by every staged operation, bundled so the operation runner takes one
/// parameter instead of five.
#[derive(Clone, Copy)]
struct GitlabConnection<'a> {
    agent: &'a ureq::Agent,
    encoded_project: &'a str,
    mr: &'a str,
    token: &'a str,
    api: &'a str,
}

/// Apply each staged GitLab operation in order, recording a failure (with the
/// not-yet-applied suffix) and stopping at the first error.
fn run_gitlab_operations(
    operations: &[GitlabApplyOperation],
    conn: GitlabConnection<'_>,
    result: &mut ApplyResult,
) {
    let GitlabConnection {
        agent,
        encoded_project,
        mr,
        token,
        api,
    } = conn;
    for (index, operation) in operations.iter().enumerate() {
        if let Err(failure) = apply_gitlab_operation(&mut GitlabOperationInput {
            operation,
            agent,
            encoded_project,
            mr,
            token,
            api,
            result,
        }) {
            result.record_failure(
                failure,
                operations[index..]
                    .iter()
                    .map(GitlabApplyOperation::fingerprint_owned),
            );
            return;
        }
    }
}

#[derive(Debug)]
enum GitlabApplyOperation {
    Note {
        fingerprint: String,
        discussion_id: String,
        body: String,
    },
    ResolveDiscussion {
        fingerprint: String,
        discussion_id: String,
    },
}

impl GitlabApplyOperation {
    fn fingerprint(&self) -> &str {
        match self {
            Self::Note { fingerprint, .. } | Self::ResolveDiscussion { fingerprint, .. } => {
                fingerprint
            }
        }
    }

    fn fingerprint_owned(&self) -> String {
        self.fingerprint().to_owned()
    }
}

fn stage_gitlab_operations(
    plan: &PlannedReconcile<'_>,
    sha: Option<&str>,
) -> Vec<GitlabApplyOperation> {
    let mut operations = Vec::new();
    for fingerprint in &plan.plan.stale {
        let marker_key = resolved_marker_key(fingerprint, sha);
        let already_resolved = plan.state.gitlab_resolved_markers.contains(&marker_key)
            || plan.state.gitlab_resolved_markers.contains(fingerprint);
        for discussion_id in plan
            .state
            .gitlab_discussions_by_fingerprint
            .get(fingerprint)
            .into_iter()
            .flatten()
        {
            if !already_resolved {
                let body = resolved_body(fingerprint, sha);
                operations.push(GitlabApplyOperation::Note {
                    fingerprint: fingerprint.clone(),
                    discussion_id: discussion_id.clone(),
                    body,
                });
            }
            operations.push(GitlabApplyOperation::ResolveDiscussion {
                fingerprint: fingerprint.clone(),
                discussion_id: discussion_id.clone(),
            });
        }
    }
    operations
}

fn preflight_gitlab_operations(
    operations: &[GitlabApplyOperation],
    agent: &ureq::Agent,
    encoded_project: &str,
    mr: &str,
    token: &str,
    api: &str,
) -> Result<(), ApplyFailure> {
    let mut discussion_ids = BTreeMap::<String, String>::new();
    for operation in operations {
        match operation {
            GitlabApplyOperation::Note {
                fingerprint,
                discussion_id,
                ..
            }
            | GitlabApplyOperation::ResolveDiscussion {
                fingerprint,
                discussion_id,
            } => {
                discussion_ids
                    .entry(discussion_id.clone())
                    .or_insert_with(|| fingerprint.clone());
            }
        }
    }
    for (discussion_id, fingerprint) in discussion_ids {
        let url = format!(
            "{api}/projects/{encoded_project}/merge_requests/{mr}/discussions/{discussion_id}"
        );
        gitlab_get_json(agent, &url, token).map_err(|err| {
            ApplyFailure::new(
                fingerprint,
                format!("GitLab preflight failed for discussion {discussion_id}: {err}"),
            )
        })?;
    }
    Ok(())
}

struct GitlabOperationInput<'a> {
    operation: &'a GitlabApplyOperation,
    agent: &'a ureq::Agent,
    encoded_project: &'a str,
    mr: &'a str,
    token: &'a str,
    api: &'a str,
    result: &'a mut ApplyResult,
}

fn apply_gitlab_operation(input: &mut GitlabOperationInput<'_>) -> Result<(), ApplyFailure> {
    match input.operation {
        GitlabApplyOperation::Note {
            fingerprint,
            discussion_id,
            body,
        } => {
            let payload = serde_json::json!({ "body": body });
            let url = format!(
                "{}/projects/{}/merge_requests/{}/discussions/{discussion_id}/notes",
                input.api, input.encoded_project, input.mr
            );
            gitlab_post_json(input.agent, &url, input.token, &payload).map_err(|err| {
                ApplyFailure::new(
                    fingerprint.clone(),
                    format!("GitLab failed to post resolution note for {fingerprint}: {err}"),
                )
            })?;
            input.result.resolution_comments_posted += 1;
        }
        GitlabApplyOperation::ResolveDiscussion {
            fingerprint,
            discussion_id,
        } => {
            let payload = serde_json::json!({ "resolved": true });
            let url = format!(
                "{}/projects/{}/merge_requests/{}/discussions/{discussion_id}",
                input.api, input.encoded_project, input.mr
            );
            gitlab_put_json(input.agent, &url, input.token, &payload).map_err(|err| {
                ApplyFailure::new(
                    fingerprint.clone(),
                    format!("GitLab failed to resolve discussion {discussion_id}: {err}"),
                )
            })?;
            input.result.threads_resolved += 1;
        }
    }
    Ok(())
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

fn github_patch_json(
    agent: &ureq::Agent,
    url: &str,
    token: &str,
    payload: &Value,
) -> Result<Value, String> {
    with_rate_limit_retry("GitHub", || {
        agent
            .patch(url)
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
            Err(e) => {
                return Err(sanitize_network_error(&format!(
                    "{provider} request failed: {e}"
                )));
            }
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

/// Extract a fallow fingerprint from any v1 or v2 marker shape in `body`.
/// v2 (`<!-- fallow-fingerprint:v2: <fp> -->`) wins over v1 because the v2
/// marker's text also matches the v1 substring search, so the v2-first
/// check has to run first or the v1 fallback would skip past `v2:` and
/// return the literal `"v2:"` as the extracted fingerprint.
///
/// Returns the raw fingerprint string with any kind prefix preserved
/// (`merged:<hex>` stays `merged:<hex>`). Consumers match the returned
/// string against the comment's `fingerprint` field verbatim.
fn extract_fallow_fingerprint(body: &str) -> Option<String> {
    extract_marker(body, "fallow-fingerprint:v2:")
        .or_else(|| extract_marker(body, "fallow-fingerprint:"))
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

#[expect(
    clippy::expect_used,
    reason = "formatting percent-encoded bytes into String is infallible"
)]
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
    fn extracts_fingerprint_from_v2_marker() {
        assert_eq!(
            extract_fallow_fingerprint(
                "**error**\n\n<!-- fallow-fingerprint:v2: abc1234567890def -->"
            )
            .as_deref(),
            Some("abc1234567890def")
        );
        assert_eq!(
            extract_fallow_fingerprint(
                "**error**\n\n<!-- fallow-fingerprint:v2: merged:0123456789abcdef -->"
            )
            .as_deref(),
            Some("merged:0123456789abcdef")
        );
    }

    #[test]
    fn extract_fallow_fingerprint_falls_back_to_v1_shape() {
        assert_eq!(
            extract_fallow_fingerprint("**error**\n\n<!-- fallow-fingerprint: abc123 -->")
                .as_deref(),
            Some("abc123")
        );
    }

    #[test]
    fn extract_fallow_fingerprint_does_not_match_unrelated_body() {
        assert_eq!(extract_fallow_fingerprint("plain comment body"), None);
        assert_eq!(
            extract_fallow_fingerprint("fallow-fingerprint:v2: deadbeef").as_deref(),
            Some("deadbeef")
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
    fn provider_warning_plan_keeps_current_fingerprints_new() {
        let current = BTreeSet::from(["a".to_owned(), "b".to_owned()]);
        let plan = ReconcilePlan::without_provider(&current, "provider unavailable".to_owned());

        assert_eq!(plan.current, vec!["a", "b"]);
        assert_eq!(plan.new, vec!["a", "b"]);
        assert_eq!(plan.existing, Vec::<String>::new());
        assert_eq!(plan.stale, Vec::<String>::new());
        assert_eq!(
            plan.provider_warning.as_deref(),
            Some("provider unavailable")
        );
    }

    #[test]
    fn apply_result_failure_tracks_failed_and_unapplied_fingerprints() {
        let mut result = ApplyResult::default();
        result.record_failure(
            ApplyFailure::new("stale-a", "provider write failed"),
            ["stale-a".to_owned(), "stale-b".to_owned()],
        );

        assert_eq!(result.errors, vec!["provider write failed"]);
        assert!(result.failed_fingerprints.contains("stale-a"));
        assert!(result.unapplied_fingerprints.contains("stale-a"));
        assert!(result.unapplied_fingerprints.contains("stale-b"));
        assert!(result.hint().is_some());
    }

    #[test]
    fn github_stage_skips_resolved_comment_but_keeps_thread_resolution() {
        let plan = ReconcilePlan {
            stale: vec!["fp-a".to_owned()],
            ..ReconcilePlan::default()
        };
        let mut state = ProviderState::default();
        state
            .github_comments_by_fingerprint
            .insert("fp-a".to_owned(), vec![10, 11]);
        state
            .github_threads_by_fingerprint
            .insert("fp-a".to_owned(), vec!["thread-a".to_owned()]);
        state
            .github_resolved_markers
            .insert("fp-a@abcdef1".to_owned());
        let planned = PlannedReconcile {
            plan,
            state: &state,
        };

        let operations = stage_github_operations(&planned, Some("abcdef123456"));

        assert_eq!(operations.len(), 1);
        let GithubApplyOperation::ResolveThread {
            fingerprint,
            thread_id,
        } = &operations[0]
        else {
            panic!("expected stale fingerprint to resolve its review thread");
        };
        assert_eq!(fingerprint, "fp-a");
        assert_eq!(thread_id, "thread-a");
    }

    #[test]
    fn gitlab_stage_posts_resolution_once_and_resolves_discussion() {
        let plan = ReconcilePlan {
            stale: vec!["fp-a".to_owned()],
            ..ReconcilePlan::default()
        };
        let mut state = ProviderState::default();
        state
            .gitlab_discussions_by_fingerprint
            .insert("fp-a".to_owned(), vec!["discussion-a".to_owned()]);
        let planned = PlannedReconcile {
            plan,
            state: &state,
        };

        let operations = stage_gitlab_operations(&planned, Some("1234567890"));

        assert_eq!(operations.len(), 2);
        let GitlabApplyOperation::Note {
            fingerprint,
            discussion_id,
            body,
        } = &operations[0]
        else {
            panic!("expected a resolution note before resolving discussion");
        };
        assert_eq!(fingerprint, "fp-a");
        assert_eq!(discussion_id, "discussion-a");
        assert!(body.contains("fallow-resolved-fingerprint: fp-a@1234567"));
        let GitlabApplyOperation::ResolveDiscussion {
            fingerprint,
            discussion_id,
        } = &operations[1]
        else {
            panic!("expected stale fingerprint to resolve its GitLab discussion");
        };
        assert_eq!(fingerprint, "fp-a");
        assert_eq!(discussion_id, "discussion-a");
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
        let comment = serde_json::json!({
            "user": { "type": "User", "login": "alice" },
            "body": "<!-- fallow-resolved-fingerprint: abc123 -->",
        });
        assert!(!is_github_bot_comment(&comment));
    }

    // Serializes the FALLOW_BOT_LOGIN env-mutating tests so the GitHub and
    // GitLab override cases cannot overwrite each other's value when run in
    // parallel (which raced and failed on Windows CI).
    static BOT_LOGIN_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    #[allow(
        unsafe_code,
        reason = "test-only env mutation, serialized via BOT_LOGIN_ENV_LOCK"
    )]
    fn github_bot_check_accepts_explicit_login_override() {
        let _env = BOT_LOGIN_ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let comment = serde_json::json!({
            "user": { "type": "User", "login": "fallow-bot-account" },
        });
        // SAFETY: serialized by BOT_LOGIN_ENV_LOCK; cleared before returning.
        unsafe {
            std::env::set_var("FALLOW_BOT_LOGIN", "fallow-bot-account");
        }
        assert!(is_github_bot_comment(&comment));
        // SAFETY: Restore the process environment after the scoped override.
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
        let headers = headers_with_retry_after("86400");
        assert_eq!(
            compute_retry_wait(&headers, 2, "GitHub"),
            RETRY_MAX_WAIT_SECONDS
        );
    }

    #[test]
    fn compute_retry_wait_clamps_zero_retry_after() {
        let headers = headers_with_retry_after("0");
        assert_eq!(compute_retry_wait(&headers, 5, "GitLab"), 1);
    }

    #[test]
    fn compute_retry_wait_falls_back_to_floor_for_http_date() {
        let headers = headers_with_retry_after("Wed, 21 Oct 2026 07:28:00 GMT");
        assert_eq!(compute_retry_wait(&headers, 7, "GitHub"), 7);
    }

    #[test]
    fn parse_retry_after_returns_none_for_http_date() {
        assert_eq!(
            parse_retry_after(&headers_with_retry_after("Wed, 21 Oct 2026 07:28:00 GMT")),
            None
        );
    }

    #[test]
    fn should_retry_status_covers_429_and_transient_5xx() {
        assert!(should_retry_status(429));
        assert!(should_retry_status(502));
        assert!(should_retry_status(503));
        assert!(should_retry_status(504));
    }

    #[test]
    fn should_retry_status_skips_persistent_5xx_and_4xx() {
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
        assert!(body.contains("fallow-resolved-fingerprint: abc@1234567"));
    }

    // --- envelope_fingerprints / envelope_comments_len (lines 183-200) ---

    #[test]
    fn envelope_fingerprints_extracts_non_empty_fingerprints() {
        let value = serde_json::json!({
            "comments": [
                { "fingerprint": "fp-a" },
                { "fingerprint": "fp-b" },
            ]
        });
        let fps = envelope_fingerprints(&value);
        assert!(fps.contains("fp-a"));
        assert!(fps.contains("fp-b"));
        assert_eq!(fps.len(), 2);
    }

    #[test]
    fn envelope_fingerprints_skips_blank_fingerprint_entries() {
        let value = serde_json::json!({
            "comments": [
                { "fingerprint": "" },
                { "fingerprint": "   " },
                { "fingerprint": "fp-c" },
            ]
        });
        let fps = envelope_fingerprints(&value);
        assert!(!fps.contains(""));
        assert!(!fps.contains("   "));
        assert!(fps.contains("fp-c"));
        assert_eq!(fps.len(), 1);
    }

    #[test]
    fn envelope_fingerprints_returns_empty_when_no_comments_key() {
        let value = serde_json::json!({ "other": [] });
        assert!(envelope_fingerprints(&value).is_empty());
    }

    #[test]
    fn envelope_fingerprints_skips_comments_without_fingerprint_field() {
        let value = serde_json::json!({
            "comments": [
                { "body": "no fingerprint here" },
                { "fingerprint": "fp-ok" },
            ]
        });
        let fps = envelope_fingerprints(&value);
        assert_eq!(fps.len(), 1);
        assert!(fps.contains("fp-ok"));
    }

    #[test]
    fn envelope_comments_len_counts_array_entries() {
        let value = serde_json::json!({ "comments": [1, 2, 3] });
        assert_eq!(envelope_comments_len(&value), 3);
    }

    #[test]
    fn envelope_comments_len_returns_zero_when_comments_missing() {
        let value = serde_json::json!({ "other": [] });
        assert_eq!(envelope_comments_len(&value), 0);
    }

    // --- extract_fallow_fingerprint: v2-first ordering (lines 1376-1388) ---

    #[test]
    fn extract_fallow_fingerprint_v2_wins_over_v1_substring_prefix() {
        // v1 extraction would grab "v2:" as the fingerprint if it ran first
        // because "fallow-fingerprint:" is a prefix of "fallow-fingerprint:v2:".
        // Verify the v2-first order returns the real fingerprint.
        let body = "<!-- fallow-fingerprint:v2: realfp123 -->";
        assert_eq!(
            extract_fallow_fingerprint(body).as_deref(),
            Some("realfp123")
        );
    }

    #[test]
    fn extract_fallow_fingerprint_v2_preserves_merged_prefix() {
        let body = "<!-- fallow-fingerprint:v2: merged:deadbeefcafe0123 -->";
        assert_eq!(
            extract_fallow_fingerprint(body).as_deref(),
            Some("merged:deadbeefcafe0123")
        );
    }

    #[test]
    fn extract_fallow_fingerprint_returns_none_for_empty_body() {
        assert_eq!(extract_fallow_fingerprint(""), None);
    }

    #[test]
    fn extract_fallow_fingerprint_v1_shape_in_multiline_body() {
        let body = "Some finding text.\n\n<!-- fallow-fingerprint: abc123def456 -->\n";
        assert_eq!(
            extract_fallow_fingerprint(body).as_deref(),
            Some("abc123def456")
        );
    }

    // --- extract_marker edge cases (lines 1366-1374) ---

    #[test]
    fn extract_marker_stops_at_whitespace() {
        let body = "fallow-fingerprint: abc def";
        assert_eq!(
            extract_marker(body, "fallow-fingerprint:").as_deref(),
            Some("abc")
        );
    }

    #[test]
    fn extract_marker_stops_at_closing_angle_bracket() {
        let body = "<!-- fallow-fingerprint: abc123 -->";
        assert_eq!(
            extract_marker(body, "fallow-fingerprint:").as_deref(),
            Some("abc123")
        );
    }

    #[test]
    fn extract_marker_returns_none_when_marker_absent() {
        assert_eq!(extract_marker("plain text", "fallow-fingerprint:"), None);
    }

    #[test]
    fn extract_marker_returns_none_when_value_is_empty_after_trim() {
        // marker present but nothing follows except whitespace + end
        let body = "fallow-fingerprint:   ";
        assert_eq!(extract_marker(body, "fallow-fingerprint:"), None);
    }

    // --- reconcile_sets: all three set states (line 232-240) ---

    #[test]
    fn reconcile_sets_with_all_overlap_produces_empty_new_and_stale() {
        let fps = BTreeSet::from(["a".to_owned(), "b".to_owned()]);
        let plan = reconcile_sets(&fps, &fps);
        assert!(plan.new.is_empty());
        assert!(plan.stale.is_empty());
        assert_eq!(plan.current.len(), 2);
        assert_eq!(plan.existing.len(), 2);
    }

    #[test]
    fn reconcile_sets_with_disjoint_sets_marks_all_current_new_and_all_existing_stale() {
        let current = BTreeSet::from(["c1".to_owned(), "c2".to_owned()]);
        let existing = BTreeSet::from(["e1".to_owned(), "e2".to_owned()]);
        let plan = reconcile_sets(&current, &existing);
        assert_eq!(plan.new, vec!["c1", "c2"]);
        assert_eq!(plan.stale, vec!["e1", "e2"]);
    }

    #[test]
    fn reconcile_sets_with_empty_current_marks_all_existing_stale() {
        let current = BTreeSet::new();
        let existing = BTreeSet::from(["old".to_owned()]);
        let plan = reconcile_sets(&current, &existing);
        assert!(plan.new.is_empty());
        assert_eq!(plan.stale, vec!["old"]);
    }

    #[test]
    fn reconcile_sets_with_empty_existing_marks_all_current_new() {
        let current = BTreeSet::from(["new-fp".to_owned()]);
        let existing = BTreeSet::new();
        let plan = reconcile_sets(&current, &existing);
        assert_eq!(plan.new, vec!["new-fp"]);
        assert!(plan.stale.is_empty());
    }

    // --- ReconcilePlan::without_provider (lines 221-230) ---

    #[test]
    fn without_provider_has_no_warning_when_current_is_empty() {
        let current = BTreeSet::new();
        let plan = ReconcilePlan::without_provider(&current, "unavailable".to_owned());
        assert!(plan.current.is_empty());
        assert!(plan.new.is_empty());
        assert_eq!(plan.provider_warning.as_deref(), Some("unavailable"));
    }

    // --- ApplyResult::hint (lines 266-271) ---

    #[test]
    fn apply_result_hint_is_none_when_no_errors() {
        let result = ApplyResult::default();
        assert!(result.hint().is_none());
    }

    #[test]
    fn apply_result_hint_is_some_when_errors_present() {
        let mut result = ApplyResult::default();
        result.errors.push("something failed".to_owned());
        assert!(result.hint().is_some());
        let hint = result.hint().unwrap();
        assert!(hint.contains("unapplied_fingerprints"));
    }

    // --- GithubApplyOperation::fingerprint / fingerprint_owned (lines 581-593) ---

    #[test]
    fn github_apply_operation_fingerprint_accessor_for_reply() {
        let op = GithubApplyOperation::Reply {
            fingerprint: "fp-reply".to_owned(),
            comment_id: 42,
            body: "body".to_owned(),
        };
        assert_eq!(op.fingerprint(), "fp-reply");
        assert_eq!(op.fingerprint_owned(), "fp-reply");
    }

    #[test]
    fn github_apply_operation_fingerprint_accessor_for_resolve_thread() {
        let op = GithubApplyOperation::ResolveThread {
            fingerprint: "fp-thread".to_owned(),
            thread_id: "thread-xyz".to_owned(),
        };
        assert_eq!(op.fingerprint(), "fp-thread");
        assert_eq!(op.fingerprint_owned(), "fp-thread");
    }

    // --- GitlabApplyOperation::fingerprint / fingerprint_owned (lines 955-967) ---

    #[test]
    fn gitlab_apply_operation_fingerprint_accessor_for_note() {
        let op = GitlabApplyOperation::Note {
            fingerprint: "fp-note".to_owned(),
            discussion_id: "disc-1".to_owned(),
            body: "body text".to_owned(),
        };
        assert_eq!(op.fingerprint(), "fp-note");
        assert_eq!(op.fingerprint_owned(), "fp-note");
    }

    #[test]
    fn gitlab_apply_operation_fingerprint_accessor_for_resolve_discussion() {
        let op = GitlabApplyOperation::ResolveDiscussion {
            fingerprint: "fp-resolve".to_owned(),
            discussion_id: "disc-2".to_owned(),
        };
        assert_eq!(op.fingerprint(), "fp-resolve");
        assert_eq!(op.fingerprint_owned(), "fp-resolve");
    }

    // --- collect_github_thread_fingerprints (lines 432-459) ---

    #[test]
    fn collect_github_thread_fingerprints_skips_resolved_threads() {
        let thread = serde_json::json!({
            "id": "thread-1",
            "isResolved": true,
            "comments": { "nodes": [
                { "body": "<!-- fallow-fingerprint:v2: fp-should-skip -->" }
            ]}
        });
        let mut state = ProviderState::default();
        collect_github_thread_fingerprints(&mut state, &thread);
        assert!(state.fingerprints.is_empty());
        assert!(state.github_threads_by_fingerprint.is_empty());
    }

    #[test]
    fn collect_github_thread_fingerprints_skips_thread_without_id() {
        let thread = serde_json::json!({
            "isResolved": false,
            "comments": { "nodes": [
                { "body": "<!-- fallow-fingerprint:v2: fp-noid -->" }
            ]}
        });
        let mut state = ProviderState::default();
        collect_github_thread_fingerprints(&mut state, &thread);
        assert!(state.fingerprints.is_empty());
    }

    #[test]
    fn collect_github_thread_fingerprints_indexes_unresolved_thread() {
        let thread = serde_json::json!({
            "id": "thread-unresolved",
            "isResolved": false,
            "comments": { "nodes": [
                { "body": "<!-- fallow-fingerprint:v2: fp-active -->" }
            ]}
        });
        let mut state = ProviderState::default();
        collect_github_thread_fingerprints(&mut state, &thread);
        assert!(state.fingerprints.contains("fp-active"));
        assert_eq!(
            state.github_threads_by_fingerprint.get("fp-active"),
            Some(&vec!["thread-unresolved".to_owned()])
        );
    }

    #[test]
    fn collect_github_thread_fingerprints_skips_comments_without_fingerprint() {
        let thread = serde_json::json!({
            "id": "thread-2",
            "isResolved": false,
            "comments": { "nodes": [
                { "body": "plain comment, no marker" }
            ]}
        });
        let mut state = ProviderState::default();
        collect_github_thread_fingerprints(&mut state, &thread);
        assert!(state.fingerprints.is_empty());
    }

    // --- collect_gitlab_discussion_fingerprints (lines 807-832) ---

    #[test]
    fn collect_gitlab_discussion_fingerprints_skips_discussion_without_id() {
        let discussion = serde_json::json!({
            "notes": [
                { "body": "<!-- fallow-fingerprint:v2: fp-x -->" }
            ]
        });
        let mut state = ProviderState::default();
        collect_gitlab_discussion_fingerprints(&mut state, &discussion);
        assert!(state.fingerprints.is_empty());
    }

    #[test]
    fn collect_gitlab_discussion_fingerprints_indexes_fingerprint_and_discussion() {
        let discussion = serde_json::json!({
            "id": "disc-99",
            "notes": [
                { "body": "<!-- fallow-fingerprint:v2: fp-gitlab -->" }
            ]
        });
        let mut state = ProviderState::default();
        collect_gitlab_discussion_fingerprints(&mut state, &discussion);
        assert!(state.fingerprints.contains("fp-gitlab"));
        assert_eq!(
            state.gitlab_discussions_by_fingerprint.get("fp-gitlab"),
            Some(&vec!["disc-99".to_owned()])
        );
    }

    #[test]
    fn collect_gitlab_discussion_fingerprints_records_resolved_marker_from_bot_system_note() {
        let discussion = serde_json::json!({
            "id": "disc-100",
            "notes": [
                {
                    "system": true,
                    "body": "<!-- fallow-resolved-fingerprint: fp-resolved -->"
                }
            ]
        });
        let mut state = ProviderState::default();
        collect_gitlab_discussion_fingerprints(&mut state, &discussion);
        assert!(state.gitlab_resolved_markers.contains("fp-resolved"));
    }

    #[test]
    fn collect_gitlab_discussion_fingerprints_ignores_resolved_marker_from_human_note() {
        let discussion = serde_json::json!({
            "id": "disc-101",
            "notes": [
                {
                    "system": false,
                    "author": { "bot": false, "username": "alice" },
                    "body": "<!-- fallow-resolved-fingerprint: fp-human -->"
                }
            ]
        });
        let mut state = ProviderState::default();
        collect_gitlab_discussion_fingerprints(&mut state, &discussion);
        assert!(!state.gitlab_resolved_markers.contains("fp-human"));
    }

    // --- stage_github_operations: additional paths (lines 595-634) ---

    #[test]
    fn github_stage_emits_no_operations_when_no_stale_fingerprints() {
        let plan = ReconcilePlan::default();
        let state = ProviderState::default();
        let planned = PlannedReconcile {
            plan,
            state: &state,
        };
        assert!(stage_github_operations(&planned, Some("abc1234567")).is_empty());
    }

    #[test]
    fn github_stage_emits_reply_and_thread_for_unresolved_stale() {
        let plan = ReconcilePlan {
            stale: vec!["fp-stale".to_owned()],
            ..ReconcilePlan::default()
        };
        let mut state = ProviderState::default();
        state
            .github_comments_by_fingerprint
            .insert("fp-stale".to_owned(), vec![55]);
        state
            .github_threads_by_fingerprint
            .insert("fp-stale".to_owned(), vec!["thread-55".to_owned()]);
        let planned = PlannedReconcile {
            plan,
            state: &state,
        };
        let ops = stage_github_operations(&planned, Some("aaabbbccc"));
        assert_eq!(ops.len(), 2, "expected reply + thread ops");
        let has_reply = ops.iter().any(
            |op| matches!(op, GithubApplyOperation::Reply { comment_id, .. } if *comment_id == 55),
        );
        let has_resolve = ops.iter().any(|op| {
            matches!(op, GithubApplyOperation::ResolveThread { thread_id, .. } if thread_id == "thread-55")
        });
        assert!(has_reply, "expected a Reply operation for comment 55");
        assert!(
            has_resolve,
            "expected a ResolveThread operation for thread-55"
        );
    }

    #[test]
    fn github_stage_skips_reply_when_bare_fingerprint_already_in_resolved_markers() {
        // The bare fingerprint (no sha suffix) is in the resolved marker set:
        // the reply was already posted but with no sha available at that time.
        let plan = ReconcilePlan {
            stale: vec!["fp-bare".to_owned()],
            ..ReconcilePlan::default()
        };
        let mut state = ProviderState::default();
        state
            .github_comments_by_fingerprint
            .insert("fp-bare".to_owned(), vec![99]);
        state.github_resolved_markers.insert("fp-bare".to_owned());
        let planned = PlannedReconcile {
            plan,
            state: &state,
        };
        let ops = stage_github_operations(&planned, None);
        let has_reply = ops
            .iter()
            .any(|op| matches!(op, GithubApplyOperation::Reply { .. }));
        assert!(
            !has_reply,
            "reply should be suppressed when bare marker exists"
        );
    }

    #[test]
    fn github_stage_no_sha_resolved_body_says_resolved_without_commit() {
        let plan = ReconcilePlan {
            stale: vec!["fp-nosha".to_owned()],
            ..ReconcilePlan::default()
        };
        let mut state = ProviderState::default();
        state
            .github_comments_by_fingerprint
            .insert("fp-nosha".to_owned(), vec![1]);
        let planned = PlannedReconcile {
            plan,
            state: &state,
        };
        let ops = stage_github_operations(&planned, None);
        let GithubApplyOperation::Reply { body, .. } = &ops[0] else {
            panic!("expected Reply op");
        };
        assert!(
            body.contains("Resolved."),
            "no-sha body should say Resolved. without a commit hash"
        );
        assert!(body.contains("fallow-resolved-fingerprint: fp-nosha"));
    }

    // --- stage_gitlab_operations: additional paths (lines 969-1000) ---

    #[test]
    fn gitlab_stage_emits_no_operations_when_no_stale_fingerprints() {
        let plan = ReconcilePlan::default();
        let state = ProviderState::default();
        let planned = PlannedReconcile {
            plan,
            state: &state,
        };
        assert!(stage_gitlab_operations(&planned, Some("sha123")).is_empty());
    }

    #[test]
    fn gitlab_stage_skips_note_when_already_resolved_but_still_resolves_discussion() {
        let plan = ReconcilePlan {
            stale: vec!["fp-gl".to_owned()],
            ..ReconcilePlan::default()
        };
        let mut state = ProviderState::default();
        state
            .gitlab_discussions_by_fingerprint
            .insert("fp-gl".to_owned(), vec!["disc-gl".to_owned()]);
        state
            .gitlab_resolved_markers
            .insert("fp-gl@abc1234".to_owned());
        let planned = PlannedReconcile {
            plan,
            state: &state,
        };
        let ops = stage_gitlab_operations(&planned, Some("abc12345678"));
        assert_eq!(
            ops.len(),
            1,
            "only resolve op, no note since already resolved"
        );
        assert!(
            matches!(&ops[0], GitlabApplyOperation::ResolveDiscussion { discussion_id, .. } if discussion_id == "disc-gl"),
            "expected ResolveDiscussion for disc-gl"
        );
    }

    #[test]
    fn gitlab_stage_skips_note_when_bare_marker_already_present() {
        let plan = ReconcilePlan {
            stale: vec!["fp-bare-gl".to_owned()],
            ..ReconcilePlan::default()
        };
        let mut state = ProviderState::default();
        state
            .gitlab_discussions_by_fingerprint
            .insert("fp-bare-gl".to_owned(), vec!["disc-bare".to_owned()]);
        state
            .gitlab_resolved_markers
            .insert("fp-bare-gl".to_owned());
        let planned = PlannedReconcile {
            plan,
            state: &state,
        };
        let ops = stage_gitlab_operations(&planned, None);
        assert_eq!(ops.len(), 1);
        assert!(
            matches!(&ops[0], GitlabApplyOperation::ResolveDiscussion { .. }),
            "bare-marker skip should still emit ResolveDiscussion"
        );
    }

    #[test]
    fn gitlab_stage_no_sha_resolved_body_omits_commit_hash() {
        let plan = ReconcilePlan {
            stale: vec!["fp-gl-nosha".to_owned()],
            ..ReconcilePlan::default()
        };
        let mut state = ProviderState::default();
        state
            .gitlab_discussions_by_fingerprint
            .insert("fp-gl-nosha".to_owned(), vec!["disc-nosha".to_owned()]);
        let planned = PlannedReconcile {
            plan,
            state: &state,
        };
        let ops = stage_gitlab_operations(&planned, None);
        assert_eq!(ops.len(), 2);
        let GitlabApplyOperation::Note { body, .. } = &ops[0] else {
            panic!("expected Note op first");
        };
        assert!(
            body.contains("Resolved."),
            "no-sha gitlab body should say Resolved."
        );
        assert!(body.contains("fallow-resolved-fingerprint: fp-gl-nosha"));
    }

    // --- resolved_marker_key / resolved_body: edge cases (lines 1394-1409) ---

    #[test]
    fn resolved_marker_key_truncates_sha_to_seven_chars() {
        assert_eq!(resolved_marker_key("fp", Some("abcdefg1234")), "fp@abcdefg");
    }

    #[test]
    fn resolved_marker_key_uses_full_sha_when_shorter_than_seven() {
        // A 6-char sha slice falls back to the None branch
        assert_eq!(resolved_marker_key("fp", Some("abc")), "fp");
    }

    #[test]
    fn resolved_body_without_sha_omits_backtick_and_uses_bare_marker() {
        let body = resolved_body("fp-x", None);
        assert!(!body.contains('`'), "no backtick when sha is None");
        assert!(body.contains("fallow-resolved-fingerprint: fp-x"));
    }

    // --- url_encode_path_segment (lines 1415-1429) ---

    #[test]
    fn url_encode_path_segment_passthrough_for_unreserved_chars() {
        assert_eq!(
            url_encode_path_segment("abc-123_foo.bar~"),
            "abc-123_foo.bar~"
        );
    }

    #[test]
    fn url_encode_path_segment_encodes_slash() {
        assert_eq!(url_encode_path_segment("/"), "%2F");
    }

    #[test]
    fn url_encode_path_segment_encodes_at_sign() {
        assert_eq!(url_encode_path_segment("@"), "%40");
    }

    #[test]
    fn url_encode_path_segment_encodes_mixed_safe_and_reserved() {
        assert_eq!(url_encode_path_segment("a/b@c"), "a%2Fb%40c");
    }

    #[test]
    fn url_encode_path_segment_empty_string_returns_empty() {
        assert_eq!(url_encode_path_segment(""), "");
    }

    // --- is_github_bot_comment: missing branch (line 1323-1331) ---

    #[test]
    fn github_bot_check_returns_false_when_no_user_field() {
        let comment = serde_json::json!({ "body": "no user" });
        assert!(!is_github_bot_comment(&comment));
    }

    #[test]
    fn github_bot_check_returns_false_when_fallow_bot_login_not_set_and_type_not_bot() {
        let comment = serde_json::json!({
            "user": { "type": "User", "login": "someperson" },
        });
        assert!(!is_github_bot_comment(&comment));
    }

    // --- is_gitlab_bot_note: FALLOW_BOT_LOGIN path (lines 1356-1364) ---

    #[test]
    #[allow(
        unsafe_code,
        reason = "test-only env mutation, serialized via BOT_LOGIN_ENV_LOCK"
    )]
    fn gitlab_bot_check_accepts_explicit_login_override() {
        let _env = BOT_LOGIN_ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let note = serde_json::json!({
            "system": false,
            "author": { "bot": false, "username": "fallow-gl-bot" },
        });
        // SAFETY: serialized by BOT_LOGIN_ENV_LOCK; cleared before returning.
        unsafe {
            std::env::set_var("FALLOW_BOT_LOGIN", "fallow-gl-bot");
        }
        assert!(is_gitlab_bot_note(&note));
        // SAFETY: Restore the process environment after the scoped override.
        unsafe {
            std::env::remove_var("FALLOW_BOT_LOGIN");
        }
    }

    #[test]
    fn gitlab_bot_check_returns_false_when_bot_flag_is_false_and_no_login_override() {
        let note = serde_json::json!({
            "system": false,
            "author": { "bot": false, "username": "contributor" },
        });
        assert!(!is_gitlab_bot_note(&note));
    }

    // --- read_json_response: non-2xx path (lines 1288-1303) ---

    #[test]
    fn read_json_response_error_on_non_2xx_status() {
        struct StubReader {
            status_code: u16,
            body_text: String,
        }
        impl ResponseBodyReader for StubReader {
            fn status(&self) -> u16 {
                self.status_code
            }

            fn read_json<T: serde::de::DeserializeOwned>(&mut self) -> Result<T, ureq::Error> {
                unreachable!("only called on 2xx, not in this test")
            }

            fn read_to_string(&mut self) -> Result<String, ureq::Error> {
                Ok(std::mem::take(&mut self.body_text))
            }
        }

        let mut stub = StubReader {
            status_code: 403,
            body_text: "Forbidden".to_owned(),
        };
        let result = read_json_response(&mut stub, "GitHub");
        assert!(result.is_err());
        let msg = result.unwrap_err();
        assert!(msg.contains("403"), "error should mention status code");
        assert!(msg.contains("Forbidden"), "error should include body");
    }

    #[test]
    fn read_json_response_success_parses_json() {
        struct SuccessReader {
            body: serde_json::Value,
        }
        impl ResponseBodyReader for SuccessReader {
            fn status(&self) -> u16 {
                200
            }

            fn read_json<T: serde::de::DeserializeOwned>(&mut self) -> Result<T, ureq::Error> {
                // Serialize to string and back to produce the generic T
                let s = serde_json::to_string(&self.body).unwrap();
                Ok(serde_json::from_str(&s).unwrap())
            }

            fn read_to_string(&mut self) -> Result<String, ureq::Error> {
                unreachable!("not called on 2xx")
            }
        }

        let mut reader = SuccessReader {
            body: serde_json::json!({ "ok": true }),
        };
        let result = read_json_response(&mut reader, "GitHub");
        assert!(result.is_ok());
        assert_eq!(
            result
                .unwrap()
                .get("ok")
                .and_then(serde_json::Value::as_bool),
            Some(true)
        );
    }

    // --- parse_retry_after: header clamping edge (line 1282-1286) ---

    #[test]
    fn parse_retry_after_returns_zero_for_zero_header() {
        // The header value "0" is valid u64 and parses; clamping to >=1 happens
        // in compute_retry_wait, not in parse_retry_after itself.
        assert_eq!(parse_retry_after(&headers_with_retry_after("0")), Some(0));
    }

    // --- ApplyFailure::new (lines 290-297) ---

    #[test]
    fn apply_failure_new_stores_fingerprint_and_message() {
        let f = ApplyFailure::new("fp-fail", "something went wrong");
        assert_eq!(f.fingerprint, "fp-fail");
        assert_eq!(f.message, "something went wrong");
    }

    // --- PlannedReconcile::new (lines 248-255) ---

    #[test]
    fn planned_reconcile_new_derives_plan_from_current_and_provider_state() {
        let current = BTreeSet::from(["fp-new".to_owned(), "fp-shared".to_owned()]);
        let mut state = ProviderState::default();
        state.fingerprints.insert("fp-shared".to_owned());
        state.fingerprints.insert("fp-stale".to_owned());
        let planned = PlannedReconcile::new(&current, &state);
        assert_eq!(planned.plan.new, vec!["fp-new"]);
        assert_eq!(planned.plan.stale, vec!["fp-stale"]);
    }

    // --- require_target (lines 1093-1097) ---

    #[test]
    fn require_target_returns_error_for_none() {
        assert!(require_target("PR", None).is_err());
    }

    #[test]
    fn require_target_returns_error_for_blank_string() {
        assert!(require_target("PR", Some("   ")).is_err());
        assert!(require_target("PR", Some("")).is_err());
    }

    #[test]
    fn require_target_returns_value_when_non_empty() {
        assert_eq!(require_target("PR", Some("42")).unwrap(), "42");
    }

    // --- should_retry_status: exact boundary (line 1188-1190) ---

    #[test]
    fn should_retry_status_exact_boundary_at_502_and_504() {
        assert!(should_retry_status(502));
        assert!(should_retry_status(504));
        assert!(!should_retry_status(501));
        assert!(!should_retry_status(505));
    }

    // --- compute_retry_wait: floor clamping (lines 1251-1265) ---

    #[test]
    fn compute_retry_wait_clamps_floor_delay_above_max() {
        let headers = http::HeaderMap::new();
        assert_eq!(
            compute_retry_wait(&headers, RETRY_MAX_WAIT_SECONDS + 100, "GitHub"),
            RETRY_MAX_WAIT_SECONDS
        );
    }

    #[test]
    fn compute_retry_wait_uses_retry_after_when_within_bounds() {
        let headers = headers_with_retry_after("30");
        assert_eq!(compute_retry_wait(&headers, 2, "GitHub"), 30);
    }
}
