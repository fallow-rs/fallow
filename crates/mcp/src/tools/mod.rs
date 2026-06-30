mod analyze;
mod api_runtime;
mod audit;
mod check_changed;
mod check_runtime_coverage;
mod code_mode;
mod decision_surface;
mod dupes;
mod explain;
mod fallback_policy;
mod fix;
mod flags;
mod health;
mod impact;
mod inspect_target;
mod list_boundaries;
mod project_info;
mod security;
mod trace;

pub use analyze::{build_analyze_args, run_analyze};
pub use audit::{build_audit_args, run_audit};
pub use check_changed::{build_check_changed_args, run_check_changed};
#[cfg(test)]
pub use check_runtime_coverage::build_get_token_blast_radius_args;
pub use check_runtime_coverage::{
    build_check_runtime_coverage_args, build_get_blast_radius_args,
    build_get_cleanup_candidates_args, build_get_hot_paths_args, build_get_importance_args,
    run_check_runtime_coverage, run_get_blast_radius, run_get_cleanup_candidates,
    run_get_hot_paths, run_get_importance, run_get_token_blast_radius,
};
pub use code_mode::execute_code_mode;
pub use decision_surface::run_decision_surface;
pub use dupes::{build_find_dupes_args, run_find_dupes};
pub use explain::{build_explain_args, run_explain};
#[cfg(test)]
pub use fix::{build_fix_apply_args, build_fix_preview_args};
pub use fix::{run_fix_apply, run_fix_preview};
pub use flags::{build_feature_flags_args, run_feature_flags};
pub use health::{build_health_args, run_health};
#[cfg(test)]
pub use impact::build_impact_all_args;
pub use impact::{build_impact_args, run_impact, run_impact_all};
pub use inspect_target::inspect_target;
pub use list_boundaries::{build_list_boundaries_args, run_list_boundaries};
pub use project_info::{build_project_info_args, run_project_info};
pub use security::{build_security_candidates_args, run_security_candidates};
pub use trace::{
    build_trace_clone_args, build_trace_dependency_args, build_trace_export_args,
    build_trace_file_args, run_trace_clone_tool, run_trace_dependency_tool, run_trace_export_tool,
    run_trace_file_tool,
};

use std::process::Stdio;
use std::time::Duration;

use fallow_types::issue_meta::MCP_ISSUE_TYPE_FLAGS;
use rmcp::ErrorData as McpError;
use rmcp::model::{CallToolResult, Content, RawContent};
use tokio::process::Command;

/// Default subprocess timeout in seconds.
const DEFAULT_TIMEOUT_SECS: u64 = 120;

/// Push a `--flag VALUE` pair onto `args` only when `value` is `Some(s)` and
/// `s` is non-empty.
fn push_str_flag(args: &mut Vec<String>, flag: &str, value: Option<&str>) {
    if let Some(s) = value
        && !s.is_empty()
    {
        args.extend([flag.to_string(), s.to_string()]);
    }
}

/// Push root directory and config file flags (shared by all tools).
fn push_global(
    args: &mut Vec<String>,
    root: Option<&str>,
    config: Option<&str>,
    no_cache: Option<bool>,
    threads: Option<usize>,
) {
    push_str_flag(args, "--root", root);
    push_str_flag(args, "--config", config);
    if no_cache == Some(true) {
        args.push("--no-cache".to_string());
    }
    if let Some(threads) = threads {
        args.extend(["--threads".to_string(), threads.to_string()]);
    }
}

/// Push production mode and workspace scope flags.
fn push_scope(args: &mut Vec<String>, production: Option<bool>, workspace: Option<&str>) {
    if production == Some(true) {
        args.push("--production".to_string());
    }
    push_str_flag(args, "--workspace", workspace);
}

/// Push baseline comparison flags.
fn push_baseline(args: &mut Vec<String>, baseline: Option<&str>, save_baseline: Option<&str>) {
    push_str_flag(args, "--baseline", baseline);
    push_str_flag(args, "--save-baseline", save_baseline);
}

/// Push regression comparison flags.
fn push_regression(
    args: &mut Vec<String>,
    fail: Option<bool>,
    tolerance: Option<&str>,
    baseline: Option<&str>,
    save: Option<&str>,
) {
    if fail == Some(true) {
        args.push("--fail-on-regression".to_string());
    }
    push_str_flag(args, "--tolerance", tolerance);
    push_str_flag(args, "--regression-baseline", baseline);
    push_str_flag(args, "--save-regression-baseline", save);
}

/// Issue type flag names mapped to their CLI flags.
pub const ISSUE_TYPE_FLAGS: &[(&str, &str)] = MCP_ISSUE_TYPE_FLAGS;

/// Valid detection modes for the `find_dupes` tool.
pub const VALID_DUPES_MODES: &[&str] = &["strict", "mild", "weak", "semantic"];

/// Valid gate values for the `audit` tool.
pub const VALID_AUDIT_GATES: &[&str] = &["new-only", "all"];

/// Build a structured validation error body matching the shape `run_fallow` emits
/// for CLI-level errors.
pub fn validation_error_body(message: impl Into<String>) -> String {
    serde_json::json!({
        "error": true,
        "message": message.into(),
        "exit_code": 2,
    })
    .to_string()
}

/// Read the subprocess timeout from `FALLOW_TIMEOUT_SECS` or fall back to the default.
fn timeout_duration() -> Duration {
    std::env::var("FALLOW_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .map_or(
            Duration::from_secs(DEFAULT_TIMEOUT_SECS),
            Duration::from_secs,
        )
}

