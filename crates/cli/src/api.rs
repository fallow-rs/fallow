//! Shared HTTP layer for fallow-cloud backend calls.
//!
//! Provides a common `ureq::Agent` builder, URL resolution (respecting the
//! `FALLOW_API_URL` env override), typed error-envelope parsing, and an
//! actionable-hint mapper for backend error codes. Consumed by:
//!
//! - `license/`: trial activation, license refresh (5s connect, 10s total).
//! - `coverage/upload_inventory`: static inventory POST (5s connect, 30s total).
//!
//! The trait [`ResponseBodyReader`] decouples the status/body accessors from
//! `ureq::Response` so error-path code can be unit-tested with a lightweight
//! stub.

use std::fmt;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use serde::Deserialize;
use serde::de::DeserializeOwned;
use ureq::tls::{PemItem, RootCerts, TlsConfig};

/// Default fallow cloud API base URL.
pub const DEFAULT_API_URL: &str = "https://api.fallow.cloud";

/// Exit code for network failures (connect error, timeout, auth rejection).
/// Used by any subcommand that reaches fallow cloud; keeps error classification
/// consistent across `license` and `coverage` surfaces.
pub const NETWORK_EXIT_CODE: u8 = 7;

/// Environment variable pointing at a PEM trust bundle for fallow cloud calls.
pub const CA_BUNDLE_ENV: &str = "FALLOW_CA_BUNDLE";

/// Maximum Retry-After sleep accepted from the server.
pub const RETRY_MAX_WAIT_SECONDS: u64 = 60;

/// Default connect timeout (seconds).
const DEFAULT_CONNECT_TIMEOUT_SECS: u64 = 5;
/// Default total request timeout (seconds).
const DEFAULT_TOTAL_TIMEOUT_SECS: u64 = 10;

/// Construct a `ureq::Agent` with the default timeouts (5s connect, 10s total).
///
/// Suitable for small-body JSON requests (license trial / refresh). For larger
/// payloads (inventory upload), use [`api_agent_with_timeout`].
#[expect(
    dead_code,
    reason = "kept as the infallible compatibility wrapper for out-of-tree users"
)]
pub fn api_agent() -> ureq::Agent {
    api_agent_with_timeout(DEFAULT_CONNECT_TIMEOUT_SECS, DEFAULT_TOTAL_TIMEOUT_SECS)
}

/// Construct a fallible `ureq::Agent` with the default API timeouts.
pub fn try_api_agent() -> Result<ureq::Agent, ApiClientError> {
    try_api_agent_with_timeout(DEFAULT_CONNECT_TIMEOUT_SECS, DEFAULT_TOTAL_TIMEOUT_SECS)
}

/// Construct a `ureq::Agent` with custom timeouts.
///
/// Both timeouts are honored: connect applies to the initial TCP handshake,
/// total bounds the full request/response cycle. `http_status_as_error(false)`
/// is set so callers can inspect non-2xx responses via [`http_status_message`]
/// instead of having them surface as transport errors.
pub fn api_agent_with_timeout(connect_timeout_secs: u64, total_timeout_secs: u64) -> ureq::Agent {
    try_api_agent_with_timeout(connect_timeout_secs, total_timeout_secs)
        .unwrap_or_else(|err| panic!("{err}"))
}

/// Construct a fallible `ureq::Agent` with custom timeouts.
///
/// This variant reports invalid `FALLOW_CA_BUNDLE` configuration as a typed
/// setup error so user-facing commands can exit with the network failure code
/// instead of panicking.
pub fn try_api_agent_with_timeout(
    connect_timeout_secs: u64,
    total_timeout_secs: u64,
) -> Result<ureq::Agent, ApiClientError> {
    let mut builder = ureq::Agent::config_builder();
    if let Some(tls_config) = tls_config_from_env()? {
        builder = builder.tls_config(tls_config);
    }
    Ok(builder
        .timeout_connect(Some(Duration::from_secs(connect_timeout_secs)))
        .timeout_global(Some(Duration::from_secs(total_timeout_secs)))
        .http_status_as_error(false)
        .build()
        .new_agent())
}

