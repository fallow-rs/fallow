use crate::params::DecisionSurfaceParams;

use fallow_api::{
    AnalysisOptions, DecisionSurfaceOptions, run_decision_surface as run_decision_surface_api,
    serialize_decision_surface_programmatic_json,
};
use rmcp::ErrorData as McpError;
use rmcp::model::{CallToolResult, ContentBlock};

use super::api_runtime::{
    changed_since_from_param, env_diff_file, json_success, non_empty_path, non_empty_string,
    programmatic_error_body, run_api_blocking,
};

/// Run the `decision_surface` tool through the typed programmatic API.
pub async fn run_decision_surface(
    _binary: &str,
    params: DecisionSurfaceParams,
) -> Result<CallToolResult, McpError> {
    let options = decision_surface_options_from_params(&params);
    let result = run_api_blocking("decision_surface", move || {
        run_decision_surface_api(&options).and_then(serialize_decision_surface_programmatic_json)
    })
    .await?
    .map_or_else(
        |err| CallToolResult::error(vec![ContentBlock::text(programmatic_error_body(&err))]),
        |value| json_success(&value),
    );
    Ok(result)
}

fn decision_surface_options_from_params(params: &DecisionSurfaceParams) -> DecisionSurfaceOptions {
    DecisionSurfaceOptions {
        analysis: AnalysisOptions {
            root: non_empty_path(params.root.as_deref()),
            config_path: non_empty_path(params.config.as_deref()),
            allow_remote_extends: params.allow_remote_extends.unwrap_or(false),
            no_cache: params.no_cache.unwrap_or(false),
            threads: params.threads,
            diff_file: env_diff_file(),
            changed_since: changed_since_from_param(None),
            workspace: non_empty_string(params.workspace.as_deref()).map(|value| vec![value]),
            explain: false,
            ..AnalysisOptions::default()
        },
        base: non_empty_string(params.base.as_deref()),
        max_decisions: params.max_decisions,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::model::ContentBlock;

    #[test]
    fn default_decision_surface_maps_to_programmatic_api_options() {
        let params = DecisionSurfaceParams::default();
        let options = decision_surface_options_from_params(&params);
        assert_eq!(options.max_decisions, None);
    }

    #[test]
    fn forwards_base_and_max_decisions() {
        let params = DecisionSurfaceParams {
            base: Some("origin/main".to_string()),
            max_decisions: Some(5),
            ..DecisionSurfaceParams::default()
        };
        let options = decision_surface_options_from_params(&params);
        assert_eq!(options.base.as_deref(), Some("origin/main"));
        assert_eq!(options.max_decisions, Some(5));
    }

    #[test]
    fn forwards_workspace_scope() {
        let params = DecisionSurfaceParams {
            workspace: Some("apps/web".to_string()),
            ..DecisionSurfaceParams::default()
        };
        let options = decision_surface_options_from_params(&params);
        assert_eq!(
            options.analysis.workspace,
            Some(vec!["apps/web".to_string()])
        );
    }

    #[tokio::test]
    async fn run_decision_surface_api_path_returns_json_without_cli_binary() {
        let project = audit_fixture();

        let result = run_decision_surface(
            "unused-binary-on-api-path",
            DecisionSurfaceParams {
                root: Some(project.path().display().to_string()),
                base: Some("HEAD".to_string()),
                no_cache: Some(true),
                ..DecisionSurfaceParams::default()
            },
        )
        .await
        .expect("api result");

        assert_eq!(result.is_error, Some(false));
        let text = match &result.content[0] {
            ContentBlock::Text(text) => &text.text,
            _ => panic!("expected text content"),
        };
        let json: serde_json::Value = serde_json::from_str(text).expect("json");
        assert_eq!(json["kind"], "decision-surface");
        assert_eq!(json["command"], "decision-surface");
        assert!(json["decisions"].is_array());
    }

    fn audit_fixture() -> tempfile::TempDir {
        let project = tempfile::tempdir().expect("project");
        std::fs::create_dir_all(project.path().join("src")).expect("create src");
        std::fs::write(
            project.path().join("package.json"),
            r#"{"name":"decision-api","type":"module","main":"src/index.ts"}"#,
        )
        .expect("write package");
        std::fs::write(
            project.path().join("src/index.ts"),
            "console.log('entry');\n",
        )
        .expect("write entry");
        git(project.path(), &["init"]);
        git(project.path(), &["add", "."]);
        git(
            project.path(),
            &[
                "-c",
                "user.email=test@example.com",
                "-c",
                "user.name=Test",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-m",
                "initial",
            ],
        );
        std::fs::write(
            project.path().join("src/feature.ts"),
            "export const unused = 1;\n",
        )
        .expect("write changed source");
        project
    }

    fn git(root: &std::path::Path, args: &[&str]) {
        let status = std::process::Command::new("git")
            .args(args)
            .current_dir(root)
            .status()
            .expect("git command");
        assert!(status.success(), "git {args:?} failed");
    }
}
