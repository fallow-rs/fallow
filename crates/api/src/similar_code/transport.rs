//! Trusted sibling discovery and bounded local provider transport.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use serde::Deserialize;
use serde::de::DeserializeOwned;

use super::protocol::{
    ANALYSIS_OPERATION, EMBEDDING_SEMANTICS_VERSION, EmbedBatchRequest, EmbedBatchResponse,
    EmbedCompletionStatus, EmbedErrorCode, EmbedFunctionRequest, MODEL_DIMENSIONS,
    MODEL_MAX_TOKENS, MODEL_REVISION, SIDECAR_BINARY, SimilarCodeProviderStatus,
    WIRE_PROTOCOL_VERSION,
};

const REQUEST_TIMEOUT: Duration = Duration::from_mins(2);
const STATUS_COMMAND_TIMEOUT: Duration = Duration::from_mins(2);
const SETUP_COMMAND_TIMEOUT: Duration = Duration::from_mins(55);
const MAX_REQUEST_BYTES: usize = 8 * 1024 * 1024;
const MAX_RESPONSE_BYTES: usize = 32 * 1024 * 1024;
const MAX_STDERR_BYTES: usize = 64 * 1024;
const BASE_ENV: &[&str] = &[
    "FALLOW_SIMILAR_CODE_CACHE_DIR",
    "HOME",
    "LOCALAPPDATA",
    "USERPROFILE",
    "XDG_CACHE_HOME",
    "TEMP",
    "TMP",
    "TMPDIR",
    "SYSTEMROOT",
    "WINDIR",
];
const DOWNLOAD_ENV: &[&str] = &[
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "NO_PROXY",
    "SSL_CERT_DIR",
    "SSL_CERT_FILE",
];

pub(super) fn discover_provider() -> Result<PathBuf, String> {
    let current_exe = std::env::current_exe()
        .map_err(|error| format!("failed to locate the active Fallow executable: {error}"))?;
    discover_provider_from(&current_exe)
}

#[expect(
    clippy::filetype_is_file,
    reason = "provider discovery accepts regular files only and rejects every special file type"
)]
fn discover_provider_from(current_exe: &Path) -> Result<PathBuf, String> {
    let current_exe = dunce::canonicalize(current_exe).map_err(|error| {
        format!(
            "failed to resolve the active Fallow executable {}: {error}",
            current_exe.display()
        )
    })?;
    let install_dir = current_exe.parent().ok_or_else(|| {
        format!(
            "the active Fallow executable {} has no install directory",
            current_exe.display()
        )
    })?;
    for name in binary_names(SIDECAR_BINARY) {
        let candidate = install_dir.join(name);
        let Ok(metadata) = candidate.symlink_metadata() else {
            continue;
        };
        if !metadata.file_type().is_file() {
            continue;
        }
        let Ok(canonical) = dunce::canonicalize(candidate) else {
            continue;
        };
        if canonical.parent() == Some(install_dir) {
            return Ok(canonical);
        }
    }
    Err(format!(
        "Similar-code companion `{SIDECAR_BINARY}` was not found next to the active Fallow executable. Install the exact-version `fallow-similar-code` package, then run `fallow similar-code status`. Project-local executables and PATH are intentionally not searched."
    ))
}

fn binary_names(binary: &str) -> Vec<String> {
    if cfg!(windows) {
        vec![binary.to_owned(), format!("{binary}.exe")]
    } else {
        vec![binary.to_owned()]
    }
}

pub(super) fn provider_status(sidecar: &Path) -> Result<SimilarCodeProviderStatus, String> {
    run_command_json(
        sidecar,
        &["status", "--json"],
        false,
        STATUS_COMMAND_TIMEOUT,
    )
}

pub(super) fn setup_provider(sidecar: &Path) -> Result<SimilarCodeProviderStatus, String> {
    run_command_json(
        sidecar,
        &["setup", "--local", "--json"],
        true,
        SETUP_COMMAND_TIMEOUT,
    )
}

