use crate::params::GuardParams;

use rmcp::ErrorData as McpError;
use rmcp::model::CallToolResult;

use super::{push_str_flag, run_tool};

/// Run the read-only architecture guard report through the CLI.
pub async fn run_guard(binary: &str, params: GuardParams) -> Result<CallToolResult, McpError> {
    let args = build_guard_args(&params);
    run_tool(binary, "guard", &args).await
}

/// Build CLI arguments for the `guard` tool.
pub fn build_guard_args(params: &GuardParams) -> Vec<String> {
    let mut args = vec![
        "guard".to_string(),
        "--format".to_string(),
        "json".to_string(),
        "--quiet".to_string(),
    ];

    push_str_flag(&mut args, "--root", params.root.as_deref());
    args.extend(params.files.iter().cloned());

    args
}
