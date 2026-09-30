//! Shared HTTP transport for the Fallow Cloud read commands.
//!
//! `coverage analyze --cloud`, `coverage review-packet` and
//! `coverage deployment-changes` all send one authenticated JSON request and
//! read one JSON answer. This module owns the parts they share:
//!
//! - `Accept-Encoding: gzip`, with the body decoded here through `flate2`.
//!   When the cloud compresses its answer, the large runtime-context answer
//!   becomes much smaller on the network. An identity answer also works.
//!   The `ureq` `gzip` feature is not used, because it changes the behavior of
//!   every other `ureq` client in the build.
//! - One retry on HTTP 502, 503 and 504, with the delay from
//!   [`crate::api::retry_delay_for_status`], and one retry on a timeout or
//!   on a socket call that a signal interrupted (EINTR). A cold cloud read
//!   can pass the gateway timeout one time and succeed on the next call. Each
//!   call here is a read, so a second attempt is safe.
//! - The `x-fallow-agent-source` attribution header, sent only with a value
//!   from the cloud allowlist.
//! - Error messages that name the cause: a timeout, a cloud outage (5xx) or a
//!   network that cannot reach the cloud.

use std::io::Read;
use std::time::{Duration, SystemTime};

use flate2::read::GzDecoder;

use super::cloud_client::CloudError;
use crate::api::{
    parse_error_envelope, retry_delay_for_status, sanitize_network_error,
    try_api_agent_with_timeout,
};

/// Connect timeout for a cloud read.
pub const CLOUD_CONNECT_TIMEOUT_SECS: u64 = 5;
/// Total timeout for one attempt of a cloud read.
///
/// The Fly proxy answers a cold read that takes too long with HTTP 502 after
/// about 30 s. This limit is longer, so that 502 arrives and gets the retry.
/// Two attempts plus the retry delay stay below the 120 s limit that the MCP
/// server sets on the CLI subprocess.
pub const CLOUD_TOTAL_TIMEOUT_SECS: u64 = 45;
/// Header that tells the cloud which coding agent sent the read.
pub const AGENT_SOURCE_HEADER: &str = "x-fallow-agent-source";
/// Upper limit on the bytes of a response body as sent.
const MAX_WIRE_BODY_BYTES: u64 = 64 * 1024 * 1024;
/// Upper limit on the bytes of a response body after gzip decoding.
const MAX_DECODED_BODY_BYTES: u64 = 256 * 1024 * 1024;
/// Number of attempts for a status that [`is_retryable_status`] accepts.
const MAX_ATTEMPTS: u8 = 2;
/// Upper limit on the delay before the second attempt.
const MAX_RETRY_DELAY_SECS: u64 = 2;

/// Agent-source values that the cloud accepts on `x-fallow-agent-source`.
/// The cloud ignores any other value, so the CLI does not send one.
const CLOUD_AGENT_SOURCES: &[&str] = &[
    "claude_code",
    "codex",
    "cursor",
    "windsurf",
    "gemini",
    "cline",
];

/// Authentication and attribution for one cloud read.
#[derive(Clone, Default)]
pub struct CloudAuth {
    pub api_key: String,
    /// Allowlisted agent source, or `None` for a read with no agent.
    pub agent_source: Option<String>,
}

impl std::fmt::Debug for CloudAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CloudAuth")
            .field("api_key", &"***")
            .field("agent_source", &self.agent_source)
            .finish()
    }
}

/// The agent source for this process, reduced to the cloud allowlist.
///
/// The value comes from the same detection that anonymous telemetry uses
/// (`FALLOW_AGENT_SOURCE`, then the environment of the agent). The MCP server
/// runs the CLI as a child process, so a call through MCP carries the agent
/// that runs the MCP server.
pub fn detected_agent_source() -> Option<String> {
    crate::telemetry::agent_source_wire_value().and_then(allowlisted_agent_source)
}

/// Keep a value only when the cloud allowlist contains it.
pub fn allowlisted_agent_source(value: &str) -> Option<String> {
    let trimmed = value.trim();
    CLOUD_AGENT_SOURCES
        .contains(&trimmed)
        .then(|| trimmed.to_owned())
}

/// A request body for a cloud read.
pub enum CloudBody<'a> {
    /// A `GET` request with no body.
    None,
    /// A `POST` request with a JSON body.
    Json(&'a serde_json::Value),
}