fn run_command_json<Response: DeserializeOwned>(
    sidecar: &Path,
    args: &[&str],
    allow_download: bool,
    command_timeout: Duration,
) -> Result<Response, String> {
    let mut command = provider_command(sidecar, allow_download)?;
    command.args(args).stdin(Stdio::null());
    let mut child = fallow_process::ScopedChild::spawn_process_tree(&mut command)
        .map_err(|error| format!("failed to spawn {}: {error}", sidecar.display()))?;
    let terminator = child
        .process_tree_terminator()
        .ok_or_else(|| "similar-code provider process tree was unavailable".to_owned())?;
    let stdout = child
        .take_stdout()
        .ok_or_else(|| "similar-code provider stdout was unavailable".to_owned())?;
    let stderr = child
        .take_stderr()
        .ok_or_else(|| "similar-code provider stderr was unavailable".to_owned())?;
    let stdout_terminator = terminator.clone();
    let stdout_reader = std::thread::spawn(move || {
        read_bounded(stdout, MAX_RESPONSE_BYTES, "stdout", stdout_terminator)
    });
    let stderr_terminator = terminator.clone();
    let stderr_reader = std::thread::spawn(move || {
        read_bounded(stderr, MAX_STDERR_BYTES, "stderr", stderr_terminator)
    });
    let timeout = ProcessTimeout::start(terminator, command_timeout);
    let status = child
        .wait()
        .map_err(|error| format!("failed to wait for similar-code provider: {error}"));
    let timeout_result = timeout.finish();
    let stdout = join_reader("stdout", stdout_reader)?;
    let stderr = join_reader("stderr", stderr_reader)?;
    timeout_result?;
    let status = status?;
    if !status.success() {
        return Err(provider_exit_error(&status.to_string(), &stdout, &stderr));
    }
    serde_json::from_slice(&stdout)
        .map_err(|error| format!("failed to parse similar-code provider response: {error}"))
}

pub(super) struct ProviderSession {
    child: Option<fallow_process::ScopedChild>,
    stdin: std::process::ChildStdin,
    stdout: BufReader<std::process::ChildStdout>,
    terminator: fallow_process::ProcessTreeTerminator,
    stderr_reader: Option<std::thread::JoinHandle<Result<Vec<u8>, String>>>,
}

impl ProviderSession {
    pub(super) fn spawn(sidecar: &Path) -> Result<Self, String> {
        let mut command = provider_command(sidecar, false)?;
        command.arg("serve");
        let mut child = fallow_process::ScopedChild::spawn_process_tree(&mut command)
            .map_err(|error| format!("failed to spawn {}: {error}", sidecar.display()))?;
        let terminator = child
            .process_tree_terminator()
            .ok_or_else(|| "similar-code provider process tree was unavailable".to_owned())?;
        let stdin = child
            .take_stdin()
            .ok_or_else(|| "similar-code provider stdin was unavailable".to_owned())?;
        let stdout = child
            .take_stdout()
            .ok_or_else(|| "similar-code provider stdout was unavailable".to_owned())?;
        let stderr = child
            .take_stderr()
            .ok_or_else(|| "similar-code provider stderr was unavailable".to_owned())?;
        let stderr_terminator = terminator.clone();
        let stderr_reader = std::thread::spawn(move || {
            read_bounded(stderr, MAX_STDERR_BYTES, "stderr", stderr_terminator)
        });
        Ok(Self {
            child: Some(child),
            stdin,
            stdout: BufReader::new(stdout),
            terminator,
            stderr_reader: Some(stderr_reader),
        })
    }

    pub(super) fn embed(
        &mut self,
        functions: &[(u32, &str)],
    ) -> Result<EmbedBatchResponse, String> {
        self.embed_with_timeout(functions, REQUEST_TIMEOUT)
    }

