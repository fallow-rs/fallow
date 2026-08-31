mod analyze;
mod api_runtime;
mod audit;
mod check_changed;
mod check_runtime_coverage;
mod code_mode;
#[cfg(test)]
mod coverage_fixture;
mod decision_surface;
mod dupes;
mod explain;
mod fallback_policy;
mod fix;
mod flags;
mod guard;
mod health;
mod impact;
mod inspect_target;
mod list_boundaries;
mod project_info;
mod recommend;
mod security;
mod semantic;
mod similar_code;
mod suppressions;
mod trace;

pub use analyze::{build_analyze_args, run_analyze};
pub use audit::{build_audit_args, run_audit};
pub use check_changed::{build_check_changed_args, run_check_changed};
#[cfg(test)]
pub use check_runtime_coverage::build_get_token_blast_radius_args;
pub use check_runtime_coverage::{
    build_check_runtime_coverage_args, build_get_blast_radius_args,
    build_get_cleanup_candidates_args, build_get_hot_paths_args, build_get_importance_args,
    run_check_runtime_coverage, run_get_blast_radius, run_get_cleanup_candidates,
    run_get_hot_paths, run_get_importance, run_get_token_blast_radius,
};
#[cfg(test)]
pub use code_mode::code_mode_subprocess_aliases;
pub use code_mode::execute_code_mode;
pub use decision_surface::run_decision_surface;
pub use dupes::{build_find_dupes_args, run_find_dupes};
pub use explain::{build_explain_args, run_explain};
#[cfg(test)]
pub use fix::{build_fix_apply_args, build_fix_preview_args};
pub use fix::{run_fix_apply, run_fix_preview};
pub use flags::{build_feature_flags_args, run_feature_flags};
#[cfg(test)]
pub use guard::build_guard_args;
pub use guard::run_guard;
pub use health::{build_health_args, run_health};
#[cfg(test)]
pub use impact::build_impact_all_args;
pub use impact::{
    build_impact_args, build_impact_closure_args, run_impact, run_impact_all, run_impact_closure,
};
pub use inspect_target::inspect_target;
pub use list_boundaries::{build_list_boundaries_args, run_list_boundaries};
pub use project_info::{build_project_info_args, run_project_info};
pub use recommend::run_recommend;
pub use security::{build_security_candidates_args, run_security_candidates};
pub use semantic::{run_symbol_impact, run_symbol_trace};
#[cfg(test)]
pub use similar_code::{build_find_similar_code_args, build_inspect_similar_code_args};
pub use similar_code::{run_find_similar_code, run_inspect_similar_code};
#[cfg(test)]
pub use suppressions::build_list_suppressions_args;
pub use suppressions::run_list_suppressions;
pub use trace::{
    build_trace_clone_args, build_trace_dependency_args, build_trace_export_args,
    build_trace_file_args, run_trace_clone_tool, run_trace_dependency_tool, run_trace_export_tool,
    run_trace_file_tool,
};

use std::io;
use std::process::{ExitStatus, Stdio};
use std::time::Duration;

pub use fallow_types::issue_meta::MCP_ISSUE_TYPE_FLAGS as ISSUE_TYPE_FLAGS;
use rmcp::ErrorData as McpError;
use rmcp::model::{CallToolResult, ContentBlock};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;

use fallow_process::{ProcessTree, cleanup_tokio_child, configure_tokio_command};

/// Default subprocess timeout in seconds.
const DEFAULT_TIMEOUT_SECS: u64 = 120;
const DEFAULT_MAX_OUTPUT_BYTES: usize = 16 * 1024 * 1024;
const PIPE_READ_CHUNK_BYTES: usize = 8 * 1024;

/// Push a `--flag VALUE` pair onto `args` only when `value` is `Some(s)` and
/// `s` is non-empty.
fn push_str_flag(args: &mut Vec<String>, flag: &str, value: Option<&str>) {
    if let Some(s) = value
        && !s.is_empty()
    {
        args.extend([flag.to_string(), s.to_string()]);
    }
}

