use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use fallow_api::{
    AUDIT_CONFIG_BASELINE_UNSUPPORTED, AnalysisOptions, CoverageInputs, ProgrammaticError,
    TYPE_AWARE_CONFIG_UNSUPPORTED, load_health_config, resolve_coverage_inputs,
};
use rmcp::ErrorData as McpError;
use rmcp::model::{CallToolResult, ContentBlock};
use tokio_util::sync::CancellationToken;

use super::cancellation::{
    ABANDONED_API_CALLS, AbandonedApiCalls, request_cancellation, request_cancelled,
};

/// Run one typed call on the blocking pool under the response deadline and the
/// cancellation of the request.
///
/// `cancellation` must be the token in the call's `AnalysisOptions`. When the
/// deadline passes or the client cancels the request, this sets the token, so
/// the analysis stops at its next `fallow-api` stage boundary, and returns a
/// structured error at once. That stop is cooperative: the call is counted in
/// [`ABANDONED_API_CALLS`] until the worker ends, the same total Code Mode
/// reads before it starts in-process work.
pub(super) async fn run_api_blocking<T, F>(
    tool: &'static str,
    cancellation: Arc<AtomicBool>,
    task: F,
) -> Result<Result<T, ProgrammaticError>, McpError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, ProgrammaticError> + Send + 'static,
{
    let call = ApiCall {
        tool,
        timeout: super::timeout_duration(),
        request: request_cancellation(),
        cancellation,
        abandoned: &ABANDONED_API_CALLS,
    };
    run_api_call(call, task).await
}

/// A fresh cancellation token for one typed call.
pub(super) fn call_cancellation() -> Arc<AtomicBool> {
    Arc::new(AtomicBool::new(false))
}

/// The stop conditions of one typed call.
struct ApiCall {
    tool: &'static str,
    timeout: Duration,
    request: Option<CancellationToken>,
    cancellation: Arc<AtomicBool>,
    abandoned: &'static AbandonedApiCalls,
}

/// Run `task` on a blocking thread until it ends, the deadline passes, or the
/// client cancels the request. A timeout returns `FALLOW_MCP_API_TIMEOUT` to
/// the client. A cancel returns `FALLOW_MCP_CANCELLED`, but rmcp drops the
/// response of a cancelled request, so no client receives that code.
async fn run_api_call<T, F>(
    call: ApiCall,
    task: F,
) -> Result<Result<T, ProgrammaticError>, McpError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, ProgrammaticError> + Send + 'static,
{
    let (state, completion) = call.abandoned.start();
    let mut task = tokio::task::spawn_blocking(move || {
        let _completion = completion;
        task()
    });
    let timed_out = tokio::select! {
        biased;
        joined = &mut task => {
            return joined.map_err(|err| {
                McpError::internal_error(format!("{} task failed: {err}", call.tool), None)
            });
        }
        () = tokio::time::sleep(call.timeout) => true,
        () = request_cancelled(call.request.as_ref()) => false,
    };
    call.cancellation.store(true, Ordering::SeqCst);
    state.abandon();
    let error = if timed_out {
        ProgrammaticError::new(
            format!("{} task timed out after {}s", call.tool, call.timeout.as_secs()),
            2,
        )
        .with_code("FALLOW_MCP_API_TIMEOUT")
        .with_help(
            "Set FALLOW_TIMEOUT_SECS to increase the response deadline. The server told the in-process analysis to stop; it stops at its next stage boundary.",
        )
    } else {
        ProgrammaticError::new(format!("{} task was cancelled by the client", call.tool), 2)
            .with_code("FALLOW_MCP_CANCELLED")
            .with_help(
                "The server told the in-process analysis to stop; it stops at its next stage boundary.",
            )
    };
    Ok(Err(error.with_context(call.tool)))
}

/// `FALLOW_DIFF_FILE` for the typed route. It goes into
/// `AnalysisOptions::ambient_diff_file`, never `diff_file`: the variable comes
/// from the environment the server inherited, so a bad file stands down with a
/// `not-applied` outcome, as it does on the CLI route, instead of failing the
/// call (issue #2799).
pub(super) fn env_diff_file() -> Option<PathBuf> {
    env_path("FALLOW_DIFF_FILE")
}

