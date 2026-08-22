use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;

use fallow_api::{
    AnalysisOptions, CoverageInputs, ProgrammaticError, load_health_config, resolve_coverage_inputs,
};
use rmcp::ErrorData as McpError;
use rmcp::model::{CallToolResult, ContentBlock};
use serde::Serialize;

pub(super) async fn run_api_blocking<T, F>(
    tool: &'static str,
    task: F,
) -> Result<Result<T, ProgrammaticError>, McpError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, ProgrammaticError> + Send + 'static,
{
    let timeout = super::timeout_duration();
    run_api_blocking_with_timeout(tool, timeout, task).await
}

async fn run_api_blocking_with_timeout<T, F>(
    tool: &'static str,
    timeout: Duration,
    task: F,
) -> Result<Result<T, ProgrammaticError>, McpError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, ProgrammaticError> + Send + 'static,
{
    let task = tokio::task::spawn_blocking(task);
    match tokio::time::timeout(timeout, task).await {
        Ok(Ok(result)) => Ok(result),
        Ok(Err(err)) => Err(McpError::internal_error(
            format!("{tool} task failed: {err}"),
            None,
        )),
        Err(_) => Ok(Err(ProgrammaticError::new(
            format!("{tool} task timed out after {}s", timeout.as_secs()),
            2,
        )
        .with_code("FALLOW_MCP_API_TIMEOUT")
        .with_help(
            "Set FALLOW_TIMEOUT_SECS to increase the response deadline. API-backed analysis may finish in-process after the MCP timeout response.",
        )
        .with_context(tool))),
    }
}

pub(super) fn env_diff_file() -> Option<PathBuf> {
    env_path("FALLOW_DIFF_FILE")
}

/// `FALLOW_COVERAGE` / `FALLOW_COVERAGE_ROOT` as the typed route's
/// environment layer, read at the adapter boundary. Empty values count as
/// unset, as they do for the CLI.
pub(super) fn env_coverage_inputs() -> CoverageInputs {
    coverage_inputs_from(|name: &str| std::env::var_os(name))
}

/// [`env_coverage_inputs`] over an injectable lookup, so the reader itself is
/// testable without mutating the process environment for the rest of the
/// suite.
fn coverage_inputs_from(lookup: impl Fn(&str) -> Option<OsString>) -> CoverageInputs {
    let path = |name: &str| {
        lookup(name)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    };
    CoverageInputs {
        coverage: path("FALLOW_COVERAGE"),
        coverage_root: path("FALLOW_COVERAGE_ROOT"),
    }
}

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// Resolve the typed route's Istanbul coverage inputs with the precedence the
/// CLI uses (#2368): the explicit tool parameters, then `env`, then
/// `health.coverage` / `health.coverageRoot` from the project config, which is
/// loaded only when a higher layer leaves an input unset. Engine
/// auto-detection still applies when every layer is empty.
/// `explicit_root_context` names the tool's own parameter when the explicit
/// root is rejected as relative.
pub(super) fn resolve_typed_coverage_inputs(
    analysis: &AnalysisOptions,
    explicit: CoverageInputs,
    env: CoverageInputs,
    explicit_root_context: &str,
) -> Result<CoverageInputs, ProgrammaticError> {
    let config_health = if CoverageInputs::needs_config_layer(&explicit, &env) {
        load_health_config(analysis)?
    } else {
        None
    };
    resolve_coverage_inputs(explicit, env, config_health.as_ref())
        .map_err(|err| err.into_programmatic_error(explicit_root_context))
}

fn env_changed_since() -> Option<String> {
    std::env::var("FALLOW_CHANGED_SINCE")
        .ok()
        .filter(|value| !value.is_empty())
}

pub(super) fn changed_since_from_param(value: Option<&str>) -> Option<String> {
    non_empty_string(value).or_else(env_changed_since)
}

pub(super) fn non_empty_path(value: Option<&str>) -> Option<PathBuf> {
    value.and_then(|value| (!value.is_empty()).then(|| PathBuf::from(value)))
}