/// Error raised while constructing the shared API client.
#[derive(Debug)]
pub struct ApiClientError {
    message: String,
}

impl ApiClientError {
    fn ca_bundle(path: &str, detail: impl fmt::Display) -> Self {
        Self {
            message: format!("{CA_BUNDLE_ENV}={path}: {detail}"),
        }
    }
}

impl fmt::Display for ApiClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ApiClientError {}

fn tls_config_from_env() -> Result<Option<TlsConfig>, ApiClientError> {
    let Some(path) = std::env::var(CA_BUNDLE_ENV)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
    else {
        return Ok(None);
    };
    let bytes = std::fs::read(PathBuf::from(&path)).map_err(|err| {
        ApiClientError::ca_bundle(&path, format!("failed to read PEM bundle: {err}"))
    })?;
    let mut certs = Vec::new();
    for item in ureq::tls::parse_pem(&bytes) {
        if let PemItem::Certificate(cert) = item.map_err(|err| {
            ApiClientError::ca_bundle(&path, format!("failed to parse PEM bundle: {err}"))
        })? {
            certs.push(cert);
        }
    }
    if certs.is_empty() {
        return Err(ApiClientError::ca_bundle(
            &path,
            "PEM bundle did not contain any certificates",
        ));
    }
    Ok(Some(
        TlsConfig::builder()
            .root_certs(RootCerts::new_with_certs(&certs))
            .build(),
    ))
}

/// Resolve an API endpoint path to a full URL.
///
/// Honors `FALLOW_API_URL` for staging/local development. Trailing slashes on
/// the base are trimmed so `/v1/...` paths never double-slash.
pub fn api_url(path: &str) -> String {
    let base = std::env::var("FALLOW_API_URL")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_API_URL.to_owned());
    format!("{}{path}", base.trim_end_matches('/'))
}

/// Structured error payload returned by fallow cloud on non-2xx responses.
#[derive(Debug, Deserialize, Default)]
pub struct ErrorEnvelope {
    /// Machine-readable code (e.g. `rate_limit_exceeded`, `payload_too_large`).
    #[serde(default)]
    pub code: Option<String>,
    /// Human-readable message from the backend.
    #[serde(default)]
    pub message: Option<String>,
}

/// Result of parsing a fallow-cloud error body.
#[derive(Debug)]
pub enum ParsedErrorEnvelope {
    Parsed(ErrorEnvelope),
    Malformed { body: String, error: String },
    Missing,
}

impl ParsedErrorEnvelope {
    pub fn code(&self) -> Option<&str> {
        match self {
            Self::Parsed(envelope) => envelope.code.as_deref(),
            Self::Malformed { .. } | Self::Missing => None,
        }
    }

    pub fn message(&self) -> Option<&str> {
        match self {
            Self::Parsed(envelope) => envelope.message.as_deref(),
            Self::Malformed { .. } | Self::Missing => None,
        }
    }

    pub fn raw_body(&self) -> Option<&str> {
        match self {
            Self::Malformed { body, .. } if !body.trim().is_empty() => Some(body),
            Self::Parsed(_) | Self::Malformed { .. } | Self::Missing => None,
        }
    }

    pub fn parse_failure(&self) -> Option<&str> {
        match self {
            Self::Malformed { error, .. } => Some(error),
            Self::Parsed(_) | Self::Missing => None,
        }
    }
}

pub fn parse_error_envelope(body: &str) -> ParsedErrorEnvelope {
    if body.trim().is_empty() {
        return ParsedErrorEnvelope::Missing;
    }
    match serde_json::from_str::<ErrorEnvelope>(body) {
        Ok(envelope) => ParsedErrorEnvelope::Parsed(envelope),
        Err(error) => ParsedErrorEnvelope::Malformed {
            body: body.to_owned(),
            error: error.to_string(),
        },
    }
}

