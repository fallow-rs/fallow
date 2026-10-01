use fallow_types::cloud::CloudCommand;
use rmcp::ErrorData as McpError;
use rmcp::model::{CallToolResult, ContentBlock};

use crate::params::{CloudDeploymentChangesParams, CloudReviewPacketParams};

use super::cloud_runtime_context::{
    MAX_PERIOD_DAYS, api_key_is_set, cloud_api_key_missing_body, cloud_repo_missing_body,
    push_trimmed_flag,
};
use super::{push_global, run_tool_with_limit, typed_validation_error_body};

/// Per-list limit of the review-packet scope on the cloud.
const REVIEW_PACKET_MAX_SCOPE: usize = 1000;
/// Page size limit of the deployment change report.
const MAX_CHANGE_LIMIT: u16 = 200;
/// Change kinds the deployment change report accepts as a filter.
const DEPLOYMENT_CHANGE_KINDS: &[&str] = &[
    "stopped",
    "new_not_called",
    "heated_up",
    "cooled_down",
    "new_called",
    "unchanged",
];

/// Run `get_cloud_review_packet`, backed by `fallow coverage review-packet`.
pub async fn run_get_cloud_review_packet(
    binary: &str,
    params: CloudReviewPacketParams,
) -> Result<CallToolResult, McpError> {
    match build_get_cloud_review_packet_args(&params, api_key_is_set()) {
        Ok(args) => {
            run_tool_with_limit(
                binary,
                "get_cloud_review_packet",
                &args,
                params.max_output_bytes,
            )
            .await
        }
        Err(body) => Ok(CallToolResult::error(vec![ContentBlock::text(body)])),
    }
}

/// Run `get_cloud_deployment_changes`, backed by
/// `fallow coverage deployment-changes`.
pub async fn run_get_cloud_deployment_changes(
    binary: &str,
    params: CloudDeploymentChangesParams,
) -> Result<CallToolResult, McpError> {
    match build_get_cloud_deployment_changes_args(&params, api_key_is_set()) {
        Ok(args) => {
            run_tool_with_limit(
                binary,
                "get_cloud_deployment_changes",
                &args,
                params.max_output_bytes,
            )
            .await
        }
        Err(body) => Ok(CallToolResult::error(vec![ContentBlock::text(body)])),
    }
}

/// Build CLI arguments for the `get_cloud_review_packet` tool.
///
/// `api_key_is_set` is injected so the refusals are testable without a
/// change to the shared process environment.
pub fn build_get_cloud_review_packet_args(
    params: &CloudReviewPacketParams,
    api_key_is_set: bool,
) -> Result<Vec<String>, String> {
    const TOOL: &str = "get_cloud_review_packet";
    if !api_key_is_set {
        return Err(cloud_api_key_missing_body(
            TOOL,
            CloudCommand::ReviewPacket,
            "Without a key, no cloud tool can answer.",
        ));
    }
    let repo = params.repo.trim();
    if repo.is_empty() {
        return Err(cloud_repo_missing_body(TOOL));
    }
    check_period(params.period_days, TOOL)?;
    let files = params.files.as_deref().unwrap_or_default();
    let functions = params.functions.as_deref().unwrap_or_default();
    if files.len() > REVIEW_PACKET_MAX_SCOPE || functions.len() > REVIEW_PACKET_MAX_SCOPE {
        return Err(typed_validation_error_body(
            format!(
                "{TOOL} takes at most {REVIEW_PACKET_MAX_SCOPE} files and {REVIEW_PACKET_MAX_SCOPE} functions"
            ),
            "cloud_scope_too_large",
            "Split the scope over more calls, or pass only the files of the change.",
            &format!("{TOOL}.files"),
        ));
    }

    let mut args = vec![
        "coverage".to_string(),
        "review-packet".to_string(),
        "--format".to_string(),
        "json".to_string(),
        "--quiet".to_string(),
        "--repo".to_string(),
        repo.to_string(),
    ];
    push_global(&mut args, params.root.as_deref(), None, None, None);
    for file in files
        .iter()
        .map(|file| file.trim())
        .filter(|file| !file.is_empty())
    {
        args.extend(["--file".to_string(), file.to_string()]);
    }
    for target in functions {
        let file = target.file.trim();
        let name = target.name.trim();
        if file.is_empty() || name.is_empty() {
            return Err(typed_validation_error_body(
                "each function target needs a non-empty `file` and `name`",
                "cloud_function_target_invalid",
                "Pass functions as { file, name, line? } with a repo-relative file.",
                &format!("{TOOL}.functions"),
            ));
        }
        let value = match target.line {
            Some(line) => format!("{file}:{name}:{line}"),
            None => format!("{file}:{name}"),
        };
        args.extend(["--function".to_string(), value]);
    }
    push_trimmed_flag(&mut args, "--project-id", params.project_id.as_deref());
    push_trimmed_flag(&mut args, "--commit-sha", params.commit_sha.as_deref());
    push_trimmed_flag(&mut args, "--base", params.base.as_deref());
    if let Some(period_days) = params.period_days {
        args.extend(["--coverage-period".to_string(), period_days.to_string()]);
    }
    Ok(args)
}

