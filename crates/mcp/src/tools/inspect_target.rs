use rmcp::ErrorData as McpError;
use rmcp::model::{CallToolResult, Content};

use crate::params::{InspectTarget, InspectTargetParams};

use super::{push_global, push_scope, run_tool, validation_error_body};

const TOOL: &str = "inspect_target";

/// Run the composed `inspect_target` MCP tool through the shared CLI inspect flow.
pub async fn inspect_target(
    binary: &str,
    params: &InspectTargetParams,
) -> Result<CallToolResult, McpError> {
    match build_inspect_args(params) {
        Ok(args) => run_tool(binary, TOOL, &args).await,
        Err(message) => Ok(CallToolResult::error(vec![Content::text(
            validation_error_body(message),
        )])),
    }
}

fn build_inspect_args(params: &InspectTargetParams) -> Result<Vec<String>, String> {
    let mut args = vec![
        "inspect".to_string(),
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

    match &params.target {
        InspectTarget::File { file } => {
            require_non_empty("target.file", file)?;
            args.extend(["--file".to_string(), file.clone()]);
        }
        InspectTarget::Symbol { file, export_name } => {
            require_non_empty("target.file", file)?;
            require_non_empty("target.export_name", export_name)?;
            args.extend(["--symbol".to_string(), format!("{file}:{export_name}")]);
        }
    }

    Ok(args)
}

fn require_non_empty(field: &str, value: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err(format!("{field} must not be empty"));
    }
    Ok(())
}
