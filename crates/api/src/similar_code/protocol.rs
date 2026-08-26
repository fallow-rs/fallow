//! Generated constants and wire DTOs for the local similar-code companion.

#![allow(
    dead_code,
    clippy::unreadable_literal,
    reason = "generated protocol fields are validated even when not consumed by this crate"
)]

use serde::{Deserialize, Serialize};

/// One immutable model artifact required by the official local provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SimilarCodeArtifact {
    /// Repository-relative model artifact path.
    pub path: &'static str,
    /// Expected artifact size in bytes.
    pub size: u64,
    /// Expected lowercase SHA-256 digest.
    pub sha256: &'static str,
}

include!(concat!(env!("OUT_DIR"), "/similar_code_protocol.rs"));

/// One transient function submitted to the local provider.
#[derive(Debug, Serialize)]
pub(super) struct EmbedFunctionRequest<'a> {
    /// Opaque request-local key. It carries no path or source identity.
    pub key: u32,
    /// Full bounded function source.
    pub source: &'a str,
}

/// One bounded inference batch.
#[derive(Debug, Serialize)]
pub(super) struct EmbedBatchRequest<'a> {
    /// Wire operation.
    pub operation: &'static str,
    /// Wire protocol version.
    pub protocol_version: u32,
    /// Required embedding calculation semantics.
    pub embedding_semantics_version: u32,
    /// Required immutable model revision.
    pub model_revision: &'static str,
    /// Expected embedding width.
    pub dimensions: usize,
    /// Maximum tokenizer length before deterministic truncation.
    pub max_tokens: usize,
    /// Functions in this batch.
    pub functions: &'a [EmbedFunctionRequest<'a>],
}

/// One provider-returned vector.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct EmbedFunctionResponse {
    /// Request-local key copied from the input.
    pub key: u32,
    /// Dense normalized embedding values.
    pub values: Vec<f32>,
    /// Whether tokenizer length bounded this source fragment.
    #[serde(default)]
    pub truncated: bool,
}

/// Provider timing for one batch.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct EmbedBatchTiming {
    /// Model inference wall time.
    pub inference_ms: f64,
}

/// Response from one bounded inference batch.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct EmbedBatchResponse {
    /// Wire protocol version used by the provider.
    pub protocol_version: u32,
    /// Embedding calculation semantics used by the provider.
    pub embedding_semantics_version: u32,
    /// Immutable model revision used by the provider.
    pub model_revision: String,
    /// Returned embedding width.
    pub dimensions: usize,
    /// Vectors in request order or keyed form.
    pub vectors: Vec<EmbedFunctionResponse>,
    /// Provider timing.
    pub timing: EmbedBatchTiming,
    /// Overall provider outcome for this request.
    pub status: EmbedCompletionStatus,
    /// Typed provider limit and completion accounting.
    pub completion: EmbedCompletion,
    /// Per-function or request-level failures.
    #[serde(default)]
    pub errors: Vec<EmbedFunctionError>,
}

/// Provider completion state for one embed request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum EmbedCompletionStatus {
    Complete,
    Partial,
    Error,
}

/// Limits the provider applied independently of caller input.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct EmbedAppliedLimits {
    pub max_functions: usize,
    pub max_total_source_bytes: usize,
    pub max_source_bytes_per_function: usize,
    pub max_tokens: usize,
    pub batch_size: usize,
    pub timeout_ms: u64,
}

/// Typed completion accounting from the provider.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct EmbedCompletion {
    pub requested_functions: usize,
    pub embedded_functions: usize,
    pub skipped_functions: usize,
    pub truncated_functions: usize,
    pub applied_limits: EmbedAppliedLimits,
}

/// Closed provider error catalogue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum EmbedErrorCode {
    InvalidRequest,
    ProtocolMismatch,
    EmbeddingSemanticsMismatch,
    ModelRevisionMismatch,
    DimensionMismatch,
    MaxTokensMismatch,
    DuplicateFunctionKey,
    FunctionLimit,
    TotalSourceBytesLimit,
    FunctionSourceBytesLimit,
    Timeout,
    ModelNotReady,
    InferenceFailed,
    RequestTooLarge,
}

/// One bounded provider error without source content.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct EmbedFunctionError {
    pub key: Option<u32>,
    pub code: EmbedErrorCode,
    pub retryable: bool,
    pub observed: Option<u64>,
    pub limit: Option<u64>,
    pub message: Option<String>,
}

/// Machine-readable companion and model availability.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SimilarCodeProviderStatus {
    /// Wire protocol version implemented by the sidecar.
    pub protocol_version: u32,
    /// Embedding calculation semantics implemented by the sidecar.
    pub embedding_semantics_version: u32,
    /// Installed sidecar package version.
    pub sidecar_version: String,
    /// Whether every pinned model artifact is present and valid.
    pub model_ready: bool,
    /// Immutable model identifier.
    pub model_id: String,
    /// Immutable model revision.
    pub model_revision: String,
    /// Embedding width.
    pub dimensions: usize,
    /// Maximum tokenizer length before deterministic truncation.
    pub max_tokens: usize,
    /// Model license identifier.
    pub license: String,
    /// User-cache directory containing model artifacts.
    pub cache_dir: String,
    /// Total expected artifact bytes.
    pub download_bytes: u64,
    /// Whether source analysis stays offline after setup.
    pub analysis_offline: bool,
    /// Whether all pinned artifacts passed size and SHA-256 validation.
    pub integrity_verified: bool,
    /// Actionable readiness problem when the model is not ready.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub problem: Option<String>,
    /// Whether setup downloaded new bytes, present only after setup.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub downloaded: Option<bool>,
}
