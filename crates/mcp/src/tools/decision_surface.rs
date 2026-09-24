use crate::params::DecisionSurfaceParams;

use fallow_api::{
    AnalysisOptions, DecisionSurfaceOptions, run_decision_surface as run_decision_surface_api,
    serialize_decision_surface_programmatic_json,
};
use rmcp::ErrorData as McpError;
use rmcp::model::{CallToolResult, ContentBlock};

use super::api_runtime::{
    env_changed_since, env_diff_file, json_success, non_empty_path, non_empty_string,
    programmatic_error_body, run_api_blocking, workspace_patterns_from_param,
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
            ambient_diff_file: env_diff_file(),
            // The base ref of this tool, not a narrowing request: the
            // runtime reads it from `changed_since`, so it stays there.
            changed_since: env_changed_since(),
            workspace: workspace_patterns_from_param(params.workspace.as_deref()),
            explain: false,
            ..AnalysisOptions::default()
        },
        base: non_empty_string(params.base.as_deref()),
        max_decisions: params.max_decisions,
    }
}

#[cfg(test)]
mod tests {
    use super::super::base_root_fixture::git;
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

    /// #2699: called without a `base`, `decision_surface` shares the audit
    /// base-ref detection, and a `root` pointing at a package added on the
    /// branch has no counterpart in the detected base commit. Both used to
    /// fail before analysis.
    #[tokio::test]
    async fn run_decision_surface_auto_detects_the_base_for_a_root_added_on_the_branch() {
        let project = super::super::base_root_fixture::new_package_repo();
        let root = project
            .path()
            .join(super::super::base_root_fixture::NEW_PACKAGE);

        let result = run_decision_surface(
            "unused-binary-on-api-path",
            DecisionSurfaceParams {
                root: Some(root.display().to_string()),
                no_cache: Some(true),
                ..DecisionSurfaceParams::default()
            },
        )
        .await
        .expect("api result");

        let text = match &result.content[0] {
            ContentBlock::Text(text) => &text.text,
            _ => panic!("expected text content"),
        };
        assert_eq!(result.is_error, Some(false), "{text}");
        let json: serde_json::Value = serde_json::from_str(text).expect("json");
        assert_eq!(json["kind"], "decision-surface", "{json}");
        assert!(json["decisions"].is_array(), "{json}");
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
}