/// A successful cloud answer: the decoded body text.
#[derive(Debug)]
pub struct CloudResponse {
    pub body: String,
}

/// A failed cloud answer that reached the server.
#[derive(Debug)]
pub struct CloudHttpFailure {
    pub status: u16,
    pub code: Option<String>,
    pub message: String,
}

/// Result of [`send`]: the answer, a failed HTTP answer, or a transport error.
pub enum CloudOutcome {
    Success(CloudResponse),
    Http(CloudHttpFailure),
}

/// Send one cloud read with gzip, one retry on 502/503/504, and the
/// agent-source header.
///
/// `operation` names the read in error messages, for example
/// `"runtime-context"`.
pub fn send(
    auth: &CloudAuth,
    url: &str,
    body: &CloudBody<'_>,
    operation: &str,
) -> Result<CloudOutcome, CloudError> {
    send_with_timing(auth, url, body, operation, CloudTiming::DEFAULT)
}

/// Timeouts and retry delay for [`send_with_timing`].
#[derive(Clone, Copy)]
struct CloudTiming {
    total_timeout_secs: u64,
    max_retry_delay: Duration,
}

impl CloudTiming {
    const DEFAULT: Self = Self {
        total_timeout_secs: CLOUD_TOTAL_TIMEOUT_SECS,
        max_retry_delay: Duration::from_secs(MAX_RETRY_DELAY_SECS),
    };
}

fn send_with_timing(
    auth: &CloudAuth,
    url: &str,
    body: &CloudBody<'_>,
    operation: &str,
    timing: CloudTiming,
) -> Result<CloudOutcome, CloudError> {
    let agent =
        try_api_agent_with_timeout(CLOUD_CONNECT_TIMEOUT_SECS, timing.total_timeout_secs)
            .map_err(|err| CloudError::Network(unreachable_message(operation, &err.to_string())))?;
    run_attempts(operation, timing, || {
        send_once(&agent, auth, url, body, operation)
    })
}

/// Run `attempt_once` until it gives an answer, with one more attempt after a
/// timeout or a 502/503/504 answer.
fn run_attempts(
    operation: &str,
    timing: CloudTiming,
    mut attempt_once: impl FnMut() -> Result<RawResponse, AttemptError>,
) -> Result<CloudOutcome, CloudError> {
    let mut attempt: u8 = 1;
    loop {
        let (status, retry_after, bytes, gzip) = match attempt_once() {
            Ok(raw) => raw,
            Err(AttemptError::Timeout) if attempt < MAX_ATTEMPTS => {
                std::thread::sleep(timing.max_retry_delay);
                attempt += 1;
                continue;
            }
            Err(AttemptError::Timeout) => {
                return Err(CloudError::Network(timeout_message(
                    operation,
                    timing.total_timeout_secs,
                    attempt,
                )));
            }
            Err(AttemptError::Interrupted(_)) if attempt < MAX_ATTEMPTS => {
                attempt += 1;
                continue;
            }
            Err(AttemptError::Failed(err) | AttemptError::Interrupted(err)) => return Err(err),
        };
        if is_retryable_status(status) && attempt < MAX_ATTEMPTS {
            let delay =
                retry_delay_for_status(status, retry_after.as_deref(), attempt, SystemTime::now());
            std::thread::sleep(delay.min(timing.max_retry_delay));
            attempt += 1;
            continue;
        }
        let text = decode_body(&bytes, gzip, operation)?;
        if (200..300).contains(&status) {
            return Ok(CloudOutcome::Success(CloudResponse { body: text }));
        }
        if (500..600).contains(&status) {
            return Err(CloudError::Network(outage_message(
                operation, status, attempt,
            )));
        }
        let envelope = parse_error_envelope(&text);
        let message = envelope
            .message()
            .filter(|message| !message.trim().is_empty())
            .unwrap_or_else(|| text.trim())
            .to_owned();
        return Ok(CloudOutcome::Http(CloudHttpFailure {
            status,
            code: envelope.code().map(str::to_owned),
            message,
        }));
    }
}

/// Status codes that get one more attempt.
const fn is_retryable_status(status: u16) -> bool {
    matches!(status, 502..=504)
}

type RawResponse = (u16, Option<String>, Vec<u8>, bool);

