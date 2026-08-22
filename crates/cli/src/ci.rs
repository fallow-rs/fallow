use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use fallow_config::OutputFormat;
use fallow_output::ReviewId;
use serde::Serialize;
use serde_json::Value;

use crate::api::{ResponseBodyReader, sanitize_network_error, try_api_agent};
use crate::coverage::upload_common::url_encode_path_segment;
use crate::error::emit_error_with_style;

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

pub fn run(
    command: CiCommand,
    output: OutputFormat,
    json_style: crate::json_style::JsonStyle,
) -> ExitCode {
    match command {
        command @ CiCommand::PlanPrComment { .. } => {
            run_plan_pr_comment(command, output, json_style)
        }
        command @ CiCommand::PostPrComment { .. } => {
            run_post_pr_comment(command, output, json_style)
        }
        command @ CiCommand::PostReview { .. } => run_post_review(command, output, json_style),
        command @ CiCommand::PostCheckRun { .. } => {
            run_post_check_run_command(command, output, json_style)
        }
        command @ CiCommand::ReconcileReview { .. } => {
            run_reconcile_review(command, output, json_style)
        }
    }
}

fn run_plan_pr_comment(
    command: CiCommand,
    output: OutputFormat,
    json_style: crate::json_style::JsonStyle,
) -> ExitCode {
    let CiCommand::PlanPrComment {
        body,
        marker_id,
        clean,
        existing_comment_id,
        existing_body,
    } = command
    else {
        unreachable!("ci plan-pr-comment runner called with different variant");
    };

    plan_pr_comment(
        &body,
        marker_id,
        clean,
        existing_comment_id,
        existing_body.as_deref(),
        output,
        json_style,
    )
}

fn run_post_review(
    command: CiCommand,
    output: OutputFormat,
    json_style: crate::json_style::JsonStyle,
) -> ExitCode {
    let CiCommand::PostReview {
        provider,
        target,
        envelope,
        repo,
        project_id,
        api_url,
        dry_run,
    } = command
    else {
        unreachable!("ci post-review runner called with different variant");
    };

    post_review(
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
        json_style,
    )
}

fn run_post_check_run_command(
    command: CiCommand,
    output: OutputFormat,
    json_style: crate::json_style::JsonStyle,
) -> ExitCode {
    let CiCommand::PostCheckRun {
        provider,
        decision,
        repo,
        head_sha,
        api_url,
        split_gates,
        dry_run,
    } = command
    else {
        unreachable!("ci post-check-run runner called with different variant");
    };

    run_post_check_run(
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
        json_style,
    )
}

fn run_reconcile_review(
    command: CiCommand,
    output: OutputFormat,
    json_style: crate::json_style::JsonStyle,
) -> ExitCode {
    let CiCommand::ReconcileReview {
        provider,
        target,
        envelope,
        repo,
        project_id,
        api_url,
        dry_run,
    } = command
    else {
        unreachable!("ci reconcile-review runner called with different variant");
    };

    reconcile_review(
        provider,
        target.as_deref(),
        &envelope,
        ReconcileOptions {
            repo: repo.as_deref(),
            project_id: project_id.as_deref(),
            api_url: api_url.as_deref(),
            dry_run,
            review_id: None,
        },
        output,
        json_style,
    )
}

fn run_post_pr_comment(
    command: CiCommand,
    output: OutputFormat,
    json_style: crate::json_style::JsonStyle,
) -> ExitCode {
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
        json_style,
    )
}

fn run_post_check_run(
    provider: CiProvider,
    input: PostCheckRunInput<'_>,
    output: OutputFormat,
    json_style: crate::json_style::JsonStyle,
) -> ExitCode {
    match provider {
        CiProvider::Github => post_check_run(&input, output, json_style),
        CiProvider::Gitlab => {
            emit_error_with_style("GitLab check runs are not supported", 2, output, json_style)
        }
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "PR comment planning keeps provider inputs explicit and now also carries JSON presentation"
)]
fn plan_pr_comment(
    body: &Path,
    marker_id: String,
    clean: bool,
    existing_comment_id: Option<String>,
    existing_body: Option<&Path>,
    output: OutputFormat,
    json_style: crate::json_style::JsonStyle,
) -> ExitCode {
    let body = match read_text_file(body, "PR comment body") {
        Ok(body) => body,
        Err(e) => return emit_error_with_style(&e, 2, output, json_style),
    };
    let existing = match existing_comment(existing_comment_id, existing_body) {
        Ok(existing) => existing,
        Err(e) => return emit_error_with_style(&e, 2, output, json_style),
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
    emit_pr_comment_post_plan(&plan, output, json_style)
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
    json_style: crate::json_style::JsonStyle,
) -> ExitCode {
    emit_ci_command_json(plan, "PR comment post plan", output, json_style)
}

#[cfg(test)]
fn serialize_ci_command_json<T: Serialize + ?Sized>(
    value: &T,
    json_style: crate::json_style::JsonStyle,
) -> Result<String, serde_json::Error> {
    json_style.serialize(value)
}

fn emit_ci_command_json<T: Serialize + ?Sized>(
    value: &T,
    kind: &str,
    output: OutputFormat,
    json_style: crate::json_style::JsonStyle,
) -> ExitCode {
    match serde_json::to_value(value) {
        Ok(value) => crate::report::emit_report_json(&value, kind, json_style),
        Err(e) => emit_error_with_style(
            &format!("JSON serialization error: {e}"),
            2,
            output,
            json_style,
        ),
    }
}

#[derive(Clone, Copy)]
struct ReconcileOptions<'a> {
    repo: Option<&'a str>,
    project_id: Option<&'a str>,
    api_url: Option<&'a str>,
    dry_run: bool,
    review_id: Option<&'a ReviewId>,
}

fn reconcile_review(
    provider: CiProvider,
    target: Option<&str>,
    envelope: &Path,
    opts: ReconcileOptions<'_>,
    output: OutputFormat,
    json_style: crate::json_style::JsonStyle,
) -> ExitCode {
    let envelope = match read_envelope(envelope) {
        Ok(value) => value,
        Err(e) => {
            return emit_error_with_style(&e, 2, output, json_style);
        }
    };
    let review_id = match validate_envelope_review_scope(&envelope) {
        Ok(review_id) => review_id,
        Err(error) => return emit_error_with_style(&error, 2, output, json_style),
    };
    let opts = ReconcileOptions {
        review_id: review_id.as_ref(),
        ..opts
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
                output,
                json_style,
            );
        }
        Err(e) => {
            return emit_error_with_style(&e, crate::api::NETWORK_EXIT_CODE, output, json_style);
        }
    };
    let plan = PlannedReconcile::new(&current, &state);

    let applied = if opts.dry_run {
        ApplyResult::default()
    } else {
        apply_provider_reconcile(provider, &plan, target, opts)
    };

    emit_reconcile_result(
        provider, target, &envelope, opts, &plan.plan, &applied, output, json_style,
    )
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
#[expect(
    clippy::too_many_arguments,
    reason = "review reconciliation renders one provider result from explicit planning and output inputs"
)]
fn emit_reconcile_result(
    provider: CiProvider,
    target: Option<&str>,
    envelope: &Value,
    opts: ReconcileOptions<'_>,
    plan: &ReconcilePlan,
    applied: &ApplyResult,
    output: OutputFormat,
    json_style: crate::json_style::JsonStyle,
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
        Ok(value) => emit_ci_command_json(&value, "review reconcile", output, json_style),
        Err(e) => emit_error_with_style(
            &format!("JSON serialization error: {e}"),
            2,
            output,
            json_style,
        ),
    }
}

