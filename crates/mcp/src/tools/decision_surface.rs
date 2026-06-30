use crate::params::DecisionSurfaceParams;

use rmcp::ErrorData as McpError;
use rmcp::model::CallToolResult;

use super::{push_global, push_scope, push_str_flag, run_tool};

/// Run the `decision_surface` tool. It is intentionally CLI-backed until the
/// audit brief/decision-surface runner is exposed from `fallow-api`.
pub async fn run_decision_surface(
    binary: &str,
    params: DecisionSurfaceParams,
) -> Result<CallToolResult, McpError> {
    let args = build_decision_surface_args(&params);
    run_tool(binary, "decision_surface", &args).await
}

/// Build CLI arguments for the `decision_surface` tool: `fallow decision-surface
/// --format json --quiet`. The separable, cheap apex; reuses the changed-code
/// (brief) analysis, NOT the full pipeline.
pub fn build_decision_surface_args(params: &DecisionSurfaceParams) -> Vec<String> {
    let mut args = vec![
        "decision-surface".to_string(),
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
    push_str_flag(&mut args, "--base", params.base.as_deref());
    // The decision surface is a read-only orientation; it has no production knob,
    // so only the workspace scope flows through.
    push_scope(&mut args, None, params.workspace.as_deref());
    if let Some(max_decisions) = params.max_decisions {
        args.extend(["--max-decisions".to_string(), max_decisions.to_string()]);
    }

    args
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_decision_surface_command_with_json_quiet() {
        let params = DecisionSurfaceParams::default();
        let args = build_decision_surface_args(&params);
        assert_eq!(args[0], "decision-surface");
        assert!(args.windows(2).any(|p| p == ["--format", "json"]));
        assert!(args.contains(&"--quiet".to_string()));
    }

    #[test]
    fn forwards_base_and_max_decisions() {
        let params = DecisionSurfaceParams {
            base: Some("origin/main".to_string()),
            max_decisions: Some(5),
            ..DecisionSurfaceParams::default()
        };
        let args = build_decision_surface_args(&params);
        assert!(args.windows(2).any(|p| p == ["--base", "origin/main"]));
        assert!(args.windows(2).any(|p| p == ["--max-decisions", "5"]));
    }

    #[test]
    fn forwards_workspace_scope() {
        let params = DecisionSurfaceParams {
            workspace: Some("apps/web".to_string()),
            ..DecisionSurfaceParams::default()
        };
        let args = build_decision_surface_args(&params);
        assert!(args.windows(2).any(|p| p == ["--workspace", "apps/web"]));
    }
}
