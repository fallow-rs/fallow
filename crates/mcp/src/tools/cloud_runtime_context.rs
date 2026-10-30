use fallow_types::cloud::{CloudCommand, cloud_api_key_missing_message};
use rmcp::ErrorData as McpError;
use rmcp::model::{CallToolResult, ContentBlock};

use crate::params::CloudRuntimeContextParams;

use super::{push_global, push_remote_extends, run_tool_with_limit, typed_validation_error_body};

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
    match build_get_cloud_runtime_context_args(&params) {
        Ok(args) => {
            run_cloud_tool(
                binary,
                "get_cloud_runtime_context",
                &args,
                params.max_output_bytes,
                || {
                    cloud_api_key_missing_body(
                        "get_cloud_runtime_context",
                        CloudCommand::Analyze,
                        RUNTIME_CONTEXT_ALTERNATIVE,
                    )
                },
            )
            .await
        }
        Err(body) => Ok(CallToolResult::error(vec![ContentBlock::text(body)])),
    }
}

const RUNTIME_CONTEXT_ALTERNATIVE: &str =
    "For a local coverage dump instead, call check_runtime_coverage with a `coverage` path.";

/// Run a cloud tool through the CLI. The CLI resolves the key with the same
/// order as every other cloud command (`FALLOW_API_KEY`, then the key that
/// `fallow cloud login` stored). The CLI checks the key before any analysis,
/// so a call without a key stops fast. That refusal comes back as the typed
/// `cloud_api_key_missing` body.
pub(super) async fn run_cloud_tool(
    binary: &str,
    tool: &'static str,
    args: &[String],
    max_output_bytes: Option<usize>,
    refusal: impl FnOnce() -> String,
) -> Result<CallToolResult, McpError> {
    let result = run_tool_with_limit(binary, tool, args, max_output_bytes).await?;
    if result.is_error == Some(true)
        && result
            .content
            .iter()
            .filter_map(|block| block.as_text())
            .any(|text| is_missing_key_refusal(&text.text))
    {
        return Ok(CallToolResult::error(vec![ContentBlock::text(refusal())]));
    }
    Ok(result)
}

/// Whether CLI output is the refusal of a cloud command without a key.
pub(super) fn is_missing_key_refusal(text: &str) -> bool {
    text.contains(MISSING_KEY_REFUSAL_START)
}

/// The first sentence of `cloud_api_key_missing_message`.
const MISSING_KEY_REFUSAL_START: &str = "Cloud runtime coverage requires an API key.";

/// The typed refusal of a cloud tool when the CLI found no key.
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
            "Run `fallow cloud login`, or set FALLOW_API_KEY in the environment the MCP server \
             runs in and restart it. The tool takes no key parameter. {alternative}"
        ),
        &format!("{tool}.api_key"),
    )
}

/// Trim the `repo` parameter of a cloud tool, or return the typed refusal.
///
/// The CLI refuses a dot-only repository too, because the value becomes one
/// URL path segment in which `.` and `..` are not percent-encoded. Refusing
/// it here returns the refusal before a subprocess starts.
pub(super) fn cloud_repo_param<'a>(tool: &str, raw: &'a str) -> Result<&'a str, String> {
    let repo = raw.trim();
    if repo.is_empty() {
        return Err(typed_validation_error_body(
            format!("repo is required for {tool}"),
            "cloud_repo_missing",
            "Pass the repository Fallow Cloud knows this project as, in `owner/repo` form.",
            &format!("{tool}.repo"),
        ));
    }
    if repo.chars().all(|c| c == '.') {
        return Err(typed_validation_error_body(
            format!("repo must name a repository (not only dots), got {repo}"),
            "cloud_repo_invalid",
            "Pass the repository Fallow Cloud knows this project as, in `owner/repo` form.",
            &format!("{tool}.repo"),
        ));
    }
    Ok(repo)
}

/// Build CLI arguments for the `get_cloud_runtime_context` tool. The key is
/// never a tool parameter: a key sent as a call argument would be recorded in
/// every transcript that made the call. The CLI reads it.
pub fn build_get_cloud_runtime_context_args(
    params: &CloudRuntimeContextParams,
) -> Result<Vec<String>, String> {
    let repo = cloud_repo_param("get_cloud_runtime_context", &params.repo)?;

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