fn read_envelope(path: &Path) -> Result<Value, String> {
    let data = std::fs::read_to_string(path)
        .map_err(|e| format!("failed to read review envelope '{}': {e}", path.display()))?;
    serde_json::from_str(&data)
        .map_err(|e| format!("failed to parse review envelope '{}': {e}", path.display()))
}

fn review_id_from_envelope(value: &Value) -> Result<Option<ReviewId>, String> {
    let Some(review_id) = value.pointer("/meta/review_id") else {
        return Ok(None);
    };
    if review_id.is_null() {
        return Ok(None);
    }
    let review_id = review_id
        .as_str()
        .ok_or_else(|| "review envelope meta.review_id must be a string".to_owned())?;
    ReviewId::parse(review_id.to_owned())
        .map(Some)
        .map_err(|error| format!("invalid review envelope meta.review_id: {error}"))
}

fn validate_envelope_review_scope(value: &Value) -> Result<Option<ReviewId>, String> {
    let review_id = review_id_from_envelope(value)?;
    let body = value
        .get("body")
        .and_then(Value::as_str)
        .ok_or_else(|| "review envelope body must be a string".to_owned())?;
    fallow_output::validate_review_body_scope(body, review_id.as_ref())
        .map_err(|error| format!("invalid review envelope body scope: {error}"))?;
    let comments = value
        .get("comments")
        .and_then(Value::as_array)
        .ok_or_else(|| "review envelope comments must be an array".to_owned())?;
    for (index, comment) in comments.iter().enumerate() {
        let body = comment
            .get("body")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("review envelope comments[{index}].body must be a string"))?;
        fallow_output::validate_review_body_scope(body, review_id.as_ref()).map_err(|error| {
            format!("invalid review envelope comments[{index}].body scope: {error}")
        })?;
    }
    Ok(review_id)
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
    /// Fingerprints whose latest owned lifecycle has not received a
    /// Fallow-authored resolution marker. Provider resolution state alone is
    /// not a lifecycle transition: a reviewer may resolve a still-current
    /// finding manually.
    fingerprints: BTreeSet<String>,
    github_discussions_by_fingerprint: BTreeMap<String, Vec<GithubDiscussion>>,
    github_unattached_resolved_markers: Vec<GithubUnattachedResolvedMarker>,
    gitlab_discussions_by_fingerprint: BTreeMap<String, Vec<GitlabDiscussion>>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum DiscussionStatus {
    #[default]
    Active,
    Resolved,
}

#[derive(Debug)]
struct GithubDiscussion {
    comment_id: u64,
    provider_position: usize,
    thread_id: Option<String>,
    status: DiscussionStatus,
    has_resolution_marker: bool,
}

#[derive(Debug)]
struct GithubUnattachedResolvedMarker {
    fingerprint: String,
    provider_position: usize,
}

#[derive(Debug)]
struct GitlabDiscussion {
    discussion_id: String,
    status: DiscussionStatus,
    has_resolution_marker: bool,
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
            "Reconcile apply stopped before all provider lifecycle operations were applied. Refresh provider state and rerun the job; fingerprints listed in unapplied_fingerprints were not fully applied.".to_owned()
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
    let mut review_comments = Vec::new();

    for page in 1..=100 {
        let url = format!(
            "{api}/repos/{repo}/pulls/{pr}/comments?per_page=100&page={page}&sort=created&direction=asc"
        );
        let value = github_get_json(&agent, &url, &token)?;
        let comments = value
            .as_array()
            .ok_or_else(|| "GitHub review comments response was not an array".to_owned())?;
        if comments.is_empty() {
            break;
        }
        review_comments.extend(comments.iter().cloned());
        if comments.len() < 100 {
            break;
        }
        if page == 100 {
            return Err(
                "GitHub review comments pagination exceeded 100 pages; refusing partial provider state"
                    .to_owned(),
            );
        }
    }

    // Root comments establish lifecycle identity. Replies are processed only
    // after every page is loaded so their markers can be tied to a root even
    // if provider ordering or pagination places the reply first.
    for (position, comment) in review_comments.iter().enumerate() {
        record_github_review_root(&mut state, comment, opts.review_id, position)?;
    }
    for (position, comment) in review_comments.iter().enumerate() {
        record_github_resolution_marker(&mut state, comment, opts.review_id, position)?;
    }

    load_github_review_threads(
        &mut state,
        GithubConnection {
            agent: &agent,
            repo: &repo,
            pr,
            token: &token,
            api,
        },
        opts.review_id,
    )?;
    finalize_github_state(&mut state);
    Ok(state)
}

fn record_github_review_root(
    state: &mut ProviderState,
    comment: &Value,
    review_id: Option<&ReviewId>,
    provider_position: usize,
) -> Result<(), String> {
    let body = comment.get("body").and_then(Value::as_str).unwrap_or("");
    if is_github_bot_comment(comment)
        && let Some(fingerprint) = extract_fallow_fingerprint(body)
    {
        if fallow_output::parse_review_id_marker(body)?.as_ref() != review_id {
            return Ok(());
        }
        match comment.get("in_reply_to_id") {
            Some(Value::Null) | None => {}
            Some(parent) if parent.as_u64().is_some() => return Ok(()),
            Some(_) => {
                return Err(format!(
                    "GitHub owned review comment for {fingerprint} contained a nonnumeric in_reply_to_id"
                ));
            }
        }
        let comment_id = comment.get("id").and_then(Value::as_u64).ok_or_else(|| {
            format!("GitHub owned review root for {fingerprint} did not contain a numeric id")
        })?;
        state
            .github_discussions_by_fingerprint
            .entry(fingerprint)
            .or_default()
            .push(GithubDiscussion {
                comment_id,
                provider_position,
                thread_id: None,
                status: DiscussionStatus::Active,
                has_resolution_marker: false,
            });
    }
    Ok(())
}

fn record_github_resolution_marker(
    state: &mut ProviderState,
    comment: &Value,
    review_id: Option<&ReviewId>,
    provider_position: usize,
) -> Result<(), String> {
    let body = comment.get("body").and_then(Value::as_str).unwrap_or("");
    if is_github_bot_comment(comment) {
        let Some(marker) = parse_resolution_marker(body)? else {
            return Ok(());
        };
        if fallow_output::parse_review_id_marker(body)?.as_ref() != review_id {
            return Ok(());
        }
        let fingerprint = resolution_marker_fingerprint(&marker);
        match comment.get("in_reply_to_id") {
            Some(Value::Null) | None => {
                comment.get("id").and_then(Value::as_u64).ok_or_else(|| {
                    format!(
                        "GitHub unattached resolution marker for {fingerprint} did not contain a numeric id"
                    )
                })?;
                state
                    .github_unattached_resolved_markers
                    .push(GithubUnattachedResolvedMarker {
                        fingerprint: fingerprint.to_owned(),
                        provider_position,
                    });
            }
            Some(parent) => {
                let root_id = parent.as_u64().ok_or_else(|| {
                    format!(
                        "GitHub resolution marker for {fingerprint} contained a nonnumeric in_reply_to_id"
                    )
                })?;
                if let Some(discussion) = state
                    .github_discussions_by_fingerprint
                    .get_mut(fingerprint)
                    .into_iter()
                    .flatten()
                    .find(|discussion| discussion.comment_id == root_id)
                {
                    discussion.has_resolution_marker = true;
                }
            }
        }
    }
    Ok(())
}

