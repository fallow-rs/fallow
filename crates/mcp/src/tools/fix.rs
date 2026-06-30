use crate::params::FixParams;

use rmcp::ErrorData as McpError;
use rmcp::model::CallToolResult;

use super::{push_global, push_scope, run_tool};

/// Run the read-only fix preview. It is CLI-backed because fix planning shares
/// the same command-owned mutation safeguards as fix apply.
pub async fn run_fix_preview(binary: &str, params: FixParams) -> Result<CallToolResult, McpError> {
    let args = build_fix_preview_args(&params);
    run_tool(binary, "fix_preview", &args).await
}

/// Run the mutating fix apply path. This intentionally remains CLI-backed.
pub async fn run_fix_apply(binary: &str, params: FixParams) -> Result<CallToolResult, McpError> {
    let args = build_fix_apply_args(&params);
    run_tool(binary, "fix_apply", &args).await
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
    push_global(
        &mut args,
        params.root.as_deref(),
        params.config.as_deref(),
        params.no_cache,
        params.threads,
    );
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
    push_global(
        &mut args,
        params.root.as_deref(),
        params.config.as_deref(),
        params.no_cache,
        params.threads,
    );
    push_scope(&mut args, params.production, params.workspace.as_deref());
    args
}
