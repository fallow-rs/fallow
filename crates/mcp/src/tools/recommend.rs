use crate::params::RecommendParams;

use rmcp::ErrorData as McpError;
use rmcp::model::CallToolResult;

use super::{push_str_flag, run_tool};

/// Build CLI arguments for the `recommend` tool.
///
/// `fallow recommend` is JSON-first and honors only `--root` and `--format`
/// (it runs project detection, not the config loader or the analysis
/// pipeline), so no other global flags are forwarded.
pub fn build_recommend_args(params: &RecommendParams) -> Vec<String> {
    let mut args = vec![
        "recommend".to_string(),
        "--format".to_string(),
        "json".to_string(),
        "--quiet".to_string(),
    ];
    push_str_flag(&mut args, "--root", params.root.as_deref());
    args
}

/// Run the `recommend` tool: emit a project-tailored config recommendation
/// (`fallow recommend --format json`). Read-only and always exit-0; the CLI
/// owns detection and classification, this is a thin subprocess wrapper.
pub async fn run_recommend(
    binary: &str,
    params: RecommendParams,
) -> Result<CallToolResult, McpError> {
    let args = build_recommend_args(&params);
    run_tool(binary, "recommend", &args).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_recommend_args_defaults_to_json_recommend() {
        let args = build_recommend_args(&RecommendParams::default());
        assert_eq!(args, ["recommend", "--format", "json", "--quiet"]);
    }

    #[test]
    fn build_recommend_args_forwards_root() {
        let args = build_recommend_args(&RecommendParams {
            root: Some("/tmp/project".to_string()),
        });
        assert_eq!(
            args,
            [
                "recommend",
                "--format",
                "json",
                "--quiet",
                "--root",
                "/tmp/project"
            ]
        );
    }

    #[test]
    fn build_recommend_args_skips_empty_root() {
        let args = build_recommend_args(&RecommendParams {
            root: Some(String::new()),
        });
        assert_eq!(args, ["recommend", "--format", "json", "--quiet"]);
    }
}