const GITHUB_REVIEW_THREADS_QUERY: &str = r"
query($owner:String!, $name:String!, $number:Int!, $cursor:String) {
  repository(owner:$owner, name:$name) {
    pullRequest(number:$number) {
      reviewThreads(first:100, after:$cursor) {
        nodes {
          id
          isResolved
          comments(first:1) { nodes { databaseId } }
        }
        pageInfo { hasNextPage endCursor }
      }
    }
  }
}";

fn load_github_review_threads(
    state: &mut ProviderState,
    conn: GithubConnection<'_>,
    review_id: Option<&ReviewId>,
) -> Result<(), String> {
    let GithubConnection {
        agent,
        repo,
        pr,
        token,
        api,
    } = conn;
    let (owner, name) = repo
        .split_once('/')
        .ok_or_else(|| format!("GitHub repo must be owner/name, got '{repo}'"))?;
    let number = pr
        .parse::<u64>()
        .map_err(|_| format!("GitHub PR must be numeric, got '{pr}'"))?;
    let mut cursor: Option<String> = None;
    for page in 1..=100 {
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
            collect_github_thread_fingerprints(state, thread, review_id)?;
        }
        let page_info = value
            .pointer("/data/repository/pullRequest/reviewThreads/pageInfo")
            .unwrap_or(&Value::Null);
        let has_next_page = page_info
            .get("hasNextPage")
            .and_then(Value::as_bool)
            .ok_or_else(|| {
                "GitHub reviewThreads pagination did not contain boolean hasNextPage".to_owned()
            })?;
        if !has_next_page {
            break;
        }
        let next_cursor = page_info
            .get("endCursor")
            .and_then(Value::as_str)
            .filter(|next| Some(*next) != cursor.as_deref())
            .map(str::to_owned)
            .ok_or_else(|| {
                "GitHub reviewThreads pagination returned a missing or non-advancing cursor"
                    .to_owned()
            })?;
        if page == 100 {
            return Err(
                "GitHub reviewThreads pagination exceeded 100 pages; refusing partial provider state"
                    .to_owned(),
            );
        }
        cursor = Some(next_cursor);
    }
    Ok(())
}

/// Attach provider lifecycle state to a Fallow-owned GitHub review thread.
fn collect_github_thread_fingerprints(
    state: &mut ProviderState,
    thread: &Value,
    _review_id: Option<&ReviewId>,
) -> Result<(), String> {
    let status = if thread.get("isResolved").and_then(Value::as_bool) == Some(true) {
        DiscussionStatus::Resolved
    } else if thread.get("isResolved").and_then(Value::as_bool) == Some(false) {
        DiscussionStatus::Active
    } else {
        return Err("GitHub review thread did not contain boolean isResolved".to_owned());
    };
    let thread_id = thread
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| "GitHub review thread did not contain a string id".to_owned())?;
    let root = thread
        .pointer("/comments/nodes")
        .and_then(Value::as_array)
        .and_then(|comments| comments.first())
        .ok_or_else(|| {
            format!("GitHub review thread {thread_id} did not contain a root comment")
        })?;
    let comment_id = root
        .get("databaseId")
        .and_then(Value::as_u64)
        .ok_or_else(|| {
            format!("GitHub review thread {thread_id} root did not contain a numeric databaseId")
        })?;
    // Join GraphQL lifecycle metadata only to a root authenticated through
    // the REST payload. A human-authored copied marker is not Fallow state.
    if let Some(discussion) = state
        .github_discussions_by_fingerprint
        .values_mut()
        .flatten()
        .find(|discussion| discussion.comment_id == comment_id)
    {
        discussion.thread_id = Some(thread_id.to_owned());
        discussion.status = status;
    }
    Ok(())
}