pub fn response_message_suffix(body: &str, envelope: &ParsedErrorEnvelope) -> String {
    if let Some(message) = envelope.message()
        && !message.trim().is_empty()
    {
        return format!(": {}", message.trim());
    }
    let raw = envelope.raw_body().unwrap_or(body);
    if !raw.trim().is_empty() {
        let parse_detail = envelope.parse_failure().map_or_else(String::new, |err| {
            format!(" (malformed error envelope: {err})")
        });
        return format!(": {}{parse_detail}", raw.trim());
    }
    String::new()
}

/// Whether a fallow-cloud response status should be retried by upload clients.
pub const fn should_retry_status(status: u16) -> bool {
    status == 429 || matches!(status, 502..=504)
}

pub fn retry_after_delay(raw: Option<&str>, now: SystemTime) -> Option<Duration> {
    let value = raw?.trim();
    if value.is_empty() {
        return None;
    }
    if let Ok(seconds) = value.parse::<u64>() {
        return Some(clamp_retry_delay(Duration::from_secs(seconds)));
    }
    let date = httpdate::parse_http_date(value).ok()?;
    let delay = date
        .duration_since(now)
        .unwrap_or_else(|_| Duration::from_secs(0));
    Some(clamp_retry_delay(delay))
}

pub fn retry_delay_for_status(
    status: u16,
    retry_after: Option<&str>,
    attempt: u8,
    now: SystemTime,
) -> Duration {
    if status == 429
        && let Some(delay) = retry_after_delay(retry_after, now)
    {
        return delay;
    }
    let millis = 100_u64.saturating_mul(u64::from(attempt.max(1)));
    Duration::from_millis(millis)
}

fn clamp_retry_delay(delay: Duration) -> Duration {
    delay.min(Duration::from_secs(RETRY_MAX_WAIT_SECONDS))
}

/// Map a backend error-code + operation pair to an actionable user-facing
/// hint. Returns `None` for unknown codes; callers fall back to the generic
/// "HTTP N: body" shape produced by [`http_status_message`].
pub fn actionable_error_hint(operation: &str, code: &str) -> Option<&'static str> {
    match (operation, code) {
        ("refresh", "token_stale") => Some(
            "your stored license is too stale to refresh. Reactivate with: fallow license activate --trial --email <addr>",
        ),
        ("refresh", "invalid_token") => Some(
            "your stored license token is missing required claims. Reactivate with: fallow license activate --trial --email <addr>",
        ),
        // Trial + refresh are license-JWT flows: a stale / invalid JWT is
        // fixed by reactivating via the trial endpoint.
        ("refresh" | "trial", "unauthorized") => Some(
            "authentication failed. Reactivate with: fallow license activate --trial --email <addr>",
        ),
        // upload-inventory uses a separate API key (`fallow_live_k1_*`), not
        // the license JWT. Reactivating the trial does NOT rotate the API
        // key. Point users at key generation instead.
        ("upload-inventory", "unauthorized") => Some(
            "authentication failed. Generate an API key at https://fallow.cloud/settings#api-keys and set FALLOW_API_KEY on the runner. Note: this key is separate from the license JWT; `fallow license activate --trial` will not fix this error.",
        ),
        ("trial", "rate_limit_exceeded") => Some(
            "trial creation is rate-limited to 5 per hour per IP. Wait an hour or retry from a different network (in CI, start the trial locally and set FALLOW_LICENSE on the runner).",
        ),
        ("upload-inventory", "payload_too_large") => Some(
            "inventory exceeds the 200,000-function server limit. Scope the walk with --exclude-paths, or open an issue if this is a legitimately large repo.",
        ),
        _ => None,
    }
}

