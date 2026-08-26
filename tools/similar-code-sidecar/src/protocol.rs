use std::collections::BTreeSet;
use std::io::{BufRead, Write};
use std::time::Instant;

use serde::{Deserialize, Serialize};

use crate::cache::ModelPaths;
use crate::constants::{
    ANALYSIS_OPERATION, DEFAULT_MAX_FUNCTIONS, DEFAULT_MAX_SOURCE_BYTES_PER_FUNCTION,
    DEFAULT_MAX_TOTAL_SOURCE_BYTES, DEFAULT_TIMEOUT_MS, EMBEDDING_SEMANTICS_VERSION,
    HARD_MAX_FUNCTIONS, HARD_MAX_SOURCE_BYTES_PER_FUNCTION, HARD_MAX_TIMEOUT_MS,
    HARD_MAX_TOTAL_SOURCE_BYTES, MAX_BATCH_SIZE, MAX_JSONL_LINE_BYTES, MODEL_DIMENSIONS,
    MODEL_MAX_TOKENS, MODEL_REVISION, PROTOCOL_VERSION,
};
use crate::model::{EmbedError, LocalModel};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EmbedRequest {
    operation: String,
    protocol_version: u32,
    embedding_semantics_version: u32,
    model_revision: String,
    dimensions: usize,
    max_tokens: usize,
    functions: Vec<FunctionInput>,
    #[serde(default)]
    limits: RequestLimits,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FunctionInput {
    key: u32,
    source: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ShutdownRequest {
    operation: String,
}

#[derive(Clone, Copy, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RequestLimits {
    max_functions: Option<usize>,
    max_total_source_bytes: Option<usize>,
    max_source_bytes_per_function: Option<usize>,
    batch_size: Option<usize>,
    timeout_ms: Option<u64>,
}

#[derive(Clone, Copy, Serialize)]
struct AppliedLimits {
    max_functions: usize,
    max_total_source_bytes: usize,
    max_source_bytes_per_function: usize,
    max_tokens: usize,
    batch_size: usize,
    timeout_ms: u64,
}

impl RequestLimits {
    fn applied(self) -> AppliedLimits {
        AppliedLimits {
            max_functions: bounded_usize(
                self.max_functions,
                DEFAULT_MAX_FUNCTIONS,
                HARD_MAX_FUNCTIONS,
            ),
            max_total_source_bytes: bounded_usize(
                self.max_total_source_bytes,
                DEFAULT_MAX_TOTAL_SOURCE_BYTES,
                HARD_MAX_TOTAL_SOURCE_BYTES,
            ),
            max_source_bytes_per_function: bounded_usize(
                self.max_source_bytes_per_function,
                DEFAULT_MAX_SOURCE_BYTES_PER_FUNCTION,
                HARD_MAX_SOURCE_BYTES_PER_FUNCTION,
            ),
            max_tokens: MODEL_MAX_TOKENS,
            batch_size: bounded_usize(self.batch_size, MAX_BATCH_SIZE, MAX_BATCH_SIZE),
            timeout_ms: self
                .timeout_ms
                .unwrap_or(DEFAULT_TIMEOUT_MS)
                .clamp(1, HARD_MAX_TIMEOUT_MS),
        }
    }
}

#[derive(Serialize)]
struct EmbedResponse {
    protocol_version: u32,
    embedding_semantics_version: u32,
    model_revision: &'static str,
    dimensions: usize,
    vectors: Vec<VectorOutput>,
    timing: Timing,
    status: CompletionStatus,
    completion: Completion,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    errors: Vec<FunctionError>,
}

#[derive(Serialize)]
struct VectorOutput {
    key: u32,
    values: Vec<f32>,
    #[serde(skip_serializing_if = "is_false")]
    truncated: bool,
}

#[derive(Serialize)]
struct Timing {
    inference_ms: u64,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
enum CompletionStatus {
    Complete,
    Partial,
    Error,
}

#[derive(Serialize)]
struct Completion {
    requested_functions: usize,
    embedded_functions: usize,
    skipped_functions: usize,
    truncated_functions: usize,
    applied_limits: AppliedLimits,
}

#[derive(Serialize)]
struct FunctionError {
    #[serde(skip_serializing_if = "Option::is_none")]
    key: Option<u32>,
    code: ErrorCode,
    retryable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    observed: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    limit: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
enum ErrorCode {
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

pub fn serve(
    input: &mut dyn BufRead,
    output: &mut dyn Write,
    paths: &ModelPaths,
) -> Result<(), String> {
    let mut model: Option<LocalModel> = None;
    loop {
        let Some(line) = read_bounded_line(input)? else {
            return Ok(());
        };
        if matches!(&line, BoundedLine::Complete(bytes) if bytes.is_empty()) {
            continue;
        }
        let response = match line {
            BoundedLine::Complete(bytes) => {
                if serde_json::from_slice::<ShutdownRequest>(&bytes)
                    .is_ok_and(|request| request.operation == "shutdown")
                {
                    return Ok(());
                }
                match serde_json::from_slice::<EmbedRequest>(&bytes) {
                    Ok(request) => process_request(request, paths, &mut model),
                    Err(_) => error_response(ErrorCode::InvalidRequest, None, false),
                }
            }
            BoundedLine::Oversized => error_response(ErrorCode::RequestTooLarge, None, false),
        };
        serde_json::to_writer(&mut *output, &response)
            .map_err(|error| format!("failed to write protocol response: {error}"))?;
        output
            .write_all(b"\n")
            .and_then(|()| output.flush())
            .map_err(|error| format!("failed to flush protocol response: {error}"))?;
    }
}

#[expect(
    clippy::needless_pass_by_value,
    clippy::too_many_lines,
    reason = "the request boundary consumes the envelope and validates it in one auditable flow"
)]
fn process_request(
    request: EmbedRequest,
    paths: &ModelPaths,
    model: &mut Option<LocalModel>,
) -> EmbedResponse {
    let limits = request.limits.applied();
    let requested_functions = request.functions.len();
    if request.operation != ANALYSIS_OPERATION {
        return error_response_with_limits(ErrorCode::InvalidRequest, limits, requested_functions);
    }
    if request.protocol_version != PROTOCOL_VERSION {
        return error_response_with_limits(
            ErrorCode::ProtocolMismatch,
            limits,
            requested_functions,
        );
    }
    if request.embedding_semantics_version != EMBEDDING_SEMANTICS_VERSION {
        return error_response_with_limits(
            ErrorCode::EmbeddingSemanticsMismatch,
            limits,
            requested_functions,
        );
    }
    if request.model_revision != MODEL_REVISION {
        return error_response_with_limits(
            ErrorCode::ModelRevisionMismatch,
            limits,
            requested_functions,
        );
    }
    if request.dimensions != MODEL_DIMENSIONS {
        return error_response_with_limits(
            ErrorCode::DimensionMismatch,
            limits,
            requested_functions,
        );
    }
    if request.max_tokens != MODEL_MAX_TOKENS {
        return error_response_with_limits(
            ErrorCode::MaxTokensMismatch,
            limits,
            requested_functions,
        );
    }
    if has_duplicate_keys(&request.functions) {
        return error_response_with_limits(
            ErrorCode::DuplicateFunctionKey,
            limits,
            requested_functions,
        );
    }
    if request.functions.is_empty() {
        return EmbedResponse {
            protocol_version: PROTOCOL_VERSION,
            embedding_semantics_version: EMBEDDING_SEMANTICS_VERSION,
            model_revision: MODEL_REVISION,
            dimensions: MODEL_DIMENSIONS,
            vectors: Vec::new(),
            timing: Timing { inference_ms: 0 },
            status: CompletionStatus::Complete,
            completion: Completion {
                requested_functions: 0,
                embedded_functions: 0,
                skipped_functions: 0,
                truncated_functions: 0,
                applied_limits: limits,
            },
            errors: Vec::new(),
        };
    }

    if model.is_none() {
        match LocalModel::load(paths) {
            Ok(loaded) => *model = Some(loaded),
            Err(error) => {
                return error_response_with_message(
                    ErrorCode::ModelNotReady,
                    limits,
                    requested_functions,
                    &error,
                );
            }
        }
    }
    let Some(model) = model.as_ref() else {
        return error_response_with_limits(ErrorCode::ModelNotReady, limits, requested_functions);
    };

    let started = Instant::now();
    let mut vectors = Vec::new();
    let mut errors = Vec::new();
    let mut inference_ms = 0_u64;
    let mut total_source_bytes = 0_usize;
    for (index, function) in request.functions.iter().enumerate() {
        if index >= limits.max_functions {
            errors.push(limit_error(
                function.key,
                ErrorCode::FunctionLimit,
                request.functions.len() as u64,
                limits.max_functions as u64,
            ));
            continue;
        }
        if started.elapsed().as_millis() >= u128::from(limits.timeout_ms) {
            errors.push(limit_error(
                function.key,
                ErrorCode::Timeout,
                u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
                limits.timeout_ms,
            ));
            continue;
        }
        let source_bytes = function.source.len();
        if source_bytes > limits.max_source_bytes_per_function {
            errors.push(limit_error(
                function.key,
                ErrorCode::FunctionSourceBytesLimit,
                source_bytes as u64,
                limits.max_source_bytes_per_function as u64,
            ));
            continue;
        }
        if total_source_bytes.saturating_add(source_bytes) > limits.max_total_source_bytes {
            errors.push(limit_error(
                function.key,
                ErrorCode::TotalSourceBytesLimit,
                total_source_bytes.saturating_add(source_bytes) as u64,
                limits.max_total_source_bytes as u64,
            ));
            continue;
        }
        total_source_bytes = total_source_bytes.saturating_add(source_bytes);
        match model.embed(&function.source) {
            Ok(embedding) => {
                inference_ms = inference_ms.saturating_add(embedding.inference_ms);
                vectors.push(VectorOutput {
                    key: function.key,
                    values: embedding.values,
                    truncated: embedding.truncated,
                });
            }
            Err(EmbedError::Inference(message)) => errors.push(FunctionError {
                key: Some(function.key),
                code: ErrorCode::InferenceFailed,
                retryable: false,
                observed: None,
                limit: None,
                message: Some(bounded_message(&message)),
            }),
        }
    }
    let skipped = errors.len();
    let truncated = vectors.iter().filter(|vector| vector.truncated).count();
    EmbedResponse {
        protocol_version: PROTOCOL_VERSION,
        embedding_semantics_version: EMBEDDING_SEMANTICS_VERSION,
        model_revision: MODEL_REVISION,
        dimensions: MODEL_DIMENSIONS,
        timing: Timing { inference_ms },
        status: if errors.is_empty() {
            CompletionStatus::Complete
        } else if vectors.is_empty() {
            CompletionStatus::Error
        } else {
            CompletionStatus::Partial
        },
        completion: Completion {
            requested_functions: request.functions.len(),
            embedded_functions: vectors.len(),
            skipped_functions: skipped,
            truncated_functions: truncated,
            applied_limits: limits,
        },
        vectors,
        errors,
    }
}

fn error_response(code: ErrorCode, key: Option<u32>, retryable: bool) -> EmbedResponse {
    let limits = RequestLimits::default().applied();
    EmbedResponse {
        protocol_version: PROTOCOL_VERSION,
        embedding_semantics_version: EMBEDDING_SEMANTICS_VERSION,
        model_revision: MODEL_REVISION,
        dimensions: MODEL_DIMENSIONS,
        vectors: Vec::new(),
        timing: Timing { inference_ms: 0 },
        status: CompletionStatus::Error,
        completion: Completion {
            requested_functions: 0,
            embedded_functions: 0,
            skipped_functions: 0,
            truncated_functions: 0,
            applied_limits: limits,
        },
        errors: vec![FunctionError {
            key,
            code,
            retryable,
            observed: None,
            limit: None,
            message: None,
        }],
    }
}

fn error_response_with_limits(
    code: ErrorCode,
    limits: AppliedLimits,
    requested_functions: usize,
) -> EmbedResponse {
    let mut response = error_response(code, None, false);
    response.completion.applied_limits = limits;
    response.completion.requested_functions = requested_functions;
    response.completion.skipped_functions = requested_functions;
    response
}

fn error_response_with_message(
    code: ErrorCode,
    limits: AppliedLimits,
    requested_functions: usize,
    message: &str,
) -> EmbedResponse {
    let mut response = error_response_with_limits(code, limits, requested_functions);
    if let Some(error) = response.errors.first_mut() {
        error.message = Some(bounded_message(message));
    }
    response
}

fn limit_error(key: u32, code: ErrorCode, observed: u64, limit: u64) -> FunctionError {
    FunctionError {
        key: Some(key),
        code,
        retryable: false,
        observed: Some(observed),
        limit: Some(limit),
        message: None,
    }
}

fn bounded_message(message: &str) -> String {
    message.chars().take(2_000).collect()
}

fn bounded_usize(value: Option<usize>, default: usize, hard_max: usize) -> usize {
    value.unwrap_or(default).clamp(1, hard_max)
}

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde skip_serializing_if requires a shared-reference predicate"
)]
fn is_false(value: &bool) -> bool {
    !value
}

