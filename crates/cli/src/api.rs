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

use std::time::Duration;

use serde::Deserialize;
use serde::de::DeserializeOwned;

/// Default fallow cloud API base URL.
pub const DEFAULT_API_URL: &str = "https://api.fallow.cloud";

/// Exit code for network failures (connect error, timeout, auth rejection).
/// Used by any subcommand that reaches fallow cloud; keeps error classification
/// consistent across `license` and `coverage` surfaces.
pub const NETWORK_EXIT_CODE: u8 = 7;

/// Default connect timeout (seconds).
const DEFAULT_CONNECT_TIMEOUT_SECS: u64 = 5;
/// Default total request timeout (seconds).
const DEFAULT_TOTAL_TIMEOUT_SECS: u64 = 10;

/// Construct a `ureq::Agent` with the default timeouts (5s connect, 10s total).
///
/// Suitable for small-body JSON requests (license trial / refresh). For larger
/// payloads (inventory upload), use [`api_agent_with_timeout`].
pub fn api_agent() -> ureq::Agent {
    api_agent_with_timeout(DEFAULT_CONNECT_TIMEOUT_SECS, DEFAULT_TOTAL_TIMEOUT_SECS)
}

/// Construct a `ureq::Agent` with custom timeouts.
///
/// Both timeouts are honored: connect applies to the initial TCP handshake,
/// total bounds the full request/response cycle. `http_status_as_error(false)`
/// is set so callers can inspect non-2xx responses via [`http_status_message`]
/// instead of having them surface as transport errors.
pub fn api_agent_with_timeout(connect_timeout_secs: u64, total_timeout_secs: u64) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(connect_timeout_secs)))
        .timeout_global(Some(Duration::from_secs(total_timeout_secs)))
        .http_status_as_error(false)
        .build()
        .new_agent()
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
    let envelope: Option<ErrorEnvelope> = serde_json::from_str(&body).ok();
    if let Some(envelope) = envelope.as_ref()
        && let Some(code) = envelope.code.as_deref()
        && let Some(hint) = actionable_error_hint(operation, code)
    {
        return format!("{hint} (HTTP {status}, code {code})");
    }
    let body_suffix = match envelope.as_ref().and_then(|e| e.message.as_deref()) {
        Some(message) if !message.trim().is_empty() => format!(": {}", message.trim()),
        _ if !body.trim().is_empty() => format!(": {}", body.trim()),
        _ => String::new(),
    };
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
