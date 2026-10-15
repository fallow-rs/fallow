use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use fallow_api::{
    AnalysisOptions, DeadCodeFilters, DeadCodeOptions, run_dead_code,
    serialize_dead_code_programmatic_json,
};
use rmcp::ErrorData as McpError;
use rmcp::model::CallToolResult;

use crate::params::CheckChangedParams;

use super::{
    api_runtime::{
        EnvScope, call_cancellation, run_api_blocking, shared_analysis_options, shared_params,
        typed_api_value, typed_tool_result, workspace_patterns_from_param,
    },
    fallback_policy::{baseline_requested, regression_requested},
    push_baseline, push_global, push_regression, push_remote_extends, push_scope, run_tool,
};

/// Run `check_changed` through the typed API when parameters map cleanly to the
/// programmatic contract, falling back to the CLI for baseline/regression
/// surfaces and for type-aware analysis that the config turns on.
pub async fn run_check_changed(
    binary: &str,
    params: CheckChangedParams,
) -> Result<CallToolResult, McpError> {
    if !requires_cli_fallback(&params) {
        let cancellation = call_cancellation();
        let options = check_changed_options_from_params(&params, Some(Arc::clone(&cancellation)));
        let result = run_api_blocking("check_changed", cancellation, move || {
            run_dead_code(&options).and_then(serialize_dead_code_programmatic_json)
        })
        .await?;
        if let Some(result) = typed_tool_result(result) {
            return Ok(result);
        }
    }

    let args = build_check_changed_args(params);
    run_tool(binary, "check_changed", &args).await
}

pub fn run_check_changed_api_value(
    params: &CheckChangedParams,
    cancellation: Option<Arc<AtomicBool>>,
) -> Result<Option<serde_json::Value>, String> {
    if requires_cli_fallback(params) {
        return Ok(None);
    }

    let options = check_changed_options_from_params(params, cancellation);
    typed_api_value(run_dead_code(&options).and_then(serialize_dead_code_programmatic_json))
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
    push_remote_extends(&mut args, params.allow_remote_extends);
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
    if params.show_cascade == Some(true) {
        args.push("--show-cascade".to_string());
    }

    args
}

fn requires_cli_fallback(params: &CheckChangedParams) -> bool {
    baseline_requested(params.baseline.as_deref(), params.save_baseline.as_deref())
        || regression_requested(
            params.fail_on_regression,
            params.tolerance.as_deref(),
            params.regression_baseline.as_deref(),
            params.save_regression_baseline.as_deref(),
        )
}

fn check_changed_options_from_params(
    params: &CheckChangedParams,
    cancellation: Option<Arc<AtomicBool>>,
) -> DeadCodeOptions {
    DeadCodeOptions {
        finding_ids: Vec::new(),
        analysis: AnalysisOptions {
            production: params.production.unwrap_or(false),
            production_override: params.production,
            // The tool's own `since` wins over any inherited ref.
            changed_since: Some(params.since.clone()),
            workspace: workspace_patterns_from_param(params.workspace.as_deref()),
            explain: true,
            ..shared_analysis_options(shared_params!(params), EnvScope::Ambient, cancellation)
        },
        filters: DeadCodeFilters::default(),
        files: Vec::new(),
        include_entry_exports: params.include_entry_exports.unwrap_or(false),
        show_cascade: params.show_cascade.unwrap_or(false),
    }
}

#[cfg(test)]
mod tests {
    use rmcp::model::ContentBlock;

    use super::super::base_root_fixture::git;
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
        let ContentBlock::Text(text) = content else {
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
            run_check_changed_api_value(&params, None)
                .expect("fallback check")
                .is_none()
        );
    }

    fn check_changed_params(since: &str) -> CheckChangedParams {
        CheckChangedParams {
            root: None,
            since: since.to_string(),
            config: None,
            allow_remote_extends: None,
            production: None,
            workspace: None,
            baseline: None,
            save_baseline: None,
            fail_on_regression: None,
            tolerance: None,
            regression_baseline: None,
            save_regression_baseline: None,
            include_entry_exports: None,
            show_cascade: None,
            no_cache: None,
            threads: None,
        }
    }

    /// `typeAware.enabled` in config makes `fallow dead-code` type-aware. The
    /// typed route and the Code Mode route both hand the call to the CLI.
    #[tokio::test]
    async fn config_type_aware_falls_back_to_the_cli() {
        use super::super::api_runtime::{
            MISSING_FALLBACK_BINARY, assert_cli_fallback_ran, write_type_aware_config,
        };
        let project = tempfile::tempdir().expect("project");
        std::fs::create_dir_all(project.path().join("src")).expect("create src");
        std::fs::write(
            project.path().join("package.json"),
            r#"{"name":"changed-api","main":"src/index.ts"}"#,
        )
        .expect("write package");
        std::fs::write(project.path().join("src/index.ts"), "export const a = 1;\n")
            .expect("write source");
        git(project.path(), &["init"]);
        write_type_aware_config(project.path(), r#"{"typeAware":{"enabled":true}}"#);
        let params = CheckChangedParams {
            root: Some(project.path().display().to_string()),
            since: "HEAD".to_string(),
            no_cache: Some(true),
            ..check_changed_params("")
        };
        assert!(!requires_cli_fallback(&params));
        assert_eq!(run_check_changed_api_value(&params, None), Ok(None));
        assert_cli_fallback_ran(run_check_changed(MISSING_FALLBACK_BINARY, params).await);
    }
}

#[cfg(test)]
mod route_parity_tests {
    use super::*;

    /// Both routes of `check_changed` express the same shared settings.
    #[test]
    fn typed_and_cli_routes_agree_on_shared_settings() {
        super::super::route_parity::assert_routes_agree(
            "check_changed",
            &serde_json::json!({ "since": "main" }),
            |params: CheckChangedParams| build_check_changed_args(params),
            |params: CheckChangedParams| check_changed_options_from_params(&params, None).analysis,
        );
    }
}
