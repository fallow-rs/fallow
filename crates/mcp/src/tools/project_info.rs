use crate::params::ProjectInfoParams;

use fallow_api::{
    AnalysisOptions, ProjectInfoOptions, run_project_info as run_api_project_info,
    serialize_project_info_programmatic_json,
};
use rmcp::ErrorData as McpError;
use rmcp::model::{CallToolResult, Content};

use super::{
    api_runtime::{
        env_diff_file, json_success, non_empty_path, programmatic_error_body, run_api_blocking,
    },
    push_global,
};

/// Run `project_info` through the typed API.
pub async fn run_project_info(
    _binary: &str,
    params: ProjectInfoParams,
) -> Result<CallToolResult, McpError> {
    let options = project_info_options_from_params(&params);
    let result = run_api_blocking("project_info", move || {
        run_api_project_info(&options).and_then(serialize_project_info_programmatic_json)
    })
    .await?
    .map_or_else(
        |err| CallToolResult::error(vec![Content::text(programmatic_error_body(&err))]),
        |value| json_success(&value),
    );
    Ok(result)
}

pub fn run_project_info_api_value(
    params: &ProjectInfoParams,
) -> Result<Option<serde_json::Value>, String> {
    let value = run_api_project_info(&project_info_options_from_params(params))
        .and_then(serialize_project_info_programmatic_json)
        .map_err(|err| programmatic_error_body(&err))?;

    Ok(Some(value))
}

/// Build CLI arguments for the `project_info` tool.
pub fn build_project_info_args(params: &ProjectInfoParams) -> Vec<String> {
    let mut args = vec![
        "list".to_string(),
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
    if params.entry_points == Some(true) {
        args.push("--entry-points".to_string());
    }
    if params.files == Some(true) {
        args.push("--files".to_string());
    }
    if params.plugins == Some(true) {
        args.push("--plugins".to_string());
    }
    if params.boundaries == Some(true) {
        args.push("--boundaries".to_string());
    }

    args
}

fn project_info_options_from_params(params: &ProjectInfoParams) -> ProjectInfoOptions {
    ProjectInfoOptions {
        analysis: AnalysisOptions {
            root: non_empty_path(params.root.as_deref()),
            config_path: non_empty_path(params.config.as_deref()),
            no_cache: params.no_cache == Some(true),
            threads: params.threads,
            diff_file: env_diff_file(),
            production: false,
            production_override: None,
            changed_since: None,
            workspace: None,
            changed_workspaces: None,
            explain: false,
            legacy_envelope: false,
        },
        entry_points: params.entry_points == Some(true),
        files: params.files == Some(true),
        plugins: params.plugins == Some(true),
        boundaries: params.boundaries == Some(true),
    }
}

#[cfg(test)]
mod tests {
    use rmcp::model::RawContent;

    use super::*;

    #[tokio::test]
    async fn run_project_info_api_path_returns_files_without_cli_binary() {
        let project = tempfile::tempdir().expect("project");
        std::fs::write(
            project.path().join("package.json"),
            r#"{"name":"project-info-api","main":"src/index.ts"}"#,
        )
        .expect("write package");
        std::fs::create_dir_all(project.path().join("src")).expect("create src");
        std::fs::write(
            project.path().join("src/index.ts"),
            "export const value = 1;\n",
        )
        .expect("write source");

        let result = run_project_info(
            "unused-binary-on-api-path",
            ProjectInfoParams {
                root: Some(project.path().display().to_string()),
                no_cache: Some(true),
                ..ProjectInfoParams::default()
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
        assert_eq!(json["file_count"], 1);
        assert_eq!(json["files"][0], "src/index.ts");
        assert_eq!(json["entry_point_count"], 1);
        assert_eq!(json["workspace_count"], 0);
        assert!(json.get("kind").is_none());
    }
}
