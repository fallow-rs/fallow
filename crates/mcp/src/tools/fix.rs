use std::time::Duration;

use crate::params::FixParams;

use rmcp::ErrorData as McpError;
use rmcp::model::{
    CallToolResponse, CallToolResult, CancelledNotificationParam, ClientResult, ElicitRequest,
    InputResponses, ServerRequest,
};
use rmcp::service::{PeerRequestOptions, RequestContext, RoleServer, ServiceError};

use super::fix_confirm::{self, Answer, ConfirmRoute, FixPlan, NotApplied, confirm_route};
use super::{push_global, push_remote_extends, push_scope, run_tool_with_limit};

/// Run the read-only fix preview. It is CLI-backed because fix planning shares
/// the same command-owned mutation safeguards as fix apply.
pub async fn run_fix_preview(binary: &str, params: FixParams) -> Result<CallToolResult, McpError> {
    let args = build_fix_preview_args(&params);
    run_tool_with_limit(binary, "fix_preview", &args, params.max_output_bytes).await
}

/// Run the mutating fix apply path. This intentionally remains CLI-backed.
pub async fn run_fix_apply(binary: &str, params: FixParams) -> Result<CallToolResult, McpError> {
    let args = build_fix_apply_args(&params);
    run_tool_with_limit(binary, "fix_apply", &args, params.max_output_bytes).await
}

/// Run `fix_apply` behind the confirmation form when the client declares form
/// elicitation. See [`fix_confirm`] for the two protocol routes.
///
/// The preview runs first. An empty plan applies without a question. A
/// decline, cancel, timeout, client failure or changed plan returns an
/// `applied: false` result and writes no file. `confirm_timeout` bounds the
/// wait for an answer to `elicitation/create`.
pub async fn run_fix_apply_confirmed(
    binary: &str,
    params: FixParams,
    context: &RequestContext<RoleServer>,
    responses: Option<&InputResponses>,
    state: Option<&str>,
    confirm_timeout: Duration,
) -> Result<CallToolResponse, McpError> {
    let route = confirm_route(
        context.protocol_version().as_ref(),
        context.client_capabilities().as_ref(),
    );
    if route == ConfirmRoute::Unconfirmed {
        return run_fix_apply(binary, params).await.map(Into::into);
    }
    let plan = match preview_plan(binary, &params).await? {
        Ok(plan) => plan,
        Err(preview) => return Ok(preview.into()),
    };
    if plan.is_empty() {
        return run_fix_apply(binary, params).await.map(Into::into);
    }
    let answer = if route == ConfirmRoute::InputRequired {
        // The retry ran a fresh preview above, and `read_retry` compares its
        // digest with the confirmed one, so the plan is current.
        match fix_confirm::read_retry(responses, state, &plan) {
            Some(answer) => Ok(answer),
            None => return Ok(fix_confirm::input_required(&plan).into()),
        }
    } else {
        request_answer(context, &plan, confirm_timeout).await
    };
    match answer {
        Ok(Answer::Accepted) => {
            if route == ConfirmRoute::Request && !plan_is_current(binary, &params, &plan).await? {
                return Ok(fix_confirm::not_applied(NotApplied::PlanChanged, &plan));
            }
            run_fix_apply(binary, params).await.map(Into::into)
        }
        Ok(Answer::Declined) => Ok(fix_confirm::not_applied(NotApplied::Declined, &plan)),
        Ok(Answer::Cancelled) => Ok(fix_confirm::not_applied(NotApplied::Cancelled, &plan)),
        Err(reason) => Ok(fix_confirm::not_applied(reason, &plan)),
    }
}

/// Run the dry run and summarize its plan. `Err` carries a preview result
/// that is no fix envelope, such as a config error, for the caller to return.
///
/// `fallow fix --dry-run` exits 2 when it skips a `content_changed` or
/// `mixed_line_endings` file. That result has `is_error` set but still holds
/// the full plan, so the form shows it.
async fn preview_plan(
    binary: &str,
    params: &FixParams,
) -> Result<Result<FixPlan, CallToolResult>, McpError> {
    let args = build_fix_preview_args(params);
    let preview = run_tool_with_limit(binary, "fix_apply", &args, None).await?;
    let plan = fix_confirm::summarize_preview(&result_text(&preview));
    if preview.is_error == Some(true) && plan.counts.is_none() {
        return Ok(Err(preview));
    }
    Ok(Ok(plan))
}

