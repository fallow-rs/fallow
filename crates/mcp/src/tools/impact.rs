use crate::params::{ImpactAllParams, ImpactClosureParams, ImpactParams};

use rmcp::ErrorData as McpError;
use rmcp::model::CallToolResult;

use super::{push_global, push_scope, push_str_flag, run_tool, validation_error_body};

/// Run the read-only `impact` value report through the CLI-backed local store
/// reader. It is not an analysis tool, and the store lifecycle remains CLI-owned.
pub async fn run_impact(binary: &str, params: ImpactParams) -> Result<CallToolResult, McpError> {
    let args = build_impact_args(&params);
    run_tool(binary, "impact", &args).await
}

/// Run the read-only cross-repo `impact_all` aggregate through the CLI-backed
/// local store reader.
pub async fn run_impact_all(
    binary: &str,
    params: ImpactAllParams,
) -> Result<CallToolResult, McpError> {
    let args = build_impact_all_args(&params);
    run_tool(binary, "impact_all", &args).await
}

/// Run the read-only `impact_closure` evidence query through the existing
/// `dead-code --impact-closure` CLI path.
pub async fn run_impact_closure(
    binary: &str,
    params: ImpactClosureParams,
) -> Result<CallToolResult, McpError> {
    let args = match build_impact_closure_args(&params) {
        Ok(args) => args,
        Err(msg) => {
            return Ok(CallToolResult::error(vec![
                rmcp::model::ContentBlock::text(msg),
            ]));
        }
    };
    run_tool(binary, "impact_closure", &args).await
}

/// Build CLI arguments for the `impact` tool.
///
/// `fallow impact` (bare, no subcommand) renders the read-only value report.
/// The mutating `enable` / `disable` subcommands are deliberately not exposed:
/// enabling local tracking is a one-time human setup step, not an agent action.
pub fn build_impact_args(params: &ImpactParams) -> Vec<String> {
    let mut args = vec![
        "impact".to_string(),
        "--format".to_string(),
        "json".to_string(),
        "--quiet".to_string(),
    ];

    push_str_flag(&mut args, "--root", params.root.as_deref());

    args
}

/// Build CLI arguments for the `impact_all` cross-repo aggregate tool.
///
/// `fallow impact --all` rolls every tracked project on this machine into one
/// read-only view. It takes no `root`: the aggregate reads the user config dir,
/// independent of any single repo. Invalid `sort` values are rejected by the
/// CLI (clap value-enum) and surface as a structured exit-2 error.
pub fn build_impact_all_args(params: &ImpactAllParams) -> Vec<String> {
    let mut args = vec![
        "impact".to_string(),
        "--all".to_string(),
        "--format".to_string(),
        "json".to_string(),
        "--quiet".to_string(),
    ];

    push_str_flag(&mut args, "--sort", params.sort.as_deref());
    if let Some(limit) = params.limit {
        args.push("--limit".to_string());
        args.push(limit.to_string());
    }

    args
}

/// Build CLI arguments for the `impact_closure` tool.
pub fn build_impact_closure_args(params: &ImpactClosureParams) -> Result<Vec<String>, String> {
    if params.path.trim().is_empty() {
        return Err(validation_error_body("path must not be empty"));
    }

    let mut args = vec![
        "dead-code".to_string(),
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
    args.extend(["--impact-closure".to_string(), params.path.clone()]);

    Ok(args)
}