/// `FALLOW_COVERAGE` / `FALLOW_COVERAGE_ROOT` as the typed route's
/// environment layer, read at the adapter boundary. Empty values count as
/// unset, as they do for the CLI.
fn env_coverage_inputs() -> CoverageInputs {
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
/// CLI uses (#2368): the explicit tool parameters, then the environment layer,
/// then `health.coverage` / `health.coverageRoot` from the project config,
/// which is loaded only when a higher layer leaves an input unset. Engine
/// auto-detection still applies when every layer is empty.
/// `explicit_root_context` names the tool's own parameter when the explicit
/// root is rejected as relative.
///
/// `env` is [`None`] on every production call, which is the single place the
/// typed route reads `FALLOW_COVERAGE` / `FALLOW_COVERAGE_ROOT` from the
/// process environment; the typed-route tests pass [`Some`] so they stay
/// deterministic under a shared environment.
/// `crates/mcp/tests/integration/typed_route_env_coverage.rs` covers the
/// [`None`] path against the real server process.
pub(super) fn resolve_typed_coverage_inputs(
    analysis: &AnalysisOptions,
    explicit: CoverageInputs,
    env: Option<CoverageInputs>,
    explicit_root_context: &str,
) -> Result<CoverageInputs, ProgrammaticError> {
    let env = env.unwrap_or_else(env_coverage_inputs);
    let config_health = if CoverageInputs::needs_config_layer(&explicit, &env) {
        load_health_config(analysis)?
    } else {
        None
    };
    resolve_coverage_inputs(explicit, env, config_health.as_ref())
        .map_err(|err| err.into_programmatic_error(explicit_root_context))
}

/// `FALLOW_CHANGED_SINCE` over an injectable lookup, so the fallback is testable
/// without the ambient value of the variable deciding the assertion.
fn changed_since_from_lookup(lookup: impl Fn(&str) -> Option<OsString>) -> Option<String> {
    lookup("FALLOW_CHANGED_SINCE")
        .and_then(|value| value.into_string().ok())
        .filter(|value| !value.is_empty())
}

/// `FALLOW_CHANGED_SINCE` for the typed route. It goes into
/// `AnalysisOptions::ambient_changed_since`, never `changed_since`: the
/// variable comes from the environment the server inherited, so a ref that
/// does not resolve stands down with a `not-applied` outcome, as a bad
/// `--changed-since` does on the CLI, instead of failing the call. A tool's own
/// `changed_since` or `since` argument goes into `changed_since` and keeps the
/// hard error, and it takes precedence over this value.
pub(super) fn env_changed_since() -> Option<String> {
    changed_since_from_lookup(|name| std::env::var_os(name))
}

/// The settings every typed route reads from its tool params. The CLI route
/// writes the same fields with `push_global` and `push_remote_extends`.
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct SharedParams<'a> {
    pub root: Option<&'a str>,
    pub config: Option<&'a str>,
    pub allow_remote_extends: Option<bool>,
    pub no_cache: Option<bool>,
    pub threads: Option<usize>,
}

/// Read the [`SharedParams`] of a tool params struct that has the usual
/// `root`, `config`, `allow_remote_extends`, `no_cache` and `threads` fields.
macro_rules! shared_params {
    ($params:expr) => {
        $crate::tools::api_runtime::SharedParams {
            root: $params.root.as_deref(),
            config: $params.config.as_deref(),
            allow_remote_extends: $params.allow_remote_extends,
            no_cache: $params.no_cache,
            threads: $params.threads,
        }
    };
}
pub(super) use shared_params;

/// Where a typed route puts the inherited `FALLOW_CHANGED_SINCE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EnvScope {
    /// `ambient_changed_since`: a ref that does not resolve stands down and
    /// the envelope states it in `request_outcomes`.
    Ambient,
    /// `changed_since`: the ref is the base ref of the tool, or the envelope
    /// has no `request_outcomes` to state a stand-down, so a bad ref keeps the
    /// hard error of an explicit ref.
    HardRef,
    /// Neither variable: the tool sets its own scope from its params.
    Own,
}

