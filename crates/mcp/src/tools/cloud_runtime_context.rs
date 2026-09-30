use fallow_types::cloud::{CloudCommand, cloud_api_key_missing_message};
use rmcp::ErrorData as McpError;
use rmcp::model::{CallToolResult, ContentBlock};

use crate::params::CloudRuntimeContextParams;

use super::{push_global, push_remote_extends, run_tool_with_limit, typed_validation_error_body};

/// Bearer token for the cloud pull. It stays an environment variable of the
/// server process rather than a tool parameter: a key sent as a call argument
/// would be recorded in every agent transcript that made the call.
const API_KEY_ENV: &str = "FALLOW_API_KEY";

/// Widest window the cloud runtime-context endpoint serves, mirroring the
/// CLI's `--coverage-period` bound so an out-of-range request is refused here
/// instead of costing a round trip.
pub(super) const MAX_PERIOD_DAYS: u16 = 90;

/// Run `get_cloud_runtime_context`. CLI-backed like the rest of the
/// runtime-coverage family: `coverage analyze --cloud` already fetches the
/// runtime-context payload and merges it into a `runtime_coverage` block, so
/// the cloud and local tools answer in one shape.
pub async fn run_get_cloud_runtime_context(
    binary: &str,
    params: CloudRuntimeContextParams,
) -> Result<CallToolResult, McpError> {
    match build_get_cloud_runtime_context_args(&params, api_key_is_set()) {
        Ok(args) => {
            run_tool_with_limit(
                binary,
                "get_cloud_runtime_context",
                &args,
                params.max_output_bytes,
            )
            .await
        }
        Err(body) => Ok(CallToolResult::error(vec![ContentBlock::text(body)])),
    }
}

/// Whether the server environment carries a usable API key. A variable set to
/// whitespace counts as unset, as it does for the CLI.
pub(super) fn api_key_is_set() -> bool {
    std::env::var(API_KEY_ENV).is_ok_and(|value| !value.trim().is_empty())
}

/// The typed refusal of a cloud tool called without `FALLOW_API_KEY`.
/// `command` is the CLI command the tool runs, so the message matches the
/// CLI refusal of that command word for word.
pub(super) fn cloud_api_key_missing_body(
    tool: &str,
    command: CloudCommand,
    alternative: &str,
) -> String {
    typed_validation_error_body(
        cloud_api_key_missing_message(command),
        "cloud_api_key_missing",
        &format!(
            "Set FALLOW_API_KEY in the environment the MCP server runs in and restart it. \
             The tool takes no key parameter. {alternative}"
        ),
        &format!("{tool}.api_key"),
    )
}

/// The typed refusal of a cloud tool called without `repo`.
pub(super) fn cloud_repo_missing_body(tool: &str) -> String {
    typed_validation_error_body(
        format!("repo is required for {tool}"),
        "cloud_repo_missing",
        "Pass the repository Fallow Cloud knows this project as, in `owner/repo` form.",
        &format!("{tool}.repo"),
    )
}

/// Build CLI arguments for the `get_cloud_runtime_context` tool.
///
/// `api_key_is_set` is injected rather than read here so the builder's
/// refusals are testable without mutating the process environment that
/// concurrent tests in this binary share.
pub fn build_get_cloud_runtime_context_args(
    params: &CloudRuntimeContextParams,
    api_key_is_set: bool,
) -> Result<Vec<String>, String> {
    if !api_key_is_set {
        return Err(cloud_api_key_missing_body(
            "get_cloud_runtime_context",
            CloudCommand::Analyze,
            "For a local coverage dump instead, call check_runtime_coverage with a `coverage` path.",
        ));
    }

    let repo = params.repo.trim();
    if repo.is_empty() {
        return Err(cloud_repo_missing_body("get_cloud_runtime_context"));
    }

    if let Some(period_days) = params.period_days
        && (period_days == 0 || period_days > MAX_PERIOD_DAYS)
    {
        return Err(typed_validation_error_body(
            format!("period_days must be between 1 and {MAX_PERIOD_DAYS}, got {period_days}"),
            "cloud_period_out_of_range",
            "Request a window the cloud serves, or omit period_days for the 30-day default.",
            "get_cloud_runtime_context.period_days",
        ));
    }

    let mut args = vec![
        "coverage".to_string(),
        "analyze".to_string(),
        "--cloud".to_string(),
        "--format".to_string(),
        "json".to_string(),
        "--quiet".to_string(),
        "--explain".to_string(),
        "--repo".to_string(),
        repo.to_string(),
    ];

    push_global(
        &mut args,
        params.root.as_deref(),
        params.config.as_deref(),
        params.no_cache,
        params.threads,
    );
    push_remote_extends(&mut args, params.allow_remote_extends);
    if params.production == Some(true) {
        args.push("--production".to_string());
    }

    push_trimmed_flag(&mut args, "--project-id", params.project_id.as_deref());
    push_trimmed_flag(&mut args, "--environment", params.environment.as_deref());
    push_trimmed_flag(&mut args, "--commit-sha", params.commit_sha.as_deref());

    if let Some(period_days) = params.period_days {
        args.extend(["--coverage-period".to_string(), period_days.to_string()]);
    }
    if let Some(min_invocations_hot) = params.min_invocations_hot {
        args.extend([
            "--min-invocations-hot".to_string(),
            min_invocations_hot.to_string(),
        ]);
    }
    if let Some(top) = params.top {
        args.extend(["--top".to_string(), top.to_string()]);
    }

    Ok(args)
}

/// Push a `--flag VALUE` pair for a cloud filter, dropping surrounding
/// whitespace. A filter sent as `" "` would otherwise reach the cloud as a
/// literal blank value and silently match nothing.
pub(super) fn push_trimmed_flag(args: &mut Vec<String>, flag: &str, value: Option<&str>) {
    if let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) {
        args.extend([flag.to_string(), value.to_string()]);
    }
}
