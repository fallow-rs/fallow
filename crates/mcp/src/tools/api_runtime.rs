use std::path::PathBuf;

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
    let task = tokio::task::spawn_blocking(task);
    match tokio::time::timeout(timeout, task).await {
        Ok(Ok(result)) => Ok(result),
        Ok(Err(err)) => Err(McpError::internal_error(
            format!("{tool} task failed: {err}"),
            None,
        )),
        Err(_) => Err(McpError::internal_error(
            format!("{tool} task timed out after {}s", timeout.as_secs()),
            None,
        )),
    }
}

pub(super) fn env_diff_file() -> Option<PathBuf> {
    std::env::var_os("FALLOW_DIFF_FILE")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
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