/// The [`AnalysisOptions`] every typed route shares: the [`SharedParams`],
/// the environment scope (`FALLOW_CHANGED_SINCE`, `FALLOW_DIFF_FILE`), and the
/// cancellation token of the call. A route sets its own fields on top with
/// struct update syntax.
pub(super) fn shared_analysis_options(
    shared: SharedParams<'_>,
    env: EnvScope,
    cancellation: Option<Arc<AtomicBool>>,
) -> AnalysisOptions {
    let (changed_since, ambient_changed_since, ambient_diff_file) = match env {
        EnvScope::Ambient => (None, env_changed_since(), env_diff_file()),
        EnvScope::HardRef => (env_changed_since(), None, env_diff_file()),
        EnvScope::Own => (None, None, None),
    };
    AnalysisOptions {
        root: non_empty_path(shared.root),
        config_path: non_empty_path(shared.config),
        allow_remote_extends: shared.allow_remote_extends == Some(true),
        no_cache: shared.no_cache == Some(true),
        threads: shared.threads,
        changed_since,
        ambient_changed_since,
        ambient_diff_file,
        cancellation,
        ..AnalysisOptions::default()
    }
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

/// Render one typed-route result.
///
/// The verdict members (`gate_outcomes`, `baseline_staleness`,
/// `workspace_diagnostics`) are envelope data, not process state, so they
/// reach this route too and are restated here for the same reason the
/// subprocess route restates them. Annotating at this single exit point is
/// what keeps a tool from answering differently depending on whether a
/// parameter happened to force the CLI fallback; it is a no-op on every
/// envelope that carries none of them.
///
/// The body obeys the same 16 MiB cap as the CLI route and is refused with
/// the same over-limit body, so a parameter that selects the typed route
/// cannot return more than the CLI route would.
pub(super) fn json_success(value: &serde_json::Value) -> CallToolResult {
    json_success_with_limit(value, super::DEFAULT_MAX_OUTPUT_BYTES)
}

fn json_success_with_limit(value: &serde_json::Value, max_output_bytes: usize) -> CallToolResult {
    let text = super::gate_verdicts::annotate_value(value).unwrap_or_else(|| value.to_string());
    if text.len() > max_output_bytes {
        return super::over_limit_result(
            &super::OverLimit {
                stream: "result",
                context: "typed",
                captured: &text,
                total: text.len(),
            },
            max_output_bytes,
        );
    }
    CallToolResult::success(vec![ContentBlock::text(text)])
}

/// The type-aware choice of a typed call. The typed route runs only when the
/// parameter does not request type-aware analysis, so it passes `false` on as
/// an explicit opt-out, and `None` leaves the choice to the config.
pub(super) fn typed_type_aware(param: Option<bool>) -> fallow_api::TypeAwareOptions {
    fallow_api::TypeAwareOptions {
        enabled: param,
        ..fallow_api::TypeAwareOptions::default()
    }
}

/// The typed route refused a run that only the CLI reproduces: the
/// environment or the project config turns on type-aware analysis, or the
/// config sets an `audit` baseline key. The tool then runs the CLI.
pub(super) fn typed_error_needs_cli(error: &ProgrammaticError) -> bool {
    matches!(
        error.code.as_deref(),
        Some(TYPE_AWARE_CONFIG_UNSUPPORTED | AUDIT_CONFIG_BASELINE_UNSUPPORTED)
    )
}

/// Render a finished typed-route result, or `None` when the tool must run the
/// CLI instead (see [`typed_error_needs_cli`]).
pub(super) fn typed_tool_result(
    result: Result<serde_json::Value, ProgrammaticError>,
) -> Option<CallToolResult> {
    match result {
        Ok(value) => Some(json_success(&value)),
        Err(err) if typed_error_needs_cli(&err) => None,
        Err(err) => Some(CallToolResult::error(vec![ContentBlock::text(
            programmatic_error_body(&err),
        )])),
    }
}

/// The `_api_value` form of [`typed_tool_result`]: `Ok(None)` asks the caller
/// to run the CLI.
pub(super) fn typed_api_value(
    result: Result<serde_json::Value, ProgrammaticError>,
) -> Result<Option<serde_json::Value>, String> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(err) if typed_error_needs_cli(&err) => Ok(None),
        Err(err) => Err(programmatic_error_body(&err)),
    }
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

