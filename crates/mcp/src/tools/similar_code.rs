use crate::params::{FindSimilarCodeParams, InspectSimilarCodeParams};

use rmcp::ErrorData as McpError;
use rmcp::model::{CallToolResult, ContentBlock};
use std::time::Duration;

use super::{
    push_global, push_remote_extends, push_str_flag, run_tool_with_stdin_timeout,
    run_tool_with_timeout, timeout_duration_with_default, validation_error_body,
};

const SIMILAR_CODE_TIMEOUT_SECS: u64 = 15 * 60;
const MAX_CANDIDATE_SNAPSHOT_BYTES: usize = 16 * 1024 * 1024;

fn similar_code_timeout() -> Duration {
    timeout_duration_with_default(SIMILAR_CODE_TIMEOUT_SECS)
}

/// Run local semantic candidate discovery without exposing setup mutation.
pub async fn run_find_similar_code(
    binary: &str,
    params: FindSimilarCodeParams,
) -> Result<CallToolResult, McpError> {
    match build_find_similar_code_args(&params) {
        Ok(args) => {
            run_tool_with_timeout(binary, "find_similar_code", &args, similar_code_timeout()).await
        }
        Err(message) => Ok(CallToolResult::error(vec![ContentBlock::text(message)])),
    }
}

/// Inspect one exact candidate snapshot and return its bounded evidence packet.
pub async fn run_inspect_similar_code(
    binary: &str,
    params: InspectSimilarCodeParams,
) -> Result<CallToolResult, McpError> {
    match build_inspect_similar_code_args(&params) {
        Ok(args) => {
            let snapshot = match serde_json::to_vec(&params.snapshot) {
                Ok(snapshot) => snapshot,
                Err(error) => {
                    return Ok(CallToolResult::error(vec![ContentBlock::text(
                        validation_error_body(format!(
                            "failed to serialize candidate snapshot: {error}"
                        )),
                    )]));
                }
            };
            if snapshot.len() > MAX_CANDIDATE_SNAPSHOT_BYTES {
                return Ok(CallToolResult::error(vec![ContentBlock::text(
                    validation_error_body(format!(
                        "candidate snapshot exceeds the {MAX_CANDIDATE_SNAPSHOT_BYTES}-byte limit"
                    )),
                )]));
            }
            run_tool_with_stdin_timeout(
                binary,
                "inspect_similar_code",
                &args,
                snapshot,
                similar_code_timeout(),
            )
            .await
        }
        Err(message) => Ok(CallToolResult::error(vec![ContentBlock::text(message)])),
    }
}

/// Build CLI arguments for `find_similar_code`.
pub fn build_find_similar_code_args(params: &FindSimilarCodeParams) -> Result<Vec<String>, String> {
    build_args(params)
}

/// Build CLI arguments for `inspect_similar_code`.
pub fn build_inspect_similar_code_args(
    params: &InspectSimilarCodeParams,
) -> Result<Vec<String>, String> {
    if params.candidate_id.trim().is_empty() {
        return Err(validation_error_body("candidate_id must not be empty"));
    }
    if params.snapshot.candidate.candidate_id != params.candidate_id.trim() {
        return Err(validation_error_body(
            "snapshot candidate identity must match candidate_id",
        ));
    }
    let mut args = vec![
        "similar-code".to_owned(),
        "--format".to_owned(),
        "json".to_owned(),
        "--quiet".to_owned(),
    ];
    push_global(
        &mut args,
        params.root.as_deref(),
        params.config.as_deref(),
        None,
        None,
    );
    push_remote_extends(&mut args, params.allow_remote_extends);
    args.extend([
        "inspect".to_owned(),
        params.candidate_id.trim().to_owned(),
        "--candidate-snapshot-stdin".to_owned(),
    ]);
    Ok(args)
}

fn build_args(params: &FindSimilarCodeParams) -> Result<Vec<String>, String> {
    if has_value(params.workspace.as_deref()) && has_value(params.changed_workspaces.as_deref()) {
        return Err(validation_error_body(
            "workspace and changed_workspaces are mutually exclusive for similar-code tools",
        ));
    }
    if params
        .threshold
        .is_some_and(|threshold| !threshold.is_finite() || !(0.0..=1.0).contains(&threshold))
    {
        return Err(validation_error_body(
            "threshold must be a finite number from 0 through 1",
        ));
    }
    if params.min_lines == Some(0) {
        return Err(validation_error_body("min_lines must be greater than zero"));
    }
    if params.top == Some(0) {
        return Err(validation_error_body("top must be greater than zero"));
    }

    let mut args = vec![
        "similar-code".to_owned(),
        "--format".to_owned(),
        "json".to_owned(),
        "--quiet".to_owned(),
    ];
    push_global(
        &mut args,
        params.root.as_deref(),
        params.config.as_deref(),
        params.no_cache,
        params.threads,
    );
    push_remote_extends(&mut args, params.allow_remote_extends);
    push_str_flag(&mut args, "--workspace", params.workspace.as_deref());
    push_str_flag(
        &mut args,
        "--changed-since",
        params.changed_since.as_deref(),
    );
    push_str_flag(
        &mut args,
        "--changed-workspaces",
        params.changed_workspaces.as_deref(),
    );
    if let Some(paths) = &params.paths {
        for path in paths {
            if path.trim().is_empty() {
                return Err(validation_error_body("paths entries must not be empty"));
            }
            args.extend(["--file".to_owned(), path.clone()]);
        }
    }
    if let Some(threshold) = params.threshold {
        args.extend(["--threshold".to_owned(), threshold.to_string()]);
    }
    if let Some(min_lines) = params.min_lines {
        args.extend(["--min-lines".to_owned(), min_lines.to_string()]);
    }
    if let Some(top) = params.top {
        args.extend(["--top".to_owned(), top.to_string()]);
    }
    Ok(args)
}

fn has_value(value: Option<&str>) -> bool {
    value.is_some_and(|value| !value.trim().is_empty())
}