fn push_type_aware(
    args: &mut Vec<String>,
    enabled: Option<bool>,
    projects: Option<&[String]>,
    require: Option<crate::params::TypeAwareRequireParam>,
) {
    if enabled == Some(true) || projects.is_some_and(|items| !items.is_empty()) || require.is_some()
    {
        args.push("--type-aware".to_string());
    }
    if let Some(projects) = projects {
        for project in projects.iter().filter(|project| !project.is_empty()) {
            args.extend(["--type-aware-project".to_string(), project.clone()]);
        }
    }
    if let Some(require) = require {
        args.extend([
            "--type-aware-require".to_string(),
            require.as_cli().to_string(),
        ]);
    }
}

/// Forward the per-request remote config trust opt-in to CLI-backed tools.
fn push_remote_extends(args: &mut Vec<String>, allow: Option<bool>) {
    if allow == Some(true) {
        args.push("--allow-remote-extends".to_string());
    }
}

/// Push root directory and config file flags (shared by all tools).
fn push_global(
    args: &mut Vec<String>,
    root: Option<&str>,
    config: Option<&str>,
    no_cache: Option<bool>,
    threads: Option<usize>,
) {
    push_str_flag(args, "--root", root);
    push_str_flag(args, "--config", config);
    if no_cache == Some(true) {
        args.push("--no-cache".to_string());
    }
    if let Some(threads) = threads {
        args.extend(["--threads".to_string(), threads.to_string()]);
    }
}

/// Push production mode and workspace scope flags.
fn push_scope(args: &mut Vec<String>, production: Option<bool>, workspace: Option<&str>) {
    if production == Some(true) {
        args.push("--production".to_string());
    }
    push_str_flag(args, "--workspace", workspace);
}

/// Push baseline comparison flags.
fn push_baseline(args: &mut Vec<String>, baseline: Option<&str>, save_baseline: Option<&str>) {
    push_str_flag(args, "--baseline", baseline);
    push_str_flag(args, "--save-baseline", save_baseline);
}

/// Push regression comparison flags.
fn push_regression(
    args: &mut Vec<String>,
    fail: Option<bool>,
    tolerance: Option<&str>,
    baseline: Option<&str>,
    save: Option<&str>,
) {
    if fail == Some(true) {
        args.push("--fail-on-regression".to_string());
    }
    push_str_flag(args, "--tolerance", tolerance);
    push_str_flag(args, "--regression-baseline", baseline);
    push_str_flag(args, "--save-regression-baseline", save);
}

/// Valid detection modes for the `find_dupes` tool.
pub const VALID_DUPES_MODES: &[&str] = &["strict", "mild", "weak", "semantic"];

/// Valid gate values for the `audit` tool.
pub const VALID_AUDIT_GATES: &[&str] = &["new-only", "all"];

/// Build a structured validation error body matching the shape `run_fallow` emits
/// for CLI-level errors.
pub fn validation_error_body(message: impl Into<String>) -> String {
    serde_json::json!({
        "error": true,
        "message": message.into(),
        "exit_code": 2,
    })
    .to_string()
}

fn timeout_duration_from(value: Option<&str>, default_secs: u64) -> Duration {
    value
        .and_then(|value| value.parse::<u64>().ok())
        .map_or_else(|| Duration::from_secs(default_secs), Duration::from_secs)
}

/// Read the subprocess timeout from `FALLOW_TIMEOUT_SECS` or use the supplied
/// tool-specific default.
fn timeout_duration_with_default(default_secs: u64) -> Duration {
    let configured = std::env::var("FALLOW_TIMEOUT_SECS").ok();
    timeout_duration_from(configured.as_deref(), default_secs)
}

fn timeout_duration() -> Duration {
    timeout_duration_with_default(DEFAULT_TIMEOUT_SECS)
}

/// Execute the fallow CLI binary with the given arguments and return the result.
///
/// Untagged variant retained for the subprocess-behavior tests (timeouts, exit
/// codes, signal handling); production tool dispatch goes through `run_tool` so
/// the spawned CLI's telemetry is attributed to the `mcp` surface.
#[cfg(test)]
pub async fn run_fallow(binary: &str, args: &[String]) -> Result<CallToolResult, McpError> {
    spawn_fallow(
        binary,
        args,
        None,
        timeout_duration(),
        DEFAULT_MAX_OUTPUT_BYTES,
        None,
    )
    .await
}