/// Why one attempt failed before a full answer arrived.
#[derive(Debug)]
enum AttemptError {
    /// The attempt passed the total timeout. It gets one more attempt.
    Timeout,
    /// A signal, or a stop and continue of the process, interrupted a socket
    /// call (EINTR). The partial answer is lost. The error gets one more
    /// attempt and is the result when the last attempt is interrupted.
    Interrupted(CloudError),
    /// Any other transport error. It gets no more attempts.
    Failed(CloudError),
}

fn send_once(
    agent: &ureq::Agent,
    auth: &CloudAuth,
    url: &str,
    body: &CloudBody<'_>,
    operation: &str,
) -> Result<RawResponse, AttemptError> {
    let bearer = format!("Bearer {}", auth.api_key);
    let result = match body {
        CloudBody::None => {
            let mut request = agent
                .get(url)
                .header("Authorization", &bearer)
                .header("Accept", "application/json")
                .header("Accept-Encoding", "gzip");
            if let Some(source) = auth.agent_source.as_deref() {
                request = request.header(AGENT_SOURCE_HEADER, source);
            }
            request.call()
        }
        CloudBody::Json(value) => {
            let mut request = agent
                .post(url)
                .header("Authorization", &bearer)
                .header("Accept", "application/json")
                .header("Accept-Encoding", "gzip");
            if let Some(source) = auth.agent_source.as_deref() {
                request = request.header(AGENT_SOURCE_HEADER, source);
            }
            request.send_json(value)
        }
    };
    let mut response = result.map_err(|err| transport_error(&err, operation))?;
    let status = response.status().as_u16();
    let retry_after = header_value(&response, "retry-after");
    let gzip = header_value(&response, "content-encoding")
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("gzip"));
    let bytes = response
        .body_mut()
        .with_config()
        .limit(MAX_WIRE_BODY_BYTES)
        .read_to_vec()
        .map_err(|err| transport_error(&err, operation))?;
    Ok((status, retry_after, bytes, gzip))
}

fn header_value(response: &ureq::http::Response<ureq::Body>, name: &str) -> Option<String> {
    response
        .headers()
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
}

/// Decode a response body. A gzip body is decoded with an upper size limit, so
/// a malformed or hostile answer cannot fill the memory.
fn decode_body(bytes: &[u8], gzip: bool, operation: &str) -> Result<String, CloudError> {
    if !gzip {
        return String::from_utf8(bytes.to_vec()).map_err(|err| {
            CloudError::Server(format!("{operation} response is not UTF-8: {err}"))
        });
    }
    let mut decoded = String::new();
    GzDecoder::new(bytes)
        .take(MAX_DECODED_BODY_BYTES)
        .read_to_string(&mut decoded)
        .map_err(|err| {
            CloudError::Server(format!("{operation} response has a bad gzip body: {err}"))
        })?;
    Ok(decoded)
}

/// Classify a transport error as a timeout, an interrupted call or a network
/// that cannot reach the cloud.
fn transport_error(err: &ureq::Error, operation: &str) -> AttemptError {
    let io_kind = match err {
        ureq::Error::Timeout(_) => return AttemptError::Timeout,
        ureq::Error::Io(io) => Some(io.kind()),
        _ => None,
    };
    if io_kind == Some(std::io::ErrorKind::TimedOut) {
        return AttemptError::Timeout;
    }
    let network = CloudError::Network(unreachable_message(
        operation,
        &sanitize_network_error(&err.to_string()),
    ));
    if io_kind == Some(std::io::ErrorKind::Interrupted) {
        return AttemptError::Interrupted(network);
    }
    AttemptError::Failed(network)
}

/// Message for a read that passed the total timeout on each attempt.
pub fn timeout_message(operation: &str, timeout_secs: u64, attempts: u8) -> String {
    let tries = if attempts > 1 {
        format!(" on {attempts} attempts")
    } else {
        String::new()
    };
    format!(
        "fallow.cloud did not answer the {operation} request in {timeout_secs} s{tries} (timeout).\n\nThe first read after a deploy can be slow while the cloud loads the data. Run the command again in a minute."
    )
}

/// Message for a cloud that answered 5xx on each attempt.
pub fn outage_message(operation: &str, status: u16, attempts: u8) -> String {
    let tries = if attempts > 1 {
        format!(" on {attempts} attempts")
    } else {
        String::new()
    };
    format!(
        "fallow.cloud answered the {operation} request with HTTP {status}{tries} (cloud outage).\n\nThe cloud is reachable but cannot serve the request now. Run the command again later."
    )
}