pub(super) fn non_empty_string(value: Option<&str>) -> Option<String> {
    value.and_then(|value| (!value.is_empty()).then(|| value.to_string()))
}

/// Split a workspace parameter into individual patterns. The schemas document
/// comma-list syntax with CLI parity, and clap splits `--workspace` on commas,
/// so the API-backed paths must split the same way.
pub(super) fn workspace_patterns_from_param(value: Option<&str>) -> Option<Vec<String>> {
    let patterns: Vec<String> = value?
        .split(',')
        .map(str::trim)
        .filter(|pattern| !pattern.is_empty())
        .map(str::to_string)
        .collect();
    (!patterns.is_empty()).then_some(patterns)
}

pub(super) fn json_success(value: &impl Serialize) -> CallToolResult {
    let text = serde_json::to_string(value).unwrap_or_else(|_| "{}".to_string());
    CallToolResult::success(vec![ContentBlock::text(text)])
}

pub(super) fn programmatic_error_body(error: &ProgrammaticError) -> String {
    serde_json::json!({
        "error": true,
        "message": error.message,
        "exit_code": error.exit_code,
        "code": error.code,
        "help": error.help,
        "context": error.context,
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn api_timeout_returns_structured_tool_error() {
        let result = run_api_blocking_with_timeout("analyze", Duration::ZERO, || {
            std::thread::sleep(Duration::from_millis(25));
            Ok::<_, ProgrammaticError>(serde_json::json!({ "ok": true }))
        })
        .await
        .expect("timeout should stay a tool result");

        let err = result.expect_err("timeout should be structured error");
        assert_eq!(err.exit_code, 2);
        assert_eq!(err.code.as_deref(), Some("FALLOW_MCP_API_TIMEOUT"));
        assert_eq!(err.context.as_deref(), Some("analyze"));
    }

    #[test]
    fn workspace_patterns_split_on_commas_like_the_cli() {
        assert_eq!(
            workspace_patterns_from_param(Some("web,admin")),
            Some(vec!["web".to_string(), "admin".to_string()])
        );
        assert_eq!(
            workspace_patterns_from_param(Some("apps/*, !apps/legacy")),
            Some(vec!["apps/*".to_string(), "!apps/legacy".to_string()])
        );
        assert_eq!(
            workspace_patterns_from_param(Some("web")),
            Some(vec!["web".to_string()])
        );
        assert_eq!(workspace_patterns_from_param(Some("")), None);
        assert_eq!(workspace_patterns_from_param(Some(",, ,")), None);
        assert_eq!(workspace_patterns_from_param(None), None);
    }

    /// #2368: the adapter reads both coverage variables and ignores empty
    /// values, as the CLI does. The lookup is injected, so no typed-route
    /// test in this binary can observe a mutated process environment.
    #[test]
    fn env_coverage_inputs_read_both_variables_and_ignore_empty_values() {
        let inputs = coverage_inputs_from(|name| match name {
            "FALLOW_COVERAGE" => Some(OsString::from("")),
            "FALLOW_COVERAGE_ROOT" => Some(OsString::from("/ci/workspace")),
            _ => None,
        });
        assert_eq!(
            inputs,
            CoverageInputs {
                coverage: None,
                coverage_root: Some(PathBuf::from("/ci/workspace")),
            }
        );

        let inputs = coverage_inputs_from(|name| {
            (name == "FALLOW_COVERAGE").then(|| OsString::from("artifacts/coverage-final.json"))
        });
        assert_eq!(
            inputs,
            CoverageInputs {
                coverage: Some(PathBuf::from("artifacts/coverage-final.json")),
                coverage_root: None,
            }
        );

        assert_eq!(
            env_coverage_inputs(),
            coverage_inputs_from(|name: &str| std::env::var_os(name)),
            "the adapter's env layer reads the process environment"
        );
    }

    #[test]
    fn changed_since_from_param_prefers_param_over_empty_env_fallback() {
        assert_eq!(
            changed_since_from_param(Some("origin/main")),
            Some("origin/main".to_string())
        );
        assert_eq!(changed_since_from_param(Some("")), env_changed_since());
    }
}