/// True when a fresh dry run gives the plan that the person confirmed.
async fn plan_is_current(
    binary: &str,
    params: &FixParams,
    confirmed: &FixPlan,
) -> Result<bool, McpError> {
    Ok(preview_plan(binary, params)
        .await?
        .is_ok_and(|plan| plan.digest == confirmed.digest))
}

/// Send `elicitation/create` and wait for the answer, the timeout, or the
/// cancellation of the tool call, whichever comes first.
async fn request_answer(
    context: &RequestContext<RoleServer>,
    plan: &FixPlan,
    timeout: Duration,
) -> Result<Answer, NotApplied> {
    let request =
        ServerRequest::ElicitRequest(ElicitRequest::new(fix_confirm::elicit_params(plan)));
    let mut options = PeerRequestOptions::no_options();
    options.timeout = Some(timeout);
    let handle = context
        .peer
        .send_request_with_option(request, options)
        .await
        .map_err(|error| {
            tracing::warn!(%error, "fix_apply confirmation request failed");
            NotApplied::Failed
        })?;
    let id = handle.id.clone();
    let result = tokio::select! {
        result = handle.await_response() => result,
        () = context.ct.cancelled() => {
            // rmcp cancels its own request only on a timeout. Without this
            // notification the client keeps the form open after the call ends.
            let cancel = CancelledNotificationParam::new(
                Some(id),
                Some("the fix_apply call was cancelled".to_string()),
            );
            let _ = context.peer.notify_cancelled(cancel).await;
            return Err(NotApplied::Cancelled);
        }
    };
    match result {
        Ok(ClientResult::ElicitResult(result)) => Ok(fix_confirm::answer_from_result(&result)),
        Err(ServiceError::Timeout { .. }) => Err(NotApplied::TimedOut),
        Err(ServiceError::Cancelled { .. }) => Err(NotApplied::Cancelled),
        Ok(_) => Err(NotApplied::Failed),
        Err(error) => {
            tracing::warn!(%error, "fix_apply confirmation request failed");
            Err(NotApplied::Failed)
        }
    }
}

/// The concatenated text blocks of a tool result.
fn result_text(result: &CallToolResult) -> String {
    result
        .content
        .iter()
        .filter_map(|block| block.as_text().map(|text| text.text.as_str()))
        .collect()
}

/// Build CLI arguments for the `fix_preview` tool.
pub fn build_fix_preview_args(params: &FixParams) -> Vec<String> {
    let mut args = vec![
        "fix".to_string(),
        "--dry-run".to_string(),
        "--format".to_string(),
        "json".to_string(),
        "--quiet".to_string(),
    ];
    if params.no_create_config == Some(true) {
        args.push("--no-create-config".to_string());
    }
    if params.show_cascade == Some(true) {
        args.push("--show-cascade".to_string());
    }
    push_global(
        &mut args,
        params.root.as_deref(),
        params.config.as_deref(),
        params.no_cache,
        params.threads,
    );
    push_remote_extends(&mut args, params.allow_remote_extends);
    push_scope(&mut args, params.production, params.workspace.as_deref());
    args
}

/// Build CLI arguments for the `fix_apply` tool.
pub fn build_fix_apply_args(params: &FixParams) -> Vec<String> {
    let mut args = vec![
        "fix".to_string(),
        "--yes".to_string(),
        "--format".to_string(),
        "json".to_string(),
        "--quiet".to_string(),
    ];
    if params.no_create_config == Some(true) {
        args.push("--no-create-config".to_string());
    }
    if params.show_cascade == Some(true) {
        args.push("--show-cascade".to_string());
    }
    push_global(
        &mut args,
        params.root.as_deref(),
        params.config.as_deref(),
        params.no_cache,
        params.threads,
    );
    push_remote_extends(&mut args, params.allow_remote_extends);
    push_scope(&mut args, params.production, params.workspace.as_deref());
    args
}
