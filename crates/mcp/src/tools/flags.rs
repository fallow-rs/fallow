use crate::params::FeatureFlagsParams;

use fallow_api::{
    AnalysisOptions, FeatureFlagsOptions, run_feature_flags as run_api_feature_flags,
    serialize_feature_flags_programmatic_json,
};
use rmcp::ErrorData as McpError;
use rmcp::model::{CallToolResult, Content};

use super::api_runtime::{
    env_diff_file, json_success, non_empty_path, non_empty_string, programmatic_error_body,
    run_api_blocking,
};

/// Run `feature_flags` through the typed API.
pub async fn run_feature_flags(
    _binary: &str,
    params: FeatureFlagsParams,
) -> Result<CallToolResult, McpError> {
    let options = feature_flags_options_from_params(&params);
    let result = run_api_blocking("feature_flags", move || {
        run_api_feature_flags(&options).and_then(serialize_feature_flags_programmatic_json)
    })
    .await?
    .map_or_else(
        |err| CallToolResult::error(vec![Content::text(programmatic_error_body(&err))]),
        |value| json_success(&value),
    );
    Ok(result)
}

pub fn run_feature_flags_api_value(
    params: &FeatureFlagsParams,
) -> Result<Option<serde_json::Value>, String> {
    let value = run_api_feature_flags(&feature_flags_options_from_params(params))
        .and_then(serialize_feature_flags_programmatic_json)
        .map_err(|err| programmatic_error_body(&err))?;

    Ok(Some(value))
}

/// Build CLI arguments for the `feature_flags` tool.
pub fn build_feature_flags_args(params: &FeatureFlagsParams) -> Vec<String> {
    let mut args = vec![
        "flags".to_string(),
        "--format".to_string(),
        "json".to_string(),
        "--quiet".to_string(),
        "--explain".to_string(),
    ];

    if let Some(ref root) = params.root {
        args.extend(["--root".to_string(), root.clone()]);
    }
    if let Some(ref config) = params.config {
        args.extend(["--config".to_string(), config.clone()]);
    }
    if params.production == Some(true) {
        args.push("--production".to_string());
    }
    if let Some(ref workspace) = params.workspace {
        args.extend(["--workspace".to_string(), workspace.clone()]);
    }
    if params.no_cache == Some(true) {
        args.push("--no-cache".to_string());
    }
    if let Some(threads) = params.threads {
        args.extend(["--threads".to_string(), threads.to_string()]);
    }
    if let Some(top) = params.top {
        args.extend(["--top".to_string(), top.to_string()]);
    }

    args
}

fn feature_flags_options_from_params(params: &FeatureFlagsParams) -> FeatureFlagsOptions {
    FeatureFlagsOptions {
        analysis: AnalysisOptions {
            root: non_empty_path(params.root.as_deref()),
            config_path: non_empty_path(params.config.as_deref()),
            no_cache: params.no_cache == Some(true),
            threads: params.threads,
            diff_file: env_diff_file(),
            production: params.production == Some(true),
            production_override: params.production,
            changed_since: None,
            workspace: non_empty_string(params.workspace.as_deref())
                .map(|workspace| vec![workspace]),
            changed_workspaces: None,
            explain: true,
            legacy_envelope: false,
        },
        top: params.top,
    }
}

#[cfg(test)]
mod tests {
    use rmcp::model::RawContent;

    use super::*;

    #[tokio::test]
    async fn run_feature_flags_api_path_returns_json_without_cli_binary() {
        let project = tempfile::tempdir().expect("project");
        std::fs::write(
            project.path().join("package.json"),
            r#"{"name":"flags-api","main":"src/index.ts"}"#,
        )
        .expect("write package");
        std::fs::create_dir_all(project.path().join("src")).expect("create src");
        std::fs::write(
            project.path().join("src/index.ts"),
            "if (process.env.FEATURE_ALPHA) {\n  console.log('on');\n}\n",
        )
        .expect("write source");

        let result = run_feature_flags(
            "unused-binary-on-api-path",
            FeatureFlagsParams {
                root: Some(project.path().display().to_string()),
                no_cache: Some(true),
                ..FeatureFlagsParams::default()
            },
        )
        .await
        .expect("mcp result");

        assert!(!result.is_error.unwrap_or(false));
        let [content] = result.content.as_slice() else {
            panic!("expected one content item");
        };
        let RawContent::Text(text) = &content.raw else {
            panic!("expected text content");
        };
        let json: serde_json::Value = serde_json::from_str(&text.text).expect("json");
        assert_eq!(json["kind"], "feature-flags");
        assert_eq!(
            json["feature_flags"][0]["flag_name"].as_str(),
            Some("FEATURE_ALPHA")
        );
    }

    #[tokio::test]
    async fn top_limit_uses_api_path_without_cli_binary() {
        let project = tempfile::tempdir().expect("project");
        std::fs::write(
            project.path().join("package.json"),
            r#"{"name":"flags-api-top","main":"src/index.ts"}"#,
        )
        .expect("write package");
        std::fs::create_dir_all(project.path().join("src")).expect("create src");
        std::fs::write(
            project.path().join("src/index.ts"),
            "if (process.env.FEATURE_ALPHA) {}\nif (process.env.FEATURE_BETA) {}\n",
        )
        .expect("write source");

        let result = run_feature_flags(
            "unused-binary-on-api-path",
            FeatureFlagsParams {
                root: Some(project.path().display().to_string()),
                no_cache: Some(true),
                top: Some(1),
                ..FeatureFlagsParams::default()
            },
        )
        .await;

        let result = result.expect("mcp result");
        assert!(!result.is_error.unwrap_or(false));
        let [content] = result.content.as_slice() else {
            panic!("expected one content item");
        };
        let RawContent::Text(text) = &content.raw else {
            panic!("expected text content");
        };
        let json: serde_json::Value = serde_json::from_str(&text.text).expect("json");
        assert_eq!(json["feature_flags"].as_array().expect("flags").len(), 1);
    }
}
