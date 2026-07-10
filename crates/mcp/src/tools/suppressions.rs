use crate::params::ListSuppressionsParams;

use rmcp::ErrorData as McpError;
use rmcp::model::CallToolResult;

use super::{push_global, push_scope, push_str_flag, run_tool};

/// Run `list_suppressions`. Subprocess-backed: the suppression inventory has
/// no command-neutral programmatic API yet, so this shells out to
/// `fallow suppressions` exactly like `security_candidates`.
pub async fn run_list_suppressions(
    binary: &str,
    params: ListSuppressionsParams,
) -> Result<CallToolResult, McpError> {
    let args = build_list_suppressions_args(&params);
    run_tool(binary, "list_suppressions", &args).await
}

/// Build CLI arguments for the `list_suppressions` tool.
pub fn build_list_suppressions_args(params: &ListSuppressionsParams) -> Vec<String> {
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
    push_scope(&mut args, params.production, params.workspace.as_deref());
    push_str_flag(
        &mut args,
        "--changed-since",
        params.changed_since.as_deref(),
    );
    if let Some(files) = params.file.as_ref() {
        for file in files {
            args.extend(["--file".to_string(), file.clone()]);
        }
    }

    args
}