/// Execute the fallow CLI for a named MCP tool. Tags the spawned process so its
/// telemetry event is attributed to the `mcp` integration surface and the
/// specific tool, instead of looking like any other `cli_json` run. The CLI
/// only reads these when telemetry is enabled; they carry no paths or
/// identifiers, and the tool name is allowlist-validated CLI-side.
pub async fn run_tool(
    binary: &str,
    tool: &'static str,
    args: &[String],
) -> Result<CallToolResult, McpError> {
    run_tool_with_timeout(binary, tool, args, timeout_duration()).await
}

/// Execute one named MCP tool with a tool-specific bounded timeout.
pub async fn run_tool_with_timeout(
    binary: &str,
    tool: &'static str,
    args: &[String],
    timeout: Duration,
) -> Result<CallToolResult, McpError> {
    spawn_fallow(
        binary,
        args,
        None,
        timeout,
        DEFAULT_MAX_OUTPUT_BYTES,
        Some(tool),
    )
    .await
}

/// Execute one named MCP tool with bounded stdin and a tool-specific timeout.
pub async fn run_tool_with_stdin_timeout(
    binary: &str,
    tool: &'static str,
    args: &[String],
    stdin: Vec<u8>,
    timeout: Duration,
) -> Result<CallToolResult, McpError> {
    spawn_fallow(
        binary,
        args,
        Some(stdin),
        timeout,
        DEFAULT_MAX_OUTPUT_BYTES,
        Some(tool),
    )
    .await
}

#[cfg(test)]
pub async fn run_fallow_with_timeout(
    binary: &str,
    args: &[String],
    timeout: Duration,
) -> Result<CallToolResult, McpError> {
    spawn_fallow(binary, args, None, timeout, DEFAULT_MAX_OUTPUT_BYTES, None).await
}

#[cfg(all(test, unix))]
pub async fn run_fallow_with_output_limit(
    binary: &str,
    args: &[String],
    max_output_bytes: usize,
) -> Result<CallToolResult, McpError> {
    spawn_fallow(
        binary,
        args,
        None,
        timeout_duration(),
        max_output_bytes,
        None,
    )
    .await
}