/// Execute the fallow CLI binary with the given arguments and return the result.
///
/// Untagged variant retained for the subprocess-behavior tests (timeouts, exit
/// codes, signal handling); production tool dispatch goes through `run_tool` so
/// the spawned CLI's telemetry is attributed to the `mcp` surface.
#[cfg(test)]
pub async fn run_fallow(binary: &str, args: &[String]) -> Result<CallToolResult, McpError> {
    spawn_fallow(binary, args, timeout_duration(), None).await
}

/// Execute the fallow CLI for a named MCP tool. Tags the spawned process so its
/// telemetry event is attributed to the `mcp` integration surface and the
/// specific tool, instead of looking like any other `cli_json` run. The CLI
/// only reads these when telemetry is enabled; they carry no paths or
/// identifiers, and the tool name is allowlist-validated CLI-side.
pub async fn run_tool(
    binary: &str,
    tool: &'static str,
    args: &[String],
) -> Result<CallToolResult, McpError> {
    spawn_fallow(binary, args, timeout_duration(), Some(tool)).await
}

#[cfg(all(test, unix))]
pub async fn run_fallow_with_timeout(
    binary: &str,
    args: &[String],
    timeout: Duration,
) -> Result<CallToolResult, McpError> {
    spawn_fallow(binary, args, timeout, None).await
}

async fn spawn_fallow(
    binary: &str,
    args: &[String],
    timeout: Duration,
    tool: Option<&'static str>,
) -> Result<CallToolResult, McpError> {
    let mut command = Command::new(binary);
    command
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(tool) = tool {
        // Re-tag the spawned CLI's telemetry event as the MCP surface + tool.
        // The CLI inherits this process's env, so the existing telemetry path
        // emits a single, correctly-attributed event (no second emit here).
        command
            .env("FALLOW_INTEGRATION_SURFACE", "mcp")
            .env("FALLOW_MCP_TOOL", tool);
    }
    let output = tokio::time::timeout(timeout, command.output())
        .await
        .map_err(|_| {
            McpError::internal_error(
                format!(
                    "fallow subprocess timed out after {}s. \
                 Set FALLOW_TIMEOUT_SECS to increase the limit.",
                    timeout.as_secs()
                ),
                None,
            )
        })?
        .map_err(|e| {
            McpError::internal_error(
                format!(
                    "Failed to execute fallow binary '{binary}': {e}. \
                 Ensure fallow is installed and available in PATH, \
                 or set the FALLOW_BIN environment variable."
                ),
                None,
            )
        })?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if !output.status.success() {
        return Ok(non_success_result(
            output.status.code().unwrap_or(-1),
            &stdout,
            &stderr,
        ));
    }

    if stdout.is_empty() {
        return Ok(CallToolResult::success(vec![Content::text(
            "{}".to_string(),
        )]));
    }

    Ok(CallToolResult::success(vec![Content::text(
        stdout.to_string(),
    )]))
}

/// Translate a non-zero CLI exit into the MCP result envelope. Exit 1 (issues
/// found) is a success carrying the JSON; structured stdout passes through as an
/// error; otherwise an error JSON is synthesized from stderr.
fn non_success_result(exit_code: i32, stdout: &str, stderr: &str) -> CallToolResult {
    if exit_code == 1 {
        let text = if stdout.is_empty() {
            "{}".to_string()
        } else {
            stdout.to_string()
        };
        return CallToolResult::success(vec![Content::text(text)]);
    }

    if !stdout.is_empty() && serde_json::from_str::<serde_json::Value>(stdout).is_ok() {
        return CallToolResult::error(vec![Content::text(stdout.to_string())]);
    }

    let message = if stderr.is_empty() {
        format!("fallow exited with code {exit_code}")
    } else {
        stderr.trim().to_string()
    };

    let error_json = serde_json::json!({
        "error": true,
        "message": message,
        "exit_code": exit_code,
    });

    CallToolResult::error(vec![Content::text(error_json.to_string())])
}

/// Execute fallow and ensure successful JSON responses have a top-level
/// `warnings` array for agent-facing runtime context tools. Untagged variant
/// retained for tests; production goes through `run_tool_with_top_level_warnings`.
#[cfg(all(test, unix))]
pub async fn run_fallow_with_top_level_warnings(
    binary: &str,
    args: &[String],
) -> Result<CallToolResult, McpError> {
    Ok(ensure_top_level_warnings(run_fallow(binary, args).await?))
}

/// Tool-attributed variant of `run_fallow_with_top_level_warnings` (see
/// `run_tool`).
pub async fn run_tool_with_top_level_warnings(
    binary: &str,
    tool: &'static str,
    args: &[String],
) -> Result<CallToolResult, McpError> {
    Ok(ensure_top_level_warnings(
        run_tool(binary, tool, args).await?,
    ))
}

fn ensure_top_level_warnings(result: CallToolResult) -> CallToolResult {
    if result.is_error == Some(true) {
        return result;
    }

    let Some(content) = result.content.first() else {
        return result;
    };
    let RawContent::Text(text) = &content.raw else {
        return result;
    };
    let Ok(mut value) = serde_json::from_str::<serde_json::Value>(&text.text) else {
        return result;
    };
    let Some(map) = value.as_object_mut() else {
        return result;
    };

    map.entry("warnings".to_string())
        .or_insert_with(|| serde_json::Value::Array(Vec::new()));

    let text = serde_json::to_string_pretty(&value).unwrap_or_else(|_| text.text.clone());
    CallToolResult::success(vec![Content::text(text)])
}