/// Abstraction over an HTTP response's status + body accessors.
///
/// Implemented for `http::Response<ureq::Body>` and exposed as a trait so
/// error-path tests can substitute a lightweight stub without a real network
/// round-trip.
pub trait ResponseBodyReader {
    /// HTTP status code (200, 401, 429, ...).
    fn status(&self) -> u16;
    /// Deserialize the response body as JSON into `T`.
    fn read_json<T: DeserializeOwned>(&mut self) -> Result<T, ureq::Error>;
    /// Read the response body as a UTF-8 string.
    fn read_to_string(&mut self) -> Result<String, ureq::Error>;
}

impl ResponseBodyReader for http::Response<ureq::Body> {
    fn status(&self) -> u16 {
        self.status().as_u16()
    }

    fn read_json<T: DeserializeOwned>(&mut self) -> Result<T, ureq::Error> {
        self.body_mut().read_json::<T>()
    }

    fn read_to_string(&mut self) -> Result<String, ureq::Error> {
        self.body_mut().read_to_string()
    }
}

/// Redact credential-bearing header substrings before surfacing a
/// network-error message to the user.
///
/// `ureq`'s `Display` impl can include the outgoing request's headers on
/// certain failure modes (TLS handshake errors, DNS errors, internal panics).
/// Any `Authorization: Bearer <key>` or `PRIVATE-TOKEN: <token>` we set on the
/// request would then bleed into stderr via `emit_error`, which lands in CI
/// logs. Route every `format!("{err}")` against a ureq error through this
/// helper to mask the secret before it reaches the user.
///
/// Token charset matches the JWT + fallow API-key alphabets
/// (`A-Za-z0-9_.\-=`); the scan stops at the first byte outside that set so
/// punctuation following the secret (e.g. `Bearer abc123.\n`) is preserved.
pub fn sanitize_network_error(detail: &str) -> String {
    let detail = redact_bearer_tokens(detail);
    redact_header_token(&detail, "PRIVATE-TOKEN")
}

fn redact_bearer_tokens(detail: &str) -> String {
    const BEARER: &str = "Bearer ";
    const REDACTED: &str = "Bearer ***";

    let bytes = detail.as_bytes();
    let mut out = String::with_capacity(detail.len());
    let mut cursor = 0;
    while let Some(rel) = detail[cursor..].find(BEARER) {
        let start = cursor + rel;
        out.push_str(&detail[cursor..start]);
        let token_start = start + BEARER.len();
        let mut token_end = token_start;
        while token_end < bytes.len() && is_token_byte(bytes[token_end]) {
            token_end += 1;
        }
        if token_end == token_start {
            // `Bearer` followed by no token character: preserve as-is and
            // advance past the literal so we do not infinite-loop.
            out.push_str(BEARER);
            cursor = token_end;
            continue;
        }
        out.push_str(REDACTED);
        cursor = token_end;
    }
    out.push_str(&detail[cursor..]);
    out
}

fn redact_header_token(detail: &str, header_name: &str) -> String {
    let bytes = detail.as_bytes();
    let header = header_name.as_bytes();
    let mut out = String::with_capacity(detail.len());
    let mut cursor = 0;
    while let Some(start) = find_ascii_case_insensitive(bytes, cursor, header) {
        out.push_str(&detail[cursor..start]);
        let mut token_start = start + header.len();
        while token_start < bytes.len() && matches!(bytes[token_start], b' ' | b'\t') {
            token_start += 1;
        }
        if token_start >= bytes.len() || bytes[token_start] != b':' {
            out.push_str(&detail[start..=start]);
            cursor = start + 1;
            continue;
        }
        token_start += 1;
        while token_start < bytes.len() && matches!(bytes[token_start], b' ' | b'\t') {
            token_start += 1;
        }

        let mut token_end = token_start;
        while token_end < bytes.len() && is_token_byte(bytes[token_end]) {
            token_end += 1;
        }
        if token_end == token_start {
            out.push_str(&detail[start..token_start]);
            cursor = token_start;
            continue;
        }
        out.push_str(&detail[start..token_start]);
        out.push_str("***");
        cursor = token_end;
    }
    out.push_str(&detail[cursor..]);
    out
}