async fn spawn_fallow(
    binary: &str,
    args: &[String],
    stdin_input: Option<Vec<u8>>,
    timeout: Duration,
    max_output_bytes: usize,
    tool: Option<&'static str>,
) -> Result<CallToolResult, McpError> {
    let type_aware_complete_required = type_aware_complete_required(args);
    let mut command = Command::new(binary);
    command
        .args(args)
        .stdin(if stdin_input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(tool) = tool {
        // Re-tag the spawned CLI's telemetry event as the MCP surface + tool.
        // The CLI inherits this process's env, so the existing telemetry path
        // emits a single, correctly-attributed event (no second emit here).
        command
            .env("FALLOW_INTEGRATION_SURFACE", "mcp")
            .env("FALLOW_MCP_TOOL", tool);
    }
    configure_tokio_command(&mut command);

    let mut child = command
        .spawn()
        .map_err(|error| subprocess_error(binary, error))?;
    let process_tree = match ProcessTree::for_tokio_child(&child) {
        Ok(process_tree) => process_tree,
        Err(error) => {
            let cleanup = cleanup_tokio_child(None, &mut child).await;
            return Err(subprocess_error_with_cleanup(
                binary,
                error,
                &cleanup.errors,
            ));
        }
    };
    let Some(stdout) = child.stdout.take() else {
        let cleanup = cleanup_tokio_child(Some(&process_tree), &mut child).await;
        return Err(subprocess_error_with_cleanup(
            binary,
            io::Error::other("stdout pipe unavailable"),
            &cleanup.errors,
        ));
    };
    let Some(stderr) = child.stderr.take() else {
        let cleanup = cleanup_tokio_child(Some(&process_tree), &mut child).await;
        return Err(subprocess_error_with_cleanup(
            binary,
            io::Error::other("stderr pipe unavailable"),
            &cleanup.errors,
        ));
    };
    let stdin_task = if let Some(input) = stdin_input {
        let Some(mut stdin) = child.stdin.take() else {
            let cleanup = cleanup_tokio_child(Some(&process_tree), &mut child).await;
            return Err(subprocess_error_with_cleanup(
                binary,
                io::Error::other("stdin pipe unavailable"),
                &cleanup.errors,
            ));
        };
        Some(tokio::spawn(async move {
            stdin.write_all(&input).await?;
            stdin.shutdown().await
        }))
    } else {
        None
    };
    let stdout_task = tokio::spawn(drain_pipe(stdout, max_output_bytes));
    let stderr_task = tokio::spawn(drain_pipe(stderr, max_output_bytes));
    let process = RunningFallowProcess {
        child,
        process_tree,
        stdin_task,
        stdout_task,
        stderr_task,
        exit_one_is_error: type_aware_complete_required,
    };
    complete_fallow_process(binary, process, timeout, max_output_bytes).await
}

struct RunningFallowProcess {
    child: tokio::process::Child,
    process_tree: ProcessTree,
    stdin_task: Option<tokio::task::JoinHandle<io::Result<()>>>,
    stdout_task: tokio::task::JoinHandle<io::Result<CapturedPipe>>,
    stderr_task: tokio::task::JoinHandle<io::Result<CapturedPipe>>,
    exit_one_is_error: bool,
}

async fn complete_fallow_process(
    binary: &str,
    process: RunningFallowProcess,
    timeout: Duration,
    max_output_bytes: usize,
) -> Result<CallToolResult, McpError> {
    let RunningFallowProcess {
        mut child,
        process_tree,
        mut stdin_task,
        mut stdout_task,
        mut stderr_task,
        exit_one_is_error,
    } = process;
    let deadline = tokio::time::Instant::now() + timeout;

    #[cfg(unix)]
    let (status, cleanup_errors) =
        match tokio::time::timeout_at(deadline, process_tree.wait_for_exit_without_reaping()).await
        {
            Ok(Ok(())) => {
                let cleanup = cleanup_tokio_child(Some(&process_tree), &mut child).await;
                let Some(status) = cleanup.status else {
                    abort_process_tasks(stdin_task.as_ref(), &stdout_task, &stderr_task);
                    return Err(subprocess_error_with_cleanup(
                        binary,
                        io::Error::other("completed subprocess status unavailable"),
                        &cleanup.errors,
                    ));
                };
                (status, cleanup.errors)
            }
            Ok(Err(error)) => {
                let cleanup = cleanup_tokio_child(Some(&process_tree), &mut child).await;
                abort_process_tasks(stdin_task.as_ref(), &stdout_task, &stderr_task);
                return Err(subprocess_error_with_cleanup(
                    binary,
                    error,
                    &cleanup.errors,
                ));
            }
            Err(_) => {
                let cleanup = cleanup_tokio_child(Some(&process_tree), &mut child).await;
                abort_process_tasks(stdin_task.as_ref(), &stdout_task, &stderr_task);
                return Ok(timeout_result(timeout, &cleanup.errors));
            }
        };

    #[cfg(not(unix))]
    let (status, cleanup_errors) = match tokio::time::timeout_at(deadline, child.wait()).await {
        Ok(Ok(status)) => {
            let cleanup = cleanup_tokio_child(Some(&process_tree), &mut child).await;
            (status, cleanup.errors)
        }
        Ok(Err(error)) => {
            let cleanup = cleanup_tokio_child(Some(&process_tree), &mut child).await;
            abort_process_tasks(stdin_task.as_ref(), &stdout_task, &stderr_task);
            return Err(subprocess_error_with_cleanup(
                binary,
                error,
                &cleanup.errors,
            ));
        }
        Err(_) => {
            let cleanup = cleanup_tokio_child(Some(&process_tree), &mut child).await;
            abort_process_tasks(stdin_task.as_ref(), &stdout_task, &stderr_task);
            return Ok(timeout_result(timeout, &cleanup.errors));
        }
    };

    let drain_output = async {
        if let Some(stdin_task) = stdin_task.as_mut() {
            stdin_task.await.map_err(io::Error::other)??;
        }
        let stdout = (&mut stdout_task).await.map_err(io::Error::other)??;
        let stderr = (&mut stderr_task).await.map_err(io::Error::other)??;
        Ok::<_, io::Error>((stdout, stderr))
    };
    let (stdout, stderr) = match tokio::time::timeout_at(deadline, drain_output).await {
        Ok(Ok(output)) => output,
        Ok(Err(error)) => {
            abort_process_tasks(stdin_task.as_ref(), &stdout_task, &stderr_task);
            return Err(subprocess_error_with_cleanup(
                binary,
                error,
                &cleanup_errors,
            ));
        }
        Err(_) => {
            abort_process_tasks(stdin_task.as_ref(), &stdout_task, &stderr_task);
            return Ok(timeout_result(timeout, &cleanup_errors));
        }
    };
    if !cleanup_errors.is_empty() {
        return Ok(cleanup_failure_result(&cleanup_errors));
    }

    let output = CapturedOutput {
        status,
        stdout,
        stderr,
    };
    Ok(captured_output_result(
        &output,
        max_output_bytes,
        exit_one_is_error,
    ))
}

fn cleanup_failure_result(cleanup_errors: &[String]) -> CallToolResult {
    let error_json = serde_json::json!({
        "error": true,
        "message": "failed to clean completed fallow subprocess tree",
        "exit_code": 2,
        "code": "FALLOW_MCP_SUBPROCESS_CLEANUP",
        "context": "subprocess",
        "cleanup_errors": cleanup_errors,
    });
    CallToolResult::error(vec![ContentBlock::text(error_json.to_string())])
}

fn captured_output_result(
    output: &CapturedOutput,
    max_output_bytes: usize,
    exit_one_is_error: bool,
) -> CallToolResult {
    if output.stdout.exceeded || output.stderr.exceeded {
        return output_limit_result(max_output_bytes);
    }

    let stdout = String::from_utf8_lossy(&output.stdout.bytes);
    let stderr = String::from_utf8_lossy(&output.stderr.bytes);

    if !output.status.success() {
        return non_success_result(
            output.status.code().unwrap_or(-1),
            &stdout,
            &stderr,
            exit_one_is_error,
        );
    }

    if stdout.is_empty() {
        return CallToolResult::success(vec![ContentBlock::text("{}".to_string())]);
    }

    CallToolResult::success(vec![ContentBlock::text(stdout.to_string())])
}

struct CapturedOutput {
    status: ExitStatus,
    stdout: CapturedPipe,
    stderr: CapturedPipe,
}

struct CapturedPipe {
    bytes: Vec<u8>,
    exceeded: bool,
}

async fn drain_pipe(
    mut pipe: impl AsyncRead + Unpin,
    max_output_bytes: usize,
) -> io::Result<CapturedPipe> {
    let mut bytes = Vec::with_capacity(max_output_bytes.min(PIPE_READ_CHUNK_BYTES));
    let mut buffer = [0; PIPE_READ_CHUNK_BYTES];
    let mut exceeded = false;

    loop {
        let read = pipe.read(&mut buffer).await?;
        if read == 0 {
            break;
        }

        let retained = read.min(max_output_bytes.saturating_sub(bytes.len()));
        bytes.extend_from_slice(&buffer[..retained]);
        exceeded |= retained < read;
    }

    Ok(CapturedPipe { bytes, exceeded })
}

fn abort_process_tasks(
    stdin_task: Option<&tokio::task::JoinHandle<io::Result<()>>>,
    stdout_task: &tokio::task::JoinHandle<io::Result<CapturedPipe>>,
    stderr_task: &tokio::task::JoinHandle<io::Result<CapturedPipe>>,
) {
    if let Some(stdin_task) = stdin_task {
        stdin_task.abort();
    }
    stdout_task.abort();
    stderr_task.abort();
}

fn subprocess_error(binary: &str, error: impl std::fmt::Display) -> McpError {
    McpError::internal_error(
        format!(
            "Failed to execute fallow binary '{binary}': {error}. \
             Ensure fallow is installed and available in PATH, \
             or set the FALLOW_BIN environment variable."
        ),
        None,
    )
}

fn subprocess_error_with_cleanup(
    binary: &str,
    error: impl std::fmt::Display,
    cleanup_errors: &[String],
) -> McpError {
    if cleanup_errors.is_empty() {
        return subprocess_error(binary, error);
    }
    subprocess_error(
        binary,
        format!("{error}; cleanup errors: {}", cleanup_errors.join("; ")),
    )
}

/// Translate a non-zero CLI exit into the MCP result envelope. Exit 1 normally
/// carries issue JSON as a success, but an explicitly incomplete type-aware
/// result is an error when the caller required complete evidence.
fn non_success_result(
    exit_code: i32,
    stdout: &str,
    stderr: &str,
    type_aware_complete_required: bool,
) -> CallToolResult {
    if exit_code == 1 {
        let complete_required =
            type_aware_complete_required || type_aware_output_requires_complete(stdout);
        if complete_required && type_aware_evidence_is_incomplete(stdout) {
            let text = if stdout.is_empty() {
                serde_json::json!({
                    "error": true,
                    "message": "type-aware completeness was required, but the semantic query was incomplete",
                    "exit_code": 1,
                    "code": "FALLOW_TYPE_AWARE_INCOMPLETE",
                    "context": "analysis.typeAware",
                })
                .to_string()
            } else {
                stdout.to_string()
            };
            return CallToolResult::error(vec![ContentBlock::text(text)]);
        }
        let text = if stdout.is_empty() {
            "{}".to_string()
        } else {
            stdout.to_string()
        };
        return CallToolResult::success(vec![ContentBlock::text(text)]);
    }

    if !stdout.is_empty() && serde_json::from_str::<serde_json::Value>(stdout).is_ok() {
        return CallToolResult::error(vec![ContentBlock::text(stdout.to_string())]);
    }

    let message = if stderr.is_empty() {
        format!("fallow exited with code {exit_code}")
    } else {
        stderr.trim().to_string()
    };

    let error_json = serde_json::json!({
        "error": true,
        "message": message,
        "exit_code": exit_code,
    });

    CallToolResult::error(vec![ContentBlock::text(error_json.to_string())])
}

fn type_aware_complete_required(args: &[String]) -> bool {
    args.windows(2)
        .any(|pair| pair == ["--type-aware-require", "complete"])
}

fn type_aware_output_requires_complete(stdout: &str) -> bool {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(stdout) else {
        return false;
    };
    contains_complete_requirement(&value)
}

fn contains_complete_requirement(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Object(map) => {
            map.get("required_completeness")
                .and_then(serde_json::Value::as_str)
                == Some("complete")
                || map.values().any(contains_complete_requirement)
        }
        serde_json::Value::Array(values) => values.iter().any(contains_complete_requirement),
        _ => false,
    }
}