fn finalize_github_state(state: &mut ProviderState) {
    for (fingerprint, discussions) in &mut state.github_discussions_by_fingerprint {
        // Legacy provider payloads can omit a reply's root id. The REST query
        // requests creation order, so an unattached marker belongs to the
        // nearest preceding still-open owned root for the same fingerprint.
        // This closes the old generation without suppressing a recurrence.
        for marker in state
            .github_unattached_resolved_markers
            .iter()
            .filter(|marker| marker.fingerprint == *fingerprint)
        {
            let candidate = discussions
                .iter()
                .enumerate()
                .filter(|(_, discussion)| !discussion.has_resolution_marker)
                .filter(|(_, discussion)| discussion.provider_position < marker.provider_position)
                .max_by_key(|(_, discussion)| discussion.provider_position)
                .map(|(index, _)| index);
            if let Some(index) = candidate {
                discussions[index].has_resolution_marker = true;
                if discussions[index].thread_id.is_none() {
                    discussions[index].status = DiscussionStatus::Resolved;
                }
            }
        }
        if discussions
            .iter()
            .any(|discussion| !discussion.has_resolution_marker)
        {
            state.fingerprints.insert(fingerprint.clone());
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
    let operations = stage_github_operations(plan, sha.as_deref(), opts.review_id);

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
    review_id: Option<&ReviewId>,
) -> Vec<GithubApplyOperation> {
    let mut operations = Vec::new();
    for fingerprint in &plan.plan.stale {
        for discussion in plan
            .state
            .github_discussions_by_fingerprint
            .get(fingerprint)
            .into_iter()
            .flatten()
            .filter(|discussion| !discussion.has_resolution_marker)
        {
            // Resolve first. If the marker reply then fails, a later run sees
            // a provider-resolved lifecycle without Fallow's marker and
            // retries only that reply. The reverse order cannot distinguish a
            // failed resolve from a genuinely reopened old lifecycle.
            if discussion.status == DiscussionStatus::Active
                && let Some(thread_id) = &discussion.thread_id
            {
                operations.push(GithubApplyOperation::ResolveThread {
                    fingerprint: fingerprint.clone(),
                    thread_id: thread_id.clone(),
                });
            }
            let body = resolved_body(fingerprint, sha, review_id);
            operations.push(GithubApplyOperation::Reply {
                fingerprint: fingerprint.clone(),
                comment_id: discussion.comment_id,
                body,
            });
        }
    }
    // A resolution marker permanently closes its generation. Re-close a
    // provider-reopened old thread without another reply; a current recurrence
    // is represented by a fresh discussion.
    for (fingerprint, discussions) in &plan.state.github_discussions_by_fingerprint {
        for discussion in discussions.iter().filter(|discussion| {
            discussion.has_resolution_marker && discussion.status == DiscussionStatus::Active
        }) {
            if let Some(thread_id) = &discussion.thread_id {
                operations.push(GithubApplyOperation::ResolveThread {
                    fingerprint: fingerprint.clone(),
                    thread_id: thread_id.clone(),
                });
            }
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
        let value = github_get_json(agent, &url, token).map_err(|err| {
            ApplyFailure::new(
                fingerprint.clone(),
                format!("GitHub preflight failed for review comment {comment_id}: {err}"),
            )
        })?;
        if value.get("id").and_then(Value::as_u64) != Some(comment_id) {
            return Err(ApplyFailure::new(
                fingerprint,
                format!(
                    "GitHub preflight returned the wrong review comment for {comment_id}: {value}"
                ),
            ));
        }
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
        if value.get("errors").is_some()
            || value.pointer("/data/node/id").and_then(Value::as_str) != Some(thread_id.as_str())
        {
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
            let value =
                github_create_json(input.agent, &url, input.token, &payload).map_err(|err| {
                    ApplyFailure::new(
                        fingerprint.clone(),
                        format!("GitHub failed to post resolution reply for {fingerprint}: {err}"),
                    )
                })?;
            if value.get("id").and_then(Value::as_u64).is_none()
                || value.get("in_reply_to_id").and_then(Value::as_u64) != Some(*comment_id)
                || !response_confirms_resolution_body(&value, body)
            {
                return Err(ApplyFailure::new(
                    fingerprint.clone(),
                    format!(
                        "GitHub resolution response did not confirm the created reply for {fingerprint}: {value}"
                    ),
                ));
            }
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
            if value.get("errors").is_some()
                || value
                    .pointer("/data/resolveReviewThread/thread/id")
                    .and_then(Value::as_str)
                    != Some(thread_id.as_str())
                || value
                    .pointer("/data/resolveReviewThread/thread/isResolved")
                    .and_then(Value::as_bool)
                    != Some(true)
            {
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
            collect_gitlab_discussion_fingerprints(&mut state, discussion, opts.review_id)?;
        }
        if discussions.len() < 100 {
            break;
        }
        if page == 100 {
            return Err(
                "GitLab discussions pagination exceeded 100 pages; refusing partial provider state"
                    .to_owned(),
            );
        }
    }
    Ok(state)
}

/// Record one GitLab discussion lifecycle and its own resolution marker.
fn collect_gitlab_discussion_fingerprints(
    state: &mut ProviderState,
    discussion: &Value,
    review_id: Option<&ReviewId>,
) -> Result<(), String> {
    let discussion_id = discussion
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| "GitLab discussion did not contain a string id".to_owned())?;
    let notes = discussion
        .get("notes")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .ok_or_else(|| {
            format!("GitLab discussion {discussion_id} did not contain a notes array")
        })?;
    let root = notes
        .first()
        .ok_or_else(|| format!("GitLab discussion {discussion_id} did not contain a root note"))?;
    {
        let body = root.get("body").and_then(Value::as_str).unwrap_or("");
        if is_gitlab_bot_note(root)
            && let Some(fingerprint) = extract_fallow_fingerprint(body)
        {
            if fallow_output::parse_review_id_marker(body)?.as_ref() != review_id {
                return Ok(());
            }
            let resolved = if let Some(value) = discussion.get("resolved") {
                value.as_bool().ok_or_else(|| {
                    format!(
                        "GitLab discussion {discussion_id} contained a non-boolean resolved status"
                    )
                })?
            } else {
                root.get("resolved")
                    .and_then(Value::as_bool)
                    .ok_or_else(|| {
                        format!(
                            "GitLab owned discussion {discussion_id} did not contain a boolean resolved status"
                        )
                    })?
            };
            let status = if resolved {
                DiscussionStatus::Resolved
            } else {
                DiscussionStatus::Active
            };
            let mut has_resolution_marker = false;
            for note in notes {
                let note_body = note.get("body").and_then(Value::as_str).unwrap_or("");
                if !is_gitlab_bot_note(note) {
                    continue;
                }
                let Some(marker) = parse_resolution_marker(note_body)? else {
                    continue;
                };
                if fallow_output::parse_review_id_marker(note_body)?.as_ref() == review_id
                    && resolution_marker_fingerprint(&marker) == fingerprint
                {
                    has_resolution_marker = true;
                    break;
                }
            }
            if !has_resolution_marker {
                state.fingerprints.insert(fingerprint.clone());
            }
            state
                .gitlab_discussions_by_fingerprint
                .entry(fingerprint)
                .or_default()
                .push(GitlabDiscussion {
                    discussion_id: discussion_id.to_owned(),
                    status,
                    has_resolution_marker,
                });
        }
    }
    Ok(())
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
    let operations = stage_gitlab_operations(plan, sha.as_deref(), opts.review_id);

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
    review_id: Option<&ReviewId>,
) -> Vec<GitlabApplyOperation> {
    let mut operations = Vec::new();
    for fingerprint in &plan.plan.stale {
        for discussion in plan
            .state
            .gitlab_discussions_by_fingerprint
            .get(fingerprint)
            .into_iter()
            .flatten()
            .filter(|discussion| !discussion.has_resolution_marker)
        {
            if discussion.status == DiscussionStatus::Active {
                operations.push(GitlabApplyOperation::ResolveDiscussion {
                    fingerprint: fingerprint.clone(),
                    discussion_id: discussion.discussion_id.clone(),
                });
            }
            let body = resolved_body(fingerprint, sha, review_id);
            operations.push(GitlabApplyOperation::Note {
                fingerprint: fingerprint.clone(),
                discussion_id: discussion.discussion_id.clone(),
                body,
            });
        }
    }
    for (fingerprint, discussions) in &plan.state.gitlab_discussions_by_fingerprint {
        for discussion in discussions.iter().filter(|discussion| {
            discussion.has_resolution_marker && discussion.status == DiscussionStatus::Active
        }) {
            operations.push(GitlabApplyOperation::ResolveDiscussion {
                fingerprint: fingerprint.clone(),
                discussion_id: discussion.discussion_id.clone(),
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
        let value = gitlab_get_json(agent, &url, token).map_err(|err| {
            ApplyFailure::new(
                fingerprint.clone(),
                format!("GitLab preflight failed for discussion {discussion_id}: {err}"),
            )
        })?;
        if value.get("id").and_then(Value::as_str) != Some(discussion_id.as_str()) {
            return Err(ApplyFailure::new(
                fingerprint,
                format!(
                    "GitLab preflight returned the wrong discussion for {discussion_id}: {value}"
                ),
            ));
        }
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
            let value =
                gitlab_create_json(input.agent, &url, input.token, &payload).map_err(|err| {
                    ApplyFailure::new(
                        fingerprint.clone(),
                        format!("GitLab failed to post resolution note for {fingerprint}: {err}"),
                    )
                })?;
            if value.get("id").and_then(Value::as_u64).is_none()
                || !response_confirms_resolution_body(&value, body)
            {
                return Err(ApplyFailure::new(
                    fingerprint.clone(),
                    format!(
                        "GitLab resolution response did not confirm the created note for {fingerprint}: {value}"
                    ),
                ));
            }
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
            let value =
                gitlab_put_json(input.agent, &url, input.token, &payload).map_err(|err| {
                    ApplyFailure::new(
                        fingerprint.clone(),
                        format!("GitLab failed to resolve discussion {discussion_id}: {err}"),
                    )
                })?;
            if value.get("id").and_then(Value::as_str) != Some(discussion_id.as_str())
                || !gitlab_discussion_is_resolved(&value)
            {
                return Err(ApplyFailure::new(
                    fingerprint.clone(),
                    format!(
                        "GitLab resolve discussion did not confirm resolution for {fingerprint}: {value}"
                    ),
                ));
            }
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

/// POST a non-idempotent GitHub creation without retrying ambiguous gateway
/// failures. A 429 is safe to retry because the provider rejected the request
/// before applying it; a 502/503/504 may hide a committed mutation.
fn github_create_json(
    agent: &ureq::Agent,
    url: &str,
    token: &str,
    payload: &Value,
) -> Result<Value, String> {
    with_retryable_status(
        "GitHub",
        |status| status == 429,
        || {
            agent
                .post(url)
                .header("Authorization", &format!("Bearer {token}"))
                .header("Accept", "application/vnd.github+json")
                .header("X-GitHub-Api-Version", "2022-11-28")
                .header("User-Agent", "fallow-cli")
                .send_json(payload)
        },
    )
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

/// POST a non-idempotent GitLab creation without retrying ambiguous gateway
/// failures. See [`github_create_json`] for the retry rationale.
fn gitlab_create_json(
    agent: &ureq::Agent,
    url: &str,
    token: &str,
    payload: &Value,
) -> Result<Value, String> {
    with_retryable_status(
        "GitLab",
        |status| status == 429,
        || {
            agent
                .post(url)
                .header("PRIVATE-TOKEN", token)
                .header("Content-Type", "application/json")
                .header("User-Agent", "fallow-cli")
                .send_json(payload)
        },
    )
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
fn with_rate_limit_retry<F>(provider: &str, op: F) -> Result<Value, String>
where
    F: FnMut() -> Result<http::Response<ureq::Body>, ureq::Error>,
{
    with_retryable_status(provider, should_retry_status, op)
}

fn with_retryable_status<F, P>(provider: &str, should_retry: P, mut op: F) -> Result<Value, String>
where
    F: FnMut() -> Result<http::Response<ureq::Body>, ureq::Error>,
    P: Fn(u16) -> bool,
{
    let max_attempts = retries_from_env();
    let floor_delay = retry_delay_from_env();
    let mut attempt: u32 = 0;
    loop {
        attempt += 1;
        match op() {
            Ok(mut response) => {
                let status = response.status().as_u16();
                if should_retry(status) && attempt < max_attempts {
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
/// Without an explicit login, GitHub's native `user.type == "Bot"` metadata
/// is the compatibility trust boundary. Setting `FALLOW_BOT_LOGIN` narrows
/// ownership to that exact posting login and also supports PAT-backed human
/// accounts that lack native bot metadata.
fn is_github_bot_comment(comment: &Value) -> bool {
    let user = comment.get("user");
    let login = user.and_then(|u| u.get("login")).and_then(Value::as_str);
    match std::env::var("FALLOW_BOT_LOGIN") {
        Ok(allow) => {
            let allow = allow.trim();
            return !allow.is_empty() && login == Some(allow);
        }
        Err(std::env::VarError::NotUnicode(_)) => return false,
        Err(std::env::VarError::NotPresent) => {}
    }
    user.and_then(|u| u.get("type")).and_then(Value::as_str) == Some("Bot")
}

/// Determine whether a GitLab MR discussion note was authored by a bot.
///
/// Without an explicit login, GitLab's native `system: true` or `author.bot`
/// metadata is the compatibility trust boundary. Setting `FALLOW_BOT_LOGIN`
/// narrows ownership to that exact username and supports project/PAT notes
/// that lack native bot metadata.
fn is_gitlab_bot_note(note: &Value) -> bool {
    let author = note.get("author");
    let username = author
        .and_then(|a| a.get("username"))
        .and_then(Value::as_str);
    match std::env::var("FALLOW_BOT_LOGIN") {
        Ok(allow) => {
            let allow = allow.trim();
            return !allow.is_empty() && username == Some(allow);
        }
        Err(std::env::VarError::NotUnicode(_)) => return false,
        Err(std::env::VarError::NotPresent) => {}
    }
    note.get("system").and_then(Value::as_bool).unwrap_or(false)
        || author.and_then(|a| a.get("bot")).and_then(Value::as_bool) == Some(true)
}

fn gitlab_discussion_is_resolved(discussion: &Value) -> bool {
    discussion
        .get("resolved")
        .and_then(Value::as_bool)
        .or_else(|| {
            discussion
                .get("notes")
                .and_then(Value::as_array)
                .and_then(|notes| notes.first())
                .and_then(|note| note.get("resolved"))
                .and_then(Value::as_bool)
        })
        .unwrap_or(false)
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

/// Return the lifecycle fingerprint encoded by either a legacy bare marker or
/// a SHA-qualified marker. The SHA is presentation/idempotency metadata for a
/// single discussion, never evidence that a new lifecycle began.
fn resolution_marker_fingerprint(marker: &str) -> &str {
    marker
        .split_once('@')
        .map_or(marker, |(fingerprint, _)| fingerprint)
}

fn response_confirms_resolution_body(value: &Value, expected_body: &str) -> bool {
    let Some(response_body) = value.get("body").and_then(Value::as_str) else {
        return false;
    };
    let Ok(expected_resolution) = parse_resolution_marker(expected_body) else {
        return false;
    };
    let Ok(response_resolution) = parse_resolution_marker(response_body) else {
        return false;
    };
    let Ok(expected_scope) = fallow_output::parse_review_id_marker(expected_body) else {
        return false;
    };
    expected_resolution.is_some()
        && response_resolution == expected_resolution
        && fallow_output::validate_review_body_scope(response_body, expected_scope.as_ref()).is_ok()
}

fn parse_resolution_marker(body: &str) -> Result<Option<String>, String> {
    const PREFIX: &str = "<!-- fallow-resolved-fingerprint: ";
    const SUFFIX: &str = " -->";
    let mut found = None;
    for line in body.lines() {
        if !line.contains("fallow-resolved-fingerprint") {
            continue;
        }
        let value = line
            .strip_prefix(PREFIX)
            .and_then(|line| line.strip_suffix(SUFFIX))
            .filter(|value| !value.is_empty() && !value.chars().any(char::is_whitespace))
            .ok_or_else(|| "malformed fallow resolved-fingerprint marker".to_owned())?;
        if found.replace(value.to_owned()).is_some() {
            return Err("duplicate fallow resolved-fingerprint marker".to_owned());
        }
    }
    Ok(found)
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

/// Compute the compatibility marker rendered in a resolution reply. Provider
/// state associates this marker with its root discussion; SHA inequality must
/// not be interpreted as force-push or recurrence evidence.
fn resolved_marker_key(fingerprint: &str, sha: Option<&str>) -> String {
    match sha.and_then(|value| value.get(..7)) {
        Some(short) => format!("{fingerprint}@{short}"),
        None => fingerprint.to_owned(),
    }
}

fn resolved_body(fingerprint: &str, sha: Option<&str>, review_id: Option<&ReviewId>) -> String {
    let marker = resolved_marker_key(fingerprint, sha);
    let mut body = match sha.and_then(|value| value.get(..7)) {
        Some(short) => {
            format!("Resolved in `{short}`.\n\n<!-- fallow-resolved-fingerprint: {marker} -->")
        }
        None => format!("Resolved.\n\n<!-- fallow-resolved-fingerprint: {marker} -->"),
    };
    if let Some(review_id) = review_id {
        body.push('\n');
        body.push_str(&fallow_output::review_id_marker(review_id));
    }
    body
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
        state.github_discussions_by_fingerprint.insert(
            "fp-a".to_owned(),
            vec![GithubDiscussion {
                comment_id: 10,
                provider_position: 0,
                thread_id: Some("thread-a".to_owned()),
                status: DiscussionStatus::Active,
                has_resolution_marker: true,
            }],
        );
        let planned = PlannedReconcile {
            plan,
            state: &state,
        };

        let operations = stage_github_operations(&planned, Some("abcdef123456"), None);

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
        state.gitlab_discussions_by_fingerprint.insert(
            "fp-a".to_owned(),
            vec![GitlabDiscussion {
                discussion_id: "discussion-a".to_owned(),
                status: DiscussionStatus::Active,
                has_resolution_marker: false,
            }],
        );
        let planned = PlannedReconcile {
            plan,
            state: &state,
        };

        let operations = stage_gitlab_operations(&planned, Some("1234567890"), None);

        assert_eq!(operations.len(), 2);
        let GitlabApplyOperation::ResolveDiscussion {
            fingerprint,
            discussion_id,
        } = &operations[0]
        else {
            panic!("expected the discussion to resolve before its marker reply");
        };
        assert_eq!(fingerprint, "fp-a");
        assert_eq!(discussion_id, "discussion-a");
        let GitlabApplyOperation::Note {
            fingerprint,
            discussion_id,
            body,
        } = &operations[1]
        else {
            panic!("expected a resolution marker after resolving the discussion");
        };
        assert_eq!(fingerprint, "fp-a");
        assert_eq!(discussion_id, "discussion-a");
        assert!(body.contains("fallow-resolved-fingerprint: fp-a@1234567"));
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
        let _env = BOT_LOGIN_ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let comment = serde_json::json!({
            "user": { "type": "Bot", "login": "github-actions[bot]" },
        });
        assert!(is_github_bot_comment(&comment));
    }

    #[test]
    fn github_bot_check_rejects_human_user_type() {
        let _env = BOT_LOGIN_ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
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
        let _env = BOT_LOGIN_ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
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
        let _env = BOT_LOGIN_ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
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
        let body = resolved_body("abc", Some("1234567890"), None);
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
    fn collect_github_thread_fingerprints_keeps_manually_resolved_lifecycle_live() {
        let thread = serde_json::json!({
            "id": "thread-1",
            "isResolved": true,
            "comments": { "nodes": [
                { "databaseId": 1 }
            ]}
        });
        let mut state = ProviderState::default();
        record_github_review_root(
            &mut state,
            &serde_json::json!({
                "id": 1,
                "body": "<!-- fallow-fingerprint:v2: fp-manual -->",
                "user": { "type": "Bot", "login": "fallow[bot]" }
            }),
            None,
            0,
        )
        .unwrap();
        collect_github_thread_fingerprints(&mut state, &thread, None).unwrap();
        finalize_github_state(&mut state);
        assert!(state.fingerprints.contains("fp-manual"));
        let discussion = &state.github_discussions_by_fingerprint["fp-manual"][0];
        assert_eq!(discussion.status, DiscussionStatus::Resolved);
        assert_eq!(discussion.thread_id.as_deref(), Some("thread-1"));
    }

    #[test]
    fn collect_github_thread_fingerprints_rejects_thread_without_id() {
        let thread = serde_json::json!({
            "isResolved": false,
            "comments": { "nodes": [
                { "body": "<!-- fallow-fingerprint:v2: fp-noid -->" }
            ]}
        });
        let mut state = ProviderState::default();
        let error = collect_github_thread_fingerprints(&mut state, &thread, None).unwrap_err();
        assert!(error.contains("string id"));
        assert!(state.fingerprints.is_empty());
    }

    #[test]
    fn collect_github_thread_fingerprints_indexes_unresolved_thread() {
        let thread = serde_json::json!({
            "id": "thread-unresolved",
            "isResolved": false,
            "comments": { "nodes": [
                { "databaseId": 1 }
            ]}
        });
        let mut state = ProviderState::default();
        record_github_review_root(
            &mut state,
            &serde_json::json!({
                "id": 1,
                "body": "<!-- fallow-fingerprint:v2: fp-active -->",
                "user": { "type": "Bot", "login": "fallow[bot]" }
            }),
            None,
            0,
        )
        .unwrap();
        collect_github_thread_fingerprints(&mut state, &thread, None).unwrap();
        finalize_github_state(&mut state);
        assert!(state.fingerprints.contains("fp-active"));
        assert_eq!(
            state.github_discussions_by_fingerprint["fp-active"][0]
                .thread_id
                .as_deref(),
            Some("thread-unresolved")
        );
    }

    #[test]
    fn github_explicit_marker_for_unowned_root_is_not_reassigned() {
        let mut state = ProviderState::default();
        record_github_review_root(
            &mut state,
            &serde_json::json!({
                "id": 20,
                "body": "<!-- fallow-fingerprint: fp-active -->",
                "user": { "type": "Bot" }
            }),
            None,
            0,
        )
        .unwrap();
        record_github_resolution_marker(
            &mut state,
            &serde_json::json!({
                "id": 30,
                "in_reply_to_id": 1,
                "body": "<!-- fallow-resolved-fingerprint: fp-active -->",
                "user": { "type": "Bot" }
            }),
            None,
            1,
        )
        .unwrap();
        finalize_github_state(&mut state);

        assert!(state.fingerprints.contains("fp-active"));
        assert!(!state.github_discussions_by_fingerprint["fp-active"][0].has_resolution_marker);
    }

    #[test]
    fn github_resolution_marker_rejects_nonnumeric_parent_id() {
        let mut state = ProviderState::default();
        let error = record_github_resolution_marker(
            &mut state,
            &serde_json::json!({
                "id": 30,
                "in_reply_to_id": "not-a-number",
                "body": "<!-- fallow-resolved-fingerprint: fp-active -->",
                "user": { "type": "Bot" }
            }),
            None,
            1,
        )
        .unwrap_err();

        assert!(error.contains("nonnumeric in_reply_to_id"));
        assert!(state.github_unattached_resolved_markers.is_empty());
    }

    #[test]
    fn github_owned_review_comment_rejects_nonnumeric_parent_id() {
        let mut state = ProviderState::default();
        let error = record_github_review_root(
            &mut state,
            &serde_json::json!({
                "id": 31,
                "in_reply_to_id": "not-a-number",
                "body": "<!-- fallow-fingerprint: fp-active -->",
                "user": { "type": "Bot" }
            }),
            None,
            1,
        )
        .unwrap_err();

        assert!(error.contains("nonnumeric in_reply_to_id"));
        assert!(state.github_discussions_by_fingerprint.is_empty());
    }

    #[test]
    fn collect_github_thread_fingerprints_skips_comments_without_fingerprint() {
        let thread = serde_json::json!({
            "id": "thread-2",
            "isResolved": false,
            "comments": { "nodes": [
                { "databaseId": 2 }
            ]}
        });
        let mut state = ProviderState::default();
        collect_github_thread_fingerprints(&mut state, &thread, None).unwrap();
        assert!(state.fingerprints.is_empty());
    }

    // --- collect_gitlab_discussion_fingerprints (lines 807-832) ---

    #[test]
    fn collect_gitlab_discussion_fingerprints_rejects_discussion_without_id() {
        let discussion = serde_json::json!({
            "notes": [
                { "body": "<!-- fallow-fingerprint:v2: fp-x -->" }
            ]
        });
        let mut state = ProviderState::default();
        let error =
            collect_gitlab_discussion_fingerprints(&mut state, &discussion, None).unwrap_err();
        assert!(error.contains("string id"));
        assert!(state.fingerprints.is_empty());
    }

    #[test]
    fn collect_gitlab_discussion_fingerprints_indexes_fingerprint_and_discussion() {
        let discussion = serde_json::json!({
            "id": "disc-99",
            "notes": [
                { "body": "<!-- fallow-fingerprint:v2: fp-gitlab -->", "resolved": false, "author": { "bot": true } }
            ]
        });
        let mut state = ProviderState::default();
        collect_gitlab_discussion_fingerprints(&mut state, &discussion, None).unwrap();
        assert!(state.fingerprints.contains("fp-gitlab"));
        let lifecycle = &state.gitlab_discussions_by_fingerprint["fp-gitlab"][0];
        assert_eq!(lifecycle.discussion_id, "disc-99");
        assert_eq!(lifecycle.status, DiscussionStatus::Active);
    }

    #[test]
    fn collect_gitlab_discussion_fingerprints_records_resolved_marker_from_bot_system_note() {
        let discussion = serde_json::json!({
            "id": "disc-100",
            "notes": [
                { "body": "<!-- fallow-fingerprint: fp-resolved -->", "resolved": true, "author": { "bot": true } },
                {
                    "system": true,
                    "body": "<!-- fallow-resolved-fingerprint: fp-resolved -->"
                }
            ]
        });
        let mut state = ProviderState::default();
        collect_gitlab_discussion_fingerprints(&mut state, &discussion, None).unwrap();
        assert!(state.gitlab_discussions_by_fingerprint["fp-resolved"][0].has_resolution_marker);
    }

    #[test]
    fn collect_gitlab_discussion_fingerprints_ignores_resolved_marker_from_human_note() {
        let discussion = serde_json::json!({
            "id": "disc-101",
            "notes": [
                { "body": "<!-- fallow-fingerprint: fp-human -->", "resolved": false, "author": { "bot": true } },
                {
                    "system": false,
                    "author": { "bot": false, "username": "alice" },
                    "body": "<!-- fallow-resolved-fingerprint: fp-human -->"
                }
            ]
        });
        let mut state = ProviderState::default();
        collect_gitlab_discussion_fingerprints(&mut state, &discussion, None).unwrap();
        assert!(!state.gitlab_discussions_by_fingerprint["fp-human"][0].has_resolution_marker);
    }

    #[test]
    fn collect_gitlab_discussion_fingerprints_rejects_missing_root_note() {
        let mut state = ProviderState::default();
        let error = collect_gitlab_discussion_fingerprints(
            &mut state,
            &serde_json::json!({ "id": "disc-empty", "notes": [] }),
            None,
        )
        .unwrap_err();

        assert!(error.contains("did not contain a root note"));
    }

    #[test]
    fn collect_gitlab_discussion_fingerprints_rejects_missing_owned_status() {
        let mut state = ProviderState::default();
        let error = collect_gitlab_discussion_fingerprints(
            &mut state,
            &serde_json::json!({
                "id": "disc-statusless",
                "notes": [{
                    "body": "<!-- fallow-fingerprint: fp-statusless -->",
                    "author": { "bot": true }
                }]
            }),
            None,
        )
        .unwrap_err();

        assert!(error.contains("did not contain a boolean resolved status"));
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
        assert!(stage_github_operations(&planned, Some("abc1234567"), None).is_empty());
    }

    #[test]
    fn github_stage_emits_reply_and_thread_for_unresolved_stale() {
        let plan = ReconcilePlan {
            stale: vec!["fp-stale".to_owned()],
            ..ReconcilePlan::default()
        };
        let mut state = ProviderState::default();
        state.github_discussions_by_fingerprint.insert(
            "fp-stale".to_owned(),
            vec![GithubDiscussion {
                comment_id: 55,
                provider_position: 0,
                thread_id: Some("thread-55".to_owned()),
                status: DiscussionStatus::Active,
                has_resolution_marker: false,
            }],
        );
        let planned = PlannedReconcile {
            plan,
            state: &state,
        };
        let ops = stage_github_operations(&planned, Some("aaabbbccc"), None);
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
        state.github_discussions_by_fingerprint.insert(
            "fp-bare".to_owned(),
            vec![GithubDiscussion {
                comment_id: 99,
                provider_position: 0,
                thread_id: None,
                status: DiscussionStatus::Active,
                has_resolution_marker: true,
            }],
        );
        let planned = PlannedReconcile {
            plan,
            state: &state,
        };
        let ops = stage_github_operations(&planned, None, None);
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
        state.github_discussions_by_fingerprint.insert(
            "fp-nosha".to_owned(),
            vec![GithubDiscussion {
                comment_id: 1,
                provider_position: 0,
                thread_id: None,
                status: DiscussionStatus::Active,
                has_resolution_marker: false,
            }],
        );
        let planned = PlannedReconcile {
            plan,
            state: &state,
        };
        let ops = stage_github_operations(&planned, None, None);
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
        assert!(stage_gitlab_operations(&planned, Some("sha123"), None).is_empty());
    }

    #[test]
    fn gitlab_stage_skips_note_when_already_resolved_but_still_resolves_discussion() {
        let plan = ReconcilePlan {
            stale: vec!["fp-gl".to_owned()],
            ..ReconcilePlan::default()
        };
        let mut state = ProviderState::default();
        state.gitlab_discussions_by_fingerprint.insert(
            "fp-gl".to_owned(),
            vec![GitlabDiscussion {
                discussion_id: "disc-gl".to_owned(),
                status: DiscussionStatus::Active,
                has_resolution_marker: true,
            }],
        );
        let planned = PlannedReconcile {
            plan,
            state: &state,
        };
        let ops = stage_gitlab_operations(&planned, Some("abc12345678"), None);
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
        state.gitlab_discussions_by_fingerprint.insert(
            "fp-bare-gl".to_owned(),
            vec![GitlabDiscussion {
                discussion_id: "disc-bare".to_owned(),
                status: DiscussionStatus::Active,
                has_resolution_marker: true,
            }],
        );
        let planned = PlannedReconcile {
            plan,
            state: &state,
        };
        let ops = stage_gitlab_operations(&planned, None, None);
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
        state.gitlab_discussions_by_fingerprint.insert(
            "fp-gl-nosha".to_owned(),
            vec![GitlabDiscussion {
                discussion_id: "disc-nosha".to_owned(),
                status: DiscussionStatus::Active,
                has_resolution_marker: false,
            }],
        );
        let planned = PlannedReconcile {
            plan,
            state: &state,
        };
        let ops = stage_gitlab_operations(&planned, None, None);
        assert_eq!(ops.len(), 2);
        let GitlabApplyOperation::Note { body, .. } = &ops[1] else {
            panic!("expected Note op after resolution");
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
        let body = resolved_body("fp-x", None, None);
        assert!(!body.contains('`'), "no backtick when sha is None");
        assert!(body.contains("fallow-resolved-fingerprint: fp-x"));
    }

    #[test]
    fn resolution_response_requires_exact_marker_and_scope() {
        let review_id = ReviewId::parse("frontend").unwrap();
        let expected = resolved_body("fp-x", Some("abcdef123"), Some(&review_id));
        assert!(response_confirms_resolution_body(
            &serde_json::json!({ "body": expected }),
            &expected,
        ));
        assert!(!response_confirms_resolution_body(
            &serde_json::json!({
                "body": "<!-- fallow-resolved-fingerprint: fp-x@abcdef1 -->"
            }),
            &expected,
        ));
        assert!(!response_confirms_resolution_body(
            &serde_json::json!({
                "body": "<!-- fallow-resolved-fingerprint: fp-x@7654321 -->\n<!-- fallow-review-id: frontend -->"
            }),
            &expected,
        ));
        assert!(!response_confirms_resolution_body(
            &serde_json::json!({
                "body": "<!-- fallow-resolved-fingerprint: fp-x@abcdef1 -->\n<!-- fallow-review-id: frontend -->\n<!-- fallow-review-id: backend -->"
            }),
            &expected,
        ));
    }

    #[test]
    #[allow(
        unsafe_code,
        reason = "test-only env mutation, serialized via BOT_LOGIN_ENV_LOCK"
    )]
    fn empty_configured_bot_login_trusts_no_native_bot() {
        let _env = BOT_LOGIN_ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // SAFETY: serialized by BOT_LOGIN_ENV_LOCK; cleared before returning.
        unsafe {
            std::env::set_var("FALLOW_BOT_LOGIN", "   ");
        }
        assert!(!is_github_bot_comment(&serde_json::json!({
            "user": { "type": "Bot", "login": "foreign[bot]" }
        })));
        assert!(!is_gitlab_bot_note(&serde_json::json!({
            "system": false,
            "author": { "bot": true, "username": "foreign-bot" }
        })));
        // SAFETY: Restore the process environment after the scoped override.
        unsafe {
            std::env::remove_var("FALLOW_BOT_LOGIN");
        }
    }

    // --- is_github_bot_comment: missing branch (line 1323-1331) ---

    #[test]
    fn github_bot_check_returns_false_when_no_user_field() {
        let _env = BOT_LOGIN_ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let comment = serde_json::json!({ "body": "no user" });
        assert!(!is_github_bot_comment(&comment));
    }

    #[test]
    fn github_bot_check_returns_false_when_fallow_bot_login_not_set_and_type_not_bot() {
        let _env = BOT_LOGIN_ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
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
        let _env = BOT_LOGIN_ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
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

    #[test]
    fn ci_command_json_serialization_honors_selected_style() {
        let value = serde_json::json!({ "action": "skip", "dry_run": true });

        let compact = serialize_ci_command_json(&value, crate::json_style::JsonStyle::Compact)
            .expect("compact CI JSON must serialize");
        let pretty = serialize_ci_command_json(&value, crate::json_style::JsonStyle::Pretty)
            .expect("pretty CI JSON must serialize");

        assert_eq!(compact, r#"{"action":"skip","dry_run":true}"#);
        assert!(pretty.contains("\n  \"action\": \"skip\""));
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&compact).expect("compact JSON must parse"),
            serde_json::from_str::<serde_json::Value>(&pretty).expect("pretty JSON must parse")
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

    #[test]
    fn github_identical_fingerprints_are_isolated_by_review_id() {
        let frontend = ReviewId::parse("frontend").unwrap();
        let backend = ReviewId::parse("backend").unwrap();
        let frontend_comment = serde_json::json!({
            "id": 1,
            "body": "finding\n<!-- fallow-fingerprint:v2: abcdef0123456789 -->\n<!-- fallow-review-id: frontend -->",
            "user": { "type": "Bot" }
        });
        let backend_comment = serde_json::json!({
            "id": 2,
            "body": "finding\n<!-- fallow-fingerprint:v2: abcdef0123456789 -->\n<!-- fallow-review-id: backend -->",
            "user": { "type": "Bot" }
        });
        let mut state = ProviderState::default();

        record_github_review_root(&mut state, &frontend_comment, Some(&frontend), 0).unwrap();
        record_github_review_root(&mut state, &backend_comment, Some(&frontend), 1).unwrap();

        assert_eq!(
            state.github_discussions_by_fingerprint["abcdef0123456789"][0].comment_id,
            1
        );
        assert!(fallow_output::body_matches_review_id(
            frontend_comment["body"].as_str().unwrap(),
            Some(&frontend)
        ));
        assert!(!fallow_output::body_matches_review_id(
            frontend_comment["body"].as_str().unwrap(),
            Some(&backend)
        ));
    }

    #[test]
    fn github_unscoped_review_ignores_scoped_and_non_root_comments() {
        let scoped = serde_json::json!({
            "id": 1,
            "body": "<!-- fallow-fingerprint:v2: abcdef0123456789 -->\n<!-- fallow-review-id: frontend -->"
        });
        let reply = serde_json::json!({
            "id": 2,
            "in_reply_to_id": 1,
            "body": "<!-- fallow-fingerprint:v2: fedcba9876543210 -->"
        });
        let mut state = ProviderState::default();

        record_github_review_root(&mut state, &scoped, None, 0).unwrap();
        record_github_review_root(&mut state, &reply, None, 1).unwrap();

        assert!(state.fingerprints.is_empty());
    }

    #[test]
    fn gitlab_identical_fingerprints_and_resolution_replies_are_scoped() {
        let frontend = ReviewId::parse("frontend").unwrap();
        let discussion = serde_json::json!({
            "id": "discussion-1",
            "notes": [
                {
                    "body": "finding\n<!-- fallow-fingerprint:v2: abcdef0123456789 -->\n<!-- fallow-review-id: frontend -->",
                    "resolved": true,
                    "author": { "bot": true }
                },
                {
                    "body": "Resolved.\n<!-- fallow-resolved-fingerprint: abcdef0123456789 -->\n<!-- fallow-review-id: frontend -->",
                    "system": true
                }
            ]
        });
        let mut scoped = ProviderState::default();
        let mut unscoped = ProviderState::default();

        collect_gitlab_discussion_fingerprints(&mut scoped, &discussion, Some(&frontend)).unwrap();
        collect_gitlab_discussion_fingerprints(&mut unscoped, &discussion, None).unwrap();

        assert!(!scoped.fingerprints.contains("abcdef0123456789"));
        assert!(
            scoped.gitlab_discussions_by_fingerprint["abcdef0123456789"][0].has_resolution_marker
        );
        assert!(unscoped.fingerprints.is_empty());
        assert!(unscoped.gitlab_discussions_by_fingerprint.is_empty());
    }

    #[test]
    fn resolution_body_retains_exact_review_marker() {
        let review_id = ReviewId::parse("frontend.check").unwrap();
        let body = resolved_body("abcdef0123456789", Some("1234567890"), Some(&review_id));

        assert!(body.ends_with("<!-- fallow-review-id: frontend.check -->"));
        assert!(fallow_output::body_matches_review_id(
            &body,
            Some(&review_id)
        ));
    }

    #[test]
    fn malformed_or_duplicate_review_markers_fail_closed() {
        assert!(
            fallow_output::parse_review_id_marker("<!-- fallow-review-id: bad value -->").is_err()
        );
        assert!(
            fallow_output::parse_review_id_marker("<!-- fallow-review-id : frontend -->").is_err()
        );
        assert!(
            fallow_output::parse_review_id_marker("<!-- fallow-review-id frontend -->").is_err()
        );
        assert!(!fallow_output::body_matches_review_id(
            "<!-- fallow-fingerprint:v2: abcdef0123456789 -->\n<!-- fallow-review-id : frontend -->",
            None
        ));
        assert!(
            fallow_output::parse_review_id_marker(
                "<!-- fallow-review-id: first -->\n<!-- fallow-review-id: second -->"
            )
            .is_err()
        );
    }

    #[test]
    fn envelope_scope_preflight_rejects_missing_unexpected_malformed_and_duplicate_markers() {
        let cases = [
            serde_json::json!({
                "body": "<!-- fallow-review-id: frontend -->",
                "comments": [{"body": "finding"}],
                "meta": {"review_id": "frontend"}
            }),
            serde_json::json!({
                "body": "<!-- fallow-review-id: frontend -->",
                "comments": []
            }),
            serde_json::json!({
                "body": "<!-- fallow-review-id : frontend -->",
                "comments": [],
                "meta": {"review_id": "frontend"}
            }),
            serde_json::json!({
                "body": "<!-- fallow-review-id: frontend -->\n<!-- fallow-review-id: frontend -->",
                "comments": [],
                "meta": {"review_id": "frontend"}
            }),
        ];

        for envelope in cases {
            assert!(validate_envelope_review_scope(&envelope).is_err());
        }
    }
}
