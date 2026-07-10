use crate::params::ListSuppressionsParams;

use rmcp::ErrorData as McpError;
use rmcp::model::{CallToolResult, ContentBlock};

use super::{
    push_global, push_remote_extends, push_scope, push_str_flag, run_tool, validation_error_body,
};

/// Run `list_suppressions`. Subprocess-backed: the suppression inventory has
/// no command-neutral programmatic API yet, so this shells out to
/// `fallow suppressions` exactly like `security_candidates`.
pub async fn run_list_suppressions(
    binary: &str,
    params: ListSuppressionsParams,
) -> Result<CallToolResult, McpError> {
    match build_list_suppressions_args(&params) {
        Ok(args) => run_tool(binary, "list_suppressions", &args).await,
        Err(msg) => Ok(CallToolResult::error(vec![ContentBlock::text(msg)])),
    }
}

/// Build CLI arguments for the `list_suppressions` tool.
pub fn build_list_suppressions_args(
    params: &ListSuppressionsParams,
) -> Result<Vec<String>, String> {
    let mut args = vec![
        "suppressions".to_string(),
        "--format".to_string(),
        "json".to_string(),
        "--quiet".to_string(),
    ];

    push_global(
        &mut args,
        params.root.as_deref(),
        params.config.as_deref(),
        params.no_cache,
        params.threads,
    );
    push_remote_extends(&mut args, params.allow_remote_extends);
    push_scope(&mut args, params.production, params.workspace.as_deref());
    push_str_flag(
        &mut args,
        "--changed-since",
        params.changed_since.as_deref(),
    );
    if let Some(files) = params.file.as_ref() {
        for file in files {
            if file.trim().is_empty() {
                return Err(validation_error_body("file entries must not be empty"));
            }
            args.extend(["--file".to_string(), file.clone()]);
        }
    }

    Ok(args)
}
