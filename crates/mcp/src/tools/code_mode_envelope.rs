//! The limits of a Code Mode snippet run and the error envelope of a run
//! that fails.

use std::path::Path;

use serde_json::json;

use crate::params::CodeExecuteParams;

use super::code_mode_rust::{is_rust_root, rust_root_envelope};
use super::{
    CodeModeCall, DEFAULT_MAX_OUTPUT_BYTES, DEFAULT_TIMEOUT_MS, MAX_CODE_BYTES, MAX_ERROR_BYTES,
    MAX_HOST_CALLS, MAX_OUTPUT_BYTES, MAX_RECORDED_CALLS, MAX_REJECTED_HOST_CALLS, MAX_TIMEOUT_MS,
    clamp_utf8,
};

/// The limits of one snippet run, from its params.
pub(super) struct SnippetLimits {
    pub(super) timeout_ms: u64,
    pub(super) max_output_bytes: usize,
    /// The `limits` block of the response envelope.
    pub(super) envelope: serde_json::Value,
    /// The byte cap of the `error` string of the response envelope.
    pub(super) error_bytes: usize,
}

impl SnippetLimits {
    pub(super) fn new(params: &CodeExecuteParams) -> Self {
        let timeout_ms = params
            .timeout_ms
            .unwrap_or(DEFAULT_TIMEOUT_MS)
            .min(MAX_TIMEOUT_MS);
        let max_output_bytes = params
            .max_output_bytes
            .unwrap_or(DEFAULT_MAX_OUTPUT_BYTES)
            .min(MAX_OUTPUT_BYTES);
        Self {
            timeout_ms,
            max_output_bytes,
            envelope: code_mode_limits(timeout_ms, max_output_bytes),
            error_bytes: max_output_bytes.max(MAX_ERROR_BYTES),
        }
    }

    /// An error envelope with no host calls.
    pub(super) fn error(&self, error: &str) -> String {
        error_envelope(error, &[], 0, &self.envelope, self.error_bytes)
    }
}

/// Refuse a snippet that must not start: an oversized snippet, or a snippet
/// on a Rust root.
pub(super) fn refuse_before_run(
    params: &CodeExecuteParams,
    limits: &SnippetLimits,
    cwd: &Path,
) -> Result<(), String> {
    if params.code.len() > MAX_CODE_BYTES {
        return Err(limits.error(&format!(
            "code mode snippet exceeded {MAX_CODE_BYTES} bytes"
        )));
    }
    if is_rust_root(params.root.as_deref(), cwd) {
        return Err(rust_root_envelope(&limits.envelope, limits.error_bytes));
    }
    Ok(())
}

/// Render the dropped-entry count only when the trace actually overflowed, so
/// an ordinary response keeps the shape its consumers already parse.
pub(super) fn omitted_field(calls_omitted: usize) -> Option<usize> {
    (calls_omitted > 0).then_some(calls_omitted)
}

/// Build a failed code-mode response, clamping `error` so a thrown message
/// cannot push megabytes into the calling agent's context the way an oversized
/// result would. The `truncated` and `error_bytes` fields appear only when the
/// clamp actually fired, so an ordinary failure keeps its existing shape.
pub(super) fn error_envelope(
    error: &str,
    calls: &[CodeModeCall],
    calls_omitted: usize,
    limits: &serde_json::Value,
    max_error_bytes: usize,
) -> String {
    let mut envelope = json!({
        "schema_version": "mcp-code-execute/v1",
        "ok": false,
        "error": clamp_utf8(error, max_error_bytes),
        "calls": calls,
        "calls_omitted": omitted_field(calls_omitted),
        "limits": limits
    });
    if error.len() > max_error_bytes
        && let Some(fields) = envelope.as_object_mut()
    {
        fields.insert("truncated".to_string(), json!(true));
        fields.insert("error_bytes".to_string(), json!(error.len()));
    }
    envelope.to_string()
}

/// Build the `limits` JSON block echoed on every code-mode response.
pub(super) fn code_mode_limits(timeout_ms: u64, max_output_bytes: usize) -> serde_json::Value {
    json!({
        "timeout_ms": timeout_ms,
        "max_output_bytes": max_output_bytes,
        "max_host_calls": MAX_HOST_CALLS,
        "max_rejected_host_calls": MAX_REJECTED_HOST_CALLS,
        "max_recorded_calls": MAX_RECORDED_CALLS
    })
}
