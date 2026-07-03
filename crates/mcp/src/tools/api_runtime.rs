use std::path::PathBuf;
use std::time::Duration;

use fallow_api::ProgrammaticError;
use rmcp::ErrorData as McpError;
use rmcp::model::{CallToolResult, Content};
use serde::Serialize;

pub(super) async fn run_api_blocking<T, F>(
    tool: &'static str,
    task: F,
) -> Result<Result<T, ProgrammaticError>, McpError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, ProgrammaticError> + Send + 'static,
{
    let timeout = super::timeout_duration();
    run_api_blocking_with_timeout(tool, timeout, task).await
}

async fn run_api_blocking_with_timeout<T, F>(
    tool: &'static str,
    timeout: Duration,
    task: F,
) -> Result<Result<T, ProgrammaticError>, McpError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, ProgrammaticError> + Send + 'static,
{
    let task = tokio::task::spawn_blocking(task);
    match tokio::time::timeout(timeout, task).await {
        Ok(Ok(result)) => Ok(result),
        Ok(Err(err)) => Err(McpError::internal_error(
            format!("{tool} task failed: {err}"),
            None,
        )),
        Err(_) => Ok(Err(ProgrammaticError::new(
            format!("{tool} task timed out after {}s", timeout.as_secs()),
            2,
        )
        .with_code("FALLOW_MCP_API_TIMEOUT")
        .with_help(
            "Set FALLOW_TIMEOUT_SECS to increase the response deadline. API-backed analysis may finish in-process after the MCP timeout response.",
        )
        .with_context(tool))),
    }
}

pub(super) fn env_diff_file() -> Option<PathBuf> {
    std::env::var_os("FALLOW_DIFF_FILE")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

pub(super) fn env_changed_since() -> Option<String> {
    std::env::var("FALLOW_CHANGED_SINCE")
        .ok()
        .filter(|value| !value.is_empty())
}

pub(super) fn changed_since_from_param(value: Option<&str>) -> Option<String> {
    non_empty_string(value).or_else(env_changed_since)
}

pub(super) fn non_empty_path(value: Option<&str>) -> Option<PathBuf> {
    value.and_then(|value| (!value.is_empty()).then(|| PathBuf::from(value)))
}

pub(super) fn non_empty_string(value: Option<&str>) -> Option<String> {
    value.and_then(|value| (!value.is_empty()).then(|| value.to_string()))
}

pub(super) fn json_success(value: &impl Serialize) -> CallToolResult {
    let text = serde_json::to_string(value).unwrap_or_else(|_| "{}".to_string());
    CallToolResult::success(vec![Content::text(text)])
}

pub(super) fn programmatic_error_body(error: &ProgrammaticError) -> String {
    serde_json::json!({
        "error": true,
        "message": error.message,
        "exit_code": error.exit_code,
        "code": error.code,
        "help": error.help,
        "context": error.context,
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn api_timeout_returns_structured_tool_error() {
        let result = run_api_blocking_with_timeout("analyze", Duration::ZERO, || {
            std::thread::sleep(Duration::from_millis(25));
            Ok::<_, ProgrammaticError>(serde_json::json!({ "ok": true }))
        })
        .await
        .expect("timeout should stay a tool result");

        let err = result.expect_err("timeout should be structured error");
        assert_eq!(err.exit_code, 2);
        assert_eq!(err.code.as_deref(), Some("FALLOW_MCP_API_TIMEOUT"));
        assert_eq!(err.context.as_deref(), Some("analyze"));
    }

    #[test]
    fn changed_since_from_param_prefers_param_over_empty_env_fallback() {
        assert_eq!(
            changed_since_from_param(Some("origin/main")),
            Some("origin/main".to_string())
        );
        assert_eq!(changed_since_from_param(Some("")), env_changed_since());
    }
}