    fn embed_with_timeout(
        &mut self,
        functions: &[(u32, &str)],
        request_timeout: Duration,
    ) -> Result<EmbedBatchResponse, String> {
        let functions = functions
            .iter()
            .map(|(key, source)| EmbedFunctionRequest { key: *key, source })
            .collect::<Vec<_>>();
        let request = EmbedBatchRequest {
            operation: ANALYSIS_OPERATION,
            protocol_version: WIRE_PROTOCOL_VERSION,
            embedding_semantics_version: EMBEDDING_SEMANTICS_VERSION,
            model_revision: MODEL_REVISION,
            dimensions: MODEL_DIMENSIONS,
            max_tokens: MODEL_MAX_TOKENS,
            functions: &functions,
        };
        let mut bytes = serde_json::to_vec(&request)
            .map_err(|error| format!("failed to serialize similar-code request: {error}"))?;
        if bytes.len() > MAX_REQUEST_BYTES {
            return Err(format!(
                "similar-code request exceeded the {MAX_REQUEST_BYTES}-byte limit"
            ));
        }
        bytes.push(b'\n');
        let response_bytes = self.exchange(&bytes, request_timeout)?;
        if response_bytes.is_empty() {
            return Err("similar-code provider closed without a response".to_owned());
        }
        if response_bytes.len() > MAX_RESPONSE_BYTES {
            return Err(format!(
                "similar-code response exceeded the {MAX_RESPONSE_BYTES}-byte limit"
            ));
        }
        let response: EmbedBatchResponse = serde_json::from_slice(&response_bytes)
            .map_err(|error| format!("failed to parse similar-code response: {error}"))?;
        validate_embed_response(&response, &functions)?;
        Ok(response)
    }

    fn exchange(&mut self, bytes: &[u8], request_timeout: Duration) -> Result<Vec<u8>, String> {
        let timeout = ProcessTimeout::start(self.terminator.clone(), request_timeout);
        let mut response_bytes = Vec::new();
        let io_result = self
            .stdin
            .write_all(bytes)
            .and_then(|()| self.stdin.flush())
            .map_err(|error| format!("failed to write similar-code request: {error}"))
            .and_then(|()| {
                self.stdout
                    .by_ref()
                    .take((MAX_RESPONSE_BYTES + 1) as u64)
                    .read_until(b'\n', &mut response_bytes)
                    .map_err(|error| format!("failed to read similar-code response: {error}"))
            });
        let timeout_result = timeout.finish();
        if let Err(error) = timeout_result {
            self.terminate();
            return Err(error);
        }
        if let Err(error) = io_result {
            self.terminate();
            return Err(error);
        }
        Ok(response_bytes)
    }

    fn terminate(&mut self) {
        let _ = self.terminator.terminate();
        if let Some(child) = self.child.take() {
            let _ = child.wait();
        }
        if let Some(reader) = self.stderr_reader.take() {
            let _ = join_reader("stderr", reader);
        }
    }
}