fn type_aware_evidence_is_incomplete(stdout: &str) -> bool {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(stdout) else {
        return true;
    };
    semantic_evidence_completeness(&value).is_none_or(|complete| !complete)
}

fn semantic_evidence_completeness(value: &serde_json::Value) -> Option<bool> {
    let mut signals = Vec::new();
    collect_semantic_completeness(value, &mut signals);
    (!signals.is_empty()).then(|| signals.into_iter().all(std::convert::identity))
}

fn collect_semantic_completeness(value: &serde_json::Value, signals: &mut Vec<bool>) {
    match value {
        serde_json::Value::Object(map) => {
            if let Some(completeness) = map
                .get("identity")
                .and_then(serde_json::Value::as_object)
                .and_then(|identity| identity.get("completeness"))
                .and_then(serde_json::Value::as_str)
            {
                signals.push(completeness == "complete");
            }
            if map.contains_key("identity")
                && let Some(status) = map.get("status").and_then(serde_json::Value::as_str)
            {
                signals.push(status == "complete");
            }
            if map.contains_key("protocol_version")
                && let Some(queries) = map.get("queries").and_then(serde_json::Value::as_array)
            {
                signals.extend(queries.iter().filter_map(|query| {
                    query
                        .get("status")
                        .and_then(serde_json::Value::as_str)
                        .map(|status| status == "complete")
                }));
            }
            for section_name in ["semantic_trace", "api_surface", "symbol_impact"] {
                let Some(section) = map.get(section_name).and_then(serde_json::Value::as_object)
                else {
                    continue;
                };
                if let Some(status) = section.get("status").and_then(serde_json::Value::as_str) {
                    signals.push(status == "ok");
                }
            }
            for child in map.values() {
                collect_semantic_completeness(child, signals);
            }
        }
        serde_json::Value::Array(values) => {
            for child in values {
                collect_semantic_completeness(child, signals);
            }
        }
        _ => {}
    }
}