/// Build CLI arguments for the `get_cloud_deployment_changes` tool.
pub fn build_get_cloud_deployment_changes_args(
    params: &CloudDeploymentChangesParams,
    api_key_is_set: bool,
) -> Result<Vec<String>, String> {
    const TOOL: &str = "get_cloud_deployment_changes";
    if !api_key_is_set {
        return Err(cloud_api_key_missing_body(
            TOOL,
            CloudCommand::DeploymentChanges,
            "Without a key, no cloud tool can answer.",
        ));
    }
    let repo = params.repo.trim();
    if repo.is_empty() {
        return Err(cloud_repo_missing_body(TOOL));
    }
    if let Some(change) = params.change.as_deref().map(str::trim)
        && !change.is_empty()
        && !DEPLOYMENT_CHANGE_KINDS.contains(&change)
    {
        return Err(typed_validation_error_body(
            format!(
                "change must be one of {}, got {change}",
                DEPLOYMENT_CHANGE_KINDS.join(", ")
            ),
            "cloud_change_kind_invalid",
            "Omit change to read every kind.",
            &format!("{TOOL}.change"),
        ));
    }
    if let Some(limit) = params.limit
        && (limit == 0 || limit > MAX_CHANGE_LIMIT)
    {
        return Err(typed_validation_error_body(
            format!("limit must be between 1 and {MAX_CHANGE_LIMIT}, got {limit}"),
            "cloud_limit_out_of_range",
            "Pass a page size the cloud serves, or omit limit.",
            &format!("{TOOL}.limit"),
        ));
    }

    let mut args = vec![
        "coverage".to_string(),
        "deployment-changes".to_string(),
        "--format".to_string(),
        "json".to_string(),
        "--quiet".to_string(),
        "--repo".to_string(),
        repo.to_string(),
    ];
    push_global(&mut args, params.root.as_deref(), None, None, None);
    push_trimmed_flag(&mut args, "--sha", params.sha.as_deref());
    push_trimmed_flag(&mut args, "--base", params.base.as_deref());
    push_trimmed_flag(&mut args, "--change", params.change.as_deref());
    if let Some(limit) = params.limit {
        args.extend(["--limit".to_string(), limit.to_string()]);
    }
    push_trimmed_flag(&mut args, "--cursor", params.cursor.as_deref());
    Ok(args)
}

fn check_period(period_days: Option<u16>, tool: &str) -> Result<(), String> {
    if let Some(period_days) = period_days
        && (period_days == 0 || period_days > MAX_PERIOD_DAYS)
    {
        return Err(typed_validation_error_body(
            format!("period_days must be between 1 and {MAX_PERIOD_DAYS}, got {period_days}"),
            "cloud_period_out_of_range",
            "Request a window the cloud serves, or omit period_days for the 30-day default.",
            &format!("{tool}.period_days"),
        ));
    }
    Ok(())
}