fn find_ascii_case_insensitive(haystack: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || from >= haystack.len() {
        return None;
    }
    haystack[from..]
        .windows(needle.len())
        .position(|window| {
            window
                .iter()
                .zip(needle)
                .all(|(actual, expected)| actual.eq_ignore_ascii_case(expected))
        })
        .map(|offset| from + offset)
}

const fn is_token_byte(byte: u8) -> bool {
    matches!(byte, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_' | b'.' | b'-' | b'=')
}

/// Format a non-2xx response into a user-facing error string.
///
/// Tries to parse the body as an [`ErrorEnvelope`]. When the envelope has a
/// known `code` for the given `operation`, the mapped hint is returned with
/// the HTTP status and code appended. Otherwise the backend's `message`
/// (or raw body) is appended to a generic "HTTP N" line.
pub fn http_status_message(response: &mut impl ResponseBodyReader, operation: &str) -> String {
    let status = response.status();
    let body = response.read_to_string().unwrap_or_default();
    let envelope = parse_error_envelope(&body);
    if let Some(code) = envelope.code()
        && let Some(hint) = actionable_error_hint(operation, code)
    {
        return format!("{hint} (HTTP {status}, code {code})");
    }
    let body_suffix = response_message_suffix(&body, &envelope);
    format!("{operation} request failed with HTTP {status}{body_suffix}")
}

#[cfg(test)]
mod tests {
    use super::*;

    struct StubResponse {
        status: u16,
        body: String,
    }

    impl ResponseBodyReader for StubResponse {
        fn status(&self) -> u16 {
            self.status
        }

        fn read_json<T: DeserializeOwned>(&mut self) -> Result<T, ureq::Error> {
            unreachable!("error-path tests do not read JSON")
        }

        fn read_to_string(&mut self) -> Result<String, ureq::Error> {
            Ok(std::mem::take(&mut self.body))
        }
    }

    #[test]
    fn refresh_token_stale_hint_points_to_reactivation() {
        let mut response = StubResponse {
            status: 401,
            body: r#"{"error":true,"message":"token stale","code":"token_stale"}"#.to_owned(),
        };
        let message = http_status_message(&mut response, "refresh");
        assert!(
            message.contains("Reactivate with: fallow license activate --trial"),
            "expected reactivation hint, got: {message}"
        );
        assert!(message.contains("token_stale"));
    }

    #[test]
    fn refresh_invalid_token_hint_points_to_reactivation() {
        let mut response = StubResponse {
            status: 401,
            body: r#"{"error":true,"code":"invalid_token"}"#.to_owned(),
        };
        let message = http_status_message(&mut response, "refresh");
        assert!(message.contains("missing required claims"));
        assert!(message.contains("invalid_token"));
    }

    #[test]
    fn upload_inventory_unauthorized_points_to_api_keys_not_trial() {
        let mut response = StubResponse {
            status: 401,
            body: r#"{"error":true,"code":"unauthorized"}"#.to_owned(),
        };
        let message = http_status_message(&mut response, "upload-inventory");
        // API keys are a distinct secret from the license JWT. Sending trial
        // users to `license activate --trial` when they get a 401 on upload
        // is a dead-end support loop. The hint MUST both direct them to the
        // API-keys page AND explain that the trial flow won't fix it, so we
        // require the disqualifier to appear adjacent to "will not fix".
        // Regression test for BLOCK 3 from the public-readiness panel.
        assert!(
            message.contains("https://fallow.cloud/settings#api-keys"),
            "expected api-keys URL, got: {message}"
        );
        assert!(
            message.contains("FALLOW_API_KEY"),
            "expected FALLOW_API_KEY mention, got: {message}"
        );
        assert!(
            message.contains("will not fix"),
            "expected explicit 'will not fix this error' disqualifier so users do not retry via --trial; got: {message}"
        );
    }

    #[test]
    fn trial_rate_limit_hint_mentions_five_per_hour() {
        let mut response = StubResponse {
            status: 429,
            body: r#"{"error":true,"code":"rate_limit_exceeded"}"#.to_owned(),
        };
        let message = http_status_message(&mut response, "trial");
        assert!(message.contains("5 per hour per IP"));
        assert!(message.contains("FALLOW_LICENSE"));
    }

    #[test]
    fn unknown_code_falls_back_to_backend_message_when_present() {
        let mut response = StubResponse {
            status: 500,
            body: r#"{"error":true,"code":"checkout_error","message":"stripe returned no session url"}"#
                .to_owned(),
        };
        let message = http_status_message(&mut response, "refresh");
        assert!(message.starts_with("refresh request failed with HTTP 500"));
        assert!(
            message.ends_with(": stripe returned no session url"),
            "expected backend message on fallback, got: {message}"
        );
    }

    #[test]
    fn unknown_code_without_message_falls_back_to_raw_body() {
        let mut response = StubResponse {
            status: 500,
            body: r#"{"error":true,"code":"checkout_error"}"#.to_owned(),
        };
        let message = http_status_message(&mut response, "refresh");
        assert!(message.starts_with("refresh request failed with HTTP 500"));
        assert!(message.contains("checkout_error"));
    }

    #[test]
    fn empty_body_still_produces_minimal_message() {
        let mut response = StubResponse {
            status: 502,
            body: String::new(),
        };
        let message = http_status_message(&mut response, "trial");
        assert_eq!(message, "trial request failed with HTTP 502");
    }

    #[test]
    fn malformed_error_envelope_preserves_raw_body_and_parse_failure() {
        let mut response = StubResponse {
            status: 500,
            body: "upstream timeout".to_owned(),
        };
        let message = http_status_message(&mut response, "refresh");
        assert!(message.contains("upstream timeout"));
        assert!(message.contains("malformed error envelope"));
    }

    #[test]
    fn retry_status_is_narrowed_to_429_and_gateway_failures() {
        assert!(should_retry_status(429));
        assert!(should_retry_status(502));
        assert!(should_retry_status(503));
        assert!(should_retry_status(504));
        assert!(!should_retry_status(500));
        assert!(!should_retry_status(501));
    }

    #[test]
    fn retry_after_delta_and_http_date_are_capped() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        assert_eq!(
            retry_after_delay(Some("120"), now),
            Some(Duration::from_secs(RETRY_MAX_WAIT_SECONDS))
        );
        let date = httpdate::fmt_http_date(now + Duration::from_mins(2));
        assert_eq!(
            retry_after_delay(Some(&date), now),
            Some(Duration::from_secs(RETRY_MAX_WAIT_SECONDS))
        );
    }

    #[test]
    fn retry_delay_ignores_retry_after_for_5xx() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        assert_eq!(
            retry_delay_for_status(503, Some("60"), 2, now),
            Duration::from_millis(200)
        );
    }

    #[test]
    #[expect(unsafe_code, reason = "env var mutation requires unsafe")]
    fn ca_bundle_read_errors_are_reported_as_client_setup_errors() {
        let prior = std::env::var(CA_BUNDLE_ENV).ok();
        // SAFETY: env mutation is unsafe because it is not thread-safe. This
        // test serializes its own writes and restores the prior value before
        // returning; no other test in this module touches FALLOW_CA_BUNDLE.
        unsafe {
            std::env::set_var(CA_BUNDLE_ENV, "/definitely/missing/fallow-ca.pem");
        }
        let err = try_api_agent().expect_err("missing bundle should fail");
        let message = err.to_string();
        assert!(message.contains(CA_BUNDLE_ENV));
        assert!(message.contains("failed to read PEM bundle"));
        // SAFETY: see the `set_var` safety note above.
        unsafe {
            if let Some(value) = prior {
                std::env::set_var(CA_BUNDLE_ENV, value);
            } else {
                std::env::remove_var(CA_BUNDLE_ENV);
            }
        }
    }

    #[test]
    fn sanitize_network_error_redacts_bearer_token() {
        let input = "tls handshake failed; sent Authorization: Bearer fallow_live_abc123.def456";
        let output = sanitize_network_error(input);
        assert!(
            output.ends_with("Bearer ***"),
            "expected sanitized tail, got: {output}"
        );
        assert!(
            !output.contains("fallow_live_abc123"),
            "secret leaked: {output}"
        );
    }

    #[test]
    fn sanitize_network_error_redacts_multiple_bearer_tokens() {
        let input = "first attempt: Bearer aaa.bbb retried as Bearer ccc.ddd";
        let output = sanitize_network_error(input);
        assert_eq!(output, "first attempt: Bearer *** retried as Bearer ***");
    }

    #[test]
    fn sanitize_network_error_redacts_gitlab_private_token_header() {
        let input = "GitLab request failed: PRIVATE-TOKEN: glpat-secret_token-123\nretry failed";
        let output = sanitize_network_error(input);
        assert_eq!(
            output,
            "GitLab request failed: PRIVATE-TOKEN: ***\nretry failed"
        );
        assert!(!output.contains("glpat-secret"));
    }

    #[test]
    fn sanitize_network_error_redacts_private_token_header_case_insensitively() {
        let input = "request headers: Private-Token:\tglpat.SECRET_123";
        let output = sanitize_network_error(input);
        assert_eq!(output, "request headers: Private-Token:\t***");
    }

    #[test]
    fn sanitize_network_error_passes_through_when_no_bearer() {
        let input = "connection refused (dns lookup failed for api.fallow.cloud)";
        let output = sanitize_network_error(input);
        assert_eq!(output, input);
    }

    #[test]
    fn sanitize_network_error_preserves_trailing_punctuation_after_token() {
        let input = "Bearer fallow_live_xyz, retry next.";
        let output = sanitize_network_error(input);
        assert_eq!(output, "Bearer ***, retry next.");
    }

    #[test]
    fn sanitize_network_error_preserves_literal_bearer_when_no_token_follows() {
        // `Bearer ` followed by a non-token byte (e.g. `@`) leaves the prefix
        // untouched so we do not corrupt non-secret prose that mentions the
        // literal `Bearer `.
        let input = "Bearer @other";
        let output = sanitize_network_error(input);
        assert_eq!(output, input);
    }

    #[test]
    fn sanitize_network_error_preserves_private_token_when_no_token_follows() {
        let input = "PRIVATE-TOKEN: @not-a-token";
        let output = sanitize_network_error(input);
        assert_eq!(output, input);
    }

    // Env-var assertions run in one test to avoid interleaving with parallel
    // tests that also touch `FALLOW_API_URL`. Restores the prior value.
    #[test]
    #[expect(unsafe_code, reason = "env var mutation requires unsafe")]
    fn api_url_respects_env_override_and_default() {
        let prior = std::env::var("FALLOW_API_URL").ok();

        // SAFETY: env mutation is unsafe because it is not thread-safe. This
        // test serializes its own writes and restores the prior value before
        // returning; no other test in this module touches FALLOW_API_URL.
        unsafe {
            std::env::remove_var("FALLOW_API_URL");
        }
        assert_eq!(
            api_url("/v1/coverage/repo/inventory"),
            "https://api.fallow.cloud/v1/coverage/repo/inventory",
        );

        // SAFETY: see the `remove_var` safety note above.
        unsafe {
            std::env::set_var("FALLOW_API_URL", "http://127.0.0.1:3000/");
        }
        assert_eq!(
            api_url("/v1/coverage/a/inventory"),
            "http://127.0.0.1:3000/v1/coverage/a/inventory",
        );

        // SAFETY: see the `remove_var` safety note above.
        unsafe {
            if let Some(value) = prior {
                std::env::set_var("FALLOW_API_URL", value);
            } else {
                std::env::remove_var("FALLOW_API_URL");
            }
        }
    }
}