fn timeout_result(timeout: Duration, cleanup_errors: &[String]) -> CallToolResult {
    let mut error_json = serde_json::json!({
        "error": true,
        "message": format!("fallow subprocess timed out after {}s", timeout.as_secs()),
        "exit_code": 2,
        "code": "FALLOW_MCP_SUBPROCESS_TIMEOUT",
        "help": "Set FALLOW_TIMEOUT_SECS to increase the limit.",
        "context": "subprocess",
    });
    if !cleanup_errors.is_empty()
        && let Some(error) = error_json.as_object_mut()
    {
        error.insert(
            "cleanup_errors".to_string(),
            serde_json::json!(cleanup_errors),
        );
    }
    CallToolResult::error(vec![ContentBlock::text(error_json.to_string())])
}

fn output_limit_result(max_output_bytes: usize) -> CallToolResult {
    let error_json = serde_json::json!({
        "error": true,
        "message": format!(
            "fallow subprocess output exceeded {max_output_bytes} bytes per stream"
        ),
        "exit_code": 2,
        "code": "FALLOW_MCP_SUBPROCESS_OUTPUT_LIMIT",
        "help": "Narrow the requested analysis or reduce subprocess output.",
        "context": "subprocess",
        "limit_bytes": max_output_bytes,
    });
    CallToolResult::error(vec![ContentBlock::text(error_json.to_string())])
}