/// A binary that does not exist. A tool test that passes it proves the CLI
/// fallback ran when the result is the spawn error.
#[cfg(test)]
pub(super) const MISSING_FALLBACK_BINARY: &str = "missing-fallow-binary-for-fallback";

/// Assert that the tool ran the CLI: it got as far as the spawn of
/// [`MISSING_FALLBACK_BINARY`].
#[cfg(test)]
#[track_caller]
pub(super) fn assert_cli_fallback_ran(result: Result<CallToolResult, McpError>) {
    let err = result.expect_err("the CLI fallback cannot start");
    assert!(
        err.message.contains(&format!(
            "Failed to execute fallow binary '{MISSING_FALLBACK_BINARY}'"
        )),
        "{err:?}"
    );
}

/// Write a project config that turns on type-aware analysis.
#[cfg(test)]
pub(super) fn write_type_aware_config(root: &std::path::Path, config: &str) {
    std::fs::write(root.join(".fallowrc.json"), config).expect("write type-aware config");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result_text(result: &CallToolResult) -> &str {
        match result.content.first() {
            Some(ContentBlock::Text(text)) => &text.text,
            other => panic!("expected a text block, got {other:?}"),
        }
    }

    #[test]
    fn typed_result_over_the_cap_is_refused_with_the_cli_route_body() {
        let value = serde_json::json!({ "files": ["x".repeat(64)] });
        let total = value.to_string().len();

        let result = json_success_with_limit(&value, 32);

        assert_eq!(result.is_error, Some(true));
        let body: serde_json::Value =
            serde_json::from_str(result_text(&result)).expect("over-limit body is JSON");
        assert_eq!(body["error"], true);
        assert_eq!(body["ok"], false);
        assert_eq!(body["exit_code"], 2);
        assert_eq!(body["truncated"], true);
        assert_eq!(body["result_bytes"], total);
        assert_eq!(body["limit_bytes"], 32);
        assert_eq!(body["code"], "FALLOW_MCP_SUBPROCESS_OUTPUT_LIMIT");
        assert_eq!(body["context"], "typed");
        assert_eq!(body["stream"], "result");
        assert_eq!(
            body["result_preview"].as_str().map(str::len),
            Some(32),
            "the preview never exceeds the cap"
        );
    }

    #[test]
    fn typed_result_within_the_cap_is_returned_unchanged() {
        let value = serde_json::json!({ "ok": true });

        let result = json_success_with_limit(&value, 1024);

        assert_eq!(result.is_error, Some(false));
        assert_eq!(result_text(&result), value.to_string());
    }

    /// Wait until the waiter sets `flag`, then report what the worker saw.
    /// The worker gives up after a bound, so a waiter that never sets the
    /// flag fails the test instead of hanging it.
    fn observe_flag(flag: &AtomicBool) -> bool {
        let give_up_at = std::time::Instant::now() + Duration::from_secs(10);
        while !flag.load(Ordering::SeqCst) && std::time::Instant::now() < give_up_at {
            std::thread::sleep(Duration::from_millis(5));
        }
        flag.load(Ordering::SeqCst)
    }

    async fn wait_until_drained(abandoned: &AbandonedApiCalls) {
        let give_up_at = tokio::time::Instant::now() + Duration::from_secs(10);
        while abandoned.saturated() && tokio::time::Instant::now() < give_up_at {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }

    /// Run `call` with a worker that waits for its own token and reports
    /// whether the token was set.
    async fn run_observed_call(call: ApiCall) -> (Result<bool, ProgrammaticError>, bool) {
        let worker_flag = Arc::clone(&call.cancellation);
        let (seen_tx, seen_rx) = std::sync::mpsc::channel();
        let result = run_api_call(call, move || {
            let seen = observe_flag(&worker_flag);
            let _ = seen_tx.send(seen);
            Ok(seen)
        })
        .await
        .expect("the stop stays a tool result");
        let seen = tokio::task::spawn_blocking(move || seen_rx.recv().expect("the worker reports"))
            .await
            .expect("the receiver ends");
        (result, seen)
    }

    #[tokio::test]
    async fn api_timeout_returns_structured_tool_error_and_stops_the_work() {
        static ABANDONED: AbandonedApiCalls = AbandonedApiCalls::new();
        let call = ApiCall {
            tool: "analyze",
            timeout: Duration::ZERO,
            request: None,
            cancellation: call_cancellation(),
            abandoned: &ABANDONED,
        };

        let (result, seen) = run_observed_call(call).await;

        let err = result.expect_err("timeout should be structured error");
        assert_eq!(err.exit_code, 2);
        assert_eq!(err.code.as_deref(), Some("FALLOW_MCP_API_TIMEOUT"));
        assert_eq!(err.context.as_deref(), Some("analyze"));
        assert!(seen, "the deadline must set the call's cancellation token");
        wait_until_drained(&ABANDONED).await;
        assert!(
            !ABANDONED.saturated(),
            "the stopped worker releases its slot"
        );
    }

    #[tokio::test]
    async fn a_cancelled_request_stops_the_typed_call() {
        static ABANDONED: AbandonedApiCalls = AbandonedApiCalls::new();
        let request = CancellationToken::new();
        let call = ApiCall {
            tool: "check_health",
            timeout: Duration::from_secs(600),
            request: Some(request.clone()),
            cancellation: call_cancellation(),
            abandoned: &ABANDONED,
        };
        let canceller = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            request.cancel();
        });

        let (result, seen) = run_observed_call(call).await;
        canceller.await.expect("the canceller ends");

        let err = result.expect_err("cancellation should be a structured error");
        assert_eq!(err.exit_code, 2);
        assert_eq!(err.code.as_deref(), Some("FALLOW_MCP_CANCELLED"));
        assert_eq!(err.context.as_deref(), Some("check_health"));
        assert!(
            seen,
            "the request cancellation must reach the analysis token"
        );
        wait_until_drained(&ABANDONED).await;
        assert!(!ABANDONED.saturated());
    }

    #[tokio::test]
    async fn a_typed_call_that_finishes_in_time_is_not_cancelled() {
        static ABANDONED: AbandonedApiCalls = AbandonedApiCalls::new();
        let cancellation = call_cancellation();
        let call = ApiCall {
            tool: "project_info",
            timeout: Duration::from_secs(600),
            request: Some(CancellationToken::new()),
            cancellation: Arc::clone(&cancellation),
            abandoned: &ABANDONED,
        };

        let result = run_api_call(call, || Ok::<_, ProgrammaticError>(7))
            .await
            .expect("the call ends");

        assert_eq!(result.expect("the call succeeds"), 7);
        assert!(!cancellation.load(Ordering::SeqCst));
        assert!(!ABANDONED.saturated());
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

    /// #2368: the adapter maps both coverage variables and ignores empty
    /// values, as the CLI does. The lookup is injected, so no typed-route
    /// test in this binary can observe a mutated process environment; that
    /// the production reader passes the real environment through this mapping
    /// is covered end to end by
    /// `crates/mcp/tests/integration/typed_route_env_coverage.rs`.
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
    }

    #[test]
    fn env_changed_since_reads_a_non_empty_value_only() {
        let env =
            |name: &str| (name == "FALLOW_CHANGED_SINCE").then(|| OsString::from("origin/release"));
        assert_eq!(
            changed_since_from_lookup(env),
            Some("origin/release".to_string())
        );
        assert_eq!(
            changed_since_from_lookup(|_| Some(OsString::from(""))),
            None
        );
        assert_eq!(changed_since_from_lookup(|_| None), None);
    }
}
