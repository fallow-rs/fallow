use fallow_api::{
    AnalysisOptions, DeadCodeFilters, DeadCodeOptions, run_dead_code,
    serialize_dead_code_programmatic_json,
};
use rmcp::ErrorData as McpError;
use rmcp::model::{CallToolResult, Content};

use crate::params::CheckChangedParams;

use super::{
    api_runtime::{
        env_diff_file, json_success, non_empty_path, non_empty_string, programmatic_error_body,
        run_api_blocking,
    },
    fallback_policy::{CliFallbackReason, baseline_fallback_reason, regression_fallback_reason},
    push_baseline, push_global, push_regression, push_scope, run_tool,
};

/// Run `check_changed` through the typed API when parameters map cleanly to the
/// programmatic contract, falling back to the CLI for baseline/regression
/// surfaces.
pub async fn run_check_changed(
    binary: &str,
    params: CheckChangedParams,
) -> Result<CallToolResult, McpError> {
    if requires_cli_fallback(&params) {
        let args = build_check_changed_args(params);
        return run_tool(binary, "check_changed", &args).await;
    }

    let options = check_changed_options_from_params(&params);
    let result = run_api_blocking("check_changed", move || {
        run_dead_code(&options).and_then(serialize_dead_code_programmatic_json)
    })
    .await?
    .map_or_else(
        |err| CallToolResult::error(vec![Content::text(programmatic_error_body(&err))]),
        |value| json_success(&value),
    );
    Ok(result)
}

pub fn run_check_changed_api_value(
    params: &CheckChangedParams,
) -> Result<Option<serde_json::Value>, String> {
    if requires_cli_fallback(params) {
        return Ok(None);
    }

    let value = run_dead_code(&check_changed_options_from_params(params))
        .and_then(serialize_dead_code_programmatic_json)
        .map_err(|err| programmatic_error_body(&err))?;

    Ok(Some(value))
}

/// Build CLI arguments for the `check_changed` tool.
pub fn build_check_changed_args(params: CheckChangedParams) -> Vec<String> {
    let mut args = vec![
        "dead-code".to_string(),
        "--format".to_string(),
        "json".to_string(),
        "--quiet".to_string(),
        "--explain".to_string(),
        "--changed-since".to_string(),
        params.since,
    ];

    push_global(
        &mut args,
        params.root.as_deref(),
        params.config.as_deref(),
        params.no_cache,
        params.threads,
    );
    push_scope(&mut args, params.production, params.workspace.as_deref());
    push_baseline(
        &mut args,
        params.baseline.as_deref(),
        params.save_baseline.as_deref(),
    );
    push_regression(
        &mut args,
        params.fail_on_regression,
        params.tolerance.as_deref(),
        params.regression_baseline.as_deref(),
        params.save_regression_baseline.as_deref(),
    );

    if params.include_entry_exports == Some(true) {
        args.push("--include-entry-exports".to_string());
    }

    args
}

fn requires_cli_fallback(params: &CheckChangedParams) -> bool {
    cli_fallback_reason(params).is_some()
}

fn cli_fallback_reason(params: &CheckChangedParams) -> Option<CliFallbackReason> {
    baseline_fallback_reason(params.baseline.as_deref(), params.save_baseline.as_deref()).or_else(
        || {
            regression_fallback_reason(
                params.fail_on_regression,
                params.tolerance.as_deref(),
                params.regression_baseline.as_deref(),
                params.save_regression_baseline.as_deref(),
            )
        },
    )
}

fn check_changed_options_from_params(params: &CheckChangedParams) -> DeadCodeOptions {
    DeadCodeOptions {
        analysis: AnalysisOptions {
            root: non_empty_path(params.root.as_deref()),
            config_path: non_empty_path(params.config.as_deref()),
            no_cache: params.no_cache.unwrap_or(false),
            threads: params.threads,
            production: params.production.unwrap_or(false),
            production_override: params.production,
            changed_since: Some(params.since.clone()),
            diff_file: env_diff_file(),
            workspace: non_empty_string(params.workspace.as_deref())
                .map(|workspace| vec![workspace]),
            explain: true,
            ..AnalysisOptions::default()
        },
        filters: DeadCodeFilters::default(),
        files: Vec::new(),
        include_entry_exports: params.include_entry_exports.unwrap_or(false),
    }
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use rmcp::model::RawContent;

    use super::*;

    #[tokio::test]
    async fn run_check_changed_api_path_returns_json_without_cli_binary() {
        let project = tempfile::tempdir().expect("project");
        std::fs::create_dir_all(project.path().join("src")).expect("create src");
        std::fs::write(
            project.path().join("package.json"),
            r#"{"name":"changed-api","main":"src/index.ts"}"#,
        )
        .expect("write package");
        std::fs::write(
            project.path().join("src/index.ts"),
            "console.log('entry');\n",
        )
        .expect("write source");
        std::fs::write(
            project.path().join("src/feature.ts"),
            "export const used = 1;\n",
        )
        .expect("write feature");
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

        let result = run_check_changed(
            "unused-binary-on-api-path",
            CheckChangedParams {
                root: Some(project.path().display().to_string()),
                since: "HEAD".to_string(),
                no_cache: Some(true),
                ..check_changed_params("")
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
        assert_eq!(json["kind"], "dead-code");
        assert!(json["summary"].is_object());
    }

    #[test]
    fn baseline_options_keep_cli_fallback() {
        let params = CheckChangedParams {
            baseline: Some("baseline.json".to_string()),
            ..check_changed_params("HEAD")
        };

        assert!(requires_cli_fallback(&params));
        assert!(
            run_check_changed_api_value(&params)
                .expect("fallback check")
                .is_none()
        );
    }

    fn git(root: &std::path::Path, args: &[&str]) {
        let status = Command::new("git")
            .args(args)
            .current_dir(root)
            .status()
            .expect("git command starts");
        assert!(status.success(), "git command failed: {args:?}");
    }

    fn check_changed_params(since: &str) -> CheckChangedParams {
        CheckChangedParams {
            root: None,
            since: since.to_string(),
            config: None,
            production: None,
            workspace: None,
            baseline: None,
            save_baseline: None,
            fail_on_regression: None,
            tolerance: None,
            regression_baseline: None,
            save_regression_baseline: None,
            include_entry_exports: None,
            no_cache: None,
            threads: None,
        }
    }
}