/// Execute fallow and ensure successful JSON responses have a top-level
/// `warnings` array for agent-facing runtime context tools. Untagged variant
/// retained for tests; production goes through `run_tool_with_top_level_warnings`.
#[cfg(all(test, unix))]
pub async fn run_fallow_with_top_level_warnings(
    binary: &str,
    args: &[String],
) -> Result<CallToolResult, McpError> {
    Ok(ensure_top_level_warnings(run_fallow(binary, args).await?))
}

/// Tool-attributed variant of `run_fallow_with_top_level_warnings` (see
/// `run_tool`).
pub async fn run_tool_with_top_level_warnings(
    binary: &str,
    tool: &'static str,
    args: &[String],
) -> Result<CallToolResult, McpError> {
    Ok(ensure_top_level_warnings(
        run_tool(binary, tool, args).await?,
    ))
}

fn ensure_top_level_warnings(result: CallToolResult) -> CallToolResult {
    if result.is_error == Some(true) {
        return result;
    }

    let Some(content) = result.content.first() else {
        return result;
    };
    let ContentBlock::Text(text) = content else {
        return result;
    };
    let Ok(mut value) = serde_json::from_str::<serde_json::Value>(&text.text) else {
        return result;
    };
    let Some(map) = value.as_object_mut() else {
        return result;
    };

    map.entry("warnings".to_string())
        .or_insert_with(|| serde_json::Value::Array(Vec::new()));

    let text = serde_json::to_string(&value).unwrap_or_else(|_| text.text.clone());
    CallToolResult::success(vec![ContentBlock::text(text)])
}

#[cfg(test)]
mod completeness_tests {
    use super::*;

    #[test]
    fn configured_complete_requirement_turns_incomplete_exit_one_into_error() {
        let output = serde_json::json!({
            "_meta": {
                "type_aware": {
                    "required_completeness": "complete",
                    "identity": { "completeness": "partial" }
                }
            }
        })
        .to_string();

        let result = non_success_result(1, &output, "", false);

        assert_eq!(result.is_error, Some(true));
    }

    #[test]
    fn best_effort_incomplete_exit_one_remains_a_successful_finding_result() {
        let output = serde_json::json!({
            "_meta": {
                "type_aware": {
                    "required_completeness": "best-effort",
                    "identity": { "completeness": "partial" }
                }
            }
        })
        .to_string();

        let result = non_success_result(1, &output, "", false);

        assert_eq!(result.is_error, Some(false));
    }

    #[test]
    fn tool_timeout_defaults_remain_distinct_and_share_the_env_override_parser() {
        assert_eq!(
            timeout_duration_from(None, DEFAULT_TIMEOUT_SECS),
            Duration::from_mins(2)
        );
        assert_eq!(
            timeout_duration_from(None, 15 * 60),
            Duration::from_mins(15)
        );
        assert_eq!(
            timeout_duration_from(Some("42"), 15 * 60),
            Duration::from_secs(42)
        );
        assert_eq!(
            timeout_duration_from(Some("invalid"), 15 * 60),
            Duration::from_mins(15)
        );
    }
}
