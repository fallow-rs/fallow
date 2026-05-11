mod analyze;
mod audit;
mod check_changed;
mod check_runtime_coverage;
mod dupes;
mod explain;
mod fix;
mod flags;
mod health;
mod list_boundaries;
mod project_info;
mod trace;

pub use analyze::build_analyze_args;
pub use audit::build_audit_args;
pub use check_changed::build_check_changed_args;
pub use check_runtime_coverage::{
    build_check_runtime_coverage_args, build_get_blast_radius_args,
    build_get_cleanup_candidates_args, build_get_hot_paths_args, build_get_importance_args,
};
pub use dupes::build_find_dupes_args;
pub use explain::build_explain_args;
pub use fix::{build_fix_apply_args, build_fix_preview_args};
pub use flags::build_feature_flags_args;
pub use health::build_health_args;
pub use list_boundaries::build_list_boundaries_args;
pub use project_info::build_project_info_args;
pub use trace::{
    build_trace_clone_args, build_trace_dependency_args, build_trace_export_args,
    build_trace_file_args,
};

use std::process::Stdio;
use std::time::Duration;

use rmcp::ErrorData as McpError;
use rmcp::model::{CallToolResult, Content, RawContent};
use tokio::process::Command;

/// Default subprocess timeout in seconds.
const DEFAULT_TIMEOUT_SECS: u64 = 120;

/// Push a `--flag VALUE` pair onto `args` only when `value` is `Some(s)` and
/// `s` is non-empty. MCP clients (especially LLM-driven ones) sometimes send
/// `""` for unset path or string params instead of omitting the field; an
/// empty string forwarded as `--flag ""` would either be rejected by clap or
/// silently mean "current directory" depending on the flag, both of which are
/// confusing failure modes.
fn push_str_flag(args: &mut Vec<String>, flag: &str, value: Option<&str>) {
    if let Some(s) = value
        && !s.is_empty()
    {
        args.extend([flag.to_string(), s.to_string()]);
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

/// Issue type flag names mapped to their CLI flags.
pub const ISSUE_TYPE_FLAGS: &[(&str, &str)] = &[
    ("unused-files", "--unused-files"),
    ("unused-exports", "--unused-exports"),
    ("unused-types", "--unused-types"),
    ("private-type-leaks", "--private-type-leaks"),
    ("unused-deps", "--unused-deps"),
    ("unused-enum-members", "--unused-enum-members"),
    ("unused-class-members", "--unused-class-members"),
    ("unresolved-imports", "--unresolved-imports"),
    ("unlisted-deps", "--unlisted-deps"),
    ("duplicate-exports", "--duplicate-exports"),
    ("circular-deps", "--circular-deps"),
    ("boundary-violations", "--boundary-violations"),
    ("stale-suppressions", "--stale-suppressions"),
    ("unused-catalog-entries", "--unused-catalog-entries"),
];

/// Valid detection modes for the `find_dupes` tool.
pub const VALID_DUPES_MODES: &[&str] = &["strict", "mild", "weak", "semantic"];

/// Valid gate values for the `audit` tool.
pub const VALID_AUDIT_GATES: &[&str] = &["new-only", "all"];

/// Build a structured validation error body matching the shape `run_fallow` emits
/// for CLI-level errors: `{"error": true, "message": "...", "exit_code": 0}`.
///
/// Used by arg builders to reject invalid input before spawning fallow. `exit_code`
/// is `0` because no subprocess ran, disambiguating validation failures from CLI
/// error exits (which use the real exit code). The returned string is compact JSON
/// ready to be wrapped in `CallToolResult::error(vec![Content::text(body)])`.
pub fn validation_error_body(message: impl Into<String>) -> String {
    serde_json::json!({
        "error": true,
        "message": message.into(),
        "exit_code": 0,
    })
    .to_string()
}

/// Read the subprocess timeout from `FALLOW_TIMEOUT_SECS` or fall back to the default.
fn timeout_duration() -> Duration {
    std::env::var("FALLOW_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .map_or(
            Duration::from_secs(DEFAULT_TIMEOUT_SECS),
            Duration::from_secs,
        )
}

/// Execute the fallow CLI binary with the given arguments and return the result.
pub async fn run_fallow(binary: &str, args: &[String]) -> Result<CallToolResult, McpError> {
    let timeout = timeout_duration();

    let output = tokio::time::timeout(
        timeout,
        Command::new(binary)
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output(),
    )
    .await
    .map_err(|_| {
        McpError::internal_error(
            format!(
                "fallow subprocess timed out after {}s. \
                 Set FALLOW_TIMEOUT_SECS to increase the limit.",
                timeout.as_secs()
            ),
            None,
        )
    })?
    .map_err(|e| {
        McpError::internal_error(
            format!(
                "Failed to execute fallow binary '{binary}': {e}. \
                 Ensure fallow is installed and available in PATH, \
                 or set the FALLOW_BIN environment variable."
            ),
            None,
        )
    })?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if !output.status.success() {
        let exit_code = output.status.code().unwrap_or(-1);

        // Exit code 1 = issues found (not an error for analysis tools)
        if exit_code == 1 {
            let text = if stdout.is_empty() {
                "{}".to_string()
            } else {
                stdout.to_string()
            };
            return Ok(CallToolResult::success(vec![Content::text(text)]));
        }

        // Exit code 2+ = real error. The CLI emits structured JSON on stdout
        // when --format json is active; prefer that over reconstructing from stderr.
        // Invariant: stdout on error exit is either valid JSON or empty — never
        // partial or non-JSON output. If a plugin/hook corrupts stdout, we fall
        // through to the stderr reconstruction path below.
        if !stdout.is_empty() && serde_json::from_str::<serde_json::Value>(&stdout).is_ok() {
            return Ok(CallToolResult::error(vec![Content::text(
                stdout.to_string(),
            )]));
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

        return Ok(CallToolResult::error(vec![Content::text(
            error_json.to_string(),
        )]));
    }

    if stdout.is_empty() {
        return Ok(CallToolResult::success(vec![Content::text(
            "{}".to_string(),
        )]));
    }

    Ok(CallToolResult::success(vec![Content::text(
        stdout.to_string(),
    )]))
}

/// Execute fallow and ensure successful JSON responses have a top-level
/// `warnings` array for agent-facing runtime context tools.
pub async fn run_fallow_with_top_level_warnings(
    binary: &str,
    args: &[String],
) -> Result<CallToolResult, McpError> {
    let result = run_fallow(binary, args).await?;
    if result.is_error == Some(true) {
        return Ok(result);
    }

    let Some(content) = result.content.first() else {
        return Ok(result);
    };
    let RawContent::Text(text) = &content.raw else {
        return Ok(result);
    };
    let Ok(mut value) = serde_json::from_str::<serde_json::Value>(&text.text) else {
        return Ok(result);
    };
    let Some(map) = value.as_object_mut() else {
        return Ok(result);
    };

    map.entry("warnings".to_string())
        .or_insert_with(|| serde_json::Value::Array(Vec::new()));

    let text = serde_json::to_string_pretty(&value).unwrap_or_else(|_| text.text.clone());
    Ok(CallToolResult::success(vec![Content::text(text)]))
}
