use crate::params::ListBoundariesParams;

use fallow_api::{
    AnalysisOptions, ListBoundariesOptions, run_list_boundaries as run_api_list_boundaries,
    serialize_list_boundaries_programmatic_json,
};
use rmcp::ErrorData as McpError;
use rmcp::model::{CallToolResult, Content};

use super::{
    api_runtime::{
        env_diff_file, json_success, non_empty_path, programmatic_error_body, run_api_blocking,
    },
    push_global,
};

/// Run `list_boundaries` through the typed API.
pub async fn run_list_boundaries(
    _binary: &str,
    params: ListBoundariesParams,
) -> Result<CallToolResult, McpError> {
    let options = list_boundaries_options_from_params(&params);
    let result = run_api_blocking("list_boundaries", move || {
        run_api_list_boundaries(&options).and_then(serialize_list_boundaries_programmatic_json)
    })
    .await?
    .map_or_else(
        |err| CallToolResult::error(vec![Content::text(programmatic_error_body(&err))]),
        |value| json_success(&value),
    );
    Ok(result)
}

pub fn run_list_boundaries_api_value(
    params: &ListBoundariesParams,
) -> Result<Option<serde_json::Value>, String> {
    let value = run_api_list_boundaries(&list_boundaries_options_from_params(params))
        .and_then(serialize_list_boundaries_programmatic_json)
        .map_err(|err| programmatic_error_body(&err))?;

    Ok(Some(value))
}

pub fn build_list_boundaries_args(params: &ListBoundariesParams) -> Vec<String> {
    let mut args = vec![
        "list".to_string(),
        "--boundaries".to_string(),
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

    args
}

fn list_boundaries_options_from_params(params: &ListBoundariesParams) -> ListBoundariesOptions {
    ListBoundariesOptions {
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
    }
}

#[cfg(test)]
mod tests {
    use rmcp::model::RawContent;

    use super::*;

    #[tokio::test]
    async fn run_list_boundaries_api_path_returns_json_without_cli_binary() {
        let project = tempfile::tempdir().expect("project");
        std::fs::write(
            project.path().join("package.json"),
            r#"{"name":"boundaries-api","main":"src/index.ts"}"#,
        )
        .expect("write package");
        std::fs::write(
            project.path().join(".fallowrc.json"),
            r#"{
                "boundaries": {
                    "zones": [
                        { "name": "app", "patterns": ["src/app/**"] },
                        { "name": "shared", "patterns": ["src/shared/**"] }
                    ],
                    "rules": [
                        { "from": "app", "allow": ["shared"] }
                    ]
                }
            }"#,
        )
        .expect("write config");
        std::fs::create_dir_all(project.path().join("src/app")).expect("create app");
        std::fs::create_dir_all(project.path().join("src/shared")).expect("create shared");
        std::fs::write(
            project.path().join("src/app/index.ts"),
            "export const app = 1;\n",
        )
        .expect("write app");
        std::fs::write(
            project.path().join("src/shared/index.ts"),
            "export const shared = 1;\n",
        )
        .expect("write shared");

        let result = run_list_boundaries(
            "unused-binary-on-api-path",
            ListBoundariesParams {
                root: Some(project.path().display().to_string()),
                no_cache: Some(true),
                ..ListBoundariesParams::default()
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
        assert_eq!(json["kind"], "list-boundaries");
        assert_eq!(json["boundaries"]["zone_count"], 2);
        assert_eq!(json["boundaries"]["rule_count"], 1);
    }
}