/// Message for a network that cannot reach the cloud.
pub fn unreachable_message(operation: &str, detail: &str) -> String {
    let suffix = if detail.trim().is_empty() {
        String::new()
    } else {
        format!(" ({})", detail.trim())
    };
    format!(
        "Could not reach fallow.cloud for the {operation} request{suffix} (network unreachable).\n\nCheck the network connection, the proxy settings and FALLOW_API_URL."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowlist_keeps_only_cloud_values() {
        assert_eq!(
            allowlisted_agent_source(" claude_code "),
            Some("claude_code".to_owned())
        );
        assert_eq!(allowlisted_agent_source("codex"), Some("codex".to_owned()));
        assert_eq!(allowlisted_agent_source("copilot"), None);
        assert_eq!(allowlisted_agent_source("none"), None);
        assert_eq!(allowlisted_agent_source(""), None);
    }

    #[test]
    fn retry_covers_only_gateway_statuses() {
        assert!(is_retryable_status(502));
        assert!(is_retryable_status(503));
        assert!(is_retryable_status(504));
        assert!(!is_retryable_status(500));
        assert!(!is_retryable_status(429));
    }

    #[test]
    fn error_messages_name_the_cause() {
        assert!(timeout_message("runtime-context", 45, 2).contains("(timeout)"));
        assert!(timeout_message("runtime-context", 45, 2).contains("in 45 s on 2 attempts"));
        assert!(outage_message("runtime-context", 502, 2).contains("(cloud outage)"));
        assert!(outage_message("runtime-context", 502, 2).contains("on 2 attempts"));
        assert!(
            unreachable_message("runtime-context", "refused").contains("(network unreachable)")
        );
    }

    #[test]
    fn auth_debug_masks_the_key() {
        let auth = CloudAuth {
            api_key: "fallow_live_secret".to_owned(),
            agent_source: None,
        };
        assert!(!format!("{auth:?}").contains("fallow_live_secret"));
    }

    fn fast_timing() -> CloudTiming {
        CloudTiming {
            total_timeout_secs: 1,
            max_retry_delay: Duration::from_millis(10),
        }
    }

    fn ok_answer() -> RawResponse {
        (200, None, b"{\"ok\":true}".to_vec(), false)
    }

    #[test]
    fn a_timed_out_attempt_gets_one_more_attempt() {
        let mut attempts = 0;
        let outcome = run_attempts("runtime-context", fast_timing(), || {
            attempts += 1;
            if attempts == 1 {
                Err(AttemptError::Timeout)
            } else {
                Ok(ok_answer())
            }
        });
        assert_eq!(attempts, 2);
        match outcome {
            Ok(CloudOutcome::Success(response)) => assert_eq!(response.body, "{\"ok\":true}"),
            Ok(CloudOutcome::Http(failure)) => panic!("unexpected HTTP failure: {failure:?}"),
            Err(err) => panic!("unexpected error: {err:?}"),
        }
    }

    #[test]
    fn a_second_timeout_ends_the_read() {
        let mut attempts = 0;
        let outcome = run_attempts("runtime-context", fast_timing(), || {
            attempts += 1;
            Err(AttemptError::Timeout)
        });
        assert_eq!(attempts, 2);
        match outcome {
            Err(CloudError::Network(message)) => {
                assert!(
                    message.contains("in 1 s on 2 attempts (timeout)"),
                    "{message}"
                );
            }
            Err(err) => panic!("unexpected error: {err:?}"),
            Ok(_) => panic!("a read with no answer must fail"),
        }
    }

    #[test]
    fn a_network_failure_gets_no_more_attempts() {
        let mut attempts = 0;
        let outcome = run_attempts("runtime-context", fast_timing(), || {
            attempts += 1;
            Err(AttemptError::Failed(CloudError::Network(
                "refused".to_owned(),
            )))
        });
        assert_eq!(attempts, 1);
        assert!(matches!(outcome, Err(CloudError::Network(message)) if message == "refused"));
    }

    #[test]
    fn an_interrupted_read_gets_one_more_attempt() {
        let mut attempts = 0;
        let outcome = run_attempts("runtime-context", fast_timing(), || {
            attempts += 1;
            if attempts == 1 {
                Err(AttemptError::Interrupted(CloudError::Network(
                    "interrupted".to_owned(),
                )))
            } else {
                Ok(ok_answer())
            }
        });
        assert_eq!(attempts, 2);
        assert!(matches!(outcome, Ok(CloudOutcome::Success(_))));
    }

    #[test]
    fn a_second_interruption_ends_the_read() {
        let mut attempts = 0;
        let outcome = run_attempts("runtime-context", fast_timing(), || {
            attempts += 1;
            Err(AttemptError::Interrupted(CloudError::Network(
                "interrupted".to_owned(),
            )))
        });
        assert_eq!(attempts, 2);
        assert!(matches!(outcome, Err(CloudError::Network(message)) if message == "interrupted"));
    }

    #[test]
    fn an_interrupted_system_call_is_classified_as_interrupted() {
        // Linux ends a socket read that has a receive timeout with EINTR when
        // a signal arrives, or when the process stops and continues, also
        // with SA_RESTART. ureq gives that error to the caller unchanged.
        let err = ureq::Error::Io(std::io::Error::from(std::io::ErrorKind::Interrupted));
        match transport_error(&err, "runtime-context") {
            AttemptError::Interrupted(CloudError::Network(message)) => {
                assert!(message.contains("runtime-context"), "{message}");
            }
            other => panic!("expected an interruption, got: {other:?}"),
        }
    }

    #[test]
    fn a_read_with_no_answer_is_a_timeout() {
        const MAX_TEST_INTERRUPTIONS: usize = 5;
        // The kernel accepts the connection into the backlog, but nothing
        // reads the request or writes an answer. The attempt must end as a
        // timeout, whatever phase the timeout reaches first.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind mock server");
        let url = format!("http://{}", listener.local_addr().expect("mock addr"));
        let agent = try_api_agent_with_timeout(CLOUD_CONNECT_TIMEOUT_SECS, 1).expect("agent");
        // A signal can interrupt the wait. That is a different result, so
        // try again until the timeout is the result.
        let mut result = send_once(
            &agent,
            &CloudAuth::default(),
            &url,
            &CloudBody::None,
            "runtime-context",
        );
        for _ in 0..MAX_TEST_INTERRUPTIONS {
            if !matches!(result, Err(AttemptError::Interrupted(_))) {
                break;
            }
            result = send_once(
                &agent,
                &CloudAuth::default(),
                &url,
                &CloudBody::None,
                "runtime-context",
            );
        }
        drop(listener);
        match result {
            Err(AttemptError::Timeout) => {}
            Err(AttemptError::Failed(err) | AttemptError::Interrupted(err)) => {
                panic!("expected a timeout, got: {err:?}")
            }
            Ok(raw) => panic!("expected a timeout, got an answer: {raw:?}"),
        }
    }

    #[test]
    fn per_attempt_timeout_waits_for_the_gateway_answer() {
        // The Fly proxy answers a slow cold read with 502 after about 30 s.
        // The attempt must wait for that answer, so the 502 retry can run.
        const GATEWAY_TIMEOUT_SECS: u64 = 32;
        const MCP_SUBPROCESS_TIMEOUT_SECS: u64 = 120;
        let default = CloudTiming::DEFAULT;
        assert!(default.total_timeout_secs > GATEWAY_TIMEOUT_SECS);
        let worst_case = u64::from(MAX_ATTEMPTS) * default.total_timeout_secs
            + default.max_retry_delay.as_secs();
        assert!(worst_case < MCP_SUBPROCESS_TIMEOUT_SECS);
    }

    #[test]
    fn decode_body_reads_gzip_and_plain_text() {
        use flate2::Compression;
        use flate2::write::GzEncoder;
        use std::io::Write;

        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(b"{\"ok\":true}").expect("write gzip");
        let compressed = encoder.finish().expect("finish gzip");
        assert_eq!(
            decode_body(&compressed, true, "test").expect("gzip decodes"),
            "{\"ok\":true}"
        );
        assert_eq!(
            decode_body(b"{\"ok\":true}", false, "test").expect("plain decodes"),
            "{\"ok\":true}"
        );
        assert!(matches!(
            decode_body(b"not gzip", true, "test"),
            Err(CloudError::Server(_))
        ));
    }
}