fn has_duplicate_keys(functions: &[FunctionInput]) -> bool {
    let mut keys = BTreeSet::new();
    functions.iter().any(|function| !keys.insert(function.key))
}

enum BoundedLine {
    Complete(Vec<u8>),
    Oversized,
}

fn read_bounded_line(input: &mut dyn BufRead) -> Result<Option<BoundedLine>, String> {
    let mut bytes = Vec::new();
    let mut oversized = false;
    loop {
        let available = input
            .fill_buf()
            .map_err(|error| format!("failed to read protocol input: {error}"))?;
        if available.is_empty() {
            if bytes.is_empty() && !oversized {
                return Ok(None);
            }
            break;
        }
        let newline = available.iter().position(|byte| *byte == b'\n');
        let consumed = newline.map_or(available.len(), |index| index + 1);
        let content = newline.map_or(available, |index| &available[..index]);
        if !oversized && bytes.len().saturating_add(content.len()) <= MAX_JSONL_LINE_BYTES {
            bytes.extend_from_slice(content);
        } else {
            oversized = true;
            bytes.clear();
        }
        input.consume(consumed);
        if newline.is_some() {
            break;
        }
    }
    if oversized {
        Ok(Some(BoundedLine::Oversized))
    } else {
        if bytes.last() == Some(&b'\r') {
            bytes.pop();
        }
        Ok(Some(BoundedLine::Complete(bytes)))
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        reason = "test fixture construction must fail immediately"
    )]

    use std::io::{BufReader, Cursor};

    use super::*;

    #[test]
    fn empty_request_is_complete_without_loading_a_model() {
        let directory = tempfile::tempdir().expect("tempdir");
        let paths = ModelPaths::from_cache_root(directory.path());
        let request = EmbedRequest {
            operation: ANALYSIS_OPERATION.to_string(),
            protocol_version: PROTOCOL_VERSION,
            embedding_semantics_version: EMBEDDING_SEMANTICS_VERSION,
            model_revision: MODEL_REVISION.to_string(),
            dimensions: MODEL_DIMENSIONS,
            max_tokens: MODEL_MAX_TOKENS,
            functions: Vec::new(),
            limits: RequestLimits::default(),
        };
        let response = process_request(request, &paths, &mut None);
        assert!(matches!(response.status, CompletionStatus::Complete));
        assert!(response.vectors.is_empty());
    }

    #[test]
    fn serve_reports_missing_model_without_echoing_source() {
        let directory = tempfile::tempdir().expect("tempdir");
        let paths = ModelPaths::from_cache_root(directory.path());
        let secret = "private-source-marker";
        let request = serde_json::json!({
            "operation": ANALYSIS_OPERATION,
            "protocol_version": PROTOCOL_VERSION,
            "embedding_semantics_version": EMBEDDING_SEMANTICS_VERSION,
            "model_revision": MODEL_REVISION,
            "dimensions": MODEL_DIMENSIONS,
            "max_tokens": MODEL_MAX_TOKENS,
            "functions": [{"key": 7, "source": secret}]
        });
        let input = format!("{request}\n");
        let mut reader = BufReader::new(Cursor::new(input));
        let mut output = Vec::new();
        serve(&mut reader, &mut output, &paths).expect("serve response");
        let rendered = String::from_utf8(output).expect("utf8");
        assert!(rendered.contains("model-not-ready"));
        assert!(!rendered.contains(secret));
        let response: serde_json::Value = serde_json::from_str(&rendered).expect("json response");
        assert_eq!(response["status"], "error");
        assert_eq!(response["completion"]["requested_functions"], 1);
        assert_eq!(response["completion"]["skipped_functions"], 1);
    }

    #[test]
    fn protocol_mismatch_is_typed() {
        let directory = tempfile::tempdir().expect("tempdir");
        let paths = ModelPaths::from_cache_root(directory.path());
        let request = EmbedRequest {
            operation: ANALYSIS_OPERATION.to_string(),
            protocol_version: PROTOCOL_VERSION + 1,
            embedding_semantics_version: EMBEDDING_SEMANTICS_VERSION,
            model_revision: MODEL_REVISION.to_string(),
            dimensions: MODEL_DIMENSIONS,
            max_tokens: MODEL_MAX_TOKENS,
            functions: Vec::new(),
            limits: RequestLimits::default(),
        };
        let response = process_request(request, &paths, &mut None);
        assert!(matches!(response.status, CompletionStatus::Error));
        assert!(matches!(
            response.errors[0].code,
            ErrorCode::ProtocolMismatch
        ));
    }

    #[test]
    fn embedding_semantics_mismatch_is_typed() {
        let directory = tempfile::tempdir().expect("tempdir");
        let paths = ModelPaths::from_cache_root(directory.path());
        let request = EmbedRequest {
            operation: ANALYSIS_OPERATION.to_string(),
            protocol_version: PROTOCOL_VERSION,
            embedding_semantics_version: EMBEDDING_SEMANTICS_VERSION + 1,
            model_revision: MODEL_REVISION.to_string(),
            dimensions: MODEL_DIMENSIONS,
            max_tokens: MODEL_MAX_TOKENS,
            functions: Vec::new(),
            limits: RequestLimits::default(),
        };
        let response = process_request(request, &paths, &mut None);
        assert!(matches!(response.status, CompletionStatus::Error));
        assert!(matches!(
            response.errors[0].code,
            ErrorCode::EmbeddingSemanticsMismatch
        ));
    }

    #[test]
    fn request_limits_are_clamped_to_hard_caps() {
        let limits = RequestLimits {
            max_functions: Some(usize::MAX),
            max_total_source_bytes: Some(usize::MAX),
            max_source_bytes_per_function: Some(usize::MAX),
            batch_size: Some(usize::MAX),
            timeout_ms: Some(u64::MAX),
        }
        .applied();
        assert_eq!(limits.max_functions, HARD_MAX_FUNCTIONS);
        assert_eq!(limits.batch_size, MAX_BATCH_SIZE);
        assert_eq!(limits.timeout_ms, HARD_MAX_TIMEOUT_MS);
    }

    #[test]
    fn duplicate_keys_are_rejected_before_model_loading() {
        let directory = tempfile::tempdir().expect("tempdir");
        let paths = ModelPaths::from_cache_root(directory.path());
        let request = EmbedRequest {
            operation: ANALYSIS_OPERATION.to_string(),
            protocol_version: PROTOCOL_VERSION,
            embedding_semantics_version: EMBEDDING_SEMANTICS_VERSION,
            model_revision: MODEL_REVISION.to_string(),
            dimensions: MODEL_DIMENSIONS,
            max_tokens: MODEL_MAX_TOKENS,
            functions: vec![
                FunctionInput {
                    key: 1,
                    source: "first".to_string(),
                },
                FunctionInput {
                    key: 1,
                    source: "second".to_string(),
                },
            ],
            limits: RequestLimits::default(),
        };
        let response = process_request(request, &paths, &mut None);
        assert!(matches!(
            response.errors[0].code,
            ErrorCode::DuplicateFunctionKey
        ));
    }

    #[test]
    fn max_tokens_mismatch_is_typed() {
        let directory = tempfile::tempdir().expect("tempdir");
        let paths = ModelPaths::from_cache_root(directory.path());
        let request = EmbedRequest {
            operation: ANALYSIS_OPERATION.to_string(),
            protocol_version: PROTOCOL_VERSION,
            embedding_semantics_version: EMBEDDING_SEMANTICS_VERSION,
            model_revision: MODEL_REVISION.to_string(),
            dimensions: MODEL_DIMENSIONS,
            max_tokens: MODEL_MAX_TOKENS - 1,
            functions: Vec::new(),
            limits: RequestLimits::default(),
        };
        let response = process_request(request, &paths, &mut None);
        assert!(matches!(
            response.errors[0].code,
            ErrorCode::MaxTokensMismatch
        ));
    }

    #[test]
    fn bounded_reader_discards_oversized_lines() {
        let input = vec![b'x'; MAX_JSONL_LINE_BYTES + 1];
        let mut reader = BufReader::new(Cursor::new(input));
        assert!(matches!(
            read_bounded_line(&mut reader).expect("bounded read"),
            Some(BoundedLine::Oversized)
        ));
    }

    #[test]
    fn malformed_json_returns_a_typed_error() {
        let directory = tempfile::tempdir().expect("tempdir");
        let paths = ModelPaths::from_cache_root(directory.path());
        let mut reader = BufReader::new(Cursor::new(b"not-json\n"));
        let mut output = Vec::new();
        serve(&mut reader, &mut output, &paths).expect("serve response");
        let response: serde_json::Value = serde_json::from_slice(&output).expect("json");
        assert_eq!(response["errors"][0]["code"], "invalid-request");
    }

    #[test]
    fn shutdown_ends_the_session_without_a_response() {
        let directory = tempfile::tempdir().expect("tempdir");
        let paths = ModelPaths::from_cache_root(directory.path());
        let mut reader = BufReader::new(Cursor::new(b"{\"operation\":\"shutdown\"}\n"));
        let mut output = Vec::new();
        serve(&mut reader, &mut output, &paths).expect("serve shutdown");
        assert!(output.is_empty());
    }
}