impl Drop for ProviderSession {
    fn drop(&mut self) {
        let _ = self.stdin.write_all(b"{\"operation\":\"shutdown\"}\n");
        let _ = self.stdin.flush();
        self.terminate();
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "the protocol validator keeps all envelope invariants in one fail-closed boundary"
)]
fn validate_embed_response(
    response: &EmbedBatchResponse,
    request: &[EmbedFunctionRequest<'_>],
) -> Result<(), String> {
    if response.protocol_version != WIRE_PROTOCOL_VERSION
        || response.embedding_semantics_version != EMBEDDING_SEMANTICS_VERSION
        || response.model_revision != MODEL_REVISION
        || response.dimensions != MODEL_DIMENSIONS
    {
        return Err("similar-code provider identity mismatch".to_owned());
    }
    if !response.timing.inference_ms.is_finite() || response.timing.inference_ms < 0.0 {
        return Err("similar-code provider returned invalid timing".to_owned());
    }
    let mut expected = request
        .iter()
        .map(|function| function.key)
        .collect::<Vec<_>>();
    let mut actual = response
        .vectors
        .iter()
        .map(|vector| vector.key)
        .collect::<Vec<_>>();
    expected.sort_unstable();
    actual.sort_unstable();
    if actual.windows(2).any(|pair| pair[0] == pair[1])
        || actual
            .iter()
            .any(|key| expected.binary_search(key).is_err())
    {
        return Err("similar-code provider returned duplicate or unknown vector keys".to_owned());
    }
    if response.vectors.iter().any(|vector| {
        vector.values.len() != MODEL_DIMENSIONS
            || vector.values.iter().any(|value| !value.is_finite())
    }) {
        return Err("similar-code provider returned an invalid vector".to_owned());
    }
    let completion = &response.completion;
    if completion.requested_functions != request.len()
        || completion.embedded_functions != response.vectors.len()
        || completion.skipped_functions
            != completion
                .requested_functions
                .saturating_sub(completion.embedded_functions)
        || completion.truncated_functions
            != response
                .vectors
                .iter()
                .filter(|vector| vector.truncated)
                .count()
        || completion.applied_limits.max_tokens != MODEL_MAX_TOKENS
        || completion.applied_limits.batch_size == 0
        || completion.applied_limits.max_functions == 0
        || completion.applied_limits.max_total_source_bytes == 0
        || completion.applied_limits.max_source_bytes_per_function == 0
        || completion.applied_limits.timeout_ms == 0
    {
        return Err("similar-code provider returned inconsistent completion accounting".to_owned());
    }
    let mut error_keys = response
        .errors
        .iter()
        .filter_map(|error| error.key)
        .collect::<Vec<_>>();
    error_keys.sort_unstable();
    if error_keys.windows(2).any(|pair| pair[0] == pair[1])
        || error_keys
            .iter()
            .any(|key| expected.binary_search(key).is_err() || actual.binary_search(key).is_ok())
        || response.errors.iter().any(|error| {
            error.retryable
                || error.message.as_ref().is_some_and(|message| {
                    message.chars().count() > 2_000 || message.chars().any(char::is_control)
                })
                || error
                    .observed
                    .zip(error.limit)
                    .is_some_and(|(observed, limit)| {
                        observed <= limit
                            && matches!(
                                error.code,
                                EmbedErrorCode::FunctionLimit
                                    | EmbedErrorCode::TotalSourceBytesLimit
                                    | EmbedErrorCode::FunctionSourceBytesLimit
                                    | EmbedErrorCode::RequestTooLarge
                            )
                    })
        })
    {
        return Err("similar-code provider returned invalid error accounting".to_owned());
    }
    match response.status {
        EmbedCompletionStatus::Complete
            if response.vectors.len() == request.len()
                && response.errors.is_empty()
                && completion.skipped_functions == 0 => {}
        EmbedCompletionStatus::Partial
            if !response.vectors.is_empty()
                && response.vectors.len() < request.len()
                && !response.errors.is_empty() => {}
        EmbedCompletionStatus::Error
            if response.vectors.is_empty()
                && completion.skipped_functions == request.len()
                && !response.errors.is_empty() => {}
        _ => {
            return Err("similar-code provider returned an invalid completion state".to_owned());
        }
    }
    Ok(())
}

pub(super) fn embed_problem(response: &EmbedBatchResponse) -> String {
    let codes = response
        .errors
        .iter()
        .map(|error| match error.code {
            EmbedErrorCode::InvalidRequest => "invalid-request",
            EmbedErrorCode::ProtocolMismatch => "protocol-mismatch",
            EmbedErrorCode::EmbeddingSemanticsMismatch => "embedding-semantics-mismatch",
            EmbedErrorCode::ModelRevisionMismatch => "model-revision-mismatch",
            EmbedErrorCode::DimensionMismatch => "dimension-mismatch",
            EmbedErrorCode::MaxTokensMismatch => "max-tokens-mismatch",
            EmbedErrorCode::DuplicateFunctionKey => "duplicate-function-key",
            EmbedErrorCode::FunctionLimit => "function-limit",
            EmbedErrorCode::TotalSourceBytesLimit => "total-source-bytes-limit",
            EmbedErrorCode::FunctionSourceBytesLimit => "function-source-bytes-limit",
            EmbedErrorCode::Timeout => "timeout",
            EmbedErrorCode::ModelNotReady => "model-not-ready",
            EmbedErrorCode::InferenceFailed => "inference-failed",
            EmbedErrorCode::RequestTooLarge => "request-too-large",
        })
        .collect::<Vec<_>>()
        .join(", ");
    let messages = response
        .errors
        .iter()
        .filter_map(|error| error.message.as_deref())
        .collect::<Vec<_>>()
        .join("; ");
    if messages.is_empty() {
        format!(
            "similar-code provider returned {:?}: {codes}",
            response.status
        )
    } else {
        format!(
            "similar-code provider returned {:?}: {codes}: {messages}",
            response.status
        )
    }
}

fn provider_command(sidecar: &Path, allow_download: bool) -> Result<Command, String> {
    let sidecar = dunce::canonicalize(sidecar).map_err(|error| {
        format!(
            "failed to resolve similar-code companion {}: {error}",
            sidecar.display()
        )
    })?;
    let install_dir = sidecar.parent().ok_or_else(|| {
        format!(
            "similar-code companion {} has no install directory",
            sidecar.display()
        )
    })?;
    let mut command = Command::new(&sidecar);
    command
        .current_dir(install_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if !allow_download {
        command.stdin(Stdio::piped());
    }
    restrict_environment(&mut command, allow_download);
    Ok(command)
}

fn restrict_environment(command: &mut Command, allow_download: bool) {
    let values = BASE_ENV
        .iter()
        .chain(allow_download.then_some(DOWNLOAD_ENV).into_iter().flatten())
        .filter_map(|key| std::env::var_os(key).map(|value| (*key, value)))
        .collect::<Vec<_>>();
    command.env_clear().envs(values);
    if !allow_download {
        command.env("FALLOW_SIMILAR_CODE_OFFLINE", "1");
    }
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "the reader thread owns its process-tree terminator"
)]
fn read_bounded(
    reader: impl Read,
    limit: usize,
    stream: &str,
    terminator: fallow_process::ProcessTreeTerminator,
) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::with_capacity(limit.min(16 * 1024));
    reader
        .take((limit + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("failed to read similar-code {stream}: {error}"))?;
    if bytes.len() > limit {
        let _ = terminator.terminate();
        return Err(format!(
            "similar-code provider {stream} exceeded the {limit}-byte limit"
        ));
    }
    Ok(bytes)
}

fn join_reader(
    stream: &str,
    reader: std::thread::JoinHandle<Result<Vec<u8>, String>>,
) -> Result<Vec<u8>, String> {
    reader
        .join()
        .map_err(|_| format!("similar-code provider {stream} reader panicked"))?
}

#[derive(Deserialize)]
struct SidecarErrorEnvelope {
    protocol_version: u32,
    kind: String,
    error: SidecarErrorDetail,
}

#[derive(Deserialize)]
struct SidecarErrorDetail {
    code: String,
    message: String,
}

fn provider_exit_error(status: &str, stdout: &[u8], stderr: &[u8]) -> String {
    if let Ok(envelope) = serde_json::from_slice::<SidecarErrorEnvelope>(stdout)
        && envelope.protocol_version == WIRE_PROTOCOL_VERSION
        && envelope.kind == "similar-code-sidecar-error"
        && !envelope.error.message.trim().is_empty()
    {
        return format!(
            "similar-code provider exited with status {status} ({}): {}",
            envelope.error.code, envelope.error.message
        );
    }
    let detail = String::from_utf8_lossy(stderr)
        .chars()
        .take(2_000)
        .collect::<String>()
        .trim()
        .to_owned();
    if detail.is_empty() {
        format!("similar-code provider exited with status {status}")
    } else {
        format!("similar-code provider exited with status {status}: {detail}")
    }
}

struct ProcessTimeout {
    done: mpsc::Sender<()>,
    timed_out: std::sync::Arc<std::sync::atomic::AtomicBool>,
    watcher: std::thread::JoinHandle<()>,
    duration: Duration,
}

impl ProcessTimeout {
    fn start(terminator: fallow_process::ProcessTreeTerminator, duration: Duration) -> Self {
        let timed_out = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let watcher_timed_out = std::sync::Arc::clone(&timed_out);
        let (done, receiver) = mpsc::channel();
        let watcher = std::thread::spawn(move || match receiver.recv_timeout(duration) {
            Err(mpsc::RecvTimeoutError::Timeout) => {
                watcher_timed_out.store(true, std::sync::atomic::Ordering::Release);
                let _ = terminator.terminate();
            }
            Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => {}
        });
        Self {
            done,
            timed_out,
            watcher,
            duration,
        }
    }

    fn finish(self) -> Result<(), String> {
        let _ = self.done.send(());
        let _ = self.watcher.join();
        if self.timed_out.load(std::sync::atomic::Ordering::Acquire) {
            return Err(format!(
                "similar-code provider timed out after {} seconds",
                self.duration.as_secs_f64()
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    reason = "test fixture construction must fail immediately"
)]
mod tests {
    use super::*;

    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn discovery_only_accepts_a_sibling_file() {
        let temp = tempfile::tempdir().unwrap();
        let fallow = temp.path().join(if cfg!(windows) {
            "fallow.exe"
        } else {
            "fallow"
        });
        std::fs::write(&fallow, b"").unwrap();
        let error = discover_provider_from(&fallow).unwrap_err();
        assert!(error.contains("not found next to"));

        let sidecar = temp.path().join(if cfg!(windows) {
            "fallow-similar-code.exe"
        } else {
            "fallow-similar-code"
        });
        std::fs::write(&sidecar, b"").unwrap();
        assert_eq!(
            discover_provider_from(&fallow).unwrap(),
            dunce::canonicalize(sidecar).unwrap()
        );
    }

    #[test]
    fn provider_environment_excludes_project_and_provider_overrides() {
        let mut command = Command::new("fallow-similar-code");
        restrict_environment(&mut command, false);
        let debug = format!("{command:?}");
        assert!(debug.contains("FALLOW_SIMILAR_CODE_OFFLINE"));
        assert!(!debug.contains("FALLOW_SIMILAR_CODE_BIN"));
        assert!(!debug.contains("PATH="));
        assert!(BASE_ENV.contains(&"FALLOW_SIMILAR_CODE_CACHE_DIR"));
        assert!(BASE_ENV.contains(&"LOCALAPPDATA"));
        assert!(BASE_ENV.contains(&"XDG_CACHE_HOME"));
    }

    #[test]
    fn setup_timeout_matches_the_direct_wrapper_budget() {
        assert_eq!(STATUS_COMMAND_TIMEOUT, Duration::from_mins(2));
        assert_eq!(SETUP_COMMAND_TIMEOUT, Duration::from_mins(55));
    }

    #[cfg(unix)]
    #[test]
    fn request_timeout_bounds_blocked_provider_stdin_and_reaps_process() {
        const SERIALIZATION_HEADROOM_BYTES: usize = 1024;

        let temp = tempfile::tempdir().unwrap();
        let sidecar = temp.path().join("fallow-similar-code");
        std::fs::write(&sidecar, "#!/bin/sh\nsleep 5\n").unwrap();
        let mut permissions = std::fs::metadata(&sidecar).unwrap().permissions();
        permissions.set_mode(0o700);
        std::fs::set_permissions(&sidecar, permissions).unwrap();

        let mut session = ProviderSession::spawn(&sidecar).unwrap();
        let source = "x".repeat(MAX_REQUEST_BYTES - SERIALIZATION_HEADROOM_BYTES);
        let error = session
            .embed_with_timeout(&[(0, &source)], Duration::from_millis(250))
            .unwrap_err();

        assert!(error.contains("timed out after 0.25 seconds"), "{error}");
        assert!(session.child.is_none(), "timed-out provider was not reaped");
    }

    #[test]
    fn provider_exit_prefers_the_typed_sidecar_error() {
        let stdout = br#"{"protocol_version":2,"kind":"similar-code-sidecar-error","error":{"code":"sidecar-error","message":"response header timed out","retryable":false}}"#;

        assert_eq!(
            provider_exit_error("exit status: 2", stdout, b"generic stderr"),
            "similar-code provider exited with status exit status: 2 (sidecar-error): response header timed out"
        );
    }
}
